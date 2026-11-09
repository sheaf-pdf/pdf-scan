//! TWAIN's states 5 to 7: a source enabled, a transfer ready, a transfer
//! done. As in [`states`](crate::states), each owns the state beneath it;
//! here every `Drop` steps down through the record in [`Twain`], which knows
//! where the source really is (a source that has said `MSG_XFERREADY` is in
//! state 6 whatever state the caller holds).

use core::fmt;
use std::time::{Duration, Instant};

use crate::codes::{ReturnCode, Triplet};
use crate::container::ContainerError;
use crate::dsm::{Dib, Dsm};
use crate::error::{Error, Refused};
use crate::ext_info::{self, ExtInfo};
use crate::ffi;
use crate::states::Source;
use crate::thread::{Level, Pumped, Twain};

/// Whether the source shows its own window when it's enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ui {
    /// Scanning with the settings as they are.
    Hidden,
    /// The source's window shown, parented to the hidden window; the user
    /// starts the scan there.
    Shown,
}

/// The floor a step down stops at: TWAIN's state number, 3 for no source.
fn number<T>(twain: &Twain<T>) -> u8 {
    match twain.source_open.get() {
        None => 3,
        Some(_) => twain.level.get() as u8,
    }
}

impl<T> Twain<T> {
    /// The open source stepped down to state `floor` (3: closed), each step
    /// from where the record says it is: `MSG_ENDXFER` (7 to 6, or to 5 when
    /// nothing is pending), `MSG_RESET` (6 to 5), `MSG_DISABLEDS` (5 to 4),
    /// `MSG_CLOSEDS` (4 to 3). A step refused is passed over to the next
    /// (the first refusal is returned), but for `MSG_CLOSEDS`: a source that
    /// won't close stays open in the record. A step refused with
    /// `TWCC_SEQERROR` means the source isn't where the record says (one that
    /// said `MSG_XFERREADY` before the pump saw it is in state 6, say); the walk
    /// then starts again once from state 7, as TWAIN.cs's rollback does, so
    /// that each step down is tried from wherever it really is, and that
    /// refusal is returned only if the floor still isn't reached.
    pub(crate) fn step_down_to(&self, floor: u8) -> Result<(), Error> {
        let Some(dsm) = self.dsm.get() else {
            return Ok(());
        };
        let mut first = None;
        let mut out_of_step = None;
        let mut restarted = false;
        while number(self) > floor {
            let level = self.level.get();
            let (triplet, rc, next) = match level {
                Level::Transferring => {
                    let (triplet, rc, count) =
                        dsm.pending_transfers(&self.app, &self.source, ffi::MSG_ENDXFER);
                    let next = if rc == ReturnCode::SUCCESS && count == 0 {
                        Level::Enabled
                    } else {
                        Level::Ready
                    };
                    (triplet, rc, Some(next))
                }
                Level::Ready => {
                    let (triplet, rc, _) =
                        dsm.pending_transfers(&self.app, &self.source, ffi::MSG_RESET);
                    (triplet, rc, Some(Level::Enabled))
                }
                Level::Enabled => {
                    let (triplet, rc) = dsm.user_interface(
                        &self.app,
                        &self.source,
                        ffi::MSG_DISABLEDS,
                        false,
                        self.window.hwnd(),
                    );
                    (triplet, rc, Some(Level::Open))
                }
                Level::Open => {
                    let (triplet, rc) = dsm.source(&self.app, &self.source, false);
                    (triplet, rc, None)
                }
            };
            if rc != ReturnCode::SUCCESS {
                let error = self.source_failure(dsm, &self.source, triplet, rc);
                let sequence = matches!(
                    error,
                    Error::Failed {
                        by_source: Some(crate::ConditionCode::SEQERROR),
                        ..
                    }
                );
                if sequence && !restarted && level < Level::Transferring {
                    restarted = true;
                    out_of_step = Some(error);
                    self.level.set(Level::Transferring);
                    continue;
                }
                first.get_or_insert(error);
            }
            match next {
                Some(next) => {
                    self.level.set(next);
                    if next <= Level::Open {
                        self.events.borrow_mut().clear();
                    }
                }
                None if rc == ReturnCode::SUCCESS => {
                    self.source_open.set(None);
                    self.level.set(Level::Open);
                    self.events.borrow_mut().clear();
                }
                None => break,
            }
        }
        if number(self) > floor {
            first = first.or(out_of_step);
        }
        first.map_or(Ok(()), Err)
    }

