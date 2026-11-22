//! Call-scoped context for [`ItemPart::can_perform_action`](crate::ItemPart::can_perform_action).

use crate::{Host, LootParamsView, Tags};

/// Call-scoped context for [`ItemPart::can_perform_action`](crate::ItemPart::can_perform_action).
///
/// Gameplay sites lend the read-only [`Host`] and its merged [`Tags`]. Loot
/// predicates (`neoforge:can_item_perform_ability`) lend the evaluator's
/// authoritative [`Tags`] and [`LootParamsView`] without fabricating a host.
// no wire: lends either the live gameplay Host or an active loot evaluation's params and tags.
// exhaustive: constructed only at the gameplay and loot ability boundaries.
pub struct ItemAbilityContext<'a>(ItemAbilityScope<'a>);

enum ItemAbilityScope<'a> {
    Game(&'a dyn Host),
    Loot {
        tags: &'a dyn Tags,
        params: LootParamsView<'a>,
    },
}

impl<'a> ItemAbilityContext<'a> {
    /// Lend the live gameplay [`Host`] and its merged tags.
    // hidden: constructed by the gameplay ability bridge and tests.
    #[doc(hidden)]
    pub fn from_host(host: &'a dyn Host) -> Self {
        Self(ItemAbilityScope::Game(host))
    }

    /// Lend an active loot evaluation's merged tags and parameters, with no gameplay [`Host`].
    // hidden: constructed by the loot ability bridge and tests.
    #[doc(hidden)]
    pub fn from_loot(tags: &'a dyn Tags, params: LootParamsView<'a>) -> Self {
        Self(ItemAbilityScope::Loot { tags, params })
    }

    /// The live read-only [`Host`] during gameplay, or `None` during a loot predicate.
    pub fn host(&self) -> Option<&dyn Host> {
        match &self.0 {
            ItemAbilityScope::Game(host) => Some(*host),
            ItemAbilityScope::Loot { .. } => None,
        }
    }

    /// Authoritative merged tags for this evaluation (the host's in gameplay, the
    /// loot evaluator's during a loot predicate).
    pub fn tags(&self) -> &dyn Tags {
        match &self.0 {
            ItemAbilityScope::Game(host) => host.tags(),
            ItemAbilityScope::Loot { tags, .. } => *tags,
        }
    }

    /// Active loot parameters during a loot predicate, or `None` during gameplay.
    pub fn loot(&self) -> Option<LootParamsView<'_>> {
        match &self.0 {
            ItemAbilityScope::Game(_) => None,
            ItemAbilityScope::Loot { params, .. } => Some(params.reborrow()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Identifier, TagKey, Text, loot_views::LootParamsParts, testing::FakeHost};

    struct MarkerTags(TagKey);

    impl Tags for MarkerTags {
        fn key(&self, id: &Identifier) -> Option<TagKey> {
            (id.as_str() == "test:loot_only").then_some(self.0)
        }
    }

    #[test]
    fn host_and_loot_contexts_stay_honest_about_their_authority() {
        let host = FakeHost::new();
        let game = ItemAbilityContext::from_host(&host);
        assert!(game.host().is_some());
        assert!(game.loot().is_none());

        let tag_id = Identifier::parse("test:loot_only").unwrap();
        let marker = crate::world::tags::tag_key(&tag_id).unwrap();
        let tags = MarkerTags(marker);
        let params = LootParamsView::new(
            LootParamsParts {
                queried_table_id: Some("minecraft:blocks/stone"),
                ..Default::default()
            },
            |_| Ok(Text::literal("")),
        );
        let loot = ItemAbilityContext::from_loot(&tags, params);
        assert!(loot.host().is_none());
        assert_eq!(loot.tags().key(&tag_id), Some(marker));
        assert_eq!(
            loot.loot().unwrap().queried_table_id(),
            "minecraft:blocks/stone"
        );
    }
}
