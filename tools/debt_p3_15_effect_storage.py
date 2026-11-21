#!/usr/bin/env python3
"""Migrate the player and mob active-effect vectors on the Part 2 base.

This one-shot rewrite leaves effects, ordering, ticking and journaling unchanged.
"""

from pathlib import Path


LIST = '''/// Engine storage for the effects held by a living entity. Structural edits
/// go through the list; slice access is for reading or updating instances.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveEffectList<T>(Vec<T>);

impl<T> Default for ActiveEffectList<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> From<Vec<T>> for ActiveEffectList<T> {
    fn from(effects: Vec<T>) -> Self {
        Self(effects)
    }
}

impl<T> Deref for ActiveEffectList<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<T> DerefMut for ActiveEffectList<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

impl<T> IntoIterator for ActiveEffectList<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a ActiveEffectList<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut ActiveEffectList<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

impl<T> Extend<T> for ActiveEffectList<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, effects: I) {
        self.0.extend(effects);
    }
}

impl<T> ActiveEffectList<T> {
    pub fn push(&mut self, effect: T) {
        self.0.push(effect);
    }

    pub fn remove(&mut self, index: usize) -> T {
        self.0.remove(index)
    }

    pub fn retain(&mut self, keep: impl FnMut(&T) -> bool) {
        self.0.retain(keep);
    }

    pub fn retain_mut(&mut self, keep: impl FnMut(&mut T) -> bool) {
        self.0.retain_mut(keep);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

'''