    /// The source the record says is open, closed: stepped down from where it
    /// is. The end of `run` does it before closing the connection: the DSM's
    /// own closing of sources left open (`apps.cpp`, `RemoveApp`) stops before
    /// the last source it lists, so it never closes a source installed alone.
    pub(crate) fn close_source(&self) -> Result<(), Error> {
        self.step_down_to(3)
    }
}

impl<'t, T> Source<'t, T> {
    /// Enables it (`MSG_ENABLEDS`, state 4 to 5), its window shown or not.
    /// Its messages come through [`Enabled::next_event`].
    pub fn enable(self, ui: Ui) -> Result<Enabled<'t, T>, Refused<Self>> {
        self.enable_with(ffi::MSG_ENABLEDS, ui == Ui::Shown)
    }

    /// Shows the source's window for its settings only
    /// (`MSG_ENABLEDSUIONLY`, state 4 to 5), where `CAP_ENABLEDSUIONLY` says
    /// it has one: no transfer follows, and the source says
    /// [`Event::CloseOk`] when the user saves them, [`Event::CloseRequest`]
    /// when they cancel.
    pub fn settings_ui(self) -> Result<Enabled<'t, T>, Refused<Self>> {
        self.enable_with(ffi::MSG_ENABLEDSUIONLY, true)
    }

    fn enable_with(self, msg: u16, show: bool) -> Result<Enabled<'t, T>, Refused<Self>> {
        let twain = self.twain();
        let dsm = self.dsm();
        let (triplet, rc) =
            dsm.user_interface(&twain.app, &twain.source, msg, show, twain.window.hwnd());
        // TWRC_CHECKSTATUS: enabled, the UI shown though not asked for (a
        // source that can't hide it).
        if rc != ReturnCode::SUCCESS && rc != ReturnCode::CHECKSTATUS {
            let error = twain.source_failure(dsm, &twain.source, triplet, rc);
            return Err(Refused { state: self, error });
        }
        // A message the callback posted inside MSG_ENABLEDS is still queued,
        // so the record moves to 5 first and the pump takes it to 6.
        if twain.level.get() < Level::Enabled {
            twain.level.set(Level::Enabled);
        }
        Ok(Enabled {
            source: Some(self),
            settings_only: msg == ffi::MSG_ENABLEDSUIONLY,
        })
    }
}

/// State 5: the source enabled, scanning or showing its window, its messages
/// to come.
pub struct Enabled<'t, T> {
    /// `None` once stepped down.
    source: Option<Source<'t, T>>,
    settings_only: bool,
}

