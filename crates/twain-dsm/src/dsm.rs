//! The Data Source Manager: its file checked before its code runs, loaded
//! once per process and never unloaded, and its entry point called through
//! one wrapper per triplet the crate sends.
//!
//! Never unloading is what makes the rest safe to hold: the entry point and
//! the memory functions `DAT_ENTRYPOINT` gives are valid for the process's
//! life, so nothing that keeps them can outlive them.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{null, null_mut};
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, GENERIC_READ, GetLastError, HANDLE,
    HMODULE, HWND, INVALID_HANDLE_VALUE, TRUST_E_NOSIGNATURE, TRUST_E_PROVIDER_UNKNOWN,
    TRUST_E_SUBJECT_FORM_UNKNOWN,
};
use windows_sys::Win32::Security::Cryptography::{
    CERT_NAME_ATTR_TYPE, CertGetNameStringW, szOID_ORGANIZATION_NAME,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvSignerFromChain,
    WTHelperProvDataFromStateData, WinVerifyTrust,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_BEGIN,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING, ReadFile, SetFilePointerEx,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

use crate::codes::{ConditionCode, ReturnCode, Triplet};
use crate::error::LoadError;
use crate::ffi::{
    self, DsmEntryProc, DsmMemAllocate, DsmMemFree, DsmMemLock, DsmMemUnlock, TW_ENTRYPOINT,
    TW_IDENTITY, TW_STATUS,
};
use crate::pe;

/// The organization a DSM's signer must name.
const SIGNER: &str = "TWAIN Working Group";

/// The loaded DSM, for the process's life.
pub(crate) struct Dsm {
    entry: DsmEntryProc,
    path: PathBuf,
    file: FileId,
    memory: OnceLock<Option<Memory>>,
}

/// The DSM's memory functions, from `DAT_ENTRYPOINT`.
#[derive(Clone, Copy)]
#[allow(dead_code)] // the containers' blocks are allocated with them
pub(crate) struct Memory {
    pub(crate) allocate: DsmMemAllocate,
    pub(crate) free: DsmMemFree,
    pub(crate) lock: DsmMemLock,
    pub(crate) unlock: DsmMemUnlock,
}

/// A file as the system knows it: its volume and its index there, the same
/// whatever path it's reached by (a 32-bit process's System32 is SysWOW64).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FileId {
    volume: u32,
    index: u64,
}

/// The DSM once loaded: the process holds one, ever.
static LOADED: Mutex<Option<&'static Dsm>> = Mutex::new(None);

/// `<system folder>\TWAINDSM.dll`: SysWOW64's in a 32-bit process, which
/// Windows redirects the System32 path to.
pub(crate) fn system_dsm() -> PathBuf {
    let mut buf = vec![0u16; 260];
    loop {
        // SAFETY: `buf` is writable for its length in u16s, as the call is
        // told.
        let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
        if n == 0 {
            // Can't happen on a working Windows; a path that isn't there is
            // refused as `NotFound`.
            return PathBuf::from("TWAINDSM.dll");
        }
        if n < buf.len() {
            let dir = PathBuf::from(OsString::from_wide(&buf[..n]));
            return dir.join("TWAINDSM.dll");
        }
        buf.resize(n + 1, 0);
    }
}

