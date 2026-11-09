//! A TWAIN session on the TWAIN thread: the state it's in, the identities, and
//! the triplets the scenarios call, each answer checked against what's
//! expected and reported.

use core::ffi::c_void;
use core::ptr::null_mut;

use windows_sys::Win32::Foundation::HWND;

use crate::dsm::Dsm;
use crate::ffi::*;
use crate::twain_thread::Out;

/// Who a triplet went to, so its condition code is asked of the same party.
#[derive(Clone, Copy)]
pub enum Party {
    Dsm,
}

pub struct Session {
    out: Out,
    hwnd: HWND,
    state: u8,
    /// The application's identity, at an address that never moves: the DSM
    /// writes its `Id` at `MSG_OPENDSM`.
    app: Box<TW_IDENTITY>,
    dsm: Option<Dsm>,
}

impl Session {
    pub fn new(out: Out, hwnd: HWND) -> Session {
        let app = TW_IDENTITY {
            Id: 0,
            Version: TW_VERSION {
                MajorNum: 0,
                MinorNum: 0,
                Language: TWLG_USA,
                Country: TWCY_USA,
                Info: TW_STR32::new("twain-spike"),
            },
            ProtocolMajor: TWON_PROTOCOLMAJOR,
            ProtocolMinor: TWON_PROTOCOLMINOR,
            SupportedGroups: DF_APP2 | DG_CONTROL | DG_IMAGE,
            Manufacturer: TW_STR32::new("pdf-scan"),
            ProductFamily: TW_STR32::new("twain-spike"),
            ProductName: TW_STR32::new("twain-spike"),
        };
        Session {
            out,
            hwnd,
            state: 1,
            app: Box::new(app),
            dsm: None,
        }
    }

    pub fn out(&self) -> &Out {
        &self.out
    }

    /// The TWAIN state, 1 to 7.
    pub fn state(&self) -> u8 {
        self.state
    }

    /// Reports a failure unless `ok`; returns `ok`.
    pub fn check(&self, ok: bool, what: impl Into<String>) -> bool {
        if !ok {
            self.out.fail(what);
        }
        ok
    }

    /// Records a move to state `to`: one step at a time, but for `MSG_ENDXFER`
    /// with nothing pending, which goes from 7 to 5.
    fn moved(&mut self, to: u8) {
        let from = self.state;
        let legal = from.abs_diff(to) == 1 || (from == 7 && to == 5);
        self.check(legal, format!("state {from} -> {to} skips a state"));
        self.out.line(format!("state {from} -> {to}"));
        self.state = to;
    }

    /// State 1 to 2: the DSM loaded, and what it is.
    pub fn load_dsm(&mut self) -> bool {
        match Dsm::load() {
            Ok(dsm) => {
                let info = dsm.info();
                let version = info.version.as_deref().unwrap_or("unknown");
                let machine = match info.machine {
                    Some(0x8664) => "x64 (0x8664)".to_owned(),
                    Some(0x014c) => "x86 (0x14C)".to_owned(),
                    Some(other) => format!("{other:#06x}"),
                    None => "unreadable".to_owned(),
                };
                self.out.line(format!(
                    "DSM loaded: {}, version {version}, machine {machine}",
                    info.path
                ));
                let expected = if cfg!(target_pointer_width = "64") {
                    0x8664
                } else {
                    0x014c
                };
                self.check(
                    info.machine == Some(expected),
                    "the DSM's machine isn't this process's",
                );
                self.dsm = Some(dsm);
                self.moved(2);
                true
            }
            Err(e) => {
                self.out.fail(format!("loading the DSM: {e}"));
                false
            }
        }
    }

    /// State 2 to 1.
    pub fn unload_dsm(&mut self) {
        if self.dsm.take().is_some() {
            self.moved(1);
        }
    }