/// What [`Enabled::next_event`] came back with.
pub enum Next<'t, T> {
    /// `MSG_XFERREADY`: a transfer is ready (state 6).
    Ready(Ready<'t, T>),
    /// Another message from the source.
    Event(Enabled<'t, T>, Event),
    /// A request a [`Remote`](crate::Remote) posted.
    Request(Enabled<'t, T>, T),
    /// The time ran out.
    Timeout(Enabled<'t, T>),
    /// The thread was asked to quit (`WM_QUIT`).
    Quit(Enabled<'t, T>, i32),
}

/// A message from an enabled source, other than a transfer ready.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// `MSG_CLOSEDSREQ`: the user closed the source's window, or cancelled;
    /// the application disables the source.
    CloseRequest,
    /// `MSG_CLOSEDSOK`: the user saved the settings (after
    /// [`Source::settings_ui`]); the application disables the source.
    CloseOk,
    /// `MSG_DEVICEEVENT`, with what `DAT_DEVICEEVENT` said of it.
    Device(Box<ffi::TW_DEVICEEVENT>),
    /// A message TWAIN doesn't define for this.
    Other(u16),
}

impl<'t, T> Enabled<'t, T> {
    fn source(&self) -> &Source<'t, T> {
        match &self.source {
            Some(source) => source,
            None => unreachable!("an Enabled without its Source"),
        }
    }

    fn twain(&self) -> &'t Twain<T> {
        self.source().twain()
    }

    fn dsm(&self) -> &'static Dsm {
        self.source().dsm()
    }

    /// Whether this is [`Source::settings_ui`]'s, which no transfer follows.
    pub fn settings_only(&self) -> bool {
        self.settings_only
    }

    /// The source's next message, or a request, pumping the thread's messages
    /// meanwhile, until `timeout`. The source's come first: the messages
    /// queued are all taken before a request is handed over.
    pub fn next_event(self, timeout: Duration) -> Result<Next<'t, T>, Refused<Self>> {
        let twain = self.twain();
        let mut request = None;
        let pumped = twain.pump(Instant::now() + timeout, |twain| {
            if !twain.events.borrow().is_empty() {
                return true;
            }
            if request.is_none() {
                request = twain.take_request();
            }
            request.is_some()
        });
        let message = twain.events.borrow_mut().pop_front();
        if let Some(message) = message {
            if let Some(request) = request {
                // Taken from the channel while looking: it waits for the next
                // call, ahead of any posted since.
                twain.requeue(request);
            }
            return self.message(message);
        }
        Ok(match (pumped, request) {
            (_, Some(request)) => Next::Request(self, request),
            (Pumped::Quit(code), None) => Next::Quit(self, code),
            _ => Next::Timeout(self),
        })
    }

    fn message(self, message: u16) -> Result<Next<'t, T>, Refused<Self>> {
        let event = match message {
            ffi::MSG_XFERREADY => {
                return Ok(Next::Ready(Ready {
                    enabled: Some(self),
                    pending: None,
                }));
            }
            ffi::MSG_CLOSEDSREQ => Event::CloseRequest,
            ffi::MSG_CLOSEDSOK => Event::CloseOk,
            ffi::MSG_DEVICEEVENT => {
                let twain = self.twain();
                let dsm = self.dsm();
                let (triplet, rc, event) = dsm.device_event(&twain.app, &twain.source);
                if rc != ReturnCode::SUCCESS {
                    let error = twain.source_failure(dsm, &twain.source, triplet, rc);
                    return Err(Refused { state: self, error });
                }
                Event::Device(Box::new(event))
            }
            other => Event::Other(other),
        };
        Ok(Next::Event(self, event))
    }

    /// Disables the source (`MSG_DISABLEDS`, state 5 to 4).
    pub fn disable(mut self) -> Result<Source<'t, T>, Refused<Self>> {
        match self.twain().step_down_to(4) {
            Ok(()) => Ok(self.source.take().expect("an Enabled has its Source")),
            Err(error) => Err(Refused { state: self, error }),
        }
    }
}

impl<T> Drop for Enabled<'_, T> {
    fn drop(&mut self) {
        if let Some(source) = &self.source {
            let twain = source.twain();
            twain.dropped(twain.step_down_to(4));
        }
    }
}

impl<T> fmt::Debug for Enabled<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Enabled")
            .field("source", self.source())
            .field("settings_only", &self.settings_only)
            .finish()
    }
}

/// State 6: a transfer ready.
pub struct Ready<'t, T> {
    /// `None` once stepped down.
    enabled: Option<Enabled<'t, T>>,
    /// The pending count `MSG_ENDXFER` gave last, if any.
    pending: Option<u16>,
}

