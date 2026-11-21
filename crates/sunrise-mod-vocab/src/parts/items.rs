use super::*;
use crate::{ClientHost, ItemStack};

// no wire: in-process behavior called with a borrowed stack and client host.
/// Client-only main-hand input. Capturing it does not make the player use or block with the
/// item. For ordinary item use on both sides, use [`ItemPart::use_item`] and
/// [`ItemPart::release_using`]. Register one owner for an item with
/// [`ItemsRegistrar::register_client_item_use`].
///
/// Every callback receives the original stack snapshot and its hotbar slot (0–8), never a
/// replacement. Track the capture in these callbacks, not through the living-item use reads.
/// A disabled owner receives no callbacks.
pub trait ClientItemUseBehavior: Send + Sync + 'static {
    /// Consume a main-hand use input before the client handles the crosshair target.
    fn use_item(&self, host: &mut dyn ClientHost, stack: &ItemStack, slot: u32);

    /// Run only on input release while the same item kind is still selected in the original
    /// slot and the player can still use it. The game clears the capture before calling this.
    /// `ticks_held` counts elapsed held ticks.
    fn release_using(
        &self,
        _host: &mut dyn ClientHost,
        _stack: &ItemStack,
        _slot: u32,
        _ticks_held: u32,
    ) {
    }

    /// Clean up after [`Self::release_using`] on a valid release, or on its own when the
    /// capture resets (for example, when the player switches slots, dies or disconnects).
    /// The game clears the capture before the end callbacks. `ticks_held` counts elapsed held
    /// ticks.
    fn on_stop_using(
        &self,
        _host: &mut dyn ClientHost,
        _stack: &ItemStack,
        _slot: u32,
        _ticks_held: u32,
    ) {
    }
}

/// The bound owner of client input for a canonical item id, if registered.
pub fn client_item_use(
    item: &str,
) -> Option<(usize, &'static str, Arc<dyn ClientItemUseBehavior>)> {
    let attached = installed()?.keys.client_item_uses.get(item)?;
    Some((attached.mod_index, attached.mod_id, attached.part.clone()))
}

#[derive(Clone)]
pub(crate) struct PlaceProviderPart {
    pub(crate) mod_id: &'static str,
    pub(crate) mod_index: usize,
    pub(crate) provider: Arc<dyn PlaceProvider>,
}

// no wire: the installed registry dispatches these slots directly to native renderers.
// exhaustive: engine-only dispatch keys, not an authoring surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VanillaTooltipAppender {
    TropicalFishPattern,
    Instrument,
    MapId,
    Bees,
    ContainerLoot,
    Container,
    BannerPatterns,
    PotDecorations,
    WrittenBookContent,
    ChargedProjectiles,
    Fireworks,
    FireworkExplosion,
    PotionContents,
    JukeboxPlayable,
    Trim,
    StoredEnchantments,
    Enchantments,
    DyedColor,
    Profile,
    Lore,
    SulfurCubeContent,
    AttributeModifiers,
    IntangibleProjectile,
    Unbreakable,
    SignTextFront,
    SignTextBack,
    OminousBottleAmplifier,
    SuspiciousStewEffects,
    BlockState,
    EntityData,
    BlockEntityData,
    CanBreak,
    CanPlaceOn,
    Damage,
}

