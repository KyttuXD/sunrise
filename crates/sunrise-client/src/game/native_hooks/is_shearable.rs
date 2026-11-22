use super::*;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    /// `IShearable.isShearable` as the entity's entity parts answer it, returning the
    /// answering part's registration slot alongside `shearable`. Server-side.
    pub(in crate::game) fn native_is_shearable(
        &self,
        entity_kind: &str,
        entity: EntityRef,
        player: Option<PlayerRef>,
        shears: mod_api::ItemStackView<'_>,
        shearable: &mut Option<bool>,
    ) -> Option<(usize, usize)> {
        mod_api::dispatch_is_shearable(self, entity_kind, entity, player, shears, shearable)
    }
}