impl<'t, T> Ready<'t, T> {
    fn enabled(&self) -> &Enabled<'t, T> {
        match &self.enabled {
            Some(enabled) => enabled,
            None => unreachable!("a Ready without its Enabled"),
        }
    }

    fn twain(&self) -> &'t Twain<T> {
        self.enabled().twain()
    }

    fn dsm(&self) -> &'static Dsm {
        self.enabled().dsm()
    }

    fn failure(&self, triplet: Triplet, rc: ReturnCode) -> Error {
        let twain = self.twain();
        twain.source_failure(self.dsm(), &twain.source, triplet, rc)
    }

    /// The transfers the source said were pending after the last one ended:
    /// `None` before the first, and for 0xFFFF, "some, how many unknown".
    pub fn pending(&self) -> Option<u16> {
        self.pending.filter(|&n| n != ffi::TWON_DONTCARE16)
    }

    /// The image to come, as `DAT_IMAGEINFO` describes it.
    pub fn image_info(&self) -> Result<ffi::TW_IMAGEINFO, Error> {
        let twain = self.twain();
        let (triplet, rc, info) = self.dsm().image_info(&twain.app, &twain.source);
        if rc != ReturnCode::SUCCESS {
            return Err(self.failure(triplet, rc));
        }
        Ok(info)
    }

    /// A native transfer (`DAT_IMAGENATIVEXFER`, state 6 to 7): the DIB, or
    /// the source's cancel.
    pub fn native(self) -> Result<Native<'t, T>, Refused<Self>> {
        let twain = self.twain();
        let (triplet, rc, dib) = self.dsm().native(&twain.app, &twain.source);
        match rc {
            ReturnCode::XFERDONE | ReturnCode::CANCEL => {
                twain.level.set(Level::Transferring);
                let transferred = Transferred { ready: Some(self) };
                Ok(match dib {
                    Some(dib) => Native::Done(dib, transferred),
                    None if rc == ReturnCode::XFERDONE => Native::Done(Dib::empty(), transferred),
                    None => Native::Cancelled(transferred),
                })
            }
            rc => {
                let error = self.failure(triplet, rc);
                Err(Refused { state: self, error })
            }
        }
    }

    /// A memory transfer begun: the source's buffer sizes
    /// (`DAT_SETUPMEMXFER`); the buffers come from
    /// [`MemoryTransfer::next_buffer`].
    pub fn memory(self) -> Result<MemoryTransfer<'t, T>, Refused<Self>> {
        let twain = self.twain();
        let (triplet, rc, setup) = self.dsm().setup_memory(&twain.app, &twain.source);
        if rc != ReturnCode::SUCCESS {
            let error = self.failure(triplet, rc);
            return Err(Refused { state: self, error });
        }
        Ok(MemoryTransfer {
            ready: Some(self),
            setup,
            ended: None,
        })
    }

    /// The transfers pending dropped (`MSG_RESET`, state 6 to 5): a scan
    /// cancelled between pages.
    pub fn reset(mut self) -> Result<Enabled<'t, T>, Refused<Self>> {
        match self.twain().step_down_to(5) {
            Ok(()) => Ok(self.enabled.take().expect("a Ready has its Enabled")),
            Err(error) => Err(Refused { state: self, error }),
        }
    }

    /// The feeder stopped after the pages it has taken (`MSG_STOPFEEDER`,
    /// staying in state 6): those still come.
    pub fn stop_feeder(&mut self) -> Result<(), Error> {
        let twain = self.twain();
        let (triplet, rc, count) =
            self.dsm()
                .pending_transfers(&twain.app, &twain.source, ffi::MSG_STOPFEEDER);
        if rc != ReturnCode::SUCCESS {
            return Err(self.failure(triplet, rc));
        }
        self.pending = Some(count);
        Ok(())
    }

    /// The `Enabled` beneath, the record as it is.
    fn into_enabled(mut self) -> Enabled<'t, T> {
        self.enabled.take().expect("a Ready has its Enabled")
    }
}

impl<T> Drop for Ready<'_, T> {
    fn drop(&mut self) {
        if let Some(enabled) = &self.enabled {
            let twain = enabled.twain();
            twain.dropped(twain.step_down_to(5));
        }
    }
}

impl<T> fmt::Debug for Ready<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ready")
            .field("pending", &self.pending())
            .finish_non_exhaustive()
    }
}

/// A native transfer's end.
#[derive(Debug)]
pub enum Native<'t, T> {
    /// The image, as a DIB.
    Done(Dib, Transferred<'t, T>),
    /// The source cancelled the transfer (`TWRC_CANCEL`): no image.
    Cancelled(Transferred<'t, T>),
}

/// State 7: a transfer done (or cancelled by the source), to be ended.
pub struct Transferred<'t, T> {
    /// `None` once stepped down.
    ready: Option<Ready<'t, T>>,
}

