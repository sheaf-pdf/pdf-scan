//! TWAIN's states as types, from 2 (the DSM loaded) up. Each owns the one
//! beneath it, steps up by consuming itself, hands itself back in a
//! [`Refused`] when a step fails, and steps down when dropped. The record in
//! [`Twain`] is what teardown trusts, not these: a state forgotten is still
//! closed at the end of [`run`](crate::run).

use core::fmt;
use std::path::{Path, PathBuf};

use crate::codes::{ConditionCode, ReturnCode, Triplet};
use crate::container::{Container, ContainerError, Frame, ItemType};
use crate::dsm::{self, Dsm};
use crate::error::{Error, LoadError, Refused};
use crate::ffi::{self, TW_IDENTITY, TW_STR32};
use crate::thread::Twain;
use crate::window;

/// Which DSM to load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Load {
    /// `TWAINDSM.dll` in the system folder: SysWOW64 in a 32-bit process.
    System,
    /// The application's own copy, by its path.
    Path(PathBuf),
}

impl<T> Twain<T> {
    /// A triplet the DSM refused itself: its code.
    fn dsm_failure(&self, dsm: &Dsm, triplet: Triplet, rc: ReturnCode) -> Error {
        Error::Failed {
            triplet,
            rc,
            by_source: None,
            by_dsm: dsm.dsm_condition(&self.app),
        }
    }

    /// A triplet to `dest` that failed: the DSM's code first (it refuses some
    /// itself, a source it doesn't have open among them), then the source's,
    /// if `dest` is the source open.
    pub(crate) fn source_failure(
        &self,
        dsm: &Dsm,
        dest: &core::cell::Cell<TW_IDENTITY>,
        triplet: Triplet,
        rc: ReturnCode,
    ) -> Error {
        let by_dsm = dsm.dsm_condition(&self.app);
        let open = self.source_open.get().is_some() && core::ptr::eq(dest, &*self.source);
        let by_source = if open {
            dsm.status(&self.app, Some(dest))
        } else {
            None
        };
        Error::Failed {
            triplet,
            rc,
            by_source,
            by_dsm,
        }
    }

    /// Loads the DSM (state 1 to 2): its file held against change, its
    /// machine, Authenticode signature and signer (the TWAIN Working Group)
    /// checked before any of its code runs, then loaded by its full path, and
    /// the module loaded compared with the file checked. Once loaded it stays
    /// so for the process's life; a later `load` of the same file, in this
    /// `run` or another, finds it, and one of another file is
    /// [`LoadError::Elsewhere`]. One [`Loaded`] at a time.
    pub fn load(&self, which: Load) -> Result<Loaded<'_, T>, LoadError> {
        if self.loaded.get() || self.connected.get() {
            return Err(LoadError::AlreadyLoaded);
        }
        let path = match which {
            Load::System => dsm::system_dsm(),
            Load::Path(path) => path,
        };
        let dsm = dsm::load(&path)?;
        self.dsm.set(Some(dsm));
        self.loaded.set(true);
        Ok(Loaded { twain: self, dsm })
    }
}

/// State 2: the DSM loaded.
pub struct Loaded<'t, T> {
    twain: &'t Twain<T>,
    dsm: &'static Dsm,
}

impl<'t, T> Loaded<'t, T> {
    /// The DSM's file.
    pub fn path(&self) -> &Path {
        self.dsm.path()
    }