# Paths below are relative to crates/sunrise-client/src, except for the list owner.
REPLACEMENTS = {
    "../../sunrise-client-render/src/effects.rs": (
        (
            "use crate::particles::{ParticleOptions, ParticleType};\n",
            "use crate::particles::{ParticleOptions, ParticleType};\n"
            "use std::ops::{Deref, DerefMut};\n",
        ),
        (
            "#[derive(Clone, Debug, Default)]\n"
            "pub struct PlayerEffects {\n"
            "    pub list: Vec<EffectInstance>,\n",
            LIST + "#[derive(Clone, Debug, Default)]\n"
            "pub struct PlayerEffects {\n"
            "    pub list: ActiveEffectList<EffectInstance>,\n",
        ),
    ),
    "mobs.rs": (
        (
            "    blocks::BlockFacts,\n    health::{DamageContainer, DamageLevel, Reduction},\n",
            "    blocks::BlockFacts,\n    effects::ActiveEffectList,\n"
            "    health::{DamageContainer, DamageLevel, Reduction},\n",
        ),
        ("    pub effects: Vec<StoredEffect>,\n", "    pub effects: ActiveEffectList<StoredEffect>,\n"),
        (
            '            m.extra.effects,\n'
            '            vec![crate::mobs::StoredEffect::new(speed, 600, 0)]',
            '            &m.extra.effects[..],\n'
            '            &[crate::mobs::StoredEffect::new(speed, 600, 0)]',
        ),
        (
            '            m.extra.effects,\n'
            '            vec![crate::mobs::StoredEffect::new(speed, 900, 0)]',
            '            &m.extra.effects[..],\n'
            '            &[crate::mobs::StoredEffect::new(speed, 900, 0)]',
        ),
        (
            '            m.extra.effects,\n'
            '            vec![crate::mobs::StoredEffect::new(\n'
            '                "minecraft:absorption",\n'
            '                110,\n'
            '                0\n'
            '            )]',
            '            &m.extra.effects[..],\n'
            '            &[crate::mobs::StoredEffect::new(\n'
            '                "minecraft:absorption",\n'
            '                110,\n'
            '                0\n'
            '            )]',
        ),
        (
            '            m.extra.effects,\n'
            '            vec![crate::mobs::StoredEffect::new("minecraft:speed", 110, 0)]',
            '            &m.extra.effects[..],\n'
            '            &[crate::mobs::StoredEffect::new("minecraft:speed", 110, 0)]',
        ),
        (
            '            m.extra.effects,\n'
            '            vec![crate::mobs::StoredEffect::new("minecraft:speed", 3, 0)]',
            '            &m.extra.effects[..],\n'
            '            &[crate::mobs::StoredEffect::new("minecraft:speed", 3, 0)]',
        ),
        (
            '            m.extra.effects,\n'
            '            vec![\n'
            '                crate::mobs::StoredEffect::new("minecraft:regeneration", 900, 1),',
            '            &m.extra.effects[..],\n'
            '            &[\n'
            '                crate::mobs::StoredEffect::new("minecraft:regeneration", 900, 1),',
        ),
    ),
    "mobs/ai.rs": (
        (
            '            mob.extra.effects,\n'
            '            vec![crate::mobs::StoredEffect::new("minecraft:wither", 40, 1)]',
            '            &mob.extra.effects[..],\n'
            '            &[crate::mobs::StoredEffect::new("minecraft:wither", 40, 1)]',
        ),
    ),
    "mobs/conversion.rs": (
        (
            '                    boosted.extra.effects,\n'
            '                    vec![crate::mobs::StoredEffect::new(',
            '                    &boosted.extra.effects[..],\n'
            '                    &[crate::mobs::StoredEffect::new(',
        ),
    ),
    "entity_nbt/mob_effect_amplifier_codec.rs": (
        (
            '        assert_eq!(mob.extra.effects, [loaded], "{value:?}");',
            '        assert_eq!(&mob.extra.effects[..], &[loaded], "{value:?}");',
        ),
    ),
    "entity_nbt/sheep_nbt_tests.rs": (
        (
            '        back.extra.effects,\n'
            '        [crate::mobs::StoredEffect::new("testfx:glow", 200, 2)]',
            '        &back.extra.effects[..],\n'
            '        &[crate::mobs::StoredEffect::new("testfx:glow", 200, 2)]',
        ),
    ),
    "game/interface/advancements/mod.rs": (
        (
            '            game.mobs[0].extra.effects,\n'
            '            vec![sunrise_client::mobs::StoredEffect::new(',
            '            &game.mobs[0].extra.effects[..],\n'
            '            &[sunrise_client::mobs::StoredEffect::new(',
        ),
    ),
    "game/interface/screen_build.rs": (
        (
            '            game.potions.effects.list = vec![sunrise_client::effects::EffectInstance::new(\n'
            '                "speed", 200, 0,\n'
            '            )];',
            '            game.potions.effects.list = vec![sunrise_client::effects::EffectInstance::new(\n'
            '                "speed", 200, 0,\n'
            '            )]\n'
            '            .into();',
        ),
        (
            '                game.potions.effects.list = vec![sunrise_client::effects::EffectInstance::new(\n'
            '                    "speed", 200, 0,\n'
            '                )];',
            '                game.potions.effects.list = vec![sunrise_client::effects::EffectInstance::new(\n'
            '                    "speed", 200, 0,\n'
            '                )]\n'
            '                .into();',
        ),
    ),
    "game/items/potions.rs": (
        (
            '                    game.potions.effects.list = vec![EffectInstance::new(case.id, 450, 1)];',
            '                    game.potions.effects.list = vec![EffectInstance::new(case.id, 450, 1)].into();',
        ),
    ),
    "game/network/lan/mod.rs": (
        (
            '            guest.effects.list = vec![faint_speed.clone(), EffectInstance::new("slowness", 600, 0)];',
            '            guest.effects.list =\n'
            '                vec![faint_speed.clone(), EffectInstance::new("slowness", 600, 0)].into();',
        ),
        (
            '                    .update(0, |g| g.effects.list = vec![faint_speed, weakness]);',
            '                    .update(0, |g| g.effects.list = vec![faint_speed, weakness].into());',
        ),
    ),
    "game/runtime/transactions.rs": (
        ('    effects::EffectInstance,\n', '    effects::{ActiveEffectList, EffectInstance},\n'),
        ('    active: Vec<StoredEffect>,\n', '    active: ActiveEffectList<StoredEffect>,\n'),
    ),
    "lan/hub.rs": (
        (
            '        let effects = self.guests[i].effects.list.clone();',
            '        let effects = self.guests[i].effects.list.to_vec();',
        ),
    ),
}


def main() -> None:
    src = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src"
    rewritten = {}
    for file, edits in REPLACEMENTS.items():
        path = src / file
        source = path.read_text(encoding="utf-8")
        for old, new in edits:
            matches = source.count(old)
            if matches != 1:
                raise SystemExit(f"{file}: expected one match, got {matches}: {old!r}")
            source = source.replace(old, new, 1)
        rewritten[path] = source
    for path, source in rewritten.items():
        path.write_text(source, encoding="utf-8")


if __name__ == "__main__":
    main()