/// The stock registrations both the ordering graph and the renderer consume, in the order of
/// `VanillaDataComponentTooltips.collectVanillaAppenders` and `ItemStack.addDetailsToTooltip`.
pub const VANILLA_TOOLTIP_APPENDERS: &[(VanillaTooltipAppender, &str)] = {
    use VanillaTooltipAppender::*;
    &[
        (TropicalFishPattern, "minecraft:tropical_fish/pattern"),
        (Instrument, "minecraft:instrument"),
        (MapId, "minecraft:map_id"),
        (Bees, "minecraft:bees"),
        (ContainerLoot, "minecraft:container_loot"),
        (Container, "minecraft:container"),
        (BannerPatterns, "minecraft:banner_patterns"),
        (PotDecorations, "minecraft:pot_decorations"),
        (WrittenBookContent, "minecraft:written_book_content"),
        (ChargedProjectiles, "minecraft:charged_projectiles"),
        (Fireworks, "minecraft:fireworks"),
        (FireworkExplosion, "minecraft:firework_explosion"),
        (PotionContents, "minecraft:potion_contents"),
        (JukeboxPlayable, "minecraft:jukebox_playable"),
        (Trim, "minecraft:trim"),
        (StoredEnchantments, "minecraft:stored_enchantments"),
        (Enchantments, "minecraft:enchantments"),
        (DyedColor, "minecraft:dyed_color"),
        (Profile, "minecraft:profile"),
        (Lore, "minecraft:lore"),
        (SulfurCubeContent, "minecraft:sulfur_cube_content"),
        (AttributeModifiers, "minecraft:attribute_modifiers"),
        (IntangibleProjectile, "minecraft:intangible_projectile"),
        (Unbreakable, "minecraft:unbreakable"),
        (SignTextFront, "minecraft:sign_text_front"),
        (SignTextBack, "minecraft:sign_text_back"),
        (OminousBottleAmplifier, "minecraft:ominous_bottle_amplifier"),
        (SuspiciousStewEffects, "minecraft:suspicious_stew_effects"),
        (BlockState, "minecraft:block_state"),
        (EntityData, "minecraft:entity_data"),
        (BlockEntityData, "minecraft:block_entity_data"),
        (CanBreak, "minecraft:can_break"),
        (CanPlaceOn, "minecraft:can_place_on"),
        (Damage, "minecraft:damage"),
    ]
};

// no wire: the installed appender owns a native callback and its session state.
/// An installed mod appender and its slot in the component tooltip order.
#[derive(Clone)]
// exhaustive: engine-only registration state; private fields prevent downstream construction.
pub struct TooltipAppenderPart {
    pub(crate) mod_id: &'static str,
    pub(crate) mod_index: usize,
    pub(crate) component: Box<str>,
    pub(crate) slot: usize,
    pub(crate) appender: Arc<dyn crate::TooltipAppender>,
    pub(crate) panicked: Arc<std::sync::atomic::AtomicBool>,
}

impl TooltipAppenderPart {
    /// The owning mod, independent of the component's namespace.
    pub fn mod_id(&self) -> &'static str {
        self.mod_id
    }
    /// The data component whose stacks it is asked about.
    pub fn component(&self) -> &str {
        &self.component
    }
    /// Run before this vanilla slot, or after the last if it equals the vanilla registry length.
    pub fn slot(&self) -> usize {
        self.slot
    }
    /// Whether the mod is enabled and its callback has not panicked.
    pub fn enabled(&self) -> bool {
        crate::callback_owner::enabled(self.mod_index, &self.panicked)
    }
    /// Collect the callback's lines; a refusal or panic contributes none.
    pub fn append(
        &self,
        host: &dyn crate::ClientHost,
        stack: &crate::ItemStack,
        context: &crate::TooltipAppendContext,
    ) -> Option<Vec<(crate::Text, u32)>> {
        crate::callback_owner::call(
            self.mod_id,
            self.mod_index,
            &self.panicked,
            "component tooltip",
            "tooltip_appender",
            || {
                let mut lines = Vec::new();
                self.appender.append(host, stack, context, &mut lines);
                lines
            },
        )
        .ok()
    }
}

