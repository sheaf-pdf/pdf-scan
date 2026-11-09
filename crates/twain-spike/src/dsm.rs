//! The Data Source Manager: `TWAINDSM.dll` loaded from the system folder, its
//! entry point, and the memory functions containers are allocated with.

use core::ffi::c_void;
use core::ptr::null_mut;

use windows_sys::Win32::Foundation::{FreeLibrary, GetLastError, GlobalFree, HMODULE};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::Win32::System::Memory::{GHND, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};

use crate::ffi::{
    DsmEntryProc, DsmMemAllocate, DsmMemFree, DsmMemLock, DsmMemUnlock, TW_ENTRYPOINT, TW_IDENTITY,
};

/// The loaded DSM. Dropping it unloads the library.
pub struct Dsm {
    module: HMODULE,
    entry: DsmEntryProc,
    allocator: Allocator,
}

#[derive(Clone, Copy)]
struct MemoryFunctions {
    allocate: DsmMemAllocate,
    free: DsmMemFree,
    lock: DsmMemLock,
    unlock: DsmMemUnlock,
}

/// How containers are allocated: through the DSM's memory functions once
/// `DAT_ENTRYPOINT` has given them, else `GlobalAlloc` and its kin. Windows'
/// DSM allocates with `GlobalAlloc` underneath, so `GlobalSize` measures
/// either kind of block. `Copy`, so a session can hold it across a call to the
/// DSM; and so every method is `unsafe`: a copy outlives nothing, and the DSM's
/// functions are only there while the [`Dsm`] that gave them is loaded.
#[derive(Clone, Copy)]
pub struct Allocator(Option<MemoryFunctions>);

impl Allocator {
    /// `GlobalAlloc` and its kin, which is also what a native transfer's DIB
    /// is on Windows: the application frees it with `GlobalFree`.
    pub const GLOBAL: Allocator = Allocator(None);

    /// Whether these are the DSM's own memory functions.
    pub fn is_dsms(self) -> bool {
        self.0.is_some()
    }

    /// Allocates `size` bytes. Null if it couldn't.
    ///
    /// # Safety
    ///
    /// The [`Dsm`] this came from must still be loaded (always so for
    /// [`Allocator::GLOBAL`]).
    pub unsafe fn alloc(self, size: u32) -> *mut c_void {
        match self.0 {
            // SAFETY: the DSM's allocator, as `DAT_ENTRYPOINT` gave it, in a
            // DSM the caller keeps loaded; it takes a size and returns a handle
            // or null.
            Some(m) => unsafe { (m.allocate)(size) },
            // SAFETY: GlobalAlloc takes flags and a size and returns a handle or
            // null.
            None => unsafe { GlobalAlloc(GHND, size as usize) },
        }
    }

    /// Locks a handle, giving a pointer to its memory, or null.
    ///
    /// # Safety
    ///
    /// `handle` must be a live handle from this allocator, and its [`Dsm`]
    /// still loaded.
    pub unsafe fn lock(self, handle: *mut c_void) -> *mut c_void {
        match self.0 {
            // SAFETY: the caller passes a live handle.
            Some(m) => unsafe { (m.lock)(handle) },
            // SAFETY: the caller passes a live handle.
            None => unsafe { GlobalLock(handle) },
        }
    }

    /// Unlocks a handle locked with [`Allocator::lock`].
    ///
    /// # Safety
    ///
    /// `handle` must be a live, locked handle from this allocator, and its
    /// [`Dsm`] still loaded.
    pub unsafe fn unlock(self, handle: *mut c_void) {
        match self.0 {
            // SAFETY: the caller passes a live, locked handle.
            Some(m) => unsafe { (m.unlock)(handle) },
            // SAFETY: the caller passes a live, locked handle.
            None => unsafe {
                GlobalUnlock(handle);
            },
        }
    }

