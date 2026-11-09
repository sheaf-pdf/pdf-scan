//! A TWAIN session on the TWAIN thread: the state it's in, the identities, and
//! the triplets the scenarios call, each answer checked against what's
//! expected and reported.

use core::ffi::c_void;
use core::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, WM_TIMER};

use crate::container::{self, Current};
use crate::dsm::{Allocator, Dsm};
use crate::ffi::*;
use crate::image::{self, Tile};
use crate::twain_thread::Out;
use crate::window::{self, SPIKE_MESSAGES, WM_SOURCE};

/// Who a triplet goes to, so its condition code is asked of the same party.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Party {
    Dsm,
    Source,
}

/// How the source's messages reach the application.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Every window message, while the source is enabled, forwarded through
    /// `MSG_PROCESSEVENT`; no callback.
    Event,
    /// A callback registered with `DAT_CALLBACK2`; window messages dispatched
    /// but not forwarded.
    Callback,
}

/// How long to wait for a message from the source before giving up, under
/// the main thread's 30-second watchdog.
const SOURCE_WAIT_MS: u32 = 20_000;
const SOURCE_TIMER: usize = 1;

/// The largest native transfer block taken: the image cap and a header.
const MAX_DIB_BYTES: usize = image::MAX_BYTES as usize + (1 << 16);
/// The largest memory transfer buffer the spike allocates.
const MAX_BUFFER: u32 = 64 << 20;
/// The most buffers one memory transfer may take.
const MAX_TILES: usize = 1 << 20;

// What the callback knows: the window it posts to, whether `MSG_ENABLEDS` is
// on the stack, and the thread it last ran on. Atomics, so the callback does
// nothing that can panic.
static CALLBACK_WINDOW: AtomicUsize = AtomicUsize::new(0);
static ENABLING: AtomicBool = AtomicBool::new(false);
static CALLBACK_THREAD: AtomicU32 = AtomicU32::new(0);

/// The application's callback: records what it was told and posts it to the
/// window, never calling the DSM; returns `TWRC_SUCCESS`, since the DSM hands
/// the return to the source, which fails the enable on anything else.
unsafe extern "system" fn callback(
    _origin: *mut TW_IDENTITY,
    _dest: *mut TW_IDENTITY,
    _dg: u32,
    _dat: u16,
    msg: u16,
    _data: *mut c_void,
) -> u16 {
    // SAFETY: no arguments; always succeeds.
    CALLBACK_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    let inside = ENABLING.load(Ordering::SeqCst);
    window::post(
        CALLBACK_WINDOW.load(Ordering::SeqCst),
        WM_SOURCE,
        usize::from(msg),
        isize::from(inside),
    );
    TWRC_SUCCESS
}

pub struct Session {
    out: Out,
    hwnd: HWND,
    state: u8,
    /// The application's identity, at an address that never moves: the DSM
    /// writes its `Id` at `MSG_OPENDSM`.
    app: Box<TW_IDENTITY>,
    /// The open source's identity, likewise.
    source: Option<Box<TW_IDENTITY>>,
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
            source: None,
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

    // States 1 to 3: the DSM.

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
    fn unload_dsm(&mut self) {
        if self.dsm.take().is_some() {
            self.moved(1);
        }
    }

    /// Down to state 1 from wherever the session is, a step at a time: what
    /// every scenario ends with, and what a failed one is left to.
    pub fn walk_down(&mut self) {
        if self.state == 7 {
            self.end_transfer();
        }
        if self.state == 6 {
            self.reset();
        }
        if self.state == 5 {
            self.disable();
        }
        if self.state == 4 {
            self.close_source();
        }
        if self.state == 3 {
            self.close_dsm();
        }
        if self.state == 2 {
            self.unload_dsm();
        }
    }

