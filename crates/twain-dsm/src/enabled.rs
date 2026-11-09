//! TWAIN's states 5 to 7: a source enabled, a transfer ready, a transfer
//! done. As in [`states`](crate::states), each owns the state beneath it;
//! here every step down goes through the record in [`Twain`], which knows
//! where the source really is (a source that has said `MSG_XFERREADY` is in
//! state 6 whatever state the caller holds).

use core::fmt;
use std::time::Duration;

use crate::codes::{ConditionCode, ReturnCode, Triplet};
use crate::container::{ContainerError, Fix32};
use crate::dsm::{Dib, Dsm};
use crate::error::{Error, Refused};
use crate::ext_info::{self, ExtInfo};
use crate::ffi;
use crate::states::{Delivery, Source};
use crate::thread::{Level, Pumped, Twain, deadline};
use crate::window;

/// Whether the source shows its own window when it's enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ui {
    /// Scanning with the settings as they are.
    Hidden,
    /// The source's window shown, parented to the hidden window; the user
    /// starts the scan there.
    Shown,
}

/// The state a step down stops at: TWAIN's number, 3 for no source.
fn number<T>(twain: &Twain<T>) -> u8 {
    match twain.source_open.get() {
        None => 3,
        Some(_) => twain.level.get() as u8,
    }
}

/// How a step down treats a refusal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Walk {
    /// A state's `Drop`, and the teardown: a refused step is passed over to
    /// the next, the record moved as if it had been taken (all that can be
    /// done is try the rest), but for `MSG_CLOSEDS`.
    Pass,
    /// `disable` and `reset`: the record moves only on a step taken; the
    /// first refusal ends the walk and is returned, the record as the source
    /// last confirmed it.
    Strict,
}

