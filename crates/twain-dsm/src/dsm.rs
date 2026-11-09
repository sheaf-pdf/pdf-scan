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
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, GENERIC_READ, GetLastError,
    GlobalFree, HANDLE, HMODULE, HWND, INVALID_HANDLE_VALUE, TRUST_E_NOSIGNATURE,
    TRUST_E_PROVIDER_UNKNOWN, TRUST_E_SUBJECT_FORM_UNKNOWN,
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
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_BEGIN, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FileIdInfo, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
    OPEN_EXISTING, ReadFile, SetFilePointerEx,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::Win32::System::Memory::{GHND, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

use crate::codes::{ConditionCode, ReturnCode, Triplet};
use crate::container::ContainerError;
use crate::error::LoadError;
use crate::ext_info;
use crate::ffi::{
    self, DsmEntryProc, DsmMemAllocate, DsmMemFree, DsmMemLock, DsmMemUnlock, TW_CALLBACK2,
    TW_CAPABILITY, TW_ENTRYPOINT, TW_IDENTITY, TW_STATUS, TwainCallbackProc,
};
use crate::pe;
use crate::window::Message;

/// A capability's answer: its container type and its block's bytes, or why
/// they weren't read; `None` unless the source succeeded.
pub(crate) type ContainerAnswer = Option<(u16, Result<Vec<u8>, ContainerError>)>;

/// The largest block a source's container or extended image info is read
/// from: no container needs more.
pub(crate) const MAX_CONTAINER: usize = 16 << 20;

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
pub(crate) struct Memory {
    pub(crate) allocate: DsmMemAllocate,
    pub(crate) free: DsmMemFree,
    pub(crate) lock: DsmMemLock,
    pub(crate) unlock: DsmMemUnlock,
}

/// A file as the system knows it: its volume and its id there
/// (`FILE_ID_INFO`, 128 bits, which ReFS needs), the same whatever path it's
/// reached by (a 32-bit process's System32 is SysWOW64).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FileId {
    volume: u64,
    id: [u8; 16],
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
        // Not this crate's (it would be in LOADED): another of the name is
        // what the loader would hand back, and the same one is another TWAIN
        // client's, which using too would race (the DSM has no locks).
        let at = module_path(present);
        return Err(if file_id_of(&at) == Some(id) {
            LoadError::InUse { loaded: at }
        } else {
            LoadError::Elsewhere {
                name: file_name(&path),
                loaded: at,
            }
        });
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

    // The file the handle has open, by its final path, so a link or a
    // junction on the way that's changed since can't load another.
    let load_path = file.final_path().unwrap_or_else(|| path.clone());
    let wide_path = wide_os(&load_path);
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

/// A block of memory TWAIN passes by handle: allocated, locked and freed
/// with the DSM's functions when it gave them, else with `GlobalAlloc` and
/// its kin (what Windows' DSM uses underneath), and measured with
/// `GlobalSize`, which knows either. Freed when dropped.
struct Block {
    handle: *mut c_void,
    memory: Option<Memory>,
}

impl Block {
    /// A block holding `bytes`, unlocked; `None` if it couldn't be
    /// allocated or written.
    fn allocate(memory: Option<Memory>, bytes: &[u8]) -> Option<Block> {
        let size = u32::try_from(bytes.len()).ok()?;
        let handle = match memory {
            // SAFETY: the DSM's allocator, as DAT_ENTRYPOINT gave it, in a DSM
            // never unloaded: it takes a size and returns a handle or null.
            Some(m) => unsafe { (m.allocate)(size) },
            // SAFETY: GlobalAlloc takes flags and a size and returns a handle
            // or null.
            None => unsafe { GlobalAlloc(GHND, bytes.len()) },
        };
        if handle.is_null() {
            return None;
        }
        let block = Block { handle, memory };
        let at = block.lock();
        if at.is_null() {
            return None;
        }
        // SAFETY: the block was just allocated `bytes.len()` bytes (at least)
        // and is locked, `at` its start; the two don't overlap.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), at.cast::<u8>(), bytes.len()) };
        block.unlock();
        Some(block)
    }

    fn lock(&self) -> *mut c_void {
        match self.memory {
            // SAFETY: `handle` is a live block of this allocator.
            Some(m) => unsafe { (m.lock)(self.handle) },
            // SAFETY: `handle` is a live global block.
            None => unsafe { GlobalLock(self.handle) },
        }
    }

    fn unlock(&self) {
        match self.memory {
            // SAFETY: `handle` is a live block of this allocator, locked.
            Some(m) => unsafe { (m.unlock)(self.handle) },
            // SAFETY: `handle` is a live global block, locked.
            None => unsafe {
                GlobalUnlock(self.handle);
            },
        }
    }

    /// Its bytes, as many as `GlobalSize` measures, copied out: empty if it
    /// can't be measured or locked, refused if there are more than `max`.
    fn read(&self, max: usize) -> Result<Vec<u8>, ContainerError> {
        // SAFETY: `handle` is a live block; both allocators' blocks are
        // GlobalAlloc's, which GlobalSize measures (0 if it can't).
        let size = unsafe { GlobalSize(self.handle) };
        if size > max {
            return Err(ContainerError::TooLarge {
                len: size as u64,
                max: max as u64,
            });
        }
        if size == 0 {
            return Ok(Vec::new());
        }
        let at = self.lock();
        if at.is_null() {
            return Ok(Vec::new());
        }
        // SAFETY: locked, the block's `size` bytes are readable from `at` until
        // it's unlocked below; they're copied out first.
        let bytes = unsafe { core::slice::from_raw_parts(at.cast::<u8>(), size) }.to_vec();
        self.unlock();
        Ok(bytes)
    }
}

impl Drop for Block {
    fn drop(&mut self) {
        match self.memory {
            // SAFETY: `handle` is a live, unlocked block of this allocator,
            // freed once, here.
            Some(m) => unsafe { (m.free)(self.handle) },
            // SAFETY: `handle` is a live, unlocked global block, freed once,
            // here.
            None => unsafe {
                GlobalFree(self.handle);
            },
        }
    }
}

/// A native transfer's image: the DIB the source made, in a global block the
/// application now owns, locked for the `Dib`'s life (so its bytes stay put
/// whether the block is fixed or moveable) and freed when it's dropped.
///
/// What it holds is TWAIN's: a packed DIB, its header, palette and rows,
/// unchecked; its length is the block's, which may run past the image.
pub struct Dib {
    handle: *mut c_void,
    at: *const u8,
    len: usize,
}

// SAFETY: a `Dib` owns its global block outright (the source handed it over),
// and global memory has no thread affinity: GlobalUnlock and GlobalFree may be
// called from any thread. Its bytes are only read, through `&self`.
unsafe impl Send for Dib {}
// SAFETY: `&Dib` gives only reads of the locked block, which nothing writes.
unsafe impl Sync for Dib {}

impl Dib {
    /// A DIB of no block: a transfer said done and gave none.
    pub(crate) fn empty() -> Dib {
        Dib {
            handle: null_mut(),
            at: null(),
            len: 0,
        }
    }

    /// The block `handle`, the application's, locked.
    fn take(handle: *mut c_void) -> Dib {
        // SAFETY: `handle` is the global block a native transfer handed over;
        // GlobalSize measures it (0 if it can't).
        let len = unsafe { GlobalSize(handle) };
        // SAFETY: as above; locked once here, unlocked once in Drop if it
        // locked.
        let at = unsafe { GlobalLock(handle) }.cast::<u8>().cast_const();
        Dib {
            handle,
            at,
            len: if at.is_null() { 0 } else { len },
        }
    }

    /// The block's bytes: empty if it couldn't be locked.
    pub fn bytes(&self) -> &[u8] {
        if self.at.is_null() {
            return &[];
        }
        // SAFETY: the block is locked for `self`'s life, `len` bytes from `at`
        // as GlobalSize measured it, and only read.
        unsafe { core::slice::from_raw_parts(self.at, self.len) }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        if self.handle.is_null() {
            return;
        }
        // SAFETY: the block is the application's, locked once in `take` if
        // `at` isn't null, unlocked and freed once, here.
        unsafe {
            if !self.at.is_null() {
                GlobalUnlock(self.handle);
            }
            GlobalFree(self.handle);
        }
    }
}

impl core::fmt::Debug for Dib {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dib").field("len", &self.len).finish()
    }
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

    /// The path of the file the handle has open, links and junctions
    /// resolved: what's loaded, so a path changed after the file was opened
    /// can't name another. Without the `\\?\` prefix a drive's or a share's
    /// path needs none of.
    fn final_path(&self) -> Option<PathBuf> {
        let mut buf = vec![0u16; 512];
        loop {
            // SAFETY: the handle is open; `buf` is writable for its length in
            // u16s, as the call is told.
            let n = unsafe {
                GetFinalPathNameByHandleW(
                    self.0,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    FILE_NAME_NORMALIZED,
                )
            } as usize;
            if n == 0 {
                return None;
            }
            if n < buf.len() {
                buf.truncate(n);
                break;
            }
            buf.resize(n + 1, 0);
        }
        let text = String::from_utf16(&buf).ok()?;
        let plain = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{rest}")
        } else if let Some(rest) = text.strip_prefix(r"\\?\") {
            rest.to_owned()
        } else {
            text
        };
        Some(PathBuf::from(plain))
    }
}

impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `File::open` and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn file_id(handle: HANDLE) -> Option<FileId> {
    let mut info = FILE_ID_INFO::default();
    // SAFETY: `handle` is open; `info` is writable for the size given, the
    // class's structure.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileIdInfo,
            (&raw mut info).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    (ok != 0).then_some(FileId {
        volume: info.VolumeSerialNumber,
        id: info.FileId.Identifier,
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
/// couldn't pin on one (`apps.cpp`, `AppGetConditionCode`, which checks the
/// `Id` first), and clears it. For `DAT_STATUS/MSG_GET` and nothing else:
/// other paths of the DSM index its table by the `Id` unchecked
/// (`AppSetConditionCode`).
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
        let rc = ReturnCode(unsafe {
            (self.entry)(
                origin.as_ptr(),
                dest,
                triplet.dg,
                triplet.dat,
                triplet.msg,
                data,
            )
        });
        #[cfg(test)]
        sent_log::push(triplet, rc);
        rc
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
    fn memory(&self) -> Option<Memory> {
        self.memory.get().copied().flatten()
    }

    /// `DG_CONTROL/DAT_IDENTITY/MSG_OPENDS` or `MSG_CLOSEDS`: the source whose
    /// identity is in `source` opened (the DSM fills in the rest) or closed.
    pub(crate) fn source(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
        open: bool,
    ) -> (Triplet, ReturnCode) {
        let msg = if open {
            ffi::MSG_OPENDS
        } else {
            ffi::MSG_CLOSEDS
        };
        let triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_IDENTITY, msg);
        // SAFETY: MSG_OPENDS and MSG_CLOSEDS, sent to the DSM, take the
        // source's TW_IDENTITY, which the DSM may write (its Id): a Cell the
        // caller holds for the call.
        (triplet, unsafe {
            self.call(app, None, triplet, source.as_ptr().cast())
        })
    }

    /// `DAT_CALLBACK2/MSG_REGISTER_CALLBACK` for the open source: `proc`, or
    /// none, which clears one registered before (the DSM keeps a registration
    /// for a source's id across closing and opening it again).
    pub(crate) fn register_callback(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
        proc: Option<TwainCallbackProc>,
    ) -> (Triplet, ReturnCode) {
        let triplet = Triplet::new(
            ffi::DG_CONTROL,
            ffi::DAT_CALLBACK2,
            ffi::MSG_REGISTER_CALLBACK,
        );
        let mut callback = TW_CALLBACK2 {
            CallBackProc: proc,
            RefCon: 0,
            Message: 0,
        };
        // SAFETY: MSG_REGISTER_CALLBACK takes a TW_CALLBACK2, which the DSM
        // copies; `callback` is on this frame for the call. The procedure, if
        // any, is `window::source_callback`, a function with
        // TWAINCALLBACKPROC's signature that lives as long as the program.
        (triplet, unsafe {
            self.call(app, Some(source), triplet, (&raw mut callback).cast())
        })
    }

    /// A `DAT_CAPABILITY` message whose answer is a container the source
    /// allocates (`MSG_GET`, `MSG_GETCURRENT`, `MSG_GETDEFAULT`, `MSG_RESET`,
    /// `MSG_QUERYSUPPORT`): its type and its bytes, copied out (refused past
    /// 16 MiB, which no container needs), the block freed. `None` unless the
    /// source succeeded. A source that failed and allocated a block anyway
    /// (TWAIN says it doesn't) leaves it unfreed: freeing what it may free
    /// itself would be worse.
    pub(crate) fn capability_read(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
        msg: u16,
        cap: u16,
    ) -> (Triplet, ReturnCode, ContainerAnswer) {
        let triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_CAPABILITY, msg);
        let mut capability = TW_CAPABILITY {
            Cap: cap,
            ConType: ffi::TWON_DONTCARE16,
            hContainer: null_mut(),
        };
        // SAFETY: DAT_CAPABILITY takes a TW_CAPABILITY, on this frame for the
        // call; for these messages the source writes a container's type and
        // the handle of a block it allocated into it.
        let rc = unsafe { self.call(app, Some(source), triplet, (&raw mut capability).cast()) };
        let handle = capability.hContainer;
        if rc != ReturnCode::SUCCESS || handle.is_null() {
            // A source that succeeded without a block answered nothing.
            let bytes = (rc == ReturnCode::SUCCESS).then(|| (capability.ConType, Ok(Vec::new())));
            return (triplet, rc, bytes);
        }
        let block = Block {
            handle,
            memory: self.memory(),
        };
        (
            triplet,
            rc,
            Some((capability.ConType, block.read(MAX_CONTAINER))),
        )
    }

    /// `DAT_USERINTERFACE`: `MSG_ENABLEDS`, `MSG_ENABLEDSUIONLY` or
    /// `MSG_DISABLEDS`, with `ui`, the one structure for an enable and its
    /// disable, as TWAIN asks.
    pub(crate) fn user_interface(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
        msg: u16,
        ui: &Cell<ffi::TW_USERINTERFACE>,
    ) -> (Triplet, ReturnCode) {
        let triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_USERINTERFACE, msg);
        // SAFETY: DAT_USERINTERFACE takes a TW_USERINTERFACE: `ui`, a Cell the
        // caller keeps at one address for the enable's length, which the source
        // may write (ModalUI) through the pointer; hParent is the hidden window,
        // which outlives the source's use of it.
        (triplet, unsafe {
            self.call(app, Some(source), triplet, ui.as_ptr().cast())
        })
    }

    /// `DAT_PENDINGXFERS`: `MSG_ENDXFER`, `MSG_RESET` or `MSG_STOPFEEDER`, and
    /// the count it answers.
    pub(crate) fn pending_transfers(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
        msg: u16,
    ) -> (Triplet, ReturnCode, u16) {
        let triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_PENDINGXFERS, msg);
        let mut pending = ffi::TW_PENDINGXFERS::default();
        // SAFETY: DAT_PENDINGXFERS writes a TW_PENDINGXFERS into `pending`, on
        // this frame for the call.
        let rc = unsafe { self.call(app, Some(source), triplet, (&raw mut pending).cast()) };
        (triplet, rc, pending.Count)
    }

    /// `DG_IMAGE/DAT_IMAGEINFO/MSG_GET`.
    pub(crate) fn image_info(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
    ) -> (Triplet, ReturnCode, ffi::TW_IMAGEINFO) {
        let triplet = Triplet::new(ffi::DG_IMAGE, ffi::DAT_IMAGEINFO, ffi::MSG_GET);
        let mut info = ffi::TW_IMAGEINFO::default();
        // SAFETY: DAT_IMAGEINFO writes a TW_IMAGEINFO into `info`, on this
        // frame for the call.
        let rc = unsafe { self.call(app, Some(source), triplet, (&raw mut info).cast()) };
        (triplet, rc, info)
    }

    /// `DG_IMAGE/DAT_IMAGENATIVEXFER/MSG_GET`: on `TWRC_XFERDONE`, the DIB the
    /// source made, now the application's.
    pub(crate) fn native(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
    ) -> (Triplet, ReturnCode, Option<Dib>) {
        let triplet = Triplet::new(ffi::DG_IMAGE, ffi::DAT_IMAGENATIVEXFER, ffi::MSG_GET);
        let mut handle: *mut c_void = null_mut();
        // SAFETY: DAT_IMAGENATIVEXFER takes a pointer to a TW_HANDLE, `handle`,
        // on this frame for the call, into which the source writes the DIB's.
        let rc = unsafe { self.call(app, Some(source), triplet, (&raw mut handle).cast()) };
        let dib = (rc == ReturnCode::XFERDONE && !handle.is_null()).then(|| Dib::take(handle));
        (triplet, rc, dib)
    }

    /// `DG_CONTROL/DAT_SETUPMEMXFER/MSG_GET`.
    pub(crate) fn setup_memory(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
    ) -> (Triplet, ReturnCode, ffi::TW_SETUPMEMXFER) {
        let triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_SETUPMEMXFER, ffi::MSG_GET);
        let mut setup = ffi::TW_SETUPMEMXFER::default();
        // SAFETY: DAT_SETUPMEMXFER writes a TW_SETUPMEMXFER into `setup`, on
        // this frame for the call.
        let rc = unsafe { self.call(app, Some(source), triplet, (&raw mut setup).cast()) };
        (triplet, rc, setup)
    }

    /// `DG_IMAGE/DAT_IMAGEMEMXFER/MSG_GET` into `buffer`, which the source
    /// writes at most `buffer.len()` bytes of (or `u32::MAX`, if it's
    /// longer); what it says of them. `BytesWritten` is the source's word,
    /// to be checked against the buffer.
    pub(crate) fn memory_buffer(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
        buffer: &mut [u8],
    ) -> (Triplet, ReturnCode, ffi::TW_IMAGEMEMXFER) {
        let triplet = Triplet::new(ffi::DG_IMAGE, ffi::DAT_IMAGEMEMXFER, ffi::MSG_GET);
        let length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        let mut transfer = ffi::TW_IMAGEMEMXFER {
            Compression: ffi::TWON_DONTCARE16,
            BytesPerRow: ffi::TWON_DONTCARE32,
            Columns: ffi::TWON_DONTCARE32,
            Rows: ffi::TWON_DONTCARE32,
            XOffset: ffi::TWON_DONTCARE32,
            YOffset: ffi::TWON_DONTCARE32,
            BytesWritten: ffi::TWON_DONTCARE32,
            Memory: ffi::TW_MEMORY {
                Flags: ffi::TWMF_APPOWNS | ffi::TWMF_POINTER,
                Length: length,
                TheMem: buffer.as_mut_ptr().cast(),
            },
        };
        // SAFETY: DAT_IMAGEMEMXFER takes a TW_IMAGEMEMXFER, on this frame for
        // the call, whose Memory is `buffer`, borrowed mutably for the call, by
        // pointer (TWMF_POINTER), the application's (TWMF_APPOWNS), `length`
        // bytes of it, which is all the source may write.
        let rc = unsafe { self.call(app, Some(source), triplet, (&raw mut transfer).cast()) };
        (triplet, rc, transfer)
    }

    /// `DG_CONTROL/DAT_EVENT/MSG_PROCESSEVENT`: `msg` offered to the source.
    /// Whether it was the source's (`TWRC_DSEVENT`), and the message it
    /// carried for the application.
    pub(crate) fn process_event(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
        msg: &mut Message,
    ) -> (ReturnCode, u16) {
        let triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_EVENT, ffi::MSG_PROCESSEVENT);
        let mut event = ffi::TW_EVENT {
            pEvent: msg.as_mut_ptr(),
            TWMessage: ffi::MSG_NULL,
        };
        // SAFETY: DAT_EVENT takes a TW_EVENT, on this frame for the call,
        // whose pEvent is the MSG `msg` holds, borrowed mutably for the call.
        let rc = unsafe { self.call(app, Some(source), triplet, (&raw mut event).cast()) };
        (rc, event.TWMessage)
    }

    /// `DG_CONTROL/DAT_DEVICEEVENT/MSG_GET`.
    pub(crate) fn device_event(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
    ) -> (Triplet, ReturnCode, ffi::TW_DEVICEEVENT) {
        let triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_DEVICEEVENT, ffi::MSG_GET);
        let mut event = ffi::TW_DEVICEEVENT::default();
        // SAFETY: DAT_DEVICEEVENT writes a TW_DEVICEEVENT into `event`, on this
        // frame for the call.
        let rc = unsafe { self.call(app, Some(source), triplet, (&raw mut event).cast()) };
        (triplet, rc, event)
    }

    /// `DG_IMAGE/DAT_EXTIMAGEINFO/MSG_GET` for `ids`: the source's answer,
    /// each entry with its data read out, inline or from the block its handle
    /// names (freed after, as the application must; a handle named twice, a
    /// source's mistake, read and freed once, since freeing it twice would
    /// corrupt the heap).
    pub(crate) fn ext_image_info(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
        ids: &[u16],
    ) -> Result<(Triplet, ReturnCode, Vec<ext_info::Answer>), ContainerError> {
        let triplet = Triplet::new(ffi::DG_IMAGE, ffi::DAT_EXTIMAGEINFO, ffi::MSG_GET);
        let request = ext_info::request(ids)?;
        // In words, so the block is aligned as TW_EXTIMAGEINFO is.
        let mut words = vec![0u64; request.len().div_ceil(8)];
        // SAFETY: `words` holds at least `request.len()` bytes; the two don't
        // overlap.
        unsafe {
            core::ptr::copy_nonoverlapping(
                request.as_ptr(),
                words.as_mut_ptr().cast::<u8>(),
                request.len(),
            )
        };
        // SAFETY: DAT_EXTIMAGEINFO takes a TW_EXTIMAGEINFO with room for
        // NumInfos entries, as `words` has (`request` laid them out), on this
        // frame for the call; the source fills them in.
        let rc = unsafe { self.call(app, Some(source), triplet, words.as_mut_ptr().cast()) };
        // SAFETY: `words` is `request.len()` bytes or more, all initialized.
        let answer =
            unsafe { core::slice::from_raw_parts(words.as_ptr().cast::<u8>(), request.len()) };
        if rc != ReturnCode::SUCCESS {
            return Ok((triplet, rc, Vec::new()));
        }
        let entries = ext_info::parse(answer, ids.len())?;
        let mut freed = std::collections::HashSet::new();
        let answers = entries
            .into_iter()
            .map(|entry| {
                let data = match entry.data() {
                    ext_info::Data::None => ext_info::Read::None,
                    ext_info::Data::Missing => ext_info::Read::Missing,
                    ext_info::Data::Inline(bytes) => ext_info::Read::Bytes(bytes),
                    ext_info::Data::Handle(handle) if !freed.insert(handle) => {
                        ext_info::Read::Repeated
                    }
                    ext_info::Data::Handle(handle) => {
                        let block = Block {
                            handle: handle as *mut c_void,
                            memory: self.memory(),
                        };
                        match block.read(MAX_CONTAINER) {
                            Ok(bytes) => ext_info::Read::Bytes(bytes),
                            Err(e) => ext_info::Read::Refused(e),
                        }
                    }
                    ext_info::Data::Unknown => ext_info::Read::Unknown,
                };
                ext_info::Answer { entry, data }
            })
            .collect();
        Ok((triplet, rc, answers))
    }

    /// `MSG_SET` (or another message that takes a container) with `bytes`,
    /// a container of `con_type`, in a block allocated for the call and freed
    /// after it. `None` if the block couldn't be allocated.
    pub(crate) fn capability_write(
        &self,
        app: &Cell<TW_IDENTITY>,
        source: &Cell<TW_IDENTITY>,
        msg: u16,
        cap: u16,
        con_type: u16,
        bytes: &[u8],
    ) -> Option<(Triplet, ReturnCode)> {
        let triplet = Triplet::new(ffi::DG_CONTROL, ffi::DAT_CAPABILITY, msg);
        let block = Block::allocate(self.memory(), bytes)?;
        let mut capability = TW_CAPABILITY {
            Cap: cap,
            ConType: con_type,
            hContainer: block.handle,
        };
        // SAFETY: DAT_CAPABILITY takes a TW_CAPABILITY, on this frame for the
        // call, its container the block allocated above, unlocked, which the
        // application owns and frees (`block`, after the call).
        let rc = unsafe { self.call(app, Some(source), triplet, (&raw mut capability).cast()) };
        drop(block);
        Some((triplet, rc))
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

/// Every triplet the crate sends, with its answer, for the crate's own tests
/// to check what a step sent.
#[cfg(test)]
pub(crate) mod sent_log {
    use std::sync::Mutex;

    use crate::codes::{ReturnCode, Triplet};

    static SENT: Mutex<Vec<(Triplet, ReturnCode)>> = Mutex::new(Vec::new());

    pub(crate) fn push(triplet: Triplet, rc: ReturnCode) {
        SENT.lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((triplet, rc));
    }

    /// What was sent since the last call.
    pub(crate) fn take() -> Vec<(Triplet, ReturnCode)> {
        core::mem::take(&mut *SENT.lock().unwrap_or_else(|e| e.into_inner()))
    }
}
