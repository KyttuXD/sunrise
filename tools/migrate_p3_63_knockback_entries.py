#!/usr/bin/env python3
"""Remove the Java-overload marker from living knockback (one-shot P3-63 migration)."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CLIENT = "crates/sunrise-client/src/"
VOCAB = "crates/sunrise-mod-vocab/src/"
GEN = "crates/sunrise-mod-api-gen/"


def edit(path, changes):
    path = ROOT / path
    text = path.read_text()
    for old, new in changes:
        assert text.count(old) == 1, (path, old)
        text = text.replace(old, new, 1)
    path.write_text(text)


def cut(path, start, end, replacement=""):
    text = (ROOT / path).read_text()
    lo = text.index(start)
    hi = text.index(end, lo + len(start))
    edit(path, [(text[lo:hi], replacement)])


cut(VOCAB + "entity/health.rs", "/// Which entry of vanilla's", "/// Where the entity a hit")
edit(VOCAB + "exports/health.rs", [("HurtSource, KnockbackEntry, Reduction", "HurtSource, Reduction")])
edit(VOCAB + "hooks/living_knockback.rs", [("        entry: KnockbackEntry,\n", "")])
cut(CLIENT + "health.rs", "    /// Which entry of `LivingEntity.knockback`", "    /// Who a push credits")
edit(CLIENT + "health.rs", [("            knock_entry: self.knock_entry,\n", "")])
path = ROOT / (CLIENT + "health.rs")
text = path.read_text()
assert text.count("            knock_entry: mod_api::KnockbackEntry::Full,\n") == 2
path.write_text(text.replace("            knock_entry: mod_api::KnockbackEntry::Full,\n", ""))
cut(CLIENT + "health.rs", "    /// Runs `push` as the short entry", "    /// The same level, on a client")
edit(CLIENT + "mobs/knockback.rs", [(
    "        level.through_short_knockback(|level| {\n"
    "            self.knockback_full(level, power, xd, zd, source, damage, false);\n"
    "        });",
    "        self.knockback_full(level, power, xd, zd, source, damage, false);",
)])
cut(CLIENT + "game/items/mace.rs", "                // The smash pushes the armor stand through the short entry", "                if canceled", "                let canceled = level.knockback_canceled(pushed, &mut knock);\n")
player = CLIENT + "game/mobs/creatures/knockback.rs"
edit(player, [
    ("use mod_api::KnockbackEntry;\n", ""),
    ("        _damage: f32,\n    ) {\n", "        damage: f32,\n    ) {\n"),
    ("self.knock_back_player_entry(power, xd, zd, source, attack, KnockbackEntry::Short);", "self.knock_back_player_full(power, xd, zd, source, attack, damage, false);"),
])
cut(player, "        self.knock_back_player_entry(power, xd, zd, source, attack, KnockbackEntry::Full);", "        // No attacker named")
cut(player, "            let mut push = |level:", "        }\n    }\n}", "            apply_player_knockback(p, level, power, (xd, zd), source);\n")
edit(CLIENT + "game/native_hooks/shared.rs", [
    ("    let entry = level.knockback_entry();\n", ""),
    ("        kill_credit_main: credit_main.as_ref(),\n        entry,\n", "        kill_credit_main: credit_main.as_ref(),\n"),
])
edit(GEN + "dispatch-tests/src/fixtures/base.rs", [
    ("            assert_eq!(event.entry, KnockbackEntry::Full);\n", ""),
    ("            entry: KnockbackEntry::Full,\n", ""),
])
(ROOT / (GEN + "dispatch-tests/src/fixtures/stubs/KnockbackEntry.rs")).unlink()
edit(GEN + "tests/knockback_fixture/preamble.rs.txt", [
    ("    fn through_short_knockback<R>(&mut self, push: impl FnOnce(&mut Self) -> R) -> R { push(self) }\n", ""),
    ("mod mod_api { #[derive(Clone, Copy)] pub enum KnockbackEntry { Short, Full } }\n", ""),
])


test = CLIENT + "game/modding/native_knockback_tests.rs"
edit(test, [
    ("    /// The entry of `LivingEntity.knockback` each push came through, as the hook named it.\n    entries: Vec<mod_api::KnockbackEntry>,\n", ""),
    ("    repeat_short: bool,\n", ""),
    ("        heard.entries.push(event.entry);\n", ""),
    ("        if heard.repeat_short && event.entry == mod_api::KnockbackEntry::Short {\n            event.strength *= 2.0;\n        }\n", ""),
])
cut(test, "/// The hook fires once per call of the full entry", "fn with_detached_credit(", '''#[test]
#[ignore = "daily"]
fn one_event_per_living_knockback_operation() {
    let heard = heard(|game, log| {
        let hit = Damage::of(DamageKind::Generic);
        zombie_at(game, 42, Vec3::ZERO);
        let mut mob = game.mobs.pop().unwrap();
        mob.on_ground = false;
        for (i, from_effect) in [false, true].into_iter().enumerate() {
            mob.velocity = Vec3::ZERO;
            with_detached_credit(game, 42, |level| {
                if from_effect {
                    mob.knockback_full(level, 0.4, 1.0, 0.0, &hit, 0.0, true);
                } else {
                    mob.knockback(level, 0.4, 1.0, 0.0, &hit, 0.0);
                }
            });
            assert_eq!(mob.velocity, Vec3::new(0.8, 0.0, 0.0));
            assert_eq!(log.lock().unwrap().knocks.len(), i + 1);
        }

        player_with_sword(game);
        game.player.as_mut().unwrap().ground.on_ground = false;
        game.knock_back_player(0.4, 1.0, 0.0, source(0), &NO_ATTACK, 0.0);
        assert_eq!(
            game.player.as_ref().unwrap().velocity,
            Vec3::new(0.8, 0.0, 0.0)
        );
    });
    assert_eq!(heard.knocks.len(), 3);
}

''')
edit("tools/ci/ignored-tests/daily-sunrise-client-issue-4451.txt", [(
    "sunrise-client game::native_knockback_tests::the_hook_names_the_entry_a_push_came_through_once_per_full_entry_call lib daily resources\n",
    "",
)])
(ROOT / "tools/ci/ignored-tests/daily-sunrise-client-debt-p3-63.txt").write_text(
    "sunrise-client game::native_knockback_tests::one_event_per_living_knockback_operation lib daily resources\n"
)