/// The generation's mod appenders, in resolved order, without its vanilla nodes.
pub fn tooltip_appenders() -> &'static [TooltipAppenderPart] {
    installed().map_or(&[], |installed| &installed.keys.tooltip_appenders)
}
/// What answers an item capability for one item: given where the stack lives, its handler, or
/// none to leave it to the next provider. A closure is one.
pub trait ItemCapabilityProvider<T: ?Sized>: Send + Sync {
    /// The handler for the stack at `at`, or none.
    fn provide(&self, host: &mut dyn ServerHost, at: &ItemPlace) -> Option<Box<T>>;
}
impl<T: ?Sized, F> ItemCapabilityProvider<T> for F
where
    F: Fn(&mut dyn ServerHost, &ItemPlace) -> Option<Box<T>> + Send + Sync,
{
    fn provide(&self, host: &mut dyn ServerHost, at: &ItemPlace) -> Option<Box<T>> {
        self(host, at)
    }
}
// no wire: trait-object alias inherits its provider trait policy; callbacks are not serialized.
/// An item capability provider, as the registrar keeps it.
pub type ItemProvider<T> = dyn ItemCapabilityProvider<T>;

// no wire: process-local registration diagnostic retains a static hook or mod identity.
/// An item registration request that the loaded item registry cannot resolve.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ItemKeyError {
    /// The requesting mod.
    pub mod_id: &'static str,
    /// The canonical requested item id.
    pub id: String,
}

impl Attached<dyn crate::ClientItemExtensions> {
    /// Bind a reload-owned item provider to the currently registering mod.
    // hidden: the engine's reload registrar uses the same ownership and enablement as item Parts.
    #[doc(hidden)]
    pub fn __client_item_extensions(
        provider: Arc<dyn crate::ClientItemExtensions>,
    ) -> Option<Self> {
        let mod_id = crate::content::crash::current_native_mod()?;
        let mod_index = crate::__private::mod_index(mod_id)?;
        Some(Self {
            mod_id,
            mod_index,
            order: 0,
            type_name: "ClientItemExtensions",
            part: provider,
        })
    }
    /// Whether the owning mod is currently enabled.
    pub fn enabled(&self) -> bool {
        crate::mod_index_on(self.mod_index)
    }
}

impl HookParts {
    pub(crate) fn item<'k>(
        &self,
        id: impl FnOnce() -> Option<&'k str>,
    ) -> &[Attached<dyn ItemPart>] {
        attached(&self.items, id)
    }
}

impl dyn Registrar + '_ {
    /// Attach `part` to every item in tag `tag` (`minecraft:axes`, without `#`).
    pub fn item_part_tag<P: ItemPart>(&mut self, tag: &str, part: P) {
        self.attach_item_part_tag(tag, type_name::<P>(), Arc::new(part));
    }
    /// Attach `part` to item `item` (`mymod:wrench`).
    pub fn item_part<P: ItemPart>(&mut self, item: crate::ItemKey, part: P) {
        self.attach_item_part(item, type_name::<P>(), Arc::new(part));
    }
    /// Answer item capability `key` for item `item` (`mymod:backpack`) with `provider`, which
    /// gives a handler for the stack at one place, or none: `r.item_capability(&ITEM_ITEMS,
    /// "mymod:backpack", |_, _| Some(Box::new(Backpack)))`. Asked as
    /// `block_entity_capability`'s are.
    pub fn item_capability<T: ?Sized + 'static>(
        &mut self,
        key: &ItemCapabilityKey<T>,
        item: &str,
        provider: impl Fn(&mut dyn ServerHost, &ItemPlace) -> Option<Box<T>> + Send + Sync + 'static,
    ) {
        self.provide_item_capability(
            key.name,
            item,
            false,
            ItemProviderOf::of(Arc::new(provider)),
        );
    }
    /// `item_capability`, asked ahead of the game's own provider.
    pub fn item_capability_before_game<T: ?Sized + 'static>(
        &mut self,
        key: &ItemCapabilityKey<T>,
        item: &str,
        provider: impl Fn(&mut dyn ServerHost, &ItemPlace) -> Option<Box<T>> + Send + Sync + 'static,
    ) {
        self.provide_item_capability(key.name, item, true, ItemProviderOf::of(Arc::new(provider)));
    }
}