    /// Opens the connection (`MSG_OPENDSM`, state 2 to 3) as `app`, the
    /// hidden window its parent, and takes the DSM's memory functions
    /// (`DAT_ENTRYPOINT`). The DSM loads every installed source's library to
    /// list it, running each one's code, and changes the process's current
    /// directory to each one's folder as it does: a relative path another
    /// thread resolves meanwhile may resolve elsewhere. Refused
    /// ([`Error::StillOpen`]) while a connection the DSM wouldn't close is
    /// open.
    pub fn open(self, app: App) -> Result<Opened<'t, T>, Refused<Self>> {
        let twain = self.twain;
        if twain.connected.get() {
            return Err(Refused {
                state: self,
                error: Error::StillOpen,
            });
        }
        twain.app.set(app.identity);
        let (triplet, rc) = self.dsm.parent(&twain.app, true, twain.window.hwnd());
        if rc != ReturnCode::SUCCESS {
            let error = self.dsm_failure(triplet, rc);
            return Err(Refused { state: self, error });
        }
        twain.connected.set(true);
        let opened = Opened { loaded: Some(self) };
        let (triplet, rc) = opened.dsm().entry_points(&twain.app);
        if rc != ReturnCode::SUCCESS {
            // A DSM 2 always answers; without its memory functions the
            // containers couldn't be allocated as it frees them.
            let error = opened.loaded().dsm_failure(triplet, rc);
            return match opened.close() {
                Ok(loaded) => Err(Refused {
                    state: loaded,
                    error,
                }),
                Err(refused) => Err(Refused {
                    state: refused.into_state().into_loaded_unclosed(),
                    error,
                }),
            };
        }
        Ok(opened)
    }

    fn dsm_failure(&self, triplet: Triplet, rc: ReturnCode) -> Error {
        self.twain.dsm_failure(self.dsm, triplet, rc)
    }
}

impl<T> Drop for Loaded<'_, T> {
    fn drop(&mut self) {
        // The DSM stays loaded; this TWAIN thread may load it again.
        self.twain.loaded.set(false);
    }
}

impl<T> fmt::Debug for Loaded<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Loaded")
            .field("path", &self.path())
            .finish()
    }
}

/// State 3: the connection to the DSM open.
pub struct Opened<'t, T> {
    /// `None` once stepped down, so `Drop` doesn't do it again.
    loaded: Option<Loaded<'t, T>>,
}