impl<T> Twain<T> {
    /// The open source stepped down to state `floor` (3: closed), each step
    /// from where the record says it is: `MSG_ENDXFER` (7 to 6, or to 5 when
    /// nothing is pending), `MSG_RESET` (6 to 5), `MSG_DISABLEDS` (5 to 4),
    /// `MSG_CLOSEDS` (4 to 3).
    ///
    /// A step refused with `TWCC_SEQERROR` means the source isn't where the
    /// record says (one that said `MSG_XFERREADY` before the pump saw it is
    /// in state 6, say): the walk starts again higher, from `MSG_RESET`, and
    /// if that's refused so too, from `MSG_ENDXFER`, as TWAIN's
    /// resynchronization does (`MSG_RESET` first, since `MSG_ENDXFER` in state
    /// 6 can make a feeder take another page). Refusals before the floor is
    /// reached are answered only if it isn't, as the specification says of
    /// the steps before the one that reaches the state wanted.
    pub(crate) fn step_down_to(&self, floor: u8, walk: Walk) -> Result<(), Error> {
        let Some(dsm) = self.dsm.get() else {
            return Ok(());
        };
        let mut first: Option<Error> = None;
        let mut restarts = 0;
        // The last state the source confirmed (the record's, to begin with),
        // for a strict walk to go back to if a restart's guess is refused.
        let mut confirmed = self.level.get();
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
                    let (triplet, rc) =
                        dsm.user_interface(&self.app, &self.source, ffi::MSG_DISABLEDS, &self.ui);
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
                        by_source: Some(ConditionCode::SEQERROR),
                        ..
                    }
                );
                let higher = if level < Level::Ready {
                    Some(Level::Ready)
                } else if level < Level::Transferring {
                    Some(Level::Transferring)
                } else {
                    None
                };
                if let (true, true, Some(higher)) = (sequence, restarts < 2, higher) {
                    restarts += 1;
                    first.get_or_insert(error);
                    self.level.set(higher);
                    continue;
                }
                first.get_or_insert(error);
                if walk == Walk::Strict || next.is_none() {
                    if walk == Walk::Strict && restarts > 0 {
                        self.level.set(confirmed);
                    }
                    break;
                }
            }
            match next {
                Some(next) => {
                    self.level.set(next);
                    if rc == ReturnCode::SUCCESS {
                        confirmed = next;
                    }
                    if next <= Level::Open {
                        // Disabled: the messages of that enable are stale.
                        window::new_generation();
                        self.events.borrow_mut().clear();
                    }
                }
                None => {
                    self.source_open.set(None);
                    self.level.set(Level::Open);
                    self.events.borrow_mut().clear();
                }
            }
        }
        if number(self) <= floor {
            return Ok(());
        }
        first.map_or(Ok(()), Err)
    }

    /// The source the record says is open, closed: stepped down from where it
    /// is. The end of `run` does it before closing the connection: the DSM's
    /// own closing of sources left open (`apps.cpp`, `RemoveApp`) stops before
    /// the last source it lists, so it never closes a source installed alone.
    pub(crate) fn close_source(&self) -> Result<(), Error> {
        self.step_down_to(3, Walk::Pass)
    }

    /// The source's messages taken (no waiting), then whether it's still
    /// where a state of `level` needs it: a `MSG_CLOSEDSREQ` in state 6 or 7
    /// moves it to 5, and the transfer can't go on.
    fn still_at(&self, level: Level) -> Result<(), Error> {
        self.poll_source();
        if self.level.get() < level {
            return Err(Error::CloseRequested);
        }
        Ok(())
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
        // Whatever an earlier enable left unread is stale: a new generation
        // for the callback, the queue emptied, and for delivery by events the
        // null procedure registered again, which drops a message the DSM still
        // holds for the source.
        window::new_generation();
        twain.events.borrow_mut().clear();
        if self.delivery() == Delivery::Events {
            let _ = dsm.register_callback(&twain.app, &twain.source, None);
        }
        twain.ui.set(ffi::TW_USERINTERFACE {
            ShowUI: u16::from(show),
            ModalUI: 0,
            hParent: twain.window.hwnd(),
        });
        let (triplet, rc) = dsm.user_interface(&twain.app, &twain.source, msg, &twain.ui);
        // TWRC_CHECKSTATUS: enabled, the UI shown though not asked for (a
        // source that can't hide it).
        if rc != ReturnCode::SUCCESS && rc != ReturnCode::CHECKSTATUS {
            let error = twain.source_failure(dsm, &twain.source, triplet, rc);
            return Err(Refused { state: self, error });
        }
        twain.forced_ui.set(rc == ReturnCode::CHECKSTATUS);
        // A message the callback kept inside MSG_ENABLEDS is still unread, so
        // the record moves to 5 first and the pump takes it to 6.
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
    /// `MSG_CLOSEDSREQ`: the user closed the source's window, or cancelled, or
    /// the source failed; the application disables the source.
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

    /// Whether the source showed its window though asked not to
    /// (`MSG_ENABLEDS` answered `TWRC_CHECKSTATUS`).
    pub fn forced_ui(&self) -> bool {
        self.twain().forced_ui.get()
    }

    /// Whether the source's window is modal (`TW_USERINTERFACE`'s `ModalUI`,
    /// as the source left it).
    pub fn modal(&self) -> bool {
        self.twain().ui.get().ModalUI != 0
    }

    /// The source's next message, or a request, pumping the thread's messages
    /// meanwhile, until `timeout` (one too long to add to now: none). The
    /// source's come first: the messages queued are all taken before a
    /// request is handed over.
    pub fn next_event(self, timeout: Duration) -> Result<Next<'t, T>, Refused<Self>> {
        let twain = self.twain();
        let mut request = None;
        let pumped = twain.pump(deadline(timeout), |twain| {
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

    /// Disables the source (`MSG_DISABLEDS`, state 5 to 4; first `MSG_RESET`
    /// if it has said a transfer is ready, which this `Enabled` hasn't handed
    /// over). Refused, the record stays as the source last confirmed it.
    pub fn disable(mut self) -> Result<Source<'t, T>, Refused<Self>> {
        let twain = self.twain();
        twain.poll_source();
        match twain.step_down_to(4, Walk::Strict) {
            Ok(()) => Ok(self.source.take().expect("an Enabled has its Source")),
            Err(error) => Err(Refused { state: self, error }),
        }
    }
}

impl<T> Drop for Enabled<'_, T> {
    fn drop(&mut self) {
        if let Some(source) = &self.source {
            let twain = source.twain();
            twain.dropped(twain.step_down_to(4, Walk::Pass));
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

/// `DAT_IMAGEINFO`'s answer: the image as the source describes it, as TWAIN
/// gives it, in fields that can be borrowed (`TW_IMAGEINFO` is packed). Only
/// the first of `bits_per_sample` is to be trusted: some sources set no
/// other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    pub x_resolution: Fix32,
    pub y_resolution: Fix32,
    /// Columns; -1 if not known (a memory transfer's, before its end).
    pub width: i32,
    /// Rows; -1 if not known (the length a feeder measures as it scans).
    pub length: i32,
    pub samples_per_pixel: i16,
    pub bits_per_sample: [i16; 8],
    pub bits_per_pixel: i16,
    pub planar: bool,
    /// `TWPT_`.
    pub pixel_type: i16,
    /// `TWCP_`.
    pub compression: u16,
}

impl From<ffi::TW_IMAGEINFO> for ImageInfo {
    fn from(info: ffi::TW_IMAGEINFO) -> Self {
        ImageInfo {
            x_resolution: info.XResolution.into(),
            y_resolution: info.YResolution.into(),
            width: info.ImageWidth,
            length: info.ImageLength,
            samples_per_pixel: info.SamplesPerPixel,
            bits_per_sample: info.BitsPerSample,
            bits_per_pixel: info.BitsPerPixel,
            planar: info.Planar != 0,
            pixel_type: info.PixelType,
            compression: info.Compression,
        }
    }
}

/// `DAT_IMAGEINFO/MSG_GET`.
fn image_info<T>(twain: &Twain<T>, dsm: &Dsm) -> Result<ImageInfo, Error> {
    let (triplet, rc, info) = dsm.image_info(&twain.app, &twain.source);
    if rc != ReturnCode::SUCCESS {
        return Err(twain.source_failure(dsm, &twain.source, triplet, rc));
    }
    Ok(info.into())
}

/// State 6: a transfer ready.
pub struct Ready<'t, T> {
    /// `None` once stepped down.
    enabled: Option<Enabled<'t, T>>,
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

    /// Whether the source has asked to be closed (`MSG_CLOSEDSREQ`: the user
    /// cancelled on its window, or it failed): then it's back in state 5, and
    /// a transfer is refused with [`Error::CloseRequested`].
    pub fn close_requested(&self) -> bool {
        self.twain().still_at(Level::Ready).is_err()
    }

    /// The image to come, as `DAT_IMAGEINFO` describes it.
    pub fn image_info(&self) -> Result<ImageInfo, Error> {
        self.twain().still_at(Level::Ready)?;
        image_info(self.twain(), self.dsm())
    }

    /// A native transfer (`DAT_IMAGENATIVEXFER`, state 6 to 7): the DIB, or
    /// the source's cancel.
    pub fn native(self) -> Result<Native<'t, T>, Refused<Self>> {
        let twain = self.twain();
        if let Err(error) = twain.still_at(Level::Ready) {
            return Err(Refused { state: self, error });
        }
        let (triplet, rc, dib) = self.dsm().native(&twain.app, &twain.source);
        match rc {
            ReturnCode::XFERDONE | ReturnCode::CANCEL => {
                twain.level.set(Level::Transferring);
                let done = rc == ReturnCode::XFERDONE;
                let transferred = Transferred {
                    ready: Some(self),
                    done,
                };
                Ok(match (done, dib) {
                    (true, Some(dib)) => Native::Done(dib, transferred),
                    (true, None) => Native::Done(Dib::empty(), transferred),
                    (false, _) => Native::Cancelled(transferred),
                })
            }
            rc => {
                let error = self.failure(triplet, rc);
                Err(Refused { state: self, error })
            }
        }
    }

    /// A memory transfer begun: the source's buffer sizes
    /// (`DAT_SETUPMEMXFER`), refused if no buffer can meet them (its smallest
    /// past 64 MiB, or larger than its largest); the buffers come from
    /// [`MemoryTransfer::next_buffer`].
    pub fn memory(self) -> Result<MemoryTransfer<'t, T>, Refused<Self>> {
        let twain = self.twain();
        if let Err(error) = twain.still_at(Level::Ready) {
            return Err(Refused { state: self, error });
        }
        let (triplet, rc, setup) = self.dsm().setup_memory(&twain.app, &twain.source);
        if rc != ReturnCode::SUCCESS {
            let error = self.failure(triplet, rc);
            return Err(Refused { state: self, error });
        }
        let (min, max) = (named(setup.MinBufSize), named(setup.MaxBufSize));
        let unusable = min.is_some_and(|min| min > MAX_BUFFER || max.is_some_and(|max| max < min));
        if unusable {
            let error = Error::BufferSizes {
                min: setup.MinBufSize,
                max: setup.MaxBufSize,
                preferred: setup.Preferred,
            };
            return Err(Refused { state: self, error });
        }
        Ok(MemoryTransfer {
            ready: Some(self),
            setup,
            first_len: None,
            ended: None,
        })
    }

    /// The transfers pending dropped (`MSG_RESET`, state 6 to 5): a scan
    /// cancelled between pages. Refused, the record stays at 6.
    pub fn reset(mut self) -> Result<Enabled<'t, T>, Refused<Self>> {
        match self.twain().step_down_to(5, Walk::Strict) {
            Ok(()) => Ok(self.enabled.take().expect("a Ready has its Enabled")),
            Err(error) => Err(Refused { state: self, error }),
        }
    }

    /// The feeder stopped after the pages it has taken (`MSG_STOPFEEDER`,
    /// staying in state 6): those still come, and `MSG_ENDXFER`'s count says
    /// how many.
    pub fn stop_feeder(&mut self) -> Result<(), Error> {
        let twain = self.twain();
        twain.still_at(Level::Ready)?;
        let (triplet, rc, _) =
            self.dsm()
                .pending_transfers(&twain.app, &twain.source, ffi::MSG_STOPFEEDER);
        if rc != ReturnCode::SUCCESS {
            return Err(self.failure(triplet, rc));
        }
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
            twain.dropped(twain.step_down_to(5, Walk::Pass));
        }
    }
}

