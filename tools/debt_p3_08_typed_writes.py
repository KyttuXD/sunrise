#!/usr/bin/env python3
"""Apply P3-08's typed level-write cut to the part-2 tree."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VOCAB = "crates/sunrise-mod-vocab/src"
GAME = "crates/sunrise-client/src/game"

REWRITES = {
    f"{VOCAB}/world/types.rs": (
        (
            '/// use mod_api::LevelSide;\n'
            '/// fn write(side: LevelSide<\'_>) {\n'
            '///     if let LevelSide::Client(level) = side { level.set_block_property([0, 0, 0], "a", "b", 0); }',
            '/// use mod_api::{BlockUpdateFlags, LevelSide};\n'
            '/// fn write(side: LevelSide<\'_>) {\n'
            '///     if let LevelSide::Client(level) = side {\n'
            '///         level.set_block_property([0, 0, 0], "a", "b", BlockUpdateFlags::NONE);\n'
            '///     }',
        ),
        (
            '    /// Change a block property through `native_set_block_property`.\n'
            '    fn set_block_property(\n'
            '        &mut self,\n'
            '        _pos: [i32; 3],\n'
            '        _key: &str,\n'
            '        _value: &str,\n'
            '        _flags: i32,',
            '    /// Change a block property with the same [`BlockUpdateFlags`] as [`Self::set_block`].\n'
            '    fn set_block_property(\n'
            '        &mut self,\n'
            '        _pos: [i32; 3],\n'
            '        _key: &str,\n'
            '        _value: &str,\n'
            '        _flags: BlockUpdateFlags,',
        ),
        (
            '    /// Play a sound for nearby players through `play_sound`.\n'
            '    fn play_sound(\n'
            '        &mut self,\n'
            '        _sound: &str,\n'
            '        _at: [f64; 3],\n'
            '        _source: usize,',
            '    /// Queue a level sound for nearby players under the given [`SoundSource`].\n'
            '    fn play_sound(\n'
            '        &mut self,\n'
            '        _sound: &str,\n'
            '        _at: [f64; 3],\n'
            '        _source: SoundSource,',
        ),
    ),
    f"{VOCAB}/host/block_states.rs": (
        (
            '    /// `native_set_block_property` with `Level.setBlock`\'s `flags`, as a\n'
            '    /// brewing stand\'s tick sets its bottles with `flags::UPDATE_CLIENTS`.\n'
            '    /// Players get the write when `UPDATE_CLIENTS` is set; false when this\n'
            '    /// window isn\'t a server, and in each case `native_set_block_property`\n'
            '    /// returns false.\n'
            '    #[access(WorldOnly)]\n'
            '    fn set_block_property_with(\n'
            '        &mut self,\n'
            '        pos: [i32; 3],\n'
            '        key: &str,\n'
            '        value: &str,\n'
            '        flags: i32,',
            '    /// Like [`Self::set_block_property`], with explicit `Level.setBlock` flags.\n'
            '    /// For example, a brewing stand updates its bottles with [`BlockUpdateFlags::CLIENTS`].\n'
            '    /// Returns false when not on the server or when no block state changes.\n'
            '    #[access(WorldOnly)]\n'
            '    fn set_block_property_with(\n'
            '        &mut self,\n'
            '        pos: [i32; 3],\n'
            '        key: &str,\n'
            '        value: &str,\n'
            '        flags: BlockUpdateFlags,',
        ),
    ),
    f"{GAME}/native_hooks/base.rs": (
        (
            '        self.set_block_property_with(pos, key, value, flags::UPDATE_ALL)',
            '        self.set_block_property_with(\n'
            '            pos,\n'
            '            key,\n'
            '            value,\n'
            '            BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS,\n'
            '        )',
        ),
        (
            '    /// `native_set_block_property` with `Level.setBlock`\'s `flags`, as a\n'
            '    /// brewing stand\'s tick sets its bottles with `flags::UPDATE_CLIENTS`.\n'
            '    /// Players get the write when `UPDATE_CLIENTS` is set; false when this\n'
            '    /// window isn\'t a server, and in each case `native_set_block_property`\n'
            '    /// returns false.\n'
            '    pub(in crate::game) fn set_block_property_with(\n'
            '        &mut self,\n'
            '        pos: [i32; 3],\n'
            '        key: &str,\n'
            '        value: &str,\n'
            '        flags: i32,',
            '    /// Like `set_block_property`, with explicit `Level.setBlock` flags.\n'
            '    pub(in crate::game) fn set_block_property_with(\n'
            '        &mut self,\n'
            '        pos: [i32; 3],\n'
            '        key: &str,\n'
            '        value: &str,\n'
            '        flags: BlockUpdateFlags,',
        ),
        (
            '        self.with_level(|b, level, ticks| b.set_block(level, ticks, at, id, flags))\n'
            '    }\n'
            '    /// Sends clients the block',
            '        self.with_level(|b, level, ticks| b.set_block(level, ticks, at, id, flags.bits()))\n'
            '    }\n'
            '    /// Sends clients the block',
        ),
    ),
    f"{GAME}/native_hooks/shared.rs": (
        (
            '/// `Level.setBlock`\'s flags, for the `native_set_block_property` helpers:\n'
            '/// `flags::UPDATE_CLIENTS` as a vanilla crop grows, `flags::UPDATE_ALL` as a\n'
            '/// furnace sets its `lit`.\n'
            'pub(super) use sunrise_world::level::flags;\n\n',
            '',
        ),
    ),
    f"{GAME}/modding/journal_level_write.rs": (
        (
            '    ParticleOptions, RandomDraw, Tags,',
            '    ParticleOptions, RandomDraw, SoundSource, Tags,',
        ),
        (
            '    fn set_block_property(&mut self, pos: [i32; 3], key: &str, value: &str, flags: i32) -> bool {',
            '    fn set_block_property(\n'
            '        &mut self,\n'
            '        pos: [i32; 3],\n'
            '        key: &str,\n'
            '        value: &str,\n'
            '        flags: BlockUpdateFlags,\n'
            '    ) -> bool {',
        ),
        (
            '        .is_some_and(|state| self.write_block(pos, state, flags))',
            '        .is_some_and(|state| self.write_block(pos, state, flags.bits()))',
        ),
        (
            '        source: usize,\n'
            '        volume: f32,\n'
            '        pitch: f32,\n'
            '    ) -> bool {\n'
            '        if self.game.lan.is_none()\n'
            '            || !super::native_hooks::mod_sound_ok(sound, at, source, volume, pitch)\n'
            '        {',
            '        source: SoundSource,\n'
            '        volume: f32,\n'
            '        pitch: f32,\n'
            '    ) -> bool {\n'
            '        if self.game.lan.is_none() || !super::native_hooks::mod_sound_ok(sound, at, volume, pitch) {',
        ),
        ('            source: source as i32,', '            source: source.index() as i32,'),
    ),
    f"{GAME}/modding/mod_api_host.rs": (
        (
            '        super::native_hooks::mod_sound_ok(sound, at, source.index(), volume, pitch)\n'
            '            && self',
            '        super::native_hooks::mod_sound_ok(sound, at, volume, pitch)\n'
            '            && self',
        ),
        (
            '        super::native_hooks::mod_sound_ok(sound, at, source.index(), volume, pitch)\n'
            '            && hit.play',
            '        super::native_hooks::mod_sound_ok(sound, at, volume, pitch)\n'
            '            && hit.play',
        ),
        (
            '    fn set_block_property(&mut self, pos: [i32; 3], key: &str, value: &str, flags: i32) -> bool {\n'
            '        self.level\n'
            '            .native_set_block_property(&mut self.ticks, pos, key, value, flags)',
            '    fn set_block_property(\n'
            '        &mut self,\n'
            '        pos: [i32; 3],\n'
            '        key: &str,\n'
            '        value: &str,\n'
            '        flags: mod_api::BlockUpdateFlags,\n'
            '    ) -> bool {\n'
            '        self.level\n'
            '            .native_set_block_property(&mut self.ticks, pos, key, value, flags.bits())',
        ),
        (
            '    fn play_sound(\n'
            '        &mut self,\n'
            '        sound: &str,\n'
            '        at: [f64; 3],\n'
            '        source: usize,',
            '    fn play_sound(\n'
            '        &mut self,\n'
            '        sound: &str,\n'
            '        at: [f64; 3],\n'
            '        source: mod_api::SoundSource,',
        ),
    ),
    f"{GAME}/native_hooks/shared/level_helpers.rs": (
        (
            '        source: usize,\n'
            '        volume: f32,\n'
            '        pitch: f32,\n'
            '    ) -> bool {\n'
            '        if !self.native_authoritative || !mod_sound_ok(sound, at, source, volume, pitch) {',
            '        source: mod_api::SoundSource,\n'
            '        volume: f32,\n'
            '        pitch: f32,\n'
            '    ) -> bool {\n'
            '        if !self.native_authoritative || !mod_sound_ok(sound, at, volume, pitch) {',
        ),
        ('                source: source as i32,', '                source: source.index() as i32,'),
    ),
    f"{GAME}/native_hooks/sound_and_shader_helpers.rs": (
        (
            '/// Whether a native mod\'s sound can be sent: a namespaced event, a sound\n'
            '/// source, a position inside the world\'s range and a finite, non-negative\n'
            '/// volume and pitch.\n'
            'pub(in crate::game) fn mod_sound_ok(\n'
            '    sound: &str,\n'
            '    at: [f64; 3],\n'
            '    source: usize,\n'
            '    volume: f32,\n'
            '    pitch: f32,\n'
            ') -> bool {\n'
            '    sound_id(sound)\n'
            '        && source < sunrise_client::settings::SOUND_SOURCES.len()\n'
            '        && level_sound_playable(at, volume, pitch)\n'
            '}',
            '/// Whether a native mod\'s sound can be sent: a namespaced event, a position\n'
            '/// inside the world\'s range and a finite, non-negative volume and pitch.\n'
            'pub(in crate::game) fn mod_sound_ok(sound: &str, at: [f64; 3], volume: f32, pitch: f32) -> bool {\n'
            '    sound_id(sound) && level_sound_playable(at, volume, pitch)\n'
            '}',
        ),
    ),
    f"{GAME}/modding/native_level_effects.rs": (
        (
            '        if !mod_sound_ok(sound, at, source.index(), volume, pitch) {',
            '        if !mod_sound_ok(sound, at, volume, pitch) {',
        ),
    ),
    f"{GAME}/modding/native_sound_refs.rs": (
        (
            '        if !mod_sound_ok(sound, at, source.index(), volume, pitch)\n'
            '            || !self.native_sound_known(sound)\n'
            '        {',
            '        if !mod_sound_ok(sound, at, volume, pitch) || !self.native_sound_known(sound) {',
        ),
    ),
    f"{GAME}/blocks/world_fluids_tests.rs": (
        (
            '                    (BlockUpdateFlags::CLIENTS | BlockUpdateFlags::KNOWN_SHAPE).bits()',
            '                    BlockUpdateFlags::CLIENTS | BlockUpdateFlags::KNOWN_SHAPE',
        ),
    ),
}


def main():
    pending = {}
    for relative, edits in REWRITES.items():
        path = ROOT / relative
        source = path.read_text(encoding="utf-8")
        for old, new in edits:
            count = source.count(old)
            if count != 1:
                raise SystemExit(f"{relative}: expected one anchor, found {count}: {old!r}")
            source = source.replace(old, new, 1)
        pending[path] = source
    for path, source in pending.items():
        path.write_text(source, encoding="utf-8")


if __name__ == "__main__":
    main()