/// What [`Transferred::end`] leads to.
pub enum Pending<'t, T> {
    /// Another transfer ready (state 6).
    More(Ready<'t, T>),
    /// None pending (state 5).
    Done(Enabled<'t, T>),
}

impl<'t, T> Transferred<'t, T> {
    fn ready(&self) -> &Ready<'t, T> {
        match &self.ready {
            Some(ready) => ready,
            None => unreachable!("a Transferred without its Ready"),
        }
    }

    /// The extended image info the source has for the transfer
    /// (`DAT_EXTIMAGEINFO`), for each `TWEI_` in `ids`.
    pub fn ext_image_info(&self, ids: &[u16]) -> Result<Vec<ExtInfo>, Error> {
        ext_image_info(self.ready(), ids)
    }

    /// Ends the transfer (`MSG_ENDXFER`): to state 6 if more are pending, else
    /// to 5.
    pub fn end(mut self) -> Result<Pending<'t, T>, Refused<Self>> {
        let ready = self.ready.take().expect("a Transferred has its Ready");
        match end(ready) {
            Ok(pending) => Ok(pending),
            Err((ready, error)) => {
                self.ready = Some(ready);
                Err(Refused { state: self, error })
            }
        }
    }
}

impl<T> Drop for Transferred<'_, T> {
    fn drop(&mut self) {
        if let Some(ready) = &self.ready {
            let twain = ready.twain();
            twain.dropped(twain.step_down_to(6));
        }
    }
}

impl<T> fmt::Debug for Transferred<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transferred").finish_non_exhaustive()
    }
}

/// `MSG_ENDXFER` from state 6 or 7, the record updated; the Ready handed back
/// on failure.
fn end<'t, T>(mut ready: Ready<'t, T>) -> Result<Pending<'t, T>, (Ready<'t, T>, Error)> {
    let twain = ready.twain();
    let dsm = ready.dsm();
    let (triplet, rc, count) = dsm.pending_transfers(&twain.app, &twain.source, ffi::MSG_ENDXFER);
    if rc != ReturnCode::SUCCESS {
        let error = ready.failure(triplet, rc);
        return Err((ready, error));
    }
    if count == 0 {
        twain.level.set(Level::Enabled);
        Ok(Pending::Done(ready.into_enabled()))
    } else {
        twain.level.set(Level::Ready);
        ready.pending = Some(count);
        Ok(Pending::More(ready))
    }
}

fn ext_image_info<T>(ready: &Ready<'_, T>, ids: &[u16]) -> Result<Vec<ExtInfo>, Error> {
    let twain = ready.twain();
    let (triplet, rc, answers) = ready.dsm().ext_image_info(&twain.app, &twain.source, ids)?;
    if rc != ReturnCode::SUCCESS {
        return Err(ready.failure(triplet, rc));
    }
    Ok(answers.into_iter().map(ext_info::decode).collect())
}

/// A memory transfer: states 6 and 7. The caller's buffers are filled one
/// at a time; the transfer ends when the source says the last is done, or
/// cancels.
pub struct MemoryTransfer<'t, T> {
    /// `None` once stepped down.
    ready: Option<Ready<'t, T>>,
    setup: ffi::TW_SETUPMEMXFER,
    /// How the transfer ended, once it has.
    ended: Option<ReturnCode>,
}

/// One buffer of a memory transfer, as `TW_IMAGEMEMXFER` describes it, its
/// bytes the part of the caller's buffer the source wrote.
#[derive(Debug)]
pub struct Strip<'b> {
    pub compression: u16,
    pub bytes_per_row: u32,
    pub columns: u32,
    pub rows: u32,
    pub x_offset: u32,
    pub y_offset: u32,
    pub bytes: &'b [u8],
}

/// The largest buffer [`MemoryTransfer::preferred_size`] asks for.
const MAX_BUFFER: u32 = 64 << 20;
/// Its size when the source names none.
const DEFAULT_BUFFER: u32 = 64 << 10;

