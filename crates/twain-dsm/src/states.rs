//! TWAIN's states as types, from 2 (the DSM loaded) up. Each owns the one
//! beneath it, steps up by consuming itself, hands itself back in a
//! [`Refused`] when a step fails, and steps down when dropped. The record in
//! [`Twain`] is what teardown trusts, not these: a state forgotten is still
//! closed at the end of [`run`](crate::run).

use core::fmt;
use std::path::{Path, PathBuf};

use crate::codes::{ReturnCode, Triplet};
use crate::container::{ContainerError, ItemType};
use crate::dsm::{self, Dsm};
use crate::error::{Error, LoadError, Refused};
use crate::ffi::{self, TW_IDENTITY, TW_STR32};
use crate::thread::Twain;

/// Which DSM to load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Load {
    /// `TWAINDSM.dll` in the system folder: SysWOW64 in a 32-bit process.
    System,
    /// The application's own copy, by its path.
    Path(PathBuf),
}

impl<T> Twain<T> {
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
    /// (`DAT_ENTRYPOINT`).
    pub fn open(self, app: App) -> Result<Opened<'t, T>, Refused<Self>> {
        let twain = self.twain;
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
        Error::Failed {
            triplet,
            rc,
            by_source: None,
            by_dsm: self.dsm.dsm_condition(&self.twain.app),
        }
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

    fn twain(&self) -> &'t Twain<T> {
        self.loaded().twain
    }

    fn dsm(&self) -> &'static Dsm {
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
            let _ = loaded.twain.close_connection();
        }
    }
}

impl<T> fmt::Debug for Opened<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let id = self.app().Id;
        f.debug_struct("Opened").field("app_id", &id).finish()
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

    pub fn product_name(&self) -> String {
        text(&self.identity.ProductName)
    }

    pub fn product_family(&self) -> String {
        text(&self.identity.ProductFamily)
    }

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