/// Whether this item's resolved Parts subscribe to `hook`, before stack conversion.
pub fn item_listens(hook: HookId, id: &str) -> bool {
    if !has_part_hook(hook) {
        return false;
    }
    parts(hook, "item routing")
        .for_hook(hook)
        .is_some_and(|table| !table.item(|| Some(id)).is_empty())
}

/// Registrar operations for items.
pub trait ItemsRegistrar {
    /// Request an item by registry id, validated after all declarations.
    fn item_key(&mut self, id: &str) -> crate::ItemKey;

    /// Register the sole client input behavior for `item` in [`crate::Hooks::register`]. An
    /// enabled owner consumes main-hand use input before crosshair handling, without sending a
    /// use or release packet. This intercepts only main-hand input; it does not enter ordinary
    /// item use or blocking state on either side. Any registered item may be used; a second
    /// registration for the same item refuses startup, even from the same mod.
    fn register_client_item_use(
        &mut self,
        item: crate::ItemKey,
        behavior: Arc<dyn ClientItemUseBehavior>,
    );

    /// Attach `part`, of kind `kind`, to item `item`.
    fn attach_item_part(
        &mut self,
        item: crate::ItemKey,
        kind: &'static str,
        part: Arc<dyn ItemPart>,
    );

    /// Attach `part`, of kind `kind`, to every item in tag `tag`.
    fn attach_item_part_tag(&mut self, tag: &str, kind: &'static str, part: Arc<dyn ItemPart>);

    /// Answer item capability `name` for item `item` with `provider`; mods reach it through
    /// `item_capability`.
    fn provide_item_capability(
        &mut self,
        name: &'static str,
        item: &str,
        before_game: bool,
        provider: ItemProviderOf,
    );

    /// Read and write the item places `ItemPlace::Mod { provider: name, .. }` with `provider`,
    /// as an accessory mod exposes its slots. One provider a name.
    fn place_provider(&mut self, name: &'static str, provider: Arc<dyn PlaceProvider>);

    /// Register the sole tooltip appender for a registered, fully qualified data component id.
    /// `placement` orders it against the other vanilla and mod appender registrations, including
    /// later declarations. As in NeoForge's `RegisterTooltipAppendersEvent`, trying to replace a
    /// vanilla appender or registering two appenders for one component is a startup error.
    /// A missing component or anchor, or a cycle, also refuses startup, naming the registering mod.
    /// The callback runs only when its mod is enabled and the stack carries and shows the component.
    fn register_tooltip_appender(
        &mut self,
        component: &str,
        placement: crate::TooltipPlacement,
        appender: Arc<dyn crate::TooltipAppender>,
    );
}

/// CapabilityProviders operations for items.
// hidden: per-kind lookup implementation, exposed for generated provider bindings
#[doc(hidden)]
pub trait ItemsCapabilityProviders {
    /// The next enabled provider of item capability `name` for item `item`, advancing `index`.
    fn item_provider(
        &self,
        name: &str,
        item: crate::ItemKey,
        index: &mut u32,
    ) -> Option<(ItemProviderOf, &'static str, bool)>;

    /// The enabled provider of the item places named `name`, with its mod.
    fn place_provider(&self, name: &str) -> Option<(Arc<dyn PlaceProvider>, &'static str)>;
}

impl ItemsCapabilityProviders for InstalledProviders {
    fn item_provider(
        &self,
        name: &str,
        item: crate::ItemKey,
        index: &mut u32,
    ) -> Option<(ItemProviderOf, &'static str, bool)> {
        let item = crate::content::registry_keys::item_id(item)?;
        next(&installed()?.keys.item_capabilities, name, item, index)
    }

    fn place_provider(&self, name: &str) -> Option<(Arc<dyn PlaceProvider>, &'static str)> {
        let part = installed()?.keys.place_providers.get(name)?;
        crate::mod_index_on(part.mod_index).then(|| (part.provider.clone(), part.mod_id))
    }
}