impl<'t, T> Opened<'t, T> {
    fn loaded(&self) -> &Loaded<'t, T> {
        match &self.loaded {
            Some(loaded) => loaded,
            // Only `close` and `into_loaded_unclosed` take it, consuming self.
            None => unreachable!("an Opened without its Loaded"),
        }
    }

    pub(crate) fn twain(&self) -> &'t Twain<T> {
        self.loaded().twain
    }

    pub(crate) fn dsm(&self) -> &'static Dsm {
        self.loaded().dsm
    }

    /// The application's identity as the DSM filled it in: its `Id` the
    /// DSM's.
    pub fn app(&self) -> TW_IDENTITY {
        self.twain().app.get()
    }

    /// The sources the DSM finds (`MSG_GETFIRST`, then `MSG_GETNEXT` to the
    /// end of the list).
    pub fn sources(&self) -> Result<Vec<SourceId>, Error> {
        let mut sources = Vec::new();
        let mut msg = ffi::MSG_GETFIRST;
        loop {
            let mut identity = TW_IDENTITY::default();
            let (triplet, rc) = self
                .dsm()
                .source_identity(&self.twain().app, msg, &mut identity);
            match rc {
                ReturnCode::SUCCESS => sources.push(SourceId { identity }),
                ReturnCode::ENDOFLIST => return Ok(sources),
                rc => return Err(self.loaded().dsm_failure(triplet, rc)),
            }
            msg = ffi::MSG_GETNEXT;
        }
    }

    /// The source the DSM would open by default (`MSG_GETDEFAULT`): `None`
    /// if there's none.
    pub fn default_source(&self) -> Result<Option<SourceId>, Error> {
        let mut identity = TW_IDENTITY::default();
        let (triplet, rc) =
            self.dsm()
                .source_identity(&self.twain().app, ffi::MSG_GETDEFAULT, &mut identity);
        match rc {
            ReturnCode::SUCCESS => Ok(Some(SourceId { identity })),
            rc => match self.loaded().dsm_failure(triplet, rc) {
                Error::Failed {
                    by_dsm: Some(crate::ConditionCode::NODS),
                    ..
                } => Ok(None),
                error => Err(error),
            },
        }
    }

    /// Opens `source` (`MSG_OPENDS`, state 3 to 4), its messages to come by
    /// `delivery`, which stays the source's until it's closed: TWAIN has no
    /// way to take a callback back, and the DSM keeps one across closing and
    /// opening the source again, so [`Delivery::Events`] registers none to
    /// clear any. [`Delivery::Callback`] is refused for a source of TWAIN 1.x,
    /// before it's opened; so is any source ([`Error::StillOpen`]) while one
    /// the DSM wouldn't close is open. The DSM changes the process's current
    /// directory to the source's folder as it loads it.
    pub fn open_source(
        self,
        source: &SourceId,
        delivery: Delivery,
    ) -> Result<Source<'t, T>, Refused<Self>> {
        if let Err(error) = check_delivery(source, delivery) {
            return Err(Refused { state: self, error });
        }
        if self.twain().source_open.get().is_some() {
            return Err(Refused {
                state: self,
                error: Error::StillOpen,
            });
        }
        let twain = self.twain();
        let dsm = self.dsm();
        twain.source.set(source.identity);
        let (triplet, rc) = dsm.source(&twain.app, &twain.source, true);
        if rc != ReturnCode::SUCCESS {
            let error = twain.dsm_failure(dsm, triplet, rc);
            return Err(Refused { state: self, error });
        }
        twain.source_open.set(Some(delivery));
        let opened = Source {
            opened: Some(self),
            delivery,
        };
        let proc = match delivery {
            Delivery::Events => None,
            Delivery::Callback => Some(window::source_callback as crate::ffi::TwainCallbackProc),
        };
        let (triplet, rc) = dsm.register_callback(&twain.app, &twain.source, proc);
        if rc != ReturnCode::SUCCESS {
            let error = twain.source_failure(dsm, &twain.source, triplet, rc);
            return match opened.close() {
                Ok(opened) => Err(Refused {
                    state: opened,
                    error,
                }),
                Err(refused) => Err(Refused {
                    state: refused.into_state().into_opened_unclosed(),
                    error,
                }),
            };
        }
        Ok(opened)
    }

    /// Closes the connection (`MSG_CLOSEDSM`, state 3 to 2).
    pub fn close(self) -> Result<Loaded<'t, T>, Refused<Self>> {
        match self.twain().close_connection() {
            Ok(()) => Ok(self.into_loaded_unclosed()),
            Err(error) => Err(Refused { state: self, error }),
        }
    }

    /// The `Loaded` beneath, the connection left as the record has it.
    fn into_loaded_unclosed(mut self) -> Loaded<'t, T> {
        match self.loaded.take() {
            Some(loaded) => loaded,
            None => unreachable!("an Opened without its Loaded"),
        }
    }
}

impl<T> Drop for Opened<'_, T> {
    fn drop(&mut self) {
        if let Some(loaded) = &self.loaded {
            // If the DSM refuses, the record keeps the connection open, and
            // the end of `run` tries again.
            loaded.twain.dropped(loaded.twain.close_connection());
        }
    }
}

impl<T> fmt::Debug for Opened<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let id = self.app().Id;
        f.debug_struct("Opened").field("app_id", &id).finish()
    }
}

/// How an open source's messages come (`MSG_XFERREADY`, `MSG_CLOSEDSREQ` and
/// the rest).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Through the window's messages: each one the loop takes, while the
    /// source is enabled, is offered to it (`MSG_PROCESSEVENT`). Every source
    /// can.
    Events,
    /// Through a callback the DSM calls (`DAT_CALLBACK2`): TWAIN 2 sources
    /// only.
    Callback,
}

/// Refused unless the source can deliver so.
pub(crate) fn check_delivery(source: &SourceId, delivery: Delivery) -> Result<(), Error> {
    if delivery == Delivery::Callback && !source.is_twain2() {
        return Err(Error::CallbackUnsupported);
    }
    Ok(())
}

/// What a capability's `MSG_SET` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOutcome {
    /// Set as asked (`TWRC_SUCCESS`).
    Exact,
    /// Set to the nearest the source could (`TWRC_CHECKSTATUS`): its current
    /// value says what.
    Adjusted,
}

