//! Client asset reload listeners and their registration context, the client
//! counterpart of `server_reload`, as NeoForge's
//! `AddClientReloadListenersEvent` beside its server event.
#[cfg(test)]
#[path = "client_reload_tests.rs"]
mod tests;

use crate::{
    FileToIdConverter, Identifier, ReloadResourceError,
    world::reload::{Boundary, ReloadCore, ReloadFiles},
    world::reload_order::ReloadOrderError,
};
use std::sync::{Arc, LazyLock};

/// Sunrise's vanilla client reload listeners and ordering boundaries, from the
/// 26.3 `Minecraft` resource-manager registration order: language, textures,
/// shaders, sounds, the atlas, fonts, the grass and foliage colour listeners,
/// the model manager, equipment assets, paletted textures, then the entity and
/// block-entity render dispatchers, particles, the game renderer, the level
/// extractor, clouds, the GPU warnlist, regional compliance and the GUI.
/// Use these methods to obtain IDs for client reload dependencies.
// no wire: namespace for local reload dependency IDs, not a payload.
#[non_exhaustive]
pub struct VanillaClientListeners;
impl VanillaClientListeners {
    /// Boundary before every reload listener.
    pub fn first() -> Identifier {
        static ID: LazyLock<Identifier> =
            LazyLock::new(|| Identifier::parse("minecraft:first").unwrap());
        ID.clone()
    }
    /// Vanilla's model manager. Use this anchor rather than its string value,
    /// which need not match NeoForge's identifier.
    pub fn model_manager() -> Identifier {
        static ID: LazyLock<Identifier> =
            LazyLock::new(|| Identifier::parse("minecraft:model_manager").unwrap());
        ID.clone()
    }
    /// Boundary after every reload listener.
    pub fn last() -> Identifier {
        static ID: LazyLock<Identifier> =
            LazyLock::new(|| Identifier::parse("minecraft:last").unwrap());
        ID.clone()
    }
}

/// Selected client asset files for one converter, with resource-pack overrides
/// resolved. Keys have their directory and extension removed. This cannot read
/// data files or filesystem paths. IDs and bytes are borrowed from this view.
// exhaustive: its fields are private, so mods cannot build or destructure it
// no wire: a local prepare view of the winning client asset files.
pub struct ClientReloadContext {
    files: ReloadFiles,
}
impl ClientReloadContext {
    pub(crate) fn new(files: ReloadFiles) -> Self {
        Self { files }
    }
    /// Converted IDs, across all namespaces, in deterministic resource-ID order.
    pub fn list(&self) -> impl Iterator<Item = &Identifier> {
        self.files.list()
    }
    /// The winning bytes for a converted ID, or None outside this directory.
    pub fn read(&self, id: &Identifier) -> Option<&[u8]> {
        self.files.read(id)
    }
}

/// A client-side `SimplePreparableReloadListener`, as `ServerReloadListener`
/// reads data: prepare reads the new pack view without changing live state;
/// the game consumes its returned action to publish the result. All prepares
/// finish before any apply, including vanilla's, runs on the game-owning thread.
/// Prepare cannot see another listener's new published state. Dependencies order
/// both phases deterministically; they do not remove the barrier. On a prepare
/// error, pending actions are dropped without running, keeping the previous assets.
pub trait ClientReloadListener: Send + Sync + 'static {
    /// Parse and validate new assets without changing the listener's live state.
    /// Return `Ok(Box::new(move || { /* publish prepared assets */ }))` with owned
    /// data. The action is consumed once and cannot report an error.
    ///
    /// As vanilla's `SimpleJsonResourceReloadListener.scanDirectory`, a
    /// listener that parses its files logs and skips one that won't parse,
    /// carrying the rest through; returning `Err` aborts the whole reload and
    /// keeps every listener's previous assets. Attach the original error to the
    /// file's ID with [`ReloadResourceError::new`] to preserve its cause.
    /// A panic in `prepare` or the returned apply stops the game; the crash
    /// report names the listener and the mod that registered it.
    fn prepare(
        &self,
        context: &ClientReloadContext,
    ) -> Result<Box<dyn FnOnce() + Send + 'static>, ReloadResourceError>;
}
impl ClientReloadListener for Boundary {
    fn prepare(
        &self,
        _: &ClientReloadContext,
    ) -> Result<Box<dyn FnOnce() + Send + 'static>, ReloadResourceError> {
        Ok(Box::new(|| {}))
    }
}

