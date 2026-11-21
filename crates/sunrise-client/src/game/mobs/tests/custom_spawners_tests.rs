use super::super::native_entity_refs::tests::with_game;
use super::*;
use mod_api::{DimensionKey, Host, ModEntry, ModifyCustomSpawnersEvent, ServerHost};
use std::sync::{Arc, Mutex};
use sunrise_client::gamerules::Value;

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Register(&'static str, DimensionKey, bool),
    Tick(&'static str, DimensionKey, u32, bool),
    Drop(&'static str, DimensionKey),
}

type Calls = Arc<Mutex<Vec<Call>>>;

struct Counter {
    name: &'static str,
    calls: Calls,
    dimension: DimensionKey,
    ticks: u32,
}

impl CustomSpawner for Counter {
    fn tick(&mut self, _: &mut dyn ServerHost, dimension: DimensionKey, spawn_enemies: bool) {
        self.ticks += 1;
        self.calls.lock().unwrap().push(Call::Tick(
            self.name,
            dimension,
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
            .push(Call::Drop(self.name, self.dimension));
    }
}

struct RegisterCounters {
    name: &'static str,
    calls: Calls,
}

#[mod_api::__subscriptions]
impl mod_api::Hooks for RegisterCounters {
    fn modify_custom_spawners(&self, _: &dyn Host, event: &mut ModifyCustomSpawnersEvent<'_>) {
        let had_cats = event.spawners.remove(SpawnerKind::Cat);
        self.calls
            .lock()
            .unwrap()
            .push(Call::Register(self.name, event.dimension, had_cats));
        event.spawners.add_custom_spawner(Box::new(Counter {
            name: self.name,
            calls: self.calls.clone(),
            dimension: event.dimension,
            ticks: 0,
        }));
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
        .map(|name| {
            ModEntry::new(
                name,
                Box::new(RegisterCounters {
                    name,
                    calls: calls.clone(),
                }),
            )
        })
        .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
            game.world_root = game.save.root.clone();
            game.creatures.cat.next_tick = 5;
            game.ui.difficulty = 2;
            assert!(game.ui.rules.set("spawn_mobs", Value::Bool(false)));
            game.native_level_opened(Dim::Overworld);
            assert_eq!(
                std::mem::take(&mut *calls.lock().unwrap()),
                [
                    Register("first", OVERWORLD, true),
                    Register("second", OVERWORLD, false)
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
                    Tick("first", NETHER, 1, false),
                    Tick("second", NETHER, 1, false),
                ],
            );
        });
    });
}

#[test]
#[ignore = "daily"]
fn a_panicking_spawner_stops_only_its_own_callback() {
    use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

    struct Panics(Arc<AtomicU32>);
    impl CustomSpawner for Panics {
        fn tick(&mut self, _: &mut dyn ServerHost, _: DimensionKey, _: bool) {
            self.0.fetch_add(1, Relaxed);
            panic!("spawner fixture panic");
        }
    }

    struct TwoSpawners(Arc<AtomicU32>, Calls);
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for TwoSpawners {
        fn modify_custom_spawners(&self, _: &dyn Host, event: &mut ModifyCustomSpawnersEvent<'_>) {
            event
                .spawners
                .add_custom_spawner(Box::new(Panics(self.0.clone())));
            event.spawners.add_custom_spawner(Box::new(Counter {
                name: "survivor",
                calls: self.1.clone(),
                dimension: event.dimension,
                ticks: 0,
            }));
        }
    }

    let attempts = Arc::new(AtomicU32::new(0));
    let calls = Calls::default();
    let mods = vec![ModEntry::new(
        "spawners",
        Box::new(TwoSpawners(attempts.clone(), calls.clone())),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
            game.ui.difficulty = 2;
            game.creatures.cat.next_tick = 5;
            game.native_level_opened(Dim::Overworld);
            game.tick_custom_spawners();
            game.tick_custom_spawners();
            assert_eq!(
                game.creatures.cat.next_tick, 3,
                "unmodified built-in still ticks"
            );
            assert_eq!(attempts.load(Relaxed), 1);
            assert_eq!(
                *calls.lock().unwrap(),
                [
                    Call::Tick("survivor", DimensionKey::OVERWORLD, 1, true),
                    Call::Tick("survivor", DimensionKey::OVERWORLD, 2, true),
                ],
            );
        });
    });
}