impl<'t, T> MemoryTransfer<'t, T> {
    fn ready(&self) -> &Ready<'t, T> {
        match &self.ready {
            Some(ready) => ready,
            None => unreachable!("a MemoryTransfer without its Ready"),
        }
    }

    /// The source's sizes, as `DAT_SETUPMEMXFER` gave them.
    pub fn setup(&self) -> ffi::TW_SETUPMEMXFER {
        self.setup
    }

    /// A buffer size to use: the source's preferred, else its largest, else
    /// 64 KiB (`TWON_DONTCARE32` meaning none), at least its smallest and at
    /// most 64 MiB.
    pub fn preferred_size(&self) -> usize {
        let named = |n: u32| (n != 0 && n != ffi::TWON_DONTCARE32).then_some(n);
        let setup = self.setup;
        let size = named(setup.Preferred)
            .or(named(setup.MaxBufSize))
            .unwrap_or(DEFAULT_BUFFER)
            .min(MAX_BUFFER);
        size.max(named(setup.MinBufSize).unwrap_or(0)) as usize
    }

    /// The next buffer (`DAT_IMAGEMEMXFER`) into `buffer`, at least the
    /// source's smallest: `None` once the transfer has ended.
    pub fn next_buffer<'b>(&mut self, buffer: &'b mut [u8]) -> Result<Option<Strip<'b>>, Error> {
        if self.ended.is_some() {
            return Ok(None);
        }
        let min = self.setup.MinBufSize;
        if min != ffi::TWON_DONTCARE32 && (buffer.len() as u64) < u64::from(min) {
            return Err(Error::BufferTooSmall {
                len: buffer.len(),
                min,
            });
        }
        let ready = self.ready();
        let twain = ready.twain();
        let (triplet, rc, transfer) = ready.dsm().memory_buffer(&twain.app, &twain.source, buffer);
        match rc {
            ReturnCode::SUCCESS | ReturnCode::XFERDONE | ReturnCode::CANCEL => {
                twain.level.set(Level::Transferring);
            }
            rc => return Err(ready.failure(triplet, rc)),
        }
        if rc != ReturnCode::SUCCESS {
            self.ended = Some(rc);
        }
        if rc == ReturnCode::CANCEL {
            return Ok(None);
        }
        let written = transfer.BytesWritten;
        let bytes = usize::try_from(written)
            .ok()
            .and_then(|n| buffer.get(..n))
            .ok_or(ContainerError::Short {
                len: buffer.len(),
                needed: u64::from(written),
            })?;
        Ok(Some(Strip {
            compression: transfer.Compression,
            bytes_per_row: transfer.BytesPerRow,
            columns: transfer.Columns,
            rows: transfer.Rows,
            x_offset: transfer.XOffset,
            y_offset: transfer.YOffset,
            bytes,
        }))
    }

    /// Whether the source cancelled the transfer (`TWRC_CANCEL`).
    pub fn cancelled(&self) -> bool {
        self.ended == Some(ReturnCode::CANCEL)
    }

    /// The extended image info for the transfer, once its last buffer came.
    pub fn ext_image_info(&self, ids: &[u16]) -> Result<Vec<ExtInfo>, Error> {
        ext_image_info(self.ready(), ids)
    }

    /// Ends the transfer (`MSG_ENDXFER`), done or not: to state 6 if more are
    /// pending, else 5.
    pub fn end(mut self) -> Result<Pending<'t, T>, Refused<Self>> {
        let ready = self.ready.take().expect("a MemoryTransfer has its Ready");
        match end(ready) {
            Ok(pending) => Ok(pending),
            Err((ready, error)) => {
                self.ready = Some(ready);
                Err(Refused { state: self, error })
            }
        }
    }
}

impl<T> Drop for MemoryTransfer<'_, T> {
    fn drop(&mut self) {
        if let Some(ready) = &self.ready {
            let twain = ready.twain();
            twain.dropped(twain.step_down_to(6));
        }
    }
}

impl<T> fmt::Debug for MemoryTransfer<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryTransfer")
            .field("preferred_size", &self.preferred_size())
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl<T> Twain<T> {
    /// A request taken while looking for a source's message, put back first.
    pub(crate) fn requeue(&self, request: T) {
        self.held.borrow_mut().push_front(request);
    }
}
