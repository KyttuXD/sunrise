use super::super::native_entity_refs::tests::{with_client_game, with_game};
use super::*;
use mod_api::{
    __private::{ModPhase, PhaseOwner},
    CustomSpawner, DimensionKey, Host, LevelLoadEvent, LevelRef, ModEntry,
    ModifyCustomSpawnersEvent, ServerHost,
};
use std::sync::{Arc, Mutex};
use sunrise_client::gamerules::Value;

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Register(&'static str, DimensionKey, bool),
    Load(&'static str),
    Tick(&'static str, DimensionKey, u32, bool),
    Drop(&'static str, DimensionKey),
}

type Calls = Arc<Mutex<Vec<Call>>>;

struct Counter {
    owner: &'static str,
    calls: Calls,
    level: LevelRef,
    ticks: u32,
}

impl CustomSpawner for Counter {
    fn tick(&mut self, _: &mut dyn ServerHost, level: LevelRef, spawn_enemies: bool) {
        assert_eq!(self.level, level);
        assert_eq!(
            mod_api::content::crash::current_native_mod(),
            Some(self.owner)
        );
        self.ticks += 1;
        self.calls.lock().unwrap().push(Call::Tick(
            self.owner,
            level.dimension(),
            self.ticks,
            spawn_enemies,
        ));
    }
}

impl Drop for Counter {
    fn drop(&mut self) {
        self.calls
            .lock()
            .unwrap()
            .push(Call::Drop(self.owner, self.level.dimension()));
    }
}

struct RegisterCounters {
    owner: &'static str,
    calls: Calls,
}

#[mod_api::__subscriptions]
impl mod_api::Hooks for RegisterCounters {
    fn modify_custom_spawners(&self, host: &dyn Host, event: &mut ModifyCustomSpawnersEvent<'_>) {
        assert_eq!(host.level(event.level.dimension_id()), Some(event.level));
        let had_cats = event.spawners.remove(SpawnerKind::Cat);
        self.calls.lock().unwrap().push(Call::Register(
            self.owner,
            event.level.dimension(),
            had_cats,
        ));
        event.spawners.add(Counter {
            owner: self.owner,
            calls: self.calls.clone(),
            level: event.level,
            ticks: 0,
        });
    }

    fn level_load(&self, _: &mut dyn ServerHost, event: &mut LevelLoadEvent<'_>) {
        if self.owner == "first" {
            let dimension = match event.dimension {
                "minecraft:overworld" => "overworld",
                "minecraft:the_nether" => "nether",
                _ => panic!("unexpected test dimension"),
            };
            self.calls.lock().unwrap().push(Call::Load(dimension));
        }
    }
}

#[test]
#[ignore = "daily"]
fn custom_spawners_follow_level_lifetime_and_tick_only_the_listed_objects() {
    use Call::*;
    const NETHER: DimensionKey = DimensionKey::NETHER;
    const OVERWORLD: DimensionKey = DimensionKey::OVERWORLD;
    let calls = Calls::default();
    let mods = ["first", "second"]
        .map(|owner| {
            ModEntry::new(
                owner,
                Box::new(RegisterCounters {
                    owner,
                    calls: calls.clone(),
                }),
            )
        })
        .into();
    mod_platform::testing::with_mods(mods, || {
        with_client_game(|game| {
            game.native_level_opened(Dim::Overworld);
            game.tick_custom_spawners();
            assert!(
                calls.lock().unwrap().is_empty(),
                "client does not register or tick spawners"
            );
        });
        with_game(|game| {
            let _phase = PhaseOwner::scope(ModPhase::World);
            game.world_root = game.save.root.clone();
            game.creatures.cat.next_tick = 5;
            game.ui.difficulty = 2;
            assert!(game.ui.rules.set("spawn_mobs", Value::Bool(false)));
            game.native_level_opened(Dim::Overworld);
            assert_eq!(
                std::mem::take(&mut *calls.lock().unwrap()),
                [
                    Register("first", OVERWORLD, true),
                    Register("second", OVERWORLD, false),
                    Load("overworld")
                ]
            );
            game.tick_spawners();
            assert!(calls.lock().unwrap().is_empty(), "spawning is off");

            assert!(game.ui.rules.set("spawn_mobs", Value::Bool(true)));
            game.tick_spawners();
            game.enter_level(Dim::Nether).unwrap();
            assert!(game.ui.rules.set("spawn_monsters", Value::Bool(false)));
            game.tick_spawners();
            game.enter_level(Dim::Overworld).unwrap();
            assert!(game.ui.rules.set("spawn_monsters", Value::Bool(true)));
            game.ui.difficulty = 0;
            game.tick_spawners();
            assert_eq!(
                game.creatures.cat.next_tick, 5,
                "the removed cat never ticks"
            );
            assert_eq!(
                std::mem::take(&mut *calls.lock().unwrap()),
                [
                    Tick("first", OVERWORLD, 1, true),
                    Tick("second", OVERWORLD, 1, true),
                    Register("first", NETHER, false),
                    Register("second", NETHER, false),
                    Load("nether"),
                    Tick("first", NETHER, 1, false),
                    Tick("second", NETHER, 1, false),
                    Tick("first", OVERWORLD, 2, false),
                    Tick("second", OVERWORLD, 2, false),
                ],
            );

            game.close_parked_level(Dim::Nether).unwrap();
            game.enter_level(Dim::Nether).unwrap();
            game.tick_spawners();
            assert_eq!(
                std::mem::take(&mut *calls.lock().unwrap()),
                [
                    Drop("first", NETHER),
                    Drop("second", NETHER),
                    Register("first", NETHER, false),
                    Register("second", NETHER, false),
                    Load("nether"),
                    Tick("first", NETHER, 1, false),
                    Tick("second", NETHER, 1, false),
                ],
            );
            game.native_levels_closing();
            assert_eq!(
                *calls.lock().unwrap(),
                [
                    Drop("first", OVERWORLD),
                    Drop("second", OVERWORLD),
                    Drop("first", NETHER),
                    Drop("second", NETHER)
                ]
            );
        });
    });
}

#[test]
#[ignore = "daily"]
fn custom_spawner_tick_distinguishes_panics_from_phase_refusals() {
    use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

    struct Panics(Arc<AtomicU32>);
    impl CustomSpawner for Panics {
        fn tick(&mut self, _: &mut dyn ServerHost, _: LevelRef, _: bool) {
            self.0.fetch_add(1, Relaxed);
            panic!("spawner fixture panic");
        }
    }

    struct Refuses(Arc<AtomicU32>);
    impl CustomSpawner for Refuses {
        fn tick(&mut self, host: &mut dyn ServerHost, _: LevelRef, _: bool) {
            self.0.fetch_add(1, Relaxed);
            let _phase = PhaseOwner::scope(ModPhase::Setup);
            let _ = host.entity(0);
            unreachable!("phase gate must refuse this call");
        }
    }

    struct ThreeSpawners {
        panics: Arc<AtomicU32>,
        refusals: Arc<AtomicU32>,
        calls: Calls,
    }
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for ThreeSpawners {
        fn modify_custom_spawners(&self, _: &dyn Host, event: &mut ModifyCustomSpawnersEvent<'_>) {
            event.spawners.add(Panics(self.panics.clone()));
            event.spawners.add(Refuses(self.refusals.clone()));
            event.spawners.add(Counter {
                owner: "spawners",
                calls: self.calls.clone(),
                level: event.level,
                ticks: 0,
            });
        }
    }

    let panics = Arc::new(AtomicU32::new(0));
    let refusals = Arc::new(AtomicU32::new(0));
    let calls = Calls::default();
    let mods = vec![ModEntry::new(
        "spawners",
        Box::new(ThreeSpawners {
            panics: panics.clone(),
            refusals: refusals.clone(),
            calls: calls.clone(),
        }),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            let _phase = PhaseOwner::scope(ModPhase::World);
            game.ui.difficulty = 2;
            game.creatures.cat.next_tick = 5;
            game.native_level_opened(Dim::Overworld);
            game.tick_custom_spawners();
            game.tick_custom_spawners();
            assert_eq!(
                game.creatures.cat.next_tick, 3,
                "unmodified built-in still ticks"
            );
            assert_eq!(panics.load(Relaxed), 1);
            assert_eq!(refusals.load(Relaxed), 2);
            assert_eq!(
                *calls.lock().unwrap(),
                [
                    Call::Tick("spawners", DimensionKey::OVERWORLD, 1, true),
                    Call::Tick("spawners", DimensionKey::OVERWORLD, 2, true),
                ],
            );
        });
    });
}