/// `DAT_IMAGELAYOUT`'s answer: the image's frame in `ICAP_UNITS`, and its
/// document, page and frame numbers in the batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ImageLayout {
    /// The rectangle to transfer, in `ICAP_UNITS`.
    pub frame: Frame,
    /// The document's number in the batch.
    pub document_number: u32,
    /// The page's number in the document.
    pub page_number: u32,
    /// The frame's number on the page.
    pub frame_number: u32,
}

impl From<ffi::TW_IMAGELAYOUT> for ImageLayout {
    fn from(layout: ffi::TW_IMAGELAYOUT) -> Self {
        ImageLayout {
            frame: layout.Frame.into(),
            document_number: layout.DocumentNumber,
            page_number: layout.PageNumber,
            frame_number: layout.FrameNumber,
        }
    }
}

/// `DAT_IMAGELAYOUT` read (`MSG_GET`, `MSG_GETDEFAULT`, `MSG_RESET`).
pub(crate) fn image_layout_read<T>(
    twain: &Twain<T>,
    dsm: &Dsm,
    msg: u16,
) -> Result<ImageLayout, Error> {
    let (triplet, rc, layout) = dsm.image_layout_read(&twain.app, &twain.source, msg);
    if rc != ReturnCode::SUCCESS {
        return Err(twain.source_failure(dsm, &twain.source, triplet, rc));
    }
    Ok(layout.into())
}

/// State 4: a source open.
pub struct Source<'t, T> {
    /// `None` once stepped down, so `Drop` doesn't do it again.
    opened: Option<Opened<'t, T>>,
    delivery: Delivery,
}

