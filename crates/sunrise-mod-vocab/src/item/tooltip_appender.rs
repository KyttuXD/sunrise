//! A data component's own tooltip lines (NeoForge's `RegisterTooltipAppendersEvent`): a mod
//! registers an appender for one component with `Registrar::register_tooltip_appender`, and the
//! game asks it for a stack that carries the component and whose tooltip shows it.

use crate::{ClientHost, ItemStack, Text};

/// One ordering constraint in the component tooltip registry. The topological sort
/// preserves registration order among currently unconstrained entries, with vanilla entries
/// registered first. `Before` and `After` require an appender for the named component, not
/// just a registered component type. Forward references to other mod appenders are allowed.
// no wire: this is a native registration constraint resolved at startup, not a mod payload.
#[derive(Clone, Debug, PartialEq, Eq)]
// exhaustive: a closed set of ordering constraints
pub enum TooltipPlacement {
    /// Before the first vanilla appender, after the item's own lines.
    BeforeAll,
    /// Before the appender for the named component; not necessarily adjacent to it.
    Before(String),
    /// After the appender for the named component; not necessarily adjacent to it.
    After(String),
    /// After the last vanilla appender (durability), before the advanced item ID and count.
    AfterAll,
}

/// Shared context for an item's own tooltip and component appenders: player,
/// flags and vanilla `Item.TooltipContext.tickRate()`.
#[derive(Clone, Copy, Debug, PartialEq)]
// exhaustive: the game builds it with a literal; mods only read it
#[derive(sunrise_mod_api_macros::Wire)]
pub struct TooltipAppendContext {
    /// The local player's UUID, including for nested projectiles where vanilla passes no player.
    pub player: u128,
    /// The local player's entity id.
    pub player_entity_id: i32,
    /// The client level's tick rate, or 20 when no client level is present.
    pub tick_rate: f32,
    /// `TooltipFlag.isAdvanced` (F3+H).
    pub advanced: bool,
    /// `TooltipFlag.isCreative`: a creative tab's own slot.
    pub creative: bool,
    /// A crossbow's loaded projectile, whose details its crossbow lists.
    pub nested: bool,
}

/// The lines a data component adds to its stack's tooltip. Kept by the game from registration on;
/// asked only for a stack that carries the component and whose `tooltip_display` shows it, and
/// only while its mod is enabled. One appender owns each component's slot.
pub trait TooltipAppender: Send + Sync + 'static {
    /// Add lines to `lines` for `stack`, each with a colour as `rgba` packs it (`u32::MAX` keeps
    /// the text's own). `context` is the tooltip's player, flags and client-level tick rate.
    /// A panic discards this call's lines, reports the mod and disables this callback for the session.
    fn append(
        &self,
        host: &dyn ClientHost,
        stack: &ItemStack,
        context: &TooltipAppendContext,
        lines: &mut Vec<(Text, u32)>,
    );
}