    /// State 2 to 3, with the window as the parent; then the DSM's entry
    /// points.
    pub fn open_dsm(&mut self) -> bool {
        let mut parent: HWND = self.hwnd;
        // SAFETY: MSG_OPENDSM takes a pointer to the parent window's handle;
        // `parent` lives on this frame for the call.
        let rc = unsafe {
            self.triplet(
                Party::Dsm,
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
        let rc = unsafe {
            self.triplet(
                Party::Dsm,
                DG_CONTROL,
                DAT_ENTRYPOINT,
                MSG_GET,
                (&raw mut ep).cast(),
            )
        };
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
            self.triplet(
                Party::Dsm,
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

    fn allocator(&self) -> Allocator {
        self.dsm.as_ref().map_or(Allocator::GLOBAL, Dsm::allocator)
    }

    /// The memory functions driven once: a block allocated, locked, written,
    /// read back, unlocked and freed.
    pub fn memory_round_trip(&mut self) {
        const LEN: usize = 64;
        let mem = self.allocator();
        let handle = mem.alloc(LEN as u32);
        if !self.check(!handle.is_null(), "allocating 64 bytes failed") {
            return;
        }
        // SAFETY: `handle` was just allocated by this allocator.
        let p = unsafe { mem.lock(handle) }.cast::<u8>();
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
            unsafe { mem.unlock(handle) };
            back == pattern
        };
        // SAFETY: the block is live and unlocked, and isn't used again.
        unsafe { mem.free(handle) };
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
            let rc = unsafe {
                self.triplet(
                    Party::Dsm,
                    DG_CONTROL,
                    DAT_IDENTITY,
                    msg,
                    (&raw mut id).cast(),
                )
            };
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

    // States 3 and 4: the source opened, its capabilities.

    /// State 3 to 4: the source with this identity opened.
    pub fn open_source(&mut self, id: &TW_IDENTITY) -> bool {
        let source: *mut TW_IDENTITY = &raw mut **self.source.insert(Box::new(*id));
        // SAFETY: MSG_OPENDS takes the source's identity, which the DSM
        // completes; it's boxed in `self.source`, so it never moves.
        let rc = unsafe {
            self.triplet(
                Party::Dsm,
                DG_CONTROL,
                DAT_IDENTITY,
                MSG_OPENDS,
                source.cast(),
            )
        };
        if !self.expect("DAT_IDENTITY/MSG_OPENDS", rc, TWRC_SUCCESS, Party::Dsm) {
            self.source = None;
            return false;
        }
        self.moved(4);
        true
    }

    /// State 4 to 3.
    pub fn close_source(&mut self) -> bool {
        let Some(source) = self.source.as_mut() else {
            return false;
        };
        let source: *mut TW_IDENTITY = &raw mut **source;
        // SAFETY: MSG_CLOSEDS takes the open source's identity, boxed in
        // `self.source`.
        let rc = unsafe {
            self.triplet(
                Party::Dsm,
                DG_CONTROL,
                DAT_IDENTITY,
                MSG_CLOSEDS,
                source.cast(),
            )
        };
        if !self.expect("DAT_IDENTITY/MSG_CLOSEDS", rc, TWRC_SUCCESS, Party::Dsm) {
            return false;
        }
        self.source = None;
        self.moved(3);
        true
    }

    /// The callback registered with the source (state 4).
    pub fn register_callback(&mut self) -> bool {
        CALLBACK_WINDOW.store(self.hwnd as usize, Ordering::SeqCst);
        let mut cb = TW_CALLBACK2 {
            CallBackProc: Some(callback),
            RefCon: 0,
            Message: 0,
        };
        // SAFETY: DAT_CALLBACK2 takes a TW_CALLBACK2, which lives on this frame
        // for the call; the DSM copies it, and `callback` lives forever.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_CONTROL,
                DAT_CALLBACK2,
                MSG_REGISTER_CALLBACK,
                (&raw mut cb).cast(),
            )
        };
        self.expect(
            "DAT_CALLBACK2/MSG_REGISTER_CALLBACK",
            rc,
            TWRC_SUCCESS,
            Party::Source,
        )
    }

    /// Sets a capability to one value, expecting success.
    pub fn set_cap(&mut self, name: &str, cap: u16, item_type: u16, raw: u32) -> bool {
        match self.cap_set(cap, item_type, raw) {
            Some(rc) => self.expect(&format!("MSG_SET {name}"), rc, TWRC_SUCCESS, Party::Source),
            None => false,
        }
    }

    /// Sets a capability to one value, expecting the source to refuse it.
    pub fn set_cap_refused(
        &mut self,
        name: &str,
        cap: u16,
        item_type: u16,
        raw: u32,
        cc: u16,
    ) -> bool {
        match self.cap_set(cap, item_type, raw) {
            Some(rc) => self.expect_failure(&format!("MSG_SET {name}"), rc, cc, Party::Source),
            None => false,
        }
    }

    /// `MSG_SET` with a `TW_ONEVALUE`, the container the DSM's allocator's; its
    /// return code.
    fn cap_set(&mut self, cap: u16, item_type: u16, raw: u32) -> Option<u16> {
        let mem = self.allocator();
        let bytes = container::onevalue(item_type, raw);
        let handle = mem.alloc(bytes.len() as u32);
        if !self.check(!handle.is_null(), "allocating a container failed") {
            return None;
        }
        // SAFETY: `handle` was just allocated by this allocator.
        let p = unsafe { mem.lock(handle) }.cast::<u8>();
        if p.is_null() {
            // SAFETY: the block is live and unlocked, and isn't used again.
            unsafe { mem.free(handle) };
            self.out.fail("locking a container failed");
            return None;
        }
        // SAFETY: `p` points to the locked block of at least 6 bytes; byte
        // copies need no alignment; it's unlocked once, after.
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            mem.unlock(handle);
        }
        let mut capability = TW_CAPABILITY {
            Cap: cap,
            ConType: TWON_ONEVALUE,
            hContainer: handle,
        };
        // SAFETY: MSG_SET takes a TW_CAPABILITY whose container is a
        // TW_ONEVALUE in a block from the DSM's allocator, as this is; it
        // lives on this frame for the call.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_CONTROL,
                DAT_CAPABILITY,
                MSG_SET,
                (&raw mut capability).cast(),
            )
        };
        // SAFETY: the application allocated the container and frees it; the
        // source has copied what it wanted.
        unsafe { mem.free(handle) };
        Some(rc)
    }

    /// A capability's current value, expecting success, reported.
    pub fn get_current(&mut self, name: &str, cap: u16) -> Option<Current> {
        let (rc, current) = self.cap_get_current(cap);
        if !self.expect(
            &format!("MSG_GETCURRENT {name}"),
            rc,
            TWRC_SUCCESS,
            Party::Source,
        ) {
            return None;
        }
        match current {
            Some(Ok(c)) => {
                self.out
                    .line(format!("{name}: {} ({})", c.value(), c.kind()));
                Some(c)
            }
            Some(Err(e)) => {
                self.out
                    .fail(format!("{name}: its container was refused: {e}"));
                None
            }
            None => {
                self.out.fail(format!("{name}: no container came back"));
                None
            }
        }
    }

    /// A capability's current value asked for, expecting the source to refuse.
    pub fn get_current_refused(&mut self, name: &str, cap: u16, cc: u16) -> bool {
        let (rc, _) = self.cap_get_current(cap);
        self.expect_failure(&format!("MSG_GETCURRENT {name}"), rc, cc, Party::Source)
    }

    /// `MSG_GETCURRENT`: its return code, and the container decoded and freed.
    fn cap_get_current(&mut self, cap: u16) -> (u16, Option<Result<Current, String>>) {
        let mut capability = TW_CAPABILITY {
            Cap: cap,
            ConType: TWON_DONTCARE16,
            hContainer: null_mut(),
        };
        // SAFETY: MSG_GETCURRENT takes a TW_CAPABILITY, into which the source
        // puts a container it allocated; `capability` lives on this frame.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_CONTROL,
                DAT_CAPABILITY,
                MSG_GETCURRENT,
                (&raw mut capability).cast(),
            )
        };
        let (handle, contype) = (capability.hContainer, capability.ConType);
        if rc != TWRC_SUCCESS || handle.is_null() {
            return (rc, None);
        }
        let mem = self.allocator();
        // SAFETY: the source allocated `handle` with the DSM's allocator and
        // the application now owns it.
        let size = unsafe { mem.size(handle) };
        // SAFETY: as above; locked once, unlocked once.
        let p = unsafe { mem.lock(handle) }.cast::<u8>();
        let decoded = if p.is_null() || size == 0 {
            Err("its container couldn't be locked or measured".to_owned())
        } else {
            // SAFETY: `p` points to the locked block of `size` bytes, as
            // GlobalSize measured it; it's read, then unlocked.
            let bytes = unsafe { core::slice::from_raw_parts(p, size) };
            let decoded = container::decode(contype, bytes).map_err(|e| e.to_string());
            // SAFETY: locked above, unlocked once; `bytes` isn't used after.
            unsafe { mem.unlock(handle) };
            decoded
        };
        // SAFETY: the application owns the container and frees it once.
        unsafe { mem.free(handle) };
        (rc, Some(decoded))
    }

    // States 4 to 7: enabled, transferring.

    /// State 4 to 5: the source enabled without its UI. The sample signals
    /// `MSG_XFERREADY` within this call.
    pub fn enable(&mut self) -> bool {
        let mut ui = TW_USERINTERFACE {
            ShowUI: 0,
            ModalUI: 0,
            hParent: self.hwnd,
        };
        ENABLING.store(true, Ordering::SeqCst);
        // SAFETY: MSG_ENABLEDS takes a TW_USERINTERFACE, which lives on this
        // frame for the call.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_CONTROL,
                DAT_USERINTERFACE,
                MSG_ENABLEDS,
                (&raw mut ui).cast(),
            )
        };
        ENABLING.store(false, Ordering::SeqCst);
        if !self.expect(
            "DAT_USERINTERFACE/MSG_ENABLEDS",
            rc,
            TWRC_SUCCESS,
            Party::Source,
        ) {
            return false;
        }
        self.moved(5);
        true
    }

    /// State 5 to 4.
    pub fn disable(&mut self) -> bool {
        let mut ui = TW_USERINTERFACE {
            ShowUI: 0,
            ModalUI: 0,
            hParent: self.hwnd,
        };
        // SAFETY: MSG_DISABLEDS takes a TW_USERINTERFACE, which lives on this
        // frame for the call.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_CONTROL,
                DAT_USERINTERFACE,
                MSG_DISABLEDS,
                (&raw mut ui).cast(),
            )
        };
        if !self.expect(
            "DAT_USERINTERFACE/MSG_DISABLEDS",
            rc,
            TWRC_SUCCESS,
            Party::Source,
        ) {
            return false;
        }
        self.moved(4);
        true
    }

    /// Waits for the source's next message, through the callback or through
    /// `MSG_PROCESSEVENT` as `mode` says, dispatching what else comes. `None`
    /// if none comes in 20 seconds.
    pub fn wait_for_source(&mut self, mode: Mode) -> Option<u16> {
        window::set_timer(self.hwnd, SOURCE_TIMER, SOURCE_WAIT_MS);
        let got = loop {
            let Some(mut msg) = window::next_message() else {
                self.out
                    .fail("the message loop ended (WM_QUIT, or GetMessageW failed)");
                break None;
            };
            if msg.message == WM_TIMER && msg.wParam == SOURCE_TIMER {
                self.out.fail(format!(
                    "no message from the source in {} seconds",
                    SOURCE_WAIT_MS / 1000
                ));
                break None;
            }
            if msg.message == WM_SOURCE {
                let twmsg = msg.wParam as u16;
                let thread = CALLBACK_THREAD.load(Ordering::SeqCst);
                let on = if thread == window::thread_id() {
                    "the TWAIN thread".to_owned()
                } else {
                    format!("thread {thread}")
                };
                let when = if msg.lParam != 0 { "inside" } else { "after" };
                self.out.line(format!(
                    "{} via the callback, {when} MSG_ENABLEDS, on {on}",
                    msg_name(twmsg)
                ));
                break Some(twmsg);
            }
            if SPIKE_MESSAGES.contains(&msg.message) {
                self.out.line(format!(
                    "request {} set aside while waiting for the source",
                    msg.wParam
                ));
                continue;
            }
            if mode == Mode::Event && (5..=7).contains(&self.state) {
                let carrier = msg.message;
                match self.process_event(&mut msg) {
                    (TWRC_DSEVENT, MSG_NULL) => {}
                    (TWRC_DSEVENT, twmsg) => {
                        self.out.line(format!(
                            "{} via MSG_PROCESSEVENT, carried by window message {carrier:#06x}",
                            msg_name(twmsg)
                        ));
                        break Some(twmsg);
                    }
                    (TWRC_NOTDSEVENT, _) => window::dispatch(&msg),
                    (rc, _) => {
                        self.expect(
                            "DAT_EVENT/MSG_PROCESSEVENT",
                            rc,
                            TWRC_NOTDSEVENT,
                            Party::Source,
                        );
                        window::dispatch(&msg);
                    }
                }
            } else {
                window::dispatch(&msg);
            }
        };
        window::kill_timer(self.hwnd, SOURCE_TIMER);
        got
    }

    /// A window message offered to the source: its return code and message.
    fn process_event(&mut self, msg: &mut MSG) -> (u16, u16) {
        let mut event = TW_EVENT {
            pEvent: (msg as *mut MSG).cast(),
            TWMessage: MSG_NULL,
        };
        // SAFETY: MSG_PROCESSEVENT takes a TW_EVENT pointing at a window
        // message; both live on this frame or the caller's for the call.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_CONTROL,
                DAT_EVENT,
                MSG_PROCESSEVENT,
                (&raw mut event).cast(),
            )
        };
        (rc, event.TWMessage)
    }

    /// State 5 to 6, on `MSG_XFERREADY`.
    pub fn transfer_ready(&mut self) {
        self.moved(6);
    }

    /// The pending image's info (state 6).
    pub fn image_info(&mut self) -> Option<TW_IMAGEINFO> {
        let mut info = TW_IMAGEINFO::default();
        // SAFETY: DAT_IMAGEINFO writes a TW_IMAGEINFO into `info`, which lives
        // on this frame for the call.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_IMAGE,
                DAT_IMAGEINFO,
                MSG_GET,
                (&raw mut info).cast(),
            )
        };
        self.expect("DAT_IMAGEINFO/MSG_GET", rc, TWRC_SUCCESS, Party::Source)
            .then_some(info)
    }

    /// A native transfer (state 6 to 7): the DIB's bytes, the handle freed.
    pub fn native(&mut self) -> Option<Vec<u8>> {
        let mut handle: *mut c_void = null_mut();
        // SAFETY: DAT_IMAGENATIVEXFER writes a TW_HANDLE into `handle`, which
        // lives on this frame for the call.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_IMAGE,
                DAT_IMAGENATIVEXFER,
                MSG_GET,
                (&raw mut handle).cast(),
            )
        };
        if !self.expect(
            "DAT_IMAGENATIVEXFER/MSG_GET",
            rc,
            TWRC_XFERDONE,
            Party::Source,
        ) {
            return None;
        }
        self.moved(7);
        if !self.check(!handle.is_null(), "the native transfer gave a null handle") {
            return None;
        }
        // SAFETY: on Windows the handle is a GlobalAlloc block holding a packed
        // DIB, now the application's.
        let size = unsafe { GlobalSize(handle) };
        // SAFETY: as above; locked once, unlocked once below.
        let p = unsafe { GlobalLock(handle) }.cast::<u8>().cast_const();
        let bytes = if p.is_null() || size == 0 || size > MAX_DIB_BYTES {
            self.out
                .fail(format!("the DIB's block: {size} bytes, locked at {p:?}"));
            None
        } else {
            // SAFETY: `p` points to the locked block of `size` bytes, as
            // GlobalSize measured it; it's copied, then unlocked.
            let bytes = unsafe { core::slice::from_raw_parts(p, size) }.to_vec();
            // SAFETY: locked above, unlocked once.
            unsafe { GlobalUnlock(handle) };
            Some(bytes)
        };
        // SAFETY: the application owns the DIB's block and frees it once.
        unsafe { GlobalFree(handle) };
        bytes
    }

    /// A memory transfer (state 6 to 7): every buffer's tile, in order.
    pub fn memory(&mut self) -> Option<Vec<Tile>> {
        let mut setup = TW_SETUPMEMXFER::default();
        // SAFETY: DAT_SETUPMEMXFER writes a TW_SETUPMEMXFER into `setup`, which
        // lives on this frame for the call.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_CONTROL,
                DAT_SETUPMEMXFER,
                MSG_GET,
                (&raw mut setup).cast(),
            )
        };
        if !self.expect("DAT_SETUPMEMXFER/MSG_GET", rc, TWRC_SUCCESS, Party::Source) {
            return None;
        }
        let (min, max, preferred) = (setup.MinBufSize, setup.MaxBufSize, setup.Preferred);
        let usable = |n: u32| n != 0 && n != TWON_DONTCARE32;
        let size = [preferred, max, min]
            .into_iter()
            .find(|&n| usable(n))
            .unwrap_or(0)
            .min(MAX_BUFFER);
        if !self.check(
            size > 0 && (!usable(min) || size >= min),
            format!("no usable buffer size: min {min}, max {max}, preferred {preferred}"),
        ) {
            return None;
        }
        self.out.line(format!(
            "memory buffers: min {min}, max {max}, preferred {preferred}: {size} used"
        ));
        let mut buffer = vec![0u8; size as usize];
        let mut tiles = Vec::new();
        loop {
            let mut mx = TW_IMAGEMEMXFER {
                Compression: TWON_DONTCARE16,
                BytesPerRow: TWON_DONTCARE32,
                Columns: TWON_DONTCARE32,
                Rows: TWON_DONTCARE32,
                XOffset: TWON_DONTCARE32,
                YOffset: TWON_DONTCARE32,
                BytesWritten: TWON_DONTCARE32,
                Memory: TW_MEMORY {
                    Flags: TWMF_APPOWNS | TWMF_POINTER,
                    Length: size,
                    TheMem: buffer.as_mut_ptr().cast(),
                },
            };
            // SAFETY: DAT_IMAGEMEMXFER takes a TW_IMAGEMEMXFER naming the
            // application's buffer, `size` bytes, which the source fills; both
            // live on this frame for the call.
            let rc = unsafe {
                self.triplet(
                    Party::Source,
                    DG_IMAGE,
                    DAT_IMAGEMEMXFER,
                    MSG_GET,
                    (&raw mut mx).cast(),
                )
            };
            if rc != TWRC_SUCCESS && rc != TWRC_XFERDONE {
                self.expect("DAT_IMAGEMEMXFER/MSG_GET", rc, TWRC_SUCCESS, Party::Source);
                return None;
            }
            if self.state == 6 {
                self.moved(7);
            }
            let compression = mx.Compression;
            if !self.check(
                compression == TWCP_NONE,
                format!("a buffer compressed with {compression}"),
            ) {
                return None;
            }
            let shape = (mx.XOffset, mx.YOffset, mx.Columns, mx.Rows, mx.BytesPerRow);
            match Tile::from_buffer(shape, mx.BytesWritten, &buffer) {
                Ok(tile) => tiles.push(tile),
                Err(e) => {
                    self.out.fail(format!("buffer {}: {e}", tiles.len()));
                    return None;
                }
            }
            if rc == TWRC_XFERDONE {
                self.out.line(format!(
                    "DAT_IMAGEMEMXFER/MSG_GET: TWRC_SUCCESS {} times, then TWRC_XFERDONE",
                    tiles.len() - 1
                ));
                break;
            }
            if !self.check(
                tiles.len() < MAX_TILES,
                format!("more than {MAX_TILES} buffers: stopped"),
            ) {
                return None;
            }
        }
        Some(tiles)
    }

    /// The extended image info asked for (state 7); the sample refuses.
    pub fn ext_image_info(&mut self) -> bool {
        let mut ext = TW_EXTIMAGEINFO {
            NumInfos: 1,
            Info: [TW_INFO {
                InfoID: TWEI_DOCUMENTNUMBER,
                ..TW_INFO::default()
            }],
        };
        // SAFETY: DAT_EXTIMAGEINFO takes a TW_EXTIMAGEINFO with room for
        // NumInfos entries, as `ext` has; it lives on this frame for the call.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_IMAGE,
                DAT_EXTIMAGEINFO,
                MSG_GET,
                (&raw mut ext).cast(),
            )
        };
        self.expect_failure(
            "DAT_EXTIMAGEINFO/MSG_GET",
            rc,
            TWCC_CAPUNSUPPORTED,
            Party::Source,
        )
    }

    /// `MSG_ENDXFER` (state 7 to 6, or to 5 with nothing pending): the count
    /// pending.
    pub fn end_transfer(&mut self) -> Option<u16> {
        let mut pending = TW_PENDINGXFERS::default();
        // SAFETY: MSG_ENDXFER writes a TW_PENDINGXFERS into `pending`, which
        // lives on this frame for the call.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_CONTROL,
                DAT_PENDINGXFERS,
                MSG_ENDXFER,
                (&raw mut pending).cast(),
            )
        };
        if !self.expect(
            "DAT_PENDINGXFERS/MSG_ENDXFER",
            rc,
            TWRC_SUCCESS,
            Party::Source,
        ) {
            return None;
        }
        let count = pending.Count;
        self.out.line(format!("pending: {}", pending_text(count)));
        self.moved(if count == 0 { 5 } else { 6 });
        Some(count)
    }

    /// `MSG_RESET` (state 6 to 5): nothing more transferred.
    pub fn reset(&mut self) -> bool {
        let mut pending = TW_PENDINGXFERS::default();
        // SAFETY: MSG_RESET writes a TW_PENDINGXFERS into `pending`, which
        // lives on this frame for the call.
        let rc = unsafe {
            self.triplet(
                Party::Source,
                DG_CONTROL,
                DAT_PENDINGXFERS,
                MSG_RESET,
                (&raw mut pending).cast(),
            )
        };
        if !self.expect(
            "DAT_PENDINGXFERS/MSG_RESET",
            rc,
            TWRC_SUCCESS,
            Party::Source,
        ) {
            return false;
        }
        let count = pending.Count;
        self.check(
            count == 0,
            format!("MSG_RESET left {} pending", pending_text(count)),
        );
        self.moved(5);
        true
    }

    // The triplets themselves.

    /// Calls a triplet: the DSM's own (no destination) or the open source's.
    ///
    /// # Safety
    ///
    /// `data` must point to what the triplet takes, live and writable for the
    /// call.
    unsafe fn triplet(
        &mut self,
        party: Party,
        dg: u32,
        dat: u16,
        msg: u16,
        data: *mut c_void,
    ) -> u16 {
        let dest: *mut TW_IDENTITY = match (party, self.source.as_mut()) {
            (Party::Dsm, _) => null_mut(),
            (Party::Source, Some(source)) => &raw mut **source,
            (Party::Source, None) => {
                self.out.fail("a source triplet with no source open");
                return TWRC_FAILURE;
            }
        };
        let Some(dsm) = self.dsm.as_ref() else {
            self.out.fail("a triplet with no DSM loaded");
            return TWRC_FAILURE;
        };
        let origin: *mut TW_IDENTITY = &raw mut *self.app;
        // SAFETY: `origin` is the application's identity and `dest` null or
        // the open source's, both boxed so they never move; the caller keeps
        // `data`'s contract.
        unsafe { dsm.entry(origin, dest, dg, dat, msg, data) }
    }

    /// The condition code for the last triplet sent to `party`.
    fn condition(&mut self, party: Party) -> Option<u16> {
        let mut status = TW_STATUS::default();
        // SAFETY: DAT_STATUS writes a TW_STATUS into `status`, which lives on
        // this frame for the call.
        let rc = unsafe {
            self.triplet(
                party,
                DG_CONTROL,
                DAT_STATUS,
                MSG_GET,
                (&raw mut status).cast(),
            )
        };
        (rc == TWRC_SUCCESS).then_some(status.ConditionCode)
    }

    /// Reports a triplet's answer, failing if it isn't `want`; on a failure the
    /// condition code is asked for and reported.
    fn expect(&mut self, what: &str, rc: u16, want: u16, party: Party) -> bool {
        if rc == want {
            self.out.line(format!("{what}: {}", rc_name(rc)));
            return true;
        }
        let cc = self
            .condition(party)
            .map_or("condition code unavailable".to_owned(), cc_name);
        self.out.fail(format!(
            "{what}: {} ({cc}), expected {}",
            rc_name(rc),
            rc_name(want)
        ));
        false
    }

    /// Reports a triplet that should fail with condition code `want`.
    fn expect_failure(&mut self, what: &str, rc: u16, want: u16, party: Party) -> bool {
        let cc = (rc == TWRC_FAILURE)
            .then(|| self.condition(party))
            .flatten();
        if rc == TWRC_FAILURE && cc == Some(want) {
            self.out.line(format!(
                "{what}: TWRC_FAILURE, {}, as expected",
                cc_name(want)
            ));
            return true;
        }
        let got = cc.map_or(String::new(), |c| format!(" ({})", cc_name(c)));
        self.out.fail(format!(
            "{what}: {}{got}, expected TWRC_FAILURE with {}",
            rc_name(rc),
            cc_name(want)
        ));
        false
    }
}

impl Drop for Session {
    /// Walks down whatever a failed scenario left up.
    fn drop(&mut self) {
        self.walk_down();
    }
}

/// A pending-transfer count as words: 0xFFFF is "don't know".
pub fn pending_text(count: u16) -> String {
    if count == TWON_DONTCARE16 {
        "unknown (-1)".to_owned()
    } else {
        count.to_string()
    }
}

/// A message from a source, by name.
pub fn msg_name(msg: u16) -> String {
    match msg {
        MSG_XFERREADY => "MSG_XFERREADY".to_owned(),
        MSG_CLOSEDSREQ => "MSG_CLOSEDSREQ".to_owned(),
        MSG_CLOSEDSOK => "MSG_CLOSEDSOK".to_owned(),
        MSG_DEVICEEVENT => "MSG_DEVICEEVENT".to_owned(),
        other => format!("message {other:#06x}"),
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