impl<'t, T> Source<'t, T> {
    fn below(&self) -> &Opened<'t, T> {
        match &self.opened {
            Some(opened) => opened,
            // Only `close` and `into_opened_unclosed` take it, consuming self.
            None => unreachable!("a Source without its Opened"),
        }
    }

    pub(crate) fn twain(&self) -> &'t Twain<T> {
        self.below().twain()
    }

    pub(crate) fn dsm(&self) -> &'static Dsm {
        self.below().dsm()
    }

    /// The connection beneath: to list the sources (`Opened::sources`) with
    /// this one open.
    pub fn opened(&self) -> &Opened<'t, T> {
        self.below()
    }

    /// The source's identity as the DSM filled it in when it opened it.
    pub fn id(&self) -> SourceId {
        SourceId {
            identity: self.twain().source.get(),
        }
    }

    /// How its messages come.
    pub fn delivery(&self) -> Delivery {
        self.delivery
    }

    /// The capability's values (`MSG_GET`): its current one and the others it
    /// may take, in whichever container the source chooses.
    pub fn get(&self, cap: u16) -> Result<Container, Error> {
        capability_read(
            self.twain(),
            self.dsm(),
            &self.twain().source,
            ffi::MSG_GET,
            cap,
        )
    }

    /// Its current value (`MSG_GETCURRENT`).
    pub fn get_current(&self, cap: u16) -> Result<Container, Error> {
        capability_read(
            self.twain(),
            self.dsm(),
            &self.twain().source,
            ffi::MSG_GETCURRENT,
            cap,
        )
    }

    /// Its default (`MSG_GETDEFAULT`).
    pub fn get_default(&self, cap: u16) -> Result<Container, Error> {
        capability_read(
            self.twain(),
            self.dsm(),
            &self.twain().source,
            ffi::MSG_GETDEFAULT,
            cap,
        )
    }

    /// Reset to its default (`MSG_RESET`), which comes back.
    pub fn reset(&self, cap: u16) -> Result<Container, Error> {
        capability_read(
            self.twain(),
            self.dsm(),
            &self.twain().source,
            ffi::MSG_RESET,
            cap,
        )
    }

    /// Which messages the source takes for it (`MSG_QUERYSUPPORT`): `TWQC_`
    /// bits.
    pub fn query_support(&self, cap: u16) -> Result<u32, Error> {
        let container = capability_read(
            self.twain(),
            self.dsm(),
            &self.twain().source,
            ffi::MSG_QUERYSUPPORT,
            cap,
        )?;
        match container {
            Container::One(item) => match item.as_i64() {
                Some(bits) => Ok(bits as u32),
                None => Err(ContainerError::MixedTypes {
                    expected: ItemType::Int32,
                    found: item.item_type(),
                }
                .into()),
            },
            other => Err(ContainerError::NotOneValue(other.con_type()).into()),
        }
    }

    /// Sets it (`MSG_SET`): a value, or a container to constrain it to.
    pub fn set(&self, cap: u16, value: impl Into<Container>) -> Result<SetOutcome, Error> {
        let container = value.into();
        let bytes = container.encode()?;
        let twain = self.twain();
        let dsm = self.dsm();
        let Some((triplet, rc)) = dsm.capability_write(
            &twain.app,
            &twain.source,
            ffi::MSG_SET,
            cap,
            container.con_type(),
            &bytes,
        ) else {
            return Err(Error::Allocation { size: bytes.len() });
        };
        match rc {
            ReturnCode::SUCCESS => Ok(SetOutcome::Exact),
            ReturnCode::CHECKSTATUS => Ok(SetOutcome::Adjusted),
            rc => Err(twain.source_failure(dsm, &twain.source, triplet, rc)),
        }
    }

    /// The source's current condition code (`DG_CONTROL/DAT_STATUS/MSG_GET`,
    /// which clears it): `None` if it has none (`TWCC_SUCCESS`) or couldn't
    /// answer.
    pub fn status(&self) -> Option<ConditionCode> {
        self.dsm()
            .status(&self.twain().app, Some(&self.twain().source))
    }

    /// The image layout (`DG_IMAGE/DAT_IMAGELAYOUT/MSG_GET`), in the source's
    /// active `ICAP_UNITS`.
    pub fn image_layout(&self) -> Result<ImageLayout, Error> {
        image_layout_read(self.twain(), self.dsm(), ffi::MSG_GET)
    }

    /// The source's default image layout (`DG_IMAGE/DAT_IMAGELAYOUT/MSG_GETDEFAULT`).
    pub fn default_image_layout(&self) -> Result<ImageLayout, Error> {
        image_layout_read(self.twain(), self.dsm(), ffi::MSG_GETDEFAULT)
    }

    /// Resets the image layout to the source's default
    /// (`DG_IMAGE/DAT_IMAGELAYOUT/MSG_RESET`), which comes back.
    pub fn reset_image_layout(&self) -> Result<ImageLayout, Error> {
        image_layout_read(self.twain(), self.dsm(), ffi::MSG_RESET)
    }

    /// Sets the image layout's frame (`DG_IMAGE/DAT_IMAGELAYOUT/MSG_SET`),
    /// leaving the document, page and frame numbers `TWON_DONTCARE32`.
    /// Refused with [`Error::InvalidFrame`] before calling the source if
    /// `frame.left >= frame.right` or `frame.top >= frame.bottom`.
    pub fn set_image_layout(&self, frame: Frame) -> Result<SetOutcome, Error> {
        if frame.left.to_f64() >= frame.right.to_f64()
            || frame.top.to_f64() >= frame.bottom.to_f64()
        {
            return Err(Error::InvalidFrame { frame });
        }
        let raw = ffi::TW_IMAGELAYOUT {
            Frame: frame.into(),
            DocumentNumber: ffi::TWON_DONTCARE32,
            PageNumber: ffi::TWON_DONTCARE32,
            FrameNumber: ffi::TWON_DONTCARE32,
        };
        let twain = self.twain();
        let dsm = self.dsm();
        let (triplet, rc) = dsm.image_layout_set(&twain.app, &twain.source, &raw);
        match rc {
            ReturnCode::SUCCESS => Ok(SetOutcome::Exact),
            ReturnCode::CHECKSTATUS => Ok(SetOutcome::Adjusted),
            rc => Err(twain.source_failure(dsm, &twain.source, triplet, rc)),
        }
    }

    /// Closes it (`MSG_CLOSEDS`, state 4 to 3).
    pub fn close(self) -> Result<Opened<'t, T>, Refused<Self>> {
        match self.twain().close_source() {
            Ok(()) => Ok(self.into_opened_unclosed()),
            Err(error) => Err(Refused { state: self, error }),
        }
    }

    /// The `Opened` beneath, the source left as the record has it.
    fn into_opened_unclosed(mut self) -> Opened<'t, T> {
        match self.opened.take() {
            Some(opened) => opened,
            None => unreachable!("a Source without its Opened"),
        }
    }
}