    /// State 2 to 3, with the window as the parent; then the DSM's entry
    /// points.
    pub fn open_dsm(&mut self) -> bool {
        let mut parent: HWND = self.hwnd;
        // SAFETY: MSG_OPENDSM takes a pointer to the parent window's handle;
        // `parent` lives on this frame for the call.
        let rc = unsafe {
            self.dsm_triplet(
                DG_CONTROL,
                DAT_PARENT,
                MSG_OPENDSM,
                (&raw mut parent).cast(),
            )
        };
        if !self.expect("DAT_PARENT/MSG_OPENDSM", rc, TWRC_SUCCESS, Party::Dsm) {
            return false;
        }
        self.moved(3);
        let id = self.app.Id;
        self.out
            .line(format!("the DSM gave this application id {id}"));

        let mut ep = TW_ENTRYPOINT {
            Size: size_of::<TW_ENTRYPOINT>() as u32,
            ..TW_ENTRYPOINT::default()
        };
        // SAFETY: DAT_ENTRYPOINT takes a TW_ENTRYPOINT, its Size set to its
        // own size, as `ep` is; it lives on this frame for the call.
        let rc =
            unsafe { self.dsm_triplet(DG_CONTROL, DAT_ENTRYPOINT, MSG_GET, (&raw mut ep).cast()) };
        if self.expect("DAT_ENTRYPOINT/MSG_GET", rc, TWRC_SUCCESS, Party::Dsm) {
            // Each field copied out first: a packed field can't be borrowed.
            let present = [
                { ep.DSM_Entry }.is_some(),
                { ep.DSM_MemAllocate }.is_some(),
                { ep.DSM_MemFree }.is_some(),
                { ep.DSM_MemLock }.is_some(),
                { ep.DSM_MemUnlock }.is_some(),
            ];
            self.check(
                present.iter().all(|&p| p),
                format!("an entry point is null: {present:?}"),
            );
            let dsm = self.dsm.as_mut().expect("the DSM is loaded in state 3");
            let own = dsm.take_entry_points(&ep);
            self.out.line(if own {
                "containers: the DSM's memory functions"
            } else {
                "containers: GlobalAlloc, the DSM giving no memory functions"
            });
        }
        true
    }

    /// State 3 to 2.
    pub fn close_dsm(&mut self) -> bool {
        let mut parent: HWND = self.hwnd;
        // SAFETY: MSG_CLOSEDSM takes a pointer to the parent window's handle,
        // the one opened with; `parent` lives on this frame for the call.
        let rc = unsafe {
            self.dsm_triplet(
                DG_CONTROL,
                DAT_PARENT,
                MSG_CLOSEDSM,
                (&raw mut parent).cast(),
            )
        };
        if !self.expect("DAT_PARENT/MSG_CLOSEDSM", rc, TWRC_SUCCESS, Party::Dsm) {
            return false;
        }
        self.moved(2);
        true
    }

    /// The memory functions driven once: a block allocated, locked, written,
    /// read back, unlocked and freed.
    pub fn memory_round_trip(&mut self) {
        let Some(dsm) = self.dsm.as_ref() else { return };
        const LEN: usize = 64;
        let handle = dsm.alloc(LEN as u32);
        if !self.check(!handle.is_null(), "allocating 64 bytes failed") {
            return;
        }
        // SAFETY: `handle` was just allocated by this allocator.
        let p = unsafe { dsm.lock(handle) }.cast::<u8>();
        let ok = if p.is_null() {
            false
        } else {
            let pattern: Vec<u8> = (0..LEN as u8).map(|i| i.wrapping_mul(37)).collect();
            // SAFETY: `p` points to the locked block of at least LEN bytes,
            // which nothing else touches; byte copies need no alignment.
            let back = unsafe {
                core::ptr::copy_nonoverlapping(pattern.as_ptr(), p, LEN);
                core::slice::from_raw_parts(p, LEN).to_vec()
            };
            // SAFETY: the block was locked above and is unlocked once.
            unsafe { dsm.unlock(handle) };
            back == pattern
        };
        // SAFETY: the block is live and unlocked, and isn't used again.
        unsafe { dsm.free(handle) };
        if self.check(ok, "a locked block didn't read back what was written") {
            self.out
                .line("memory: 64 bytes allocated, locked, written, read back, unlocked, freed");
        }
    }