/// The DSM at `path`, loaded and checked, or the one this process loaded
/// before, if it's that file.
pub(crate) fn load(path: &Path) -> Result<&'static Dsm, LoadError> {
    let path = std::path::absolute(path).map_err(|error| LoadError::Open {
        path: path.to_owned(),
        error,
    })?;
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(dsm) = *loaded {
        if file_id_of(&path) == Some(dsm.file) {
            return Ok(dsm);
        }
        return Err(LoadError::Elsewhere {
            name: file_name(&path),
            loaded: dsm.path.clone(),
        });
    }

    // A module of that name loaded already would be what the loader hands
    // back: refused unless it's this file.
    let name = wide_os(Path::new(&file_name(&path)));
    // SAFETY: `name` is NUL-terminated UTF-16; the call only looks.
    let present = unsafe { GetModuleHandleW(name.as_ptr()) };
    let file = File::open(&path)?;
    let id = file.id().ok_or_else(|| LoadError::Open {
        path: path.clone(),
        error: std::io::Error::last_os_error(),
    })?;
    if !present.is_null() {
        let at = module_path(present);
        if file_id_of(&at) != Some(id) {
            return Err(LoadError::Elsewhere {
                name: file_name(&path),
                loaded: at,
            });
        }
    }

    // Checked through the handle held: its machine, then its signature and
    // signer, before any of its code runs.
    let head = file.head(4096);
    let found = pe::machine(&head);
    if found != Some(pe::this_machine()) {
        return Err(LoadError::Machine {
            found,
            expected: pe::this_machine(),
        });
    }
    file.rewind();
    verify(&path, &file)?;

    let wide_path = wide_os(&path);
    // SAFETY: `wide_path` is NUL-terminated UTF-16; no file handle is passed
    // (it's reserved); the flags look for the DLL's own dependencies in its
    // folder and System32 only. The file stays held against change by
    // `file` while it loads.
    let module = unsafe {
        LoadLibraryExW(
            wide_path.as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        // SAFETY: read straight after the failed call.
        let code = unsafe { GetLastError() };
        return Err(LoadError::Loader { code });
    }
    // From here the module is never freed: once loaded, it stays.
    let at = module_path(module);
    if file_id_of(&at) != Some(id) {
        return Err(LoadError::NotTheFileChecked { loaded: at });
    }
    // SAFETY: `module` is loaded (and stays so); the name is a NUL-terminated
    // ASCII string.
    let proc = unsafe { GetProcAddress(module, c"DSM_Entry".as_ptr().cast()) };
    let Some(proc) = proc else {
        return Err(LoadError::MissingEntry);
    };
    // SAFETY: `DSM_Entry` is declared in twain.h as `DSMENTRYPROC`, the
    // signature `DsmEntryProc` gives it (`TW_CALLINGSTYLE` being `PASCAL`:
    // `extern "system"`); both are function pointers of one size. The file was
    // checked as the TWAIN Working Group's DSM above.
    let entry =
        unsafe { core::mem::transmute::<unsafe extern "system" fn() -> isize, DsmEntryProc>(proc) };
    drop(file);
    let dsm: &'static Dsm = Box::leak(Box::new(Dsm {
        entry,
        path,
        file: id,
        memory: OnceLock::new(),
    }));
    *loaded = Some(dsm);
    Ok(dsm)
}

/// The file's Authenticode signature verified, offline, and its signer's
/// organization read.
fn verify(path: &Path, file: &File) -> Result<(), LoadError> {
    let wide_path = wide_os(path);
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide_path.as_ptr(),
        hFile: file.0,
        pgKnownSubject: null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: null_mut(),
        pSIPClientData: null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut info },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: null_mut(),
        pwszURLReference: null_mut(),
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        dwUIContext: 0,
        pSignatureSettings: null_mut(),
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: `data` and the `info` it points to are initialized and live for
    // the call, `info` naming the open file by handle and path; no UI (the
    // window is INVALID_HANDLE_VALUE's, which says so); the state it keeps is
    // closed below.
    let status = unsafe {
        WinVerifyTrust(
            INVALID_HANDLE_VALUE as HWND,
            &mut action,
            (&raw mut data).cast(),
        )
    };
    let result = match status {
        0 => signer(data.hWVTStateData).map_or(
            Err(LoadError::WrongSigner {
                subject: String::new(),
            }),
            |subject| {
                if subject == SIGNER {
                    Ok(())
                } else {
                    Err(LoadError::WrongSigner { subject })
                }
            },
        ),
        TRUST_E_NOSIGNATURE | TRUST_E_SUBJECT_FORM_UNKNOWN | TRUST_E_PROVIDER_UNKNOWN => {
            Err(LoadError::Unsigned)
        }
        code => Err(LoadError::BadSignature { code }),
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: closes the state the verifying call opened in `data`, which is
    // as it left it but for the action.
    unsafe {
        WinVerifyTrust(
            INVALID_HANDLE_VALUE as HWND,
            &mut action,
            (&raw mut data).cast(),
        )
    };
    result
}

