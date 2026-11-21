#!/usr/bin/env python3
"""Carry resolved mob step and fall sound names through the event queue without copying text."""
from pathlib import Path


CLIENT = Path('crates/sunrise-client')


def change(path, *pairs):
    file = CLIENT / path
    text = file.read_text()
    for old, new in pairs:
        if text.count(old) != 1:
            raise SystemExit(f'{file}: expected one occurrence of {old!r}')
        text = text.replace(old, new)
    file.write_text(text)


change('src/mobs.rs', ('    ProfessionWorkSound(Arc<str>, [f64; 3], f32, f32),',
                       '    ProfessionWorkSound(Arc<str>, [f64; 3], f32, f32),\n'
                       '    /// A step or fall carrying its already resolved event.\n'
                       '    BlockSound(mod_api::SoundEventKey, [f64; 3], f32, f32),'))
change('src/game/mobs/creatures/mod.rs', (
    '                MobEvent::ProfessionWorkSound(name, at, volume, pitch) => {',
    '                MobEvent::BlockSound(event, at, volume, pitch) => {\n'
    '                    self.audio.play(event.as_str(), Some(at), volume, pitch)\n'
    '                }\n'
    '                MobEvent::ProfessionWorkSound(name, at, volume, pitch) => {'))
change('examples/probe/mob_ai_probe.rs', (
    '                    let _ = writeln!(out, "{t} sound {name} {} {}", f(*volume), f(*pitch));\n'
    '                } else if player_hurt(player.as_ref(), e) {',
    '                    let _ = writeln!(out, "{t} sound {name} {} {}", f(*volume), f(*pitch));\n'
    '                } else if let MobEvent::BlockSound(event, _, volume, pitch) = e {\n'
    '                    let _ = writeln!(out, "{t} sound {} {} {}", event.as_str(), f(*volume), f(*pitch));\n'
    '                } else if player_hurt(player.as_ref(), e) {'))
change('src/mobs/entity_sounds_tests.rs', (
    '            MobEvent::SoundId(id, at, volume, pitch) => server.play(id, Some(*at), *volume, *pitch),',
    '            MobEvent::SoundId(id, at, volume, pitch) => server.play(id, Some(*at), *volume, *pitch),\n'
    '            MobEvent::BlockSound(id, at, volume, pitch) => server.play(id.as_str(), Some(*at), *volume, *pitch),'))
change('src/mobs/ai/step.rs',
       ('use super::{Mob, MobWorld, Species};\nuse std::borrow::Cow;', 'use super::{Mob, MobEvent, MobWorld, Species};'),
       ("    state: sunrise_world::registry::BlockStateId,\n) -> Option<(Cow<'static, str>, f32, f32)> {\n"
        "    use Species as S;\n    let baby = m.is_baby();\n    let quiet = |s: &'static str| Some((Cow::Borrowed(s), 0.15, 1.0));",
        '    state: sunrise_world::registry::BlockStateId,\n) -> Option<MobEvent> {\n'
        '    use Species as S;\n    let baby = m.is_baby();\n'
        '    let at = [m.position.x, m.position.y, m.position.z];\n'
        '    let sound = |name, volume, pitch| Some(MobEvent::Sound(name, at, volume, pitch));\n'
        '    let quiet = |name| sound(name, 0.15, 1.0);'),
       ('Some((Cow::Borrowed(name), volume, pitch))', 'sound(name, volume, pitch)'),
       ('Some((Cow::Borrowed("entity.iron_golem.step"), 1.0, 1.0))', 'sound("entity.iron_golem.step", 1.0, 1.0)'),
       ('Some((\n            Cow::Borrowed(crate::mobs::copper_golem::sound(m, "step")),\n            1.0,\n            1.0,\n        ))',
        'sound(crate::mobs::copper_golem::sound(m, "step"), 1.0, 1.0)'),
       ('Some((Cow::Borrowed("entity.strider.step_lava"), 1.0, 1.0))', 'sound("entity.strider.step_lava", 1.0, 1.0)'),
       ('Some((Cow::Borrowed("entity.strider.step"), 1.0, 1.0))', 'sound("entity.strider.step", 1.0, 1.0)'),
       ('Some((Cow::Borrowed("entity.warden.step"), 10.0, 1.0))', 'sound("entity.warden.step", 10.0, 1.0)'),
       ('            let step = Cow::Owned(sounds.step.as_str().to_owned());\n'
        '            Some((step, sounds.volume * 0.15, sounds.pitch))',
        '            Some(MobEvent::BlockSound(\n'
        '                sounds.step.clone(),\n'
        '                at,\n                sounds.volume * 0.15,\n                sounds.pitch,\n            ))'),
       ('                assert_eq!(stepped(Some("minecraft:stone")), None, "loaded");\n'
        '                let empty = stepped(None);\n'
        '                assert!(empty.is_some(), "an empty cube takes the block\'s step");\n'
        '                assert_eq!(empty.unwrap().1, 0.15, "the block\'s 15%");',
        '                assert!(stepped(Some("minecraft:stone")).is_none(), "loaded");\n'
        '                let empty = stepped(None);\n'
        '                assert!(matches!(empty, Some(MobEvent::BlockSound(event, _, 0.15, 1.0))\n'
        '                    if event.as_str() == "minecraft:block.stone.step"));'))
change('src/mobs/entity_sounds.rs',
       (") -> Option<(Cow<'static, str>, f32, f32)> {", ') -> Option<super::MobEvent> {'),
       ('        Selection::Id(id) => Some((Cow::Borrowed(id), 0.15, 1.0)),',
        '        Selection::Id(id) => Some(super::MobEvent::Sound(\n'
        '            id, [m.position.x, m.position.y, m.position.z], 0.15, 1.0,\n        )),'))
change('src/mobs/living.rs',
       ('                } else if let Some((sound, volume, pitch)) =\n'
        '                    super::entity_sounds::step(self, w, legacy, legacy_state)\n'
        '                    && heard\n                    && !self.silent\n                {\n'
        '                    let at = [self.position.x, self.position.y, self.position.z];\n'
        '                    events.push(match sound {\n'
        '                        std::borrow::Cow::Borrowed(name) => {\n'
        '                            MobEvent::Sound(name, at, volume, pitch)\n                        }\n'
        '                        std::borrow::Cow::Owned(name) => MobEvent::SoundId(name, at, volume, pitch),\n'
        '                    });',
        '                } else if let Some(sound) = super::entity_sounds::step(self, w, legacy, legacy_state)\n'
        '                    && heard\n                    && !self.silent\n                {\n'
        '                    events.push(sound);'),
       ('                events.push(MobEvent::SoundId(\n                    sounds.fall.as_str().to_owned(),',
        '                events.push(MobEvent::BlockSound(\n'
        '                    sounds.fall.clone(),'),
       ('MobEvent::SoundId(id, _, 0.5, 0.75), ..] if id == block_sound',
        'MobEvent::BlockSound(id, _, 0.5, 0.75), ..] if id.as_str() == block_sound'))
change('src/game/modding/native_block_sounds.rs', (
    '            assert!(matches!(&step.0, std::borrow::Cow::Owned(_)));\n'
    '            assert_eq!(step.0, "mymod:block.x.step");\n'
    '            assert_eq!(step.1, 0.3);\n'
    '            assert_eq!(step.2, 1.5);',
    '            assert!(matches!(step, MobEvent::BlockSound(event, _, 0.3, 1.5)\n'
    '                if event.as_str() == "mymod:block.x.step"));'))