impl<T> Drop for Source<'_, T> {
    fn drop(&mut self) {
        if let Some(opened) = &self.opened {
            // If the source won't close, the record keeps it open, and the
            // end of `run` tries again.
            let twain = opened.twain();
            twain.dropped(twain.close_source());
        }
    }
}

impl<T> fmt::Debug for Source<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Source")
            .field("id", &self.id())
            .field("delivery", &self.delivery)
            .finish()
    }
}

/// A capability message whose answer is a container, sent to `dest`.
pub(crate) fn capability_read<T>(
    twain: &Twain<T>,
    dsm: &Dsm,
    dest: &core::cell::Cell<TW_IDENTITY>,
    msg: u16,
    cap: u16,
) -> Result<Container, Error> {
    let (triplet, rc, answer) = dsm.capability_read(&twain.app, dest, msg, cap);
    match answer {
        Some((con_type, bytes)) => Ok(Container::decode(con_type, &bytes?)?),
        None => Err(twain.source_failure(dsm, dest, triplet, rc)),
    }
}

/// The application, as it names itself to the DSM.
#[derive(Clone, Copy)]
pub struct App {
    identity: TW_IDENTITY,
}

impl App {
    /// An application of `manufacturer`, `product_family` and `product_name`
    /// (each at most 32 bytes, no NUL), version 1.0, speaking TWAIN 2.5 with
    /// image data. TWAIN's strings are in the system's code page: ASCII is
    /// safe.
    pub fn new(manufacturer: &str, product_family: &str, product_name: &str) -> Result<App, Error> {
        let identity = TW_IDENTITY {
            Id: 0,
            Version: ffi::TW_VERSION {
                MajorNum: 1,
                MinorNum: 0,
                Language: ffi::TWLG_USA as u16,
                Country: ffi::TWCY_USA,
                Info: [0; 34],
            },
            ProtocolMajor: ffi::TWON_PROTOCOLMAJOR,
            ProtocolMinor: ffi::TWON_PROTOCOLMINOR,
            SupportedGroups: ffi::DG_CONTROL | ffi::DG_IMAGE | ffi::DF_APP2,
            Manufacturer: str32(manufacturer)?,
            ProductFamily: str32(product_family)?,
            ProductName: str32(product_name)?,
        };
        Ok(App { identity })
    }

    /// Its version, and the text that goes with it (at most 32 bytes).
    pub fn version(mut self, major: u16, minor: u16, info: &str) -> Result<App, Error> {
        self.identity.Version.MajorNum = major;
        self.identity.Version.MinorNum = minor;
        self.identity.Version.Info = str32(info)?;
        Ok(self)
    }

    /// The identity it gives the DSM.
    pub fn identity(&self) -> TW_IDENTITY {
        self.identity
    }
}

impl fmt::Debug for App {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("App")
            .field("product_name", &text(&self.identity.ProductName))
            .finish_non_exhaustive()
    }
}

fn str32(text: &str) -> Result<TW_STR32, Error> {
    let bytes = text.as_bytes();
    if let Some(at) = bytes.iter().position(|&b| b == 0) {
        return Err(ContainerError::Nul { at }.into());
    }
    if bytes.len() > 32 {
        return Err(ContainerError::TooLong {
            item_type: ItemType::Str32,
            len: bytes.len(),
            max: 32,
        }
        .into());
    }
    let mut s = [0; 34];
    s[..bytes.len()].copy_from_slice(bytes);
    Ok(s)
}