/// The organization (`O=`) of a verified signature's signing certificate.
fn signer(state: HANDLE) -> Option<String> {
    // SAFETY: `state` is the state WinVerifyTrust kept for a verified file,
    // not yet closed.
    let provider = unsafe { WTHelperProvDataFromStateData(state) };
    if provider.is_null() {
        return None;
    }
    // SAFETY: `provider` is the state's provider data; signer 0, not a
    // countersigner.
    let sgnr = unsafe { WTHelperGetProvSignerFromChain(provider, 0, 0, 0) };
    if sgnr.is_null() {
        return None;
    }
    // SAFETY: `sgnr` points to the provider's signer, live until the state is
    // closed.
    let (count, chain) = unsafe { ((*sgnr).csCertChain, (*sgnr).pasCertChain) };
    if count == 0 || chain.is_null() {
        return None;
    }
    // SAFETY: the chain holds `count` (> 0) certificates; the first is the
    // signer's.
    let cert = unsafe { (*chain).pCert };
    if cert.is_null() {
        return None;
    }
    let mut buf = [0u16; 256];
    // SAFETY: `cert` is a live certificate context; the type asks for an
    // attribute of the subject, named by its OID string; `buf` is writable
    // for its length in u16s, as the call is told.
    let n = unsafe {
        CertGetNameStringW(
            cert,
            CERT_NAME_ATTR_TYPE,
            0,
            szOID_ORGANIZATION_NAME.cast(),
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    } as usize;
    // The count includes the NUL; 1 means none.
    (n > 1).then(|| String::from_utf16_lossy(&buf[..n.min(buf.len()) - 1]))
}

/// A file held open for reading, others kept from writing or deleting it.
struct File(HANDLE);

impl File {
    fn open(path: &Path) -> Result<File, LoadError> {
        let name = wide_os(path);
        // SAFETY: `name` is NUL-terminated UTF-16; read access, sharing only
        // reading; no security attributes or template.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ,
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            // SAFETY: read straight after the failed call.
            let code = unsafe { GetLastError() };
            return Err(
                if code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND {
                    LoadError::NotFound {
                        path: path.to_owned(),
                    }
                } else {
                    LoadError::Open {
                        path: path.to_owned(),
                        error: std::io::Error::from_raw_os_error(code as i32),
                    }
                },
            );
        }
        Ok(File(handle))
    }

    /// Up to `len` bytes from where the file is read to.
    fn head(&self, len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        let mut read = 0u32;
        // SAFETY: `buf` is writable for `len` bytes, as the call is told;
        // `read` is writable; no overlapped I/O.
        let ok = unsafe { ReadFile(self.0, buf.as_mut_ptr(), len as u32, &mut read, null_mut()) };
        buf.truncate(if ok != 0 { read as usize } else { 0 });
        buf
    }

    fn rewind(&self) {
        // SAFETY: the handle is open; the new position isn't wanted back.
        unsafe { SetFilePointerEx(self.0, 0, null_mut(), FILE_BEGIN) };
    }

    fn id(&self) -> Option<FileId> {
        file_id(self.0)
    }
}

impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `File::open` and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn file_id(handle: HANDLE) -> Option<FileId> {
    // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is a valid value of it.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { core::mem::zeroed() };
    // SAFETY: `handle` is open; `info` is writable.
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
    (ok != 0).then(|| FileId {
        volume: info.dwVolumeSerialNumber,
        index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

/// The identity of the file at `path`, if it can be opened at all.
fn file_id_of(path: &Path) -> Option<FileId> {
    let name = wide_os(path);
    // SAFETY: `name` is NUL-terminated UTF-16; attributes only, every kind of
    // sharing, so it opens whatever else holds the file; the handle is closed
    // below.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let id = file_id(handle);
    // SAFETY: opened above, closed once.
    unsafe { CloseHandle(handle) };
    id
}

fn module_path(module: HMODULE) -> PathBuf {
    let mut buf = vec![0u16; 260];
    loop {
        // SAFETY: `module` is loaded; `buf` is writable for its length in
        // u16s, as the call is told.
        let n = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
        if n < buf.len() || buf.len() >= 32 * 1024 {
            return PathBuf::from(OsString::from_wide(&buf[..n.min(buf.len())]));
        }
        buf.resize(buf.len() * 2, 0);
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn wide_os(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// An identity for asking the DSM its own condition code: its `Id` is no
/// application's, so the DSM answers with the code it keeps for refusals it
/// couldn't pin on one (`apps.cpp`, `AppGetConditionCode`), and clears it.
fn nobody() -> Cell<TW_IDENTITY> {
    Cell::new(TW_IDENTITY {
        Id: u32::MAX,
        ..TW_IDENTITY::default()
    })
}

const OPENDSM: Triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_PARENT, ffi::MSG_OPENDSM);
const CLOSEDSM: Triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_PARENT, ffi::MSG_CLOSEDSM);
const ENTRYPOINT: Triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_ENTRYPOINT, ffi::MSG_GET);
const STATUS: Triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_STATUS, ffi::MSG_GET);

impl Dsm {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Calls `DSM_Entry`.
    ///
    /// # Safety
    ///
    /// `data` must be what `triplet` takes, valid for the DSM and the source
    /// to read and write for the call; `dest` an open source's identity or
    /// `None` for the DSM itself.
    unsafe fn call(
        &self,
        origin: &Cell<TW_IDENTITY>,
        dest: Option<&Cell<TW_IDENTITY>>,
        triplet: Triplet,
        data: *mut c_void,
    ) -> ReturnCode {
        let dest = dest.map_or(null_mut(), Cell::as_ptr);
        // SAFETY: the entry point is the loaded DSM's, never unloaded. The
        // identities are Cells the caller holds for the call, so the DSM may
        // write them (it fills in the application's `Id`) through their
        // pointers; a Cell hands out no references to its contents that this
        // could alias. The caller keeps `data`'s contract.
        ReturnCode(unsafe {
            (self.entry)(
                origin.as_ptr(),
                dest,
                triplet.dg,
                triplet.dat,
                triplet.msg,
                data,
            )
        })
    }

    /// `MSG_OPENDSM` or `MSG_CLOSEDSM`, with the parent window.
    pub(crate) fn parent(
        &self,
        app: &Cell<TW_IDENTITY>,
        open: bool,
        window: HWND,
    ) -> (Triplet, ReturnCode) {
        let triplet = if open { OPENDSM } else { CLOSEDSM };
        let mut parent = window;
        // SAFETY: DAT_PARENT takes a pointer to the parent window's handle,
        // `parent`, on this frame for the call.
        (triplet, unsafe {
            self.call(app, None, triplet, (&raw mut parent).cast())
        })
    }

    /// `DAT_ENTRYPOINT`: the DSM's memory functions, kept the first time it
    /// gives all four. Whether they're held.
    pub(crate) fn entry_points(&self, app: &Cell<TW_IDENTITY>) -> (Triplet, ReturnCode) {
        let mut ep = TW_ENTRYPOINT {
            Size: size_of::<TW_ENTRYPOINT>() as u32,
            ..TW_ENTRYPOINT::default()
        };
        // SAFETY: DAT_ENTRYPOINT/MSG_GET fills a TW_ENTRYPOINT whose Size is
        // set, `ep`, on this frame for the call.
        let rc = unsafe { self.call(app, None, ENTRYPOINT, (&raw mut ep).cast()) };
        if rc == ReturnCode::SUCCESS {
            let (allocate, free, lock, unlock) = (
                ep.DSM_MemAllocate,
                ep.DSM_MemFree,
                ep.DSM_MemLock,
                ep.DSM_MemUnlock,
            );
            let memory = match (allocate, free, lock, unlock) {
                (Some(allocate), Some(free), Some(lock), Some(unlock)) => Some(Memory {
                    allocate,
                    free,
                    lock,
                    unlock,
                }),
                _ => None,
            };
            // The same DSM gives the same functions: the first answer stands.
            let _ = self.memory.set(memory);
        }
        (ENTRYPOINT, rc)
    }

    /// The memory functions, once `DAT_ENTRYPOINT` gave them.
    #[allow(dead_code)] // the containers' blocks are allocated with them
    pub(crate) fn memory(&self) -> Option<Memory> {
        self.memory.get().copied().flatten()
    }

    /// `DG_CONTROL/DAT_IDENTITY/msg` to the DSM: `MSG_GETFIRST`,
    /// `MSG_GETNEXT`, `MSG_GETDEFAULT`, writing a source's identity into
    /// `out`.
    pub(crate) fn source_identity(
        &self,
        app: &Cell<TW_IDENTITY>,
        msg: u16,
        out: &mut TW_IDENTITY,
    ) -> (Triplet, ReturnCode) {
        debug_assert!(matches!(
            msg,
            ffi::MSG_GETFIRST | ffi::MSG_GETNEXT | ffi::MSG_GETDEFAULT
        ));
        let triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_IDENTITY, msg);
        // SAFETY: these messages of DAT_IDENTITY, sent to the DSM, write a
        // TW_IDENTITY into `out`, borrowed for the call.
        (triplet, unsafe {
            self.call(app, None, triplet, (out as *mut TW_IDENTITY).cast())
        })
    }

    /// A party's condition code (`DAT_STATUS`, which clears it): the DSM's for
    /// `dest` `None`, else the open source's. `None` if it couldn't be asked,
    /// or there's none.
    pub(crate) fn status(
        &self,
        origin: &Cell<TW_IDENTITY>,
        dest: Option<&Cell<TW_IDENTITY>>,
    ) -> Option<ConditionCode> {
        let mut status = TW_STATUS::default();
        // SAFETY: DAT_STATUS/MSG_GET writes a TW_STATUS into `status`, on this
        // frame for the call.
        let rc = unsafe { self.call(origin, dest, STATUS, (&raw mut status).cast()) };
        let code = ConditionCode(status.ConditionCode);
        (rc == ReturnCode::SUCCESS && code != ConditionCode::SUCCESS).then_some(code)
    }

    /// The DSM's condition code after it refused something: the code it keeps
    /// for no application first, as reading the application's own clears
    /// both (`apps.cpp`, `AppGetConditionCode`); then the application's,
    /// which wins if it has one.
    pub(crate) fn dsm_condition(&self, app: &Cell<TW_IDENTITY>) -> Option<ConditionCode> {
        let global = self.status(&nobody(), None);
        let own = self.status(app, None);
        own.or(global)
    }
}
