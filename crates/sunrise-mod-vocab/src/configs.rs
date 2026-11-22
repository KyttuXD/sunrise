//! Native configuration declarations and owner-bound lifecycle callbacks.
use crate::{ConfigKind, HostSide, callback_owner::CallbackOwner};
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

/// A native config file and the text to write if it is missing.
// no wire: this declaration is consumed by the in-process startup registrar.
// exhaustive: private fields keep the specification opaque.
pub struct ConfigSpec {
    kind: ConfigKind,
    default: String,
    file: Option<PathBuf>,
}

impl ConfigSpec {
    /// Declare a config named `<registering mod id>-<kind>.toml` with the given default text.
    pub fn new(kind: ConfigKind, default: impl Into<String>) -> Self {
        Self {
            kind,
            default: default.into(),
            file: None,
        }
    }

    /// Use this native path instead of the default filename.
    pub fn file(mut self, file: impl Into<PathBuf>) -> Self {
        self.file = Some(file.into());
        self
    }
}

/// The moment at which a registered config's listener is called (NeoForge's
/// `ModConfigEvent`).
// no wire: the in-process config loader delivers this event directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigEvent {
    /// The config was read for the first time in this load (`ModConfigEvent.Loading`).
    Loading,
    /// A loaded config's text changed (`ModConfigEvent.Reloading`).
    Reloading,
    /// A world config is being dropped; the snapshot is its last loaded text
    /// (`ModConfigEvent.Unloading`). This runs after `server_stopped`, or
    /// when dedicated world preparation fails after loading the config.
    Unloading,
}

/// The loaded file supplied to the config's registered owner.
// no wire: the snapshot contains paths and text from this process's filesystem.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConfigSnapshot {
    /// The ID of the mod that registered the config.
    pub mod_id: &'static str,
    /// When and on which side this config loads.
    pub kind: ConfigKind,
    /// The filename supplied at registration (or the generated default name;
    /// NeoForge's `ModConfig.getFileName`).
    pub file: PathBuf,
    /// The actual path the loader used, which can be in the world's `serverconfig/`
    /// (NeoForge's `ModConfig.getFullPath`).
    pub path: PathBuf,
    /// The text as read by the game.
    pub text: String,
}

/// An optional listener for one registered config, called only for its own lifecycle.
/// An error is logged and does not disable the listener; a panic disables it for the session.
// no wire: this callback borrows an in-process host and snapshot.
pub type ConfigListener = dyn for<'h, 'f> Fn(HostSide<'h>, ConfigEvent, &'f ConfigSnapshot) -> Result<(), String>
    + Send
    + Sync;

/// A config specification refused by the startup registrar.
// no wire: this error reports an in-process registration conflict.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigRegistrationError {
    /// The filename is already registered, regardless of config kind.
    Duplicate {
        /// The requested filename.
        file: PathBuf,
        /// The mod that registered the filename first.
        owner: &'static str,
    },
}

impl fmt::Display for ConfigRegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate { file, owner } => {
                write!(
                    f,
                    "config file {file:?} is already registered by mod {owner}"
                )
            }
        }
    }
}

impl std::error::Error for ConfigRegistrationError {}

/// A frozen config declaration. Every world borrows the same callback owner.
// no wire: the frozen row contains the callback and its session-local owner state.
// exhaustive: the callback owner and listener are private.
// hidden: engine lookup and authoring transfer, not a mod construction surface.
#[doc(hidden)]
pub struct RegisteredConfig {
    owner: CallbackOwner,
    /// When and on which side this config loads.
    pub kind: ConfigKind,
    /// The chosen filename.
    pub file: PathBuf,
    /// The text to write if the file is missing.
    pub default: String,
    listener: Option<Arc<ConfigListener>>,
}

impl RegisteredConfig {
    /// Stage one declaration under the current registrar owner, preserving registration order.
    pub fn register(
        configs: &mut Vec<Self>,
        mod_id: &'static str,
        mod_index: usize,
        spec: ConfigSpec,
        listener: Option<Arc<ConfigListener>>,
    ) -> Result<(), ConfigRegistrationError> {
        let file = spec
            .file
            .unwrap_or_else(|| format!("{mod_id}-{}.toml", spec.kind.suffix()).into());
        // FML's ConfigTracker.trackConfig compares the registered filename as written,
        // not the resolved path. Path equality would normalize dots and separators.
        if let Some(first) = configs
            .iter()
            .find(|registered| registered.file.as_os_str() == file.as_os_str())
        {
            let error = ConfigRegistrationError::Duplicate {
                file,
                owner: first.mod_id(),
            };
            log::error!("mod {mod_id}: {error}");
            return Err(error);
        }
        configs.push(Self {
            owner: CallbackOwner::for_index(mod_id, mod_index),
            kind: spec.kind,
            file,
            default: spec.default,
            listener,
        });
        Ok(())
    }

    /// The ID of the mod that registered this config.
    pub fn mod_id(&self) -> &'static str {
        self.owner.id()
    }

    /// Build a snapshot for this registered config from the resolved path and text.
    pub fn snapshot(&self, path: PathBuf, text: String) -> ConfigSnapshot {
        ConfigSnapshot {
            mod_id: self.mod_id(),
            kind: self.kind,
            file: self.file.clone(),
            path,
            text,
        }
    }

    /// Deliver the event for an already-built snapshot after the engine has done its file I/O.
    pub fn notify_snapshot(
        &self,
        host: HostSide<'_>,
        event: ConfigEvent,
        snapshot: &ConfigSnapshot,
    ) {
        let Some(listener) = &self.listener else {
            return;
        };
        let operation = match event {
            ConfigEvent::Loading => "config_loading",
            ConfigEvent::Reloading => "config_reloading",
            ConfigEvent::Unloading => "config_unloading",
        };
        // CallbackOwner reports phase refusals and panics. Only the listener's own
        // returned error is reported here, independently on every invocation.
        if let Ok(Err(error)) = self.owner.call("config listener", operation, || {
            listener(host, event, snapshot)
        }) {
            log::error!(
                "mod {}: {operation} for {:?}: {error}",
                self.mod_id(),
                snapshot.path
            );
        }
    }

    /// Deliver the event after the engine has done its file I/O.
    pub fn notify(&self, host: HostSide<'_>, event: ConfigEvent, path: &Path, text: &str) {
        let snapshot = self.snapshot(path.to_owned(), text.to_owned());
        self.notify_snapshot(host, event, &snapshot);
    }
}