/// The bytes up to the first NUL, read as UTF-8 with replacement: TWAIN's
/// strings are in the system's code page.
fn text(s: &[u8]) -> String {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..end]).into_owned()
}

/// A source, as the DSM lists it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SourceId {
    identity: TW_IDENTITY,
}

impl SourceId {
    /// The DSM's id for it, in this connection.
    pub fn id(&self) -> u32 {
        self.identity.Id
    }

    /// Its `ProductName` string (`TW_STR32`, decoded).
    pub fn product_name(&self) -> String {
        text(&self.identity.ProductName)
    }

    /// Its `ProductFamily` string (`TW_STR32`, decoded).
    pub fn product_family(&self) -> String {
        text(&self.identity.ProductFamily)
    }

    /// Its `Manufacturer` string (`TW_STR32`, decoded).
    pub fn manufacturer(&self) -> String {
        text(&self.identity.Manufacturer)
    }

    /// The TWAIN version it speaks: (major, minor).
    pub fn protocol(&self) -> (u16, u16) {
        (self.identity.ProtocolMajor, self.identity.ProtocolMinor)
    }

    /// Whether it's a TWAIN 2 source (`DF_DS2`), which can signal through a
    /// callback.
    pub fn is_twain2(&self) -> bool {
        self.identity.SupportedGroups & ffi::DF_DS2 != 0
    }

    /// Its whole identity, as the DSM gave it.
    pub fn identity(&self) -> TW_IDENTITY {
        self.identity
    }
}

impl fmt::Debug for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceId")
            .field("id", &self.id())
            .field("product_name", &self.product_name())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{self, Outcome};

    fn source(groups: u32) -> SourceId {
        SourceId {
            identity: TW_IDENTITY {
                SupportedGroups: groups,
                ..TW_IDENTITY::default()
            },
        }
    }

    #[test]
    fn a_callback_is_refused_for_a_twain_1_source() {
        let old = source(ffi::DG_CONTROL | ffi::DG_IMAGE);
        assert!(matches!(
            check_delivery(&old, Delivery::Callback),
            Err(Error::CallbackUnsupported)
        ));
        assert!(check_delivery(&old, Delivery::Events).is_ok());
        let new = source(ffi::DG_CONTROL | ffi::DG_IMAGE | ffi::DF_DS2);
        assert!(check_delivery(&new, Delivery::Callback).is_ok());
    }

    /// A capability asked of a source the DSM lists but hasn't open: the DSM
    /// refuses it itself, with its own code for this application, and the
    /// source isn't asked.
    #[test]
    fn a_triplet_to_a_source_not_open_is_the_dsms_refusal() {
        test_support::fork(
            rusty_fork::rusty_fork_test_name!(a_triplet_to_a_source_not_open_is_the_dsms_refusal),
            rusty_fork::rusty_fork_id!(),
            || {
                crate::run::<(), _>(|twain| {
                    let loaded = match twain.load(Load::System) {
                        Ok(loaded) => loaded,
                        Err(LoadError::NotFound { .. }) => {
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
                    let Some(first) = sources.first() else {
                        return Outcome::Skip("no source installed".into());
                    };
                    let never_opened = core::cell::Cell::new(first.identity());
                    let refused = capability_read(
                        twain,
                        opened.dsm(),
                        &never_opened,
                        ffi::MSG_GETCURRENT,
                        ffi::ICAP_XFERMECH,
                    );
                    match refused {
                        Err(Error::Failed {
                            rc,
                            by_source,
                            by_dsm,
                            triplet,
                        }) => {
                            assert_eq!(rc, ReturnCode::FAILURE);
                            assert_eq!(
                                triplet.to_string(),
                                "DG_CONTROL/DAT_CAPABILITY/MSG_GETCURRENT"
                            );
                            assert_eq!(by_dsm, Some(crate::ConditionCode::OPERATIONERROR));
                            assert_eq!(by_source, None);
                        }
                        other => panic!("a source not open: {other:?}"),
                    }
                    Outcome::Done
                })
                .expect("TWAIN runs")
                .value
            },
        );
    }
}