impl<T> fmt::Debug for Ready<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ready").finish_non_exhaustive()
    }
}

/// A native transfer's end.
#[derive(Debug)]
pub enum Native<'t, T> {
    /// The source says it's done: the image, as a DIB. The `Dib` is empty
    /// if the source gave no block, or the block couldn't be locked.
    Done(Dib, Transferred<'t, T>),
    /// The source cancelled the transfer (`TWRC_CANCEL`): no image.
    Cancelled(Transferred<'t, T>),
}

/// State 7: a transfer done (or cancelled by the source), to be ended.
pub struct Transferred<'t, T> {
    /// `None` once stepped down.
    ready: Option<Ready<'t, T>>,
    /// Whether it ended with `TWRC_XFERDONE`.
    done: bool,
}

/// What `end` leads to.
pub enum Pending<'t, T> {
    /// Another transfer ready (state 6), and how many the source says are
    /// pending: `None` for 0xFFFF, "some, how many unknown".
    More(Ready<'t, T>, Option<u16>),
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

    /// Whether the source has asked to be closed: see
    /// [`Ready::close_requested`].
    pub fn close_requested(&self) -> bool {
        self.ready().twain().still_at(Level::Transferring).is_err()
    }

    /// The image as transferred (`DAT_IMAGEINFO` in state 7), once the
    /// transfer ended with `TWRC_XFERDONE`.
    pub fn image_info(&self) -> Result<ImageInfo, Error> {
        let ready = self.ready();
        if !self.done {
            return Err(Error::TransferNotDone);
        }
        ready.twain().still_at(Level::Transferring)?;
        image_info(ready.twain(), ready.dsm())
    }

    /// The extended image info the source has for the transfer
    /// (`DAT_EXTIMAGEINFO`), for each `TWEI_` in `ids`: once the transfer
    /// ended with `TWRC_XFERDONE`, as TWAIN allows it only then.
    pub fn ext_image_info(&self, ids: &[u16]) -> Result<Vec<ExtInfo>, Error> {
        if !self.done {
            return Err(Error::TransferNotDone);
        }
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
            twain.dropped(twain.step_down_to(6, Walk::Pass));
        }
    }
}