    /// Every source the DSM lists (state 3).
    pub fn list_sources(&mut self) -> Vec<TW_IDENTITY> {
        let mut sources = Vec::new();
        let mut msg = MSG_GETFIRST;
        loop {
            let mut id = TW_IDENTITY::default();
            // SAFETY: MSG_GETFIRST and MSG_GETNEXT write a TW_IDENTITY into
            // `id`, which lives on this frame for the call.
            let rc =
                unsafe { self.dsm_triplet(DG_CONTROL, DAT_IDENTITY, msg, (&raw mut id).cast()) };
            let name = if msg == MSG_GETFIRST {
                "DAT_IDENTITY/MSG_GETFIRST"
            } else {
                "DAT_IDENTITY/MSG_GETNEXT"
            };
            if rc == TWRC_ENDOFLIST {
                self.out.line(format!("{name}: TWRC_ENDOFLIST"));
                break;
            }
            if !self.expect(name, rc, TWRC_SUCCESS, Party::Dsm) {
                break;
            }
            sources.push(id);
            msg = MSG_GETNEXT;
            if sources.len() > 256 {
                self.out.fail("more than 256 sources listed: stopped");
                break;
            }
        }
        sources
    }

    /// Calls a triplet with no destination (the DSM's own).
    ///
    /// # Safety
    ///
    /// `data` must point to the struct the triplet takes, live and writable
    /// for the call.
    unsafe fn dsm_triplet(&mut self, dg: u32, dat: u16, msg: u16, data: *mut c_void) -> u16 {
        let Some(dsm) = self.dsm.as_ref() else {
            self.out.fail("a DSM triplet with no DSM loaded");
            return TWRC_FAILURE;
        };
        let origin: *mut TW_IDENTITY = &raw mut *self.app;
        // SAFETY: `origin` is the application's identity, boxed so it never
        // moves; there's no destination; the caller keeps `data`'s contract.
        unsafe { dsm.entry(origin, null_mut(), dg, dat, msg, data) }
    }

    /// The condition code for the last triplet sent to `party`.
    fn condition(&mut self, party: Party) -> Option<u16> {
        match party {
            Party::Dsm => {
                let mut status = TW_STATUS::default();
                // SAFETY: DAT_STATUS writes a TW_STATUS into `status`, which
                // lives on this frame for the call.
                let rc = unsafe {
                    self.dsm_triplet(DG_CONTROL, DAT_STATUS, MSG_GET, (&raw mut status).cast())
                };
                (rc == TWRC_SUCCESS).then_some(status.ConditionCode)
            }
        }
    }

    /// Reports a triplet's answer, failing if it isn't `want`. On a failure
    /// the condition code is asked for and reported.
    fn expect(&mut self, what: &str, rc: u16, want: u16, party: Party) -> bool {
        if rc == want {
            self.out.line(format!("{what}: {}", rc_name(rc)));
            return true;
        }
        let cc = match self.condition(party) {
            Some(cc) => cc_name(cc),
            None => "condition code unavailable".to_owned(),
        };
        self.out.fail(format!(
            "{what}: {} ({cc}), expected {}",
            rc_name(rc),
            rc_name(want)
        ));
        false
    }
}

impl Drop for Session {
    /// Walks down whatever a failed scenario left up.
    fn drop(&mut self) {
        if self.state >= 3 {
            self.close_dsm();
        }
        self.unload_dsm();
    }
}

/// An identity as a line.
pub fn describe(id: &TW_IDENTITY) -> String {
    let (major, minor) = (id.Version.MajorNum, id.Version.MinorNum);
    let (pmaj, pmin) = (id.ProtocolMajor, id.ProtocolMinor);
    let groups = id.SupportedGroups;
    format!(
        "{} / {} / {}, version {major}.{minor} \"{}\", protocol {pmaj}.{pmin}, groups {groups:#010x}",
        id.Manufacturer.text(),
        id.ProductFamily.text(),
        id.ProductName.text(),
        id.Version.Info.text(),
    )
}