/// The listener registry for the current `add_client_reload_listeners` registration.
/// Its read-only views borrow this reload's registry and cannot be used across a
/// mutable registration call. Listener names need not belong to the registering mod.
pub trait ClientReloadListeners {
    /// Register a unique listener and converter, e.g. `FileToIdConverter::json("machines")`.
    /// Keep the `Arc` to read the listener's published state outside reloads.
    fn add_listener(
        &mut self,
        id: Identifier,
        converter: FileToIdConverter,
        listener: Arc<dyn ClientReloadListener>,
    ) -> Result<(), ReloadOrderError>;
    /// Require `first` before `second`; both names must already be registered.
    fn add_dependency(
        &mut self,
        first: Identifier,
        second: Identifier,
    ) -> Result<(), ReloadOrderError>;
    /// Borrow the registered names in registration order, boundaries included.
    fn get_registry(&self) -> ClientReloadRegistryView<'_>;
    /// Borrow the dependency graph as `(first, second)` names.
    fn get_graph(&self) -> ClientReloadGraphView<'_>;
    /// Borrow the vanilla anchor preceding mod listeners by default
    /// ([`VanillaClientListeners::model_manager`]).
    fn get_last_vanilla_listener(&self) -> &Identifier;
}

// no wire: borrows the live client reload registry during registration.
/// The names registered so far in one client reload, borrowed from the registry.
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct ClientReloadRegistryView<'a> {
    core: &'a ReloadCore<dyn ClientReloadListener>,
}

impl<'a> ClientReloadRegistryView<'a> {
    /// Iterate over registered names in registration order, boundaries included.
    pub fn iter(&self) -> impl Iterator<Item = &'a Identifier> + use<'a> {
        self.core.registry().map(|(id, _)| id)
    }
}

// no wire: borrows the live client reload dependency graph during registration.
/// The current client reload dependencies, borrowed from the registry.
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct ClientReloadGraphView<'a> {
    core: &'a ReloadCore<dyn ClientReloadListener>,
}

impl<'a> ClientReloadGraphView<'a> {
    /// Iterate over dependency edges in insertion order. Each `(first, second)`
    /// edge requires `first` to run before `second`; the edges include the
    /// built-in ordering boundaries and are not a sorted list of listeners.
    pub fn iter(&self) -> impl Iterator<Item = (&'a Identifier, &'a Identifier)> + use<'a> {
        self.core.graph()
    }
}

// no wire: process-local listener or callback storage; trait objects are invoked through host dispatch, not serialized.
/// The game's [`ClientReloadListeners`].
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct ClientListenerRegistry {
    core: ReloadCore<dyn ClientReloadListener>,
    default_anchor: Identifier,
}
impl ClientReloadListeners for ClientListenerRegistry {
    fn add_listener(
        &mut self,
        id: Identifier,
        converter: FileToIdConverter,
        listener: Arc<dyn ClientReloadListener>,
    ) -> Result<(), ReloadOrderError> {
        self.core.add_listener(id, converter, listener)
    }
    fn add_dependency(
        &mut self,
        first: Identifier,
        second: Identifier,
    ) -> Result<(), ReloadOrderError> {
        self.core.add_dependency(first, second)
    }
    fn get_registry(&self) -> ClientReloadRegistryView<'_> {
        ClientReloadRegistryView { core: &self.core }
    }
    fn get_graph(&self) -> ClientReloadGraphView<'_> {
        ClientReloadGraphView { core: &self.core }
    }
    fn get_last_vanilla_listener(&self) -> &Identifier {
        &self.default_anchor
    }
}
impl ClientListenerRegistry {
    pub(crate) fn new() -> Self {
        let default_anchor = VanillaClientListeners::model_manager();
        Self {
            core: ReloadCore::new(
                VanillaClientListeners::first(),
                default_anchor.clone(),
                VanillaClientListeners::last(),
                || -> Arc<dyn ClientReloadListener> { Arc::new(Boundary) },
            ),
            default_anchor,
        }
    }
    pub(crate) fn finish(
        self,
    ) -> Result<
        Vec<crate::world::reload::RegisteredListener<dyn ClientReloadListener>>,
        ReloadOrderError,
    > {
        self.core.finish(&self.default_anchor)
    }
}