impl<T> fmt::Debug for Transferred<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transferred")
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

/// `MSG_ENDXFER` from state 6 or 7, the record updated; the Ready handed back
/// on failure.
fn end<'t, T>(ready: Ready<'t, T>) -> Result<Pending<'t, T>, (Ready<'t, T>, Error)> {
    let twain = ready.twain();
    if let Err(error) = twain.still_at(Level::Ready) {
        return Err((ready, error));
    }
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
        let pending = (count != ffi::TWON_DONTCARE16).then_some(count);
        Ok(Pending::More(ready, pending))
    }
}

fn ext_image_info<T>(ready: &Ready<'_, T>, ids: &[u16]) -> Result<Vec<ExtInfo>, Error> {
    let twain = ready.twain();
    twain.still_at(Level::Transferring)?;
    let (triplet, rc, answers) = ready.dsm().ext_image_info(&twain.app, &twain.source, ids)?;
    if rc != ReturnCode::SUCCESS {
        return Err(ready.failure(triplet, rc));
    }
    Ok(answers.into_iter().map(ext_info::decode).collect())
}

/// A size `TW_SETUPMEMXFER` names: not 0 or `TWON_DONTCARE32`.
fn named(n: u32) -> Option<u32> {
    (n != 0 && n != ffi::TWON_DONTCARE32).then_some(n)
}