    /// Frees a handle.
    ///
    /// # Safety
    ///
    /// `handle` must be a live, unlocked handle from this allocator, never used
    /// again, and its [`Dsm`] still loaded.
    pub unsafe fn free(self, handle: *mut c_void) {
        match self.0 {
            // SAFETY: the caller passes a live handle and drops it.
            Some(m) => unsafe { (m.free)(handle) },
            // SAFETY: the caller passes a live handle and drops it.
            None => unsafe {
                GlobalFree(handle);
            },
        }
    }

    /// A block's size in bytes, 0 if it can't be told.
    ///
    /// # Safety
    ///
    /// `handle` must be a live handle from this allocator.
    pub unsafe fn size(self, handle: *mut c_void) -> usize {
        // SAFETY: the caller passes a live handle; both kinds of block are
        // GlobalAlloc's.
        unsafe { GlobalSize(handle) }
    }
}

/// What the loaded module is.
pub struct ModuleInfo {
    /// Its path as the loader gives it: a 32-bit process sees the SysWOW64
    /// copy under the System32 name.
    pub path: String,
    /// Its file version, from its version resource.
    pub version: Option<String>,
    /// The `Machine` field of its PE header: 0x8664 x64, 0x14C x86.
    pub machine: Option<u16>,
}

impl Dsm {
    /// Loads `TWAINDSM.dll` from the system folder only, never the
    /// application's folder or the current directory.
    pub fn load() -> Result<Dsm, String> {
        let name = wide("TWAINDSM.dll");
        // SAFETY: `name` is NUL-terminated UTF-16 that outlives the call; no
        // file handle is passed; the flag limits the search to System32
        // (SysWOW64 in a 32-bit process).
        let module =
            unsafe { LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if module.is_null() {
            // SAFETY: no other call intervenes since LoadLibraryExW failed.
            let error = unsafe { GetLastError() };
            return Err(format!(
                "LoadLibraryExW(TWAINDSM.dll, system folder): error {error}"
            ));
        }
        // SAFETY: `module` is a loaded module; the name is a NUL-terminated
        // ASCII string.
        let proc = unsafe { GetProcAddress(module, c"DSM_Entry".as_ptr().cast()) };
        let Some(proc) = proc else {
            // SAFETY: `module` was loaded above and nothing else holds it.
            unsafe { FreeLibrary(module) };
            return Err("TWAINDSM.dll exports no DSM_Entry".to_owned());
        };
        // SAFETY: `DSM_Entry` is declared in twain.h as `DSMENTRYPROC`, the
        // signature `DsmEntryProc` gives it (`TW_CALLINGSTYLE` being `PASCAL`,
        // `extern "system"`); both are function pointers of one size.
        let entry = unsafe {
            core::mem::transmute::<unsafe extern "system" fn() -> isize, DsmEntryProc>(proc)
        };
        Ok(Dsm {
            module,
            entry,
            allocator: Allocator::GLOBAL,
        })
    }

    /// Calls `DSM_Entry`.
    ///
    /// # Safety
    ///
    /// `origin` must point to the application's identity, `dest` be null or
    /// point to an opened source's identity, and `data` be what the triplet
    /// (`dg`, `dat`, `msg`) takes, valid for the DSM and the source to read and
    /// write for the call.
    pub unsafe fn entry(
        &self,
        origin: *mut TW_IDENTITY,
        dest: *mut TW_IDENTITY,
        dg: u32,
        dat: u16,
        msg: u16,
        data: *mut c_void,
    ) -> u16 {
        // SAFETY: the caller keeps the contract above; `entry` is the loaded
        // DSM's `DSM_Entry`, valid while `self` holds the module.
        unsafe { (self.entry)(origin, dest, dg, dat, msg, data) }
    }

    /// Takes the memory functions from `DAT_ENTRYPOINT`'s answer, if it gives
    /// all four; otherwise containers stay with `GlobalAlloc`. Returns whether
    /// the DSM's are used.
    ///
    /// # Safety
    ///
    /// `ep` must be this DSM's own answer to `DAT_ENTRYPOINT`: its pointers are
    /// called as the functions `twain.h` declares, for as long as `self` is
    /// loaded.
    pub unsafe fn take_entry_points(&mut self, ep: &TW_ENTRYPOINT) -> bool {
        let (allocate, free, lock, unlock) = (
            ep.DSM_MemAllocate,
            ep.DSM_MemFree,
            ep.DSM_MemLock,
            ep.DSM_MemUnlock,
        );
        self.allocator = match (allocate, free, lock, unlock) {
            (Some(allocate), Some(free), Some(lock), Some(unlock)) => {
                Allocator(Some(MemoryFunctions {
                    allocate,
                    free,
                    lock,
                    unlock,
                }))
            }
            _ => Allocator::GLOBAL,
        };
        self.allocator.is_dsms()
    }

    /// How containers are allocated.
    pub fn allocator(&self) -> Allocator {
        self.allocator
    }

    /// What the loaded module is: its path, version and machine.
    pub fn info(&self) -> ModuleInfo {
        let path = module_path(self.module);
        let version = file_version(&path);
        ModuleInfo {
            path,
            version,
            machine: pe_machine(self.module),
        }
    }
}

impl Drop for Dsm {
    fn drop(&mut self) {
        // SAFETY: `module` was loaded by `Dsm::load` and is released once, here;
        // the entry point and memory functions die with `self`.
        unsafe { FreeLibrary(self.module) };
    }
}

/// NUL-terminated UTF-16.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn module_path(module: HMODULE) -> String {
    let mut buf = vec![0u16; 1024];
    // SAFETY: `module` is loaded; `buf` is writable for its length in u16s.
    let n = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

fn file_version(path: &str) -> Option<String> {
    let name = wide(path);
    let mut ignored = 0u32;
    // SAFETY: `name` is NUL-terminated UTF-16; `ignored` is writable.
    let size = unsafe { GetFileVersionInfoSizeW(name.as_ptr(), &mut ignored) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0u8; size as usize];
    // SAFETY: `block` is writable for `size` bytes, as the call is told.
    let ok = unsafe { GetFileVersionInfoW(name.as_ptr(), 0, size, block.as_mut_ptr().cast()) };
    if ok == 0 {
        return None;
    }
    let root = wide("\\");
    let mut info: *mut c_void = null_mut();
    let mut len = 0u32;
    // SAFETY: `block` holds the version resource just read; `info` and `len`
    // are writable; the answer points into `block`.
    let ok = unsafe { VerQueryValueW(block.as_ptr().cast(), root.as_ptr(), &mut info, &mut len) };
    if ok == 0 || info.is_null() || (len as usize) < size_of::<VS_FIXEDFILEINFO>() {
        return None;
    }
    // SAFETY: `info` points into `block`, at a VS_FIXEDFILEINFO of `len` bytes
    // (checked above), which may not be aligned.
    let fixed = unsafe { info.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
    let (ms, ls) = (fixed.dwFileVersionMS, fixed.dwFileVersionLS);
    Some(format!(
        "{}.{}.{}.{}",
        ms >> 16,
        ms & 0xffff,
        ls >> 16,
        ls & 0xffff
    ))
}

/// The PE header's `Machine`, read from the mapped image.
fn pe_machine(module: HMODULE) -> Option<u16> {
    let base = module.cast::<u8>().cast_const();
    // SAFETY: a module loaded as an image (not as data) starts at its DOS
    // header, 64 readable bytes.
    let (mz, e_lfanew) = unsafe {
        (
            base.cast::<u16>().read_unaligned(),
            base.add(0x3c).cast::<i32>().read_unaligned(),
        )
    };
    if mz != 0x5a4d || !(0x40..0x400).contains(&e_lfanew) {
        return None;
    }
    let nt = e_lfanew as usize;
    // SAFETY: the NT headers start at `e_lfanew`, within the headers' first
    // page (checked above), mapped and readable.
    let (signature, machine) = unsafe {
        (
            base.add(nt).cast::<u32>().read_unaligned(),
            base.add(nt + 4).cast::<u16>().read_unaligned(),
        )
    };
    (signature == 0x0000_4550).then_some(machine)
}