/// A memory transfer: states 6 and 7. The caller's buffers are filled one
/// at a time; the transfer ends when the source says the last is done, or
/// cancels.
pub struct MemoryTransfer<'t, T> {
    /// `None` once stepped down.
    ready: Option<Ready<'t, T>>,
    setup: ffi::TW_SETUPMEMXFER,
    /// The first buffer's length: every buffer must be the same (a buffer's
    /// length is a `TW_UINT32`).
    first_len: Option<u32>,
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
    /// 64 KiB (`TWON_DONTCARE32` meaning none), at most its largest and 64
    /// MiB, at least its smallest, and a multiple of 4.
    pub fn preferred_size(&self) -> usize {
        let setup = self.setup;
        let (min, max) = (named(setup.MinBufSize), named(setup.MaxBufSize));
        let size = named(setup.Preferred)
            .or(max)
            .unwrap_or(DEFAULT_BUFFER)
            .min(max.unwrap_or(MAX_BUFFER))
            .min(MAX_BUFFER)
            .max(min.unwrap_or(0));
        // `memory` refused a smallest past 64 MiB, so this stays at most 64
        // MiB.
        let rounded = size & !3;
        (if rounded < min.unwrap_or(0) {
            rounded + 4
        } else {
            rounded
        })
        .max(4) as usize
    }

    /// The next buffer (`DAT_IMAGEMEMXFER`) into `buffer`: `None` once the
    /// transfer has ended. TWAIN's rules for the buffer are checked first:
    /// at least the source's smallest and at most its largest, an even
    /// length at a 4-byte boundary, and the same length every time.
    pub fn next_buffer<'b>(&mut self, buffer: &'b mut [u8]) -> Result<Option<Strip<'b>>, Error> {
        if self.ended.is_some() {
            return Ok(None);
        }
        let len = buffer.len();
        let (min, max) = (named(self.setup.MinBufSize), named(self.setup.MaxBufSize));
        if let Some(min) = min.filter(|&min| (len as u64) < u64::from(min)) {
            return Err(Error::BufferTooSmall { len, min });
        }
        if let Some(max) = max.filter(|&max| (len as u64) > u64::from(max)) {
            return Err(Error::BufferTooLarge { len, max });
        }
        if !len.is_multiple_of(2) || !(buffer.as_ptr() as usize).is_multiple_of(4) {
            return Err(Error::BufferMisaligned { len });
        }
        let len32 = u32::try_from(len).unwrap_or(u32::MAX);
        if let Some(first) = self.first_len.filter(|&first| first != len32) {
            return Err(Error::BufferSizeChanged {
                len,
                first: first as usize,
            });
        }
        let ready = self.ready();
        let twain = ready.twain();
        twain.still_at(Level::Ready)?;
        let (triplet, rc, transfer) = ready.dsm().memory_buffer(&twain.app, &twain.source, buffer);
        match rc {
            ReturnCode::SUCCESS | ReturnCode::XFERDONE | ReturnCode::CANCEL => {
                twain.level.set(Level::Transferring);
            }
            rc => return Err(ready.failure(triplet, rc)),
        }
        self.first_len = Some(len32);
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
                len,
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

    /// Whether the source has asked to be closed: see
    /// [`Ready::close_requested`].
    pub fn close_requested(&self) -> bool {
        self.ready().twain().still_at(Level::Ready).is_err()
    }

    fn done(&self) -> Result<(), Error> {
        if self.ended != Some(ReturnCode::XFERDONE) {
            return Err(Error::TransferNotDone);
        }
        Ok(())
    }

    /// The image as transferred (`DAT_IMAGEINFO` in state 7), once the last
    /// buffer came (`TWRC_XFERDONE`): the length a feeder measured, for one
    /// whose `image_info` before the transfer said -1.
    pub fn image_info(&self) -> Result<ImageInfo, Error> {
        self.done()?;
        let ready = self.ready();
        ready.twain().still_at(Level::Transferring)?;
        image_info(ready.twain(), ready.dsm())
    }

    /// The extended image info for the transfer, once its last buffer came
    /// (`TWRC_XFERDONE`), as TWAIN allows it only then.
    pub fn ext_image_info(&self, ids: &[u16]) -> Result<Vec<ExtInfo>, Error> {
        self.done()?;
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
            twain.dropped(twain.step_down_to(6, Walk::Pass));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::Item;
    use crate::dsm::sent_log;
    use crate::states::{App, Load};
    use crate::test_support::{self, Outcome};

    /// `reset` and `disable` each send their own triplet, and only it: the
    /// record isn't moved for them by a step that wasn't sent.
    #[test]
    fn reset_and_disable_send_their_triplets() {
        test_support::fork(
            rusty_fork::rusty_fork_test_name!(reset_and_disable_send_their_triplets),
            rusty_fork::rusty_fork_id!(),
            || {
                crate::run::<(), _>(|twain| {
                    let loaded = match twain.load(Load::System) {
                        Ok(loaded) => loaded,
                        Err(crate::LoadError::NotFound { .. }) => {
                            return Outcome::Skip("no DSM installed".into());
                        }
                        Err(e) => panic!("the system's DSM: {e}"),
                    };
                    let app = App::new("pdf-scan", "twain-dsm tests", "unit").expect("fits");
                    let opened = loaded
                        .open(app)
                        .map_err(Refused::into_error)
                        .expect("opens");
                    let sources = opened.sources().expect("listed");
                    let Some(sample) = sources
                        .iter()
                        .find(|s| s.product_name() == "TWAIN2 Software Scanner")
                    else {
                        return Outcome::Skip("the sample source isn't installed".into());
                    };
                    let source = opened
                        .open_source(sample, Delivery::Callback)
                        .map_err(Refused::into_error)
                        .expect("opens");
                    source
                        .set(ffi::CAP_XFERCOUNT, Item::Int16(-1))
                        .expect("set");
                    let mut enabled = source
                        .enable(Ui::Hidden)
                        .map_err(Refused::into_error)
                        .expect("enables");
                    let ready = loop {
                        match enabled
                            .next_event(Duration::from_secs(20))
                            .map_err(Refused::into_error)
                            .expect("next")
                        {
                            Next::Ready(ready) => break ready,
                            Next::Timeout(_) => panic!("no MSG_XFERREADY"),
                            Next::Event(back, _) | Next::Request(back, _) | Next::Quit(back, _) => {
                                enabled = back
                            }
                        }
                    };
                    let reset =
                        Triplet::new(ffi::DG_CONTROL, ffi::DAT_PENDINGXFERS, ffi::MSG_RESET);
                    let disable =
                        Triplet::new(ffi::DG_CONTROL, ffi::DAT_USERINTERFACE, ffi::MSG_DISABLEDS);
                    sent_log::take();
                    let enabled = ready
                        .reset()
                        .map_err(Refused::into_error)
                        .expect("MSG_RESET");
                    assert_eq!(sent_log::take(), vec![(reset, ReturnCode::SUCCESS)]);
                    assert_eq!(twain.level.get(), Level::Enabled);
                    let source = enabled
                        .disable()
                        .map_err(Refused::into_error)
                        .expect("MSG_DISABLEDS");
                    assert_eq!(sent_log::take(), vec![(disable, ReturnCode::SUCCESS)]);
                    assert_eq!(twain.level.get(), Level::Open);
                    drop(source);
                    Outcome::Done
                })
                .expect("TWAIN runs")
                .value
            },
        );
    }
}
