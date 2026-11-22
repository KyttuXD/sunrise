//! A level's custom spawners (`ServerLevel.customSpawners`) and their tick
//! (`ServerLevel.tickCustomSpawners`): vanilla's built-ins (for the Overworld; empty for other
//! dimensions, matching `MinecraftServer#createLevels`), minus what a mod's
//! `modify_custom_spawners` took out, plus the owned `CustomSpawner` objects it added.
use super::{Dim, Game};
use mod_api::{__private::CustomSpawnerList, SpawnerKind};

/// The chunk spawning phase's inputs for the registered tail, run after Phantom.
pub(super) enum CustomSpawnerTick {
    Skip,
    Run { spawn_enemies: bool },
}

impl Game<'_> {
    /// Build `dim`'s custom spawner list once per open logical-server level, firing
    /// `modify_custom_spawners` when the level opens (or lazily if ticked without
    /// `native_level_opened`). Does nothing on a logical client.
    pub(in crate::game) fn init_custom_spawners(&mut self, dim: Dim) {
        if self.remote.is_some() || !self.is_logical_server() {
            return;
        }
        if self.creatures.custom_spawners.contains_key(&dim) {
            return;
        }
        let mut list = CustomSpawnerList::new(dim == Dim::Overworld);
        if mod_api::__private::listened(mod_api::HookId::ModifyCustomSpawners) {
            self.native_modify_custom_spawners(dim.id(), &mut list);
        }
        self.creatures.custom_spawners.insert(dim, list);
    }

    /// Whether vanilla's `kind` is still in this level's list.
    pub(super) fn has_custom_spawner(&mut self, kind: SpawnerKind) -> bool {
        if self.remote.is_some() || !self.is_logical_server() {
            return false;
        }
        let dim = self.dim;
        self.init_custom_spawners(dim);
        self.creatures
            .custom_spawners
            .get(&dim)
            .is_some_and(|list| list.builtins().any(|builtin| builtin == kind))
    }

    /// Keep the built-ins at their existing tick sites, including Phantom after the mobs.
    pub(super) fn tick_builtin_custom_spawners(&mut self) {
        if self.remote.is_some() || !self.is_logical_server() {
            return;
        }
        self.init_custom_spawners(self.dim);
        let Some(list) = self.creatures.custom_spawners.get(&self.dim) else {
            return;
        };
        for kind in list.builtins() {
            match kind {
                SpawnerKind::WanderingTrader => self.tick_wandering_trader(),
                SpawnerKind::Patrol => self.tick_patrol_spawner(),
                SpawnerKind::Cat => self.tick_cat_spawner(),
                SpawnerKind::VillageSiege => self.tick_village_siege(),
                _ => {} // Phantom ticks in `spawn_phantoms`.
            }
        }
    }

    /// `MinecraftServer.updateMobSpawningFlags`: all levels share the same rules. The chunk
    /// setting changes at spawn settings refresh, not on every tick or `spawn_mobs` edit.
    pub(super) fn update_mob_spawning_flags(&mut self) {
        use sunrise_client::gamerules::vanilla::{SPAWN_MOBS, SPAWN_MONSTERS};
        self.chunk_spawn_enemies =
            self.ui.rules.bool_at(SPAWN_MOBS) && self.ui.rules.bool_at(SPAWN_MONSTERS);
    }

    /// Tick the level's registered tail directly, after every preceding built-in including
    /// Phantom. Runtime level changes are queued until the level loop finishes.
    pub(super) fn tick_registered_custom_spawners(&mut self, spawn_enemies: bool) {
        let dim = self.dim;
        let Some(list) = self.creatures.custom_spawners.get_mut(&dim) else {
            return;
        };
        if list.registered.is_empty() {
            return;
        }
        let mut registered = std::mem::take(&mut list.registered);
        let level = self.level_ref(self.lend_epoch(), dim.key());
        for spawner in &mut registered {
            if spawner.is_enabled() {
                let _ = mod_api::run_transaction(self, |game, _| {
                    spawner.tick(game, level, spawn_enemies)
                });
            }
        }
        if let Some(list) = self.creatures.custom_spawners.get_mut(&dim) {
            list.registered = registered;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::{with_client_game, with_game};
    use super::*;
    use mod_api::{
        CustomSpawner, CustomSpawnerEntry, Host, LevelRef, ObjectiveRenderType, ServerHost, Text,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    type Log = Arc<Mutex<Vec<String>>>;

    struct StatefulSpawner {
        name: &'static str,
        ticks: u32,
        log: Log,
    }
    impl CustomSpawner for StatefulSpawner {
        fn tick(&mut self, host: &mut dyn ServerHost, level: LevelRef, spawn_enemies: bool) {
            self.ticks += 1;
            self.log.lock().unwrap().push(format!(
                "{} tick {} dim={} enemies={}",
                self.name,
                self.ticks,
                level.dimension().id(host).unwrap(),
                spawn_enemies
            ));
        }
    }

    /// Two listeners edit the same level's logical list.
    struct Spawns {
        name: &'static str,
        log: Log,
    }
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for Spawns {
        fn modify_custom_spawners(
            &self,
            _: &dyn mod_api::Host,
            event: &mut mod_api::ModifyCustomSpawnersEvent<'_>,
        ) {
            self.log
                .lock()
                .unwrap()
                .push(format!("{} modify", self.name));
            if self.name == "amod" {
                event.spawners.remove(SpawnerKind::Cat);
                event.spawners.add(
                    "discard",
                    StatefulSpawner {
                        name: "discard",
                        ticks: 0,
                        log: self.log.clone(),
                    },
                );
            } else {
                let unwanted = event.spawners.entries().position(|entry| {
                    matches!(
                        entry,
                        CustomSpawnerEntry::Registered {
                            mod_id: "amod",
                            label: "discard",
                            ..
                        }
                    )
                });
                if let Some(index) = unwanted {
                    event.spawners.remove_at(index);
                }
            }
            event.spawners.add(
                "keep",
                StatefulSpawner {
                    name: self.name,
                    ticks: 0,
                    log: self.log.clone(),
                },
            );
        }

        fn player_spawn_phantoms(
            &self,
            _: &mut dyn ServerHost,
            _: &mut mod_api::PlayerSpawnPhantomsEvent<'_>,
        ) -> Option<mod_api::PhantomSpawnResult> {
            if self.name == "amod" {
                self.log.lock().unwrap().push("phantom pass".into());
            }
            Some(mod_api::PhantomSpawnResult::Deny)
        }
    }

    /// Registration order includes all the preceding built-ins, even Phantom. A later
    /// listener can remove an earlier custom entry without disturbing its siblings.
    #[test]
    #[ignore = "daily"]
    fn custom_spawners_tick_in_list_order_and_a_removed_one_never_ticks() {
        with_game(|game| {
            game.creatures.cat.next_tick = 5;
            game.tick_mobs().unwrap();
            assert_eq!(
                game.creatures.cat.next_tick, 4,
                "no listener: vanilla's cat spawner ticks"
            );
            let builtins: Vec<_> = game.creatures.custom_spawners[&game.dim]
                .builtins()
                .collect();
            assert_eq!(
                builtins,
                [
                    SpawnerKind::WanderingTrader,
                    SpawnerKind::Patrol,
                    SpawnerKind::Cat,
                    SpawnerKind::VillageSiege,
                    SpawnerKind::Phantom,
                ]
            );
        });
        let log = Log::default();
        let mods = ["amod", "bmod"]
            .map(|name| {
                mod_api::ModEntry::new(
                    name,
                    Box::new(Spawns {
                        name,
                        log: log.clone(),
                    }),
                )
            })
            .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                super::super::bossbars::audience_tests::add_real_and_fake(game);
                game.creatures.cat.next_tick = 5;
                game.world.day_time = 18_000;
                game.stats.set_phantom_tick(0);
                game.tick_mobs().unwrap();
                game.tick_mobs().unwrap();
                assert_eq!(
                    game.creatures.cat.next_tick, 5,
                    "the removed cat spawner never ticks"
                );
            });
        });
        let log = log.lock().unwrap();
        assert_eq!(
            log[..2],
            ["amod modify", "bmod modify"],
            "one modify event per level, in dependency order"
        );
        assert_eq!(log.iter().filter(|l| l.ends_with("modify")).count(), 2);
        let ticks: Vec<&str> = log[2..].iter().map(String::as_str).collect();
        assert_eq!(
            ticks,
            [
                "phantom pass",
                "amod tick 1 dim=minecraft:overworld enemies=true",
                "bmod tick 1 dim=minecraft:overworld enemies=true",
                "amod tick 2 dim=minecraft:overworld enemies=true",
                "bmod tick 2 dim=minecraft:overworld enemies=true",
            ]
        );
    }

    #[test]
    #[ignore = "daily"]
    fn custom_spawners_are_owned_per_level_empty_outside_overworld_and_dropped_on_unload() {
        let log = Log::default();
        let overworld_drops = Arc::new(AtomicUsize::new(0));
        let nether_drops = Arc::new(AtomicUsize::new(0));

        struct LevelOwnedSpawner {
            dim: String,
            ticks: u32,
            log: Log,
            drops: Arc<AtomicUsize>,
        }
        impl CustomSpawner for LevelOwnedSpawner {
            fn tick(&mut self, host: &mut dyn ServerHost, level: LevelRef, _: bool) {
                self.ticks += 1;
                self.log.lock().unwrap().push(format!(
                    "{}:{}({})",
                    self.dim,
                    self.ticks,
                    level.dimension().id(host).unwrap()
                ));
            }
        }
        impl Drop for LevelOwnedSpawner {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::Relaxed);
            }
        }

        struct PerLevelMod {
            log: Log,
            overworld_drops: Arc<AtomicUsize>,
            nether_drops: Arc<AtomicUsize>,
        }
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for PerLevelMod {
            fn modify_custom_spawners(
                &self,
                _: &dyn mod_api::Host,
                event: &mut mod_api::ModifyCustomSpawnersEvent<'_>,
            ) {
                let builtin_count = event.spawners.builtins().count();
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("modify:{}={builtin_count}", event.dimension));
                let drops = if event.dimension == Dim::Overworld.id() {
                    self.overworld_drops.clone()
                } else {
                    self.nether_drops.clone()
                };
                event.spawners.add(
                    "level-owned",
                    LevelOwnedSpawner {
                        dim: event.dimension.to_owned(),
                        ticks: 0,
                        log: self.log.clone(),
                        drops,
                    },
                );
            }
        }

        let mods = vec![mod_api::ModEntry::new(
            "per_level_spawner_mod",
            Box::new(PerLevelMod {
                log: log.clone(),
                overworld_drops: overworld_drops.clone(),
                nether_drops: nether_drops.clone(),
            }),
        )];
        mod_platform::testing::with_mods(mods.into(), || {
            with_game(|game| {
                game.native_level_opened(Dim::Overworld);
                game.native_level_opened(Dim::Nether);
                // Re-opening an already open level must not fire again.
                game.native_level_opened(Dim::Overworld);

                game.dim = Dim::Overworld;
                game.tick_mobs().unwrap();
                game.tick_mobs().unwrap();
                game.dim = Dim::Nether;
                game.tick_mobs().unwrap();

                // Closing the Nether drops its spawner immediately without touching Overworld.
                game.dim = Dim::Overworld;
                game.native_level_closed(Dim::Nether);
                assert_eq!(nether_drops.load(Ordering::Relaxed), 1);
                assert_eq!(overworld_drops.load(Ordering::Relaxed), 0);

                // Overworld's spawner keeps its state and continues ticking.
                game.tick_mobs().unwrap();

                // Closing the world drops Overworld's spawner too.
                game.native_lifecycle_world_closed();
                assert_eq!(overworld_drops.load(Ordering::Relaxed), 1);
            });
        });

        let log = log.lock().unwrap();
        assert_eq!(
            log.as_slice(),
            [
                "modify:minecraft:overworld=5",
                "modify:minecraft:the_nether=0",
                "minecraft:overworld:1(minecraft:overworld)",
                "minecraft:overworld:2(minecraft:overworld)",
                "minecraft:the_nether:1(minecraft:the_nether)",
                "minecraft:overworld:3(minecraft:overworld)",
            ]
        );
    }

    #[test]
    #[ignore = "daily"]
    fn custom_spawners_do_not_run_on_client_side() {
        let calls = Arc::new(AtomicUsize::new(0));
        struct ClientProbe(Arc<AtomicUsize>);
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for ClientProbe {
            fn modify_custom_spawners(
                &self,
                _: &dyn mod_api::Host,
                _: &mut mod_api::ModifyCustomSpawnersEvent<'_>,
            ) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let mods = vec![mod_api::ModEntry::new(
            "client_spawner_probe",
            Box::new(ClientProbe(calls.clone())),
        )];
        mod_platform::testing::with_mods(mods.into(), || {
            with_client_game(|game| {
                game.init_custom_spawners(Dim::Overworld);
                assert!(!game.has_custom_spawner(SpawnerKind::Phantom));
                game.tick_mobs().unwrap();
                assert!(game.creatures.custom_spawners.is_empty());
            });
        });
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    #[ignore = "daily"]
    fn registered_spawners_use_chunk_phase_eligibility_and_spawn_settings() {
        use sunrise_client::gamerules::Value;

        struct SpawnFlagsProbe(Arc<Mutex<Vec<bool>>>);
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for SpawnFlagsProbe {
            fn modify_custom_spawners(
                &self,
                _: &dyn Host,
                event: &mut mod_api::ModifyCustomSpawnersEvent<'_>,
            ) {
                let flags = self.0.clone();
                event.spawners.add(
                    "flags",
                    move |_: &mut dyn ServerHost, _: LevelRef, enemies| {
                        flags.lock().unwrap().push(enemies);
                    },
                );
            }
        }

        let flags = Arc::new(Mutex::new(Vec::new()));
        let mods = [mod_api::ModEntry::new(
            "spawn_flags_probe",
            Box::new(SpawnFlagsProbe(flags.clone())),
        )];
        mod_platform::testing::with_mods(mods.into(), || {
            with_game(|game| {
                let mobs = game.ui.rules.rule("spawn_mobs").unwrap().clone();
                let monsters = game.ui.rules.rule("spawn_monsters").unwrap().clone();
                game.native_level_opened(Dim::Overworld);
                game.set_difficulty_forced(0);
                game.tick_mobs().unwrap(); // Peaceful with both gamerules enabled: true.
                game.world_debug = true;
                game.tick_mobs().unwrap(); // No custom callback in a debug world.
                game.world_debug = false;
                game.set_game_rule(&mobs, Value::Bool(false), None);
                game.set_game_rule(&monsters, Value::Bool(false), None);
                game.set_game_rule(&monsters, Value::Bool(true), None);
                game.tick_mobs().unwrap(); // SPAWN_MOBS off: no custom callback.
                game.set_game_rule(&mobs, Value::Bool(true), None);
                game.tick_mobs().unwrap(); // SPAWN_MOBS alone did not refresh the chunk flag.
                game.set_difficulty_forced(1);
                game.tick_mobs().unwrap(); // Difficulty change refreshes from the two gamerules.
                game.set_game_rule(&monsters, Value::Bool(false), None);
                game.tick_mobs().unwrap(); // Friendly custom spawners still tick.
            });
        });
        assert_eq!(*flags.lock().unwrap(), [true, false, true, false]);
    }

    #[test]
    #[ignore = "daily"]
    fn refused_or_panicking_registration_does_not_take_earlier_objects() {
        use mod_api::__private::{ModPhase, PhaseOwner};

        struct Editor {
            name: &'static str,
            log: Log,
        }
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for Editor {
            fn modify_custom_spawners(
                &self,
                host: &dyn Host,
                event: &mut mod_api::ModifyCustomSpawnersEvent<'_>,
            ) {
                if matches!(self.name, "b_refuse" | "c_panic") {
                    self.log
                        .lock()
                        .unwrap()
                        .push(format!("{} called", self.name));
                    let earlier = event.spawners.entries().position(|entry| {
                        matches!(
                            entry,
                            CustomSpawnerEntry::Registered {
                                label: "a_kept",
                                ..
                            }
                        )
                    });
                    if let Some(index) = earlier {
                        event.spawners.remove_at(index);
                    }
                }
                event.spawners.add(
                    self.name,
                    StatefulSpawner {
                        name: self.name,
                        ticks: 0,
                        log: self.log.clone(),
                    },
                );
                match self.name {
                    "b_refuse" => {
                        let _setup = PhaseOwner::scope(ModPhase::Setup);
                        let _ = host.level(event.dimension);
                    }
                    "c_panic" => panic!("registration test panic"),
                    _ => {}
                }
            }
        }

        let log = Log::default();
        let mods = ["a_kept", "b_refuse", "c_panic", "d_kept"].map(|name| {
            mod_api::ModEntry::new(
                name,
                Box::new(Editor {
                    name,
                    log: log.clone(),
                }),
            )
        });
        mod_platform::testing::with_mods(mods.into(), || {
            with_game(|game| {
                game.native_level_opened(Dim::Overworld);
                game.tick_mobs().unwrap();
                game.native_level_opened(Dim::Nether);
                game.dim = Dim::Nether;
                game.tick_mobs().unwrap();
            });
        });
        assert_eq!(
            log.lock().unwrap().as_slice(),
            [
                "b_refuse called",
                "c_panic called",
                "a_kept tick 1 dim=minecraft:overworld enemies=true",
                "d_kept tick 1 dim=minecraft:overworld enemies=true",
                "b_refuse called",
                "a_kept tick 1 dim=minecraft:the_nether enemies=true",
                "d_kept tick 1 dim=minecraft:the_nether enemies=true",
            ]
        );
    }

    #[test]
    #[ignore = "daily"]
    fn custom_spawner_panic_and_phase_refusal_containment() {
        use mod_api::__private::{ModPhase, PhaseOwner};

        const PANIC_MOD: &str = "spawner_panic_mod";
        const HEALTHY_MOD: &str = "spawner_healthy_mod";

        let panic_ticks = Arc::new(AtomicUsize::new(0));
        let refusal_ticks = Arc::new(AtomicUsize::new(0));
        let healthy_ticks = Arc::new(AtomicUsize::new(0));

        struct MaybePanickingSpawner {
            ticks: Arc<AtomicUsize>,
            panic: bool,
        }
        impl CustomSpawner for MaybePanickingSpawner {
            fn tick(&mut self, host: &mut dyn ServerHost, _: LevelRef, _: bool) {
                self.ticks.fetch_add(1, Ordering::Relaxed);
                if !self.panic {
                    return;
                }
                host.add_objective(
                    "spawner_panic_write",
                    "dummy",
                    Text::literal("Panic"),
                    ObjectiveRenderType::Integer,
                )
                .unwrap();
                panic!("custom spawner test panic");
            }
        }

        struct RefusingOnceSpawner {
            ticks: Arc<AtomicUsize>,
            should_refuse: bool,
        }
        impl CustomSpawner for RefusingOnceSpawner {
            fn tick(&mut self, host: &mut dyn ServerHost, _: LevelRef, _: bool) {
                self.ticks.fetch_add(1, Ordering::Relaxed);
                host.add_objective(
                    "spawner_refusal_write",
                    "dummy",
                    Text::literal("Refusal"),
                    ObjectiveRenderType::Integer,
                )
                .unwrap();
                if self.should_refuse {
                    self.should_refuse = false;
                    let _setup = PhaseOwner::scope(ModPhase::Setup);
                    let _ = host.objective("spawner_refusal_write");
                }
            }
        }

        struct HealthySpawner(Arc<AtomicUsize>);
        impl CustomSpawner for HealthySpawner {
            fn tick(&mut self, host: &mut dyn ServerHost, _: LevelRef, _: bool) {
                let n = self.0.fetch_add(1, Ordering::Relaxed) + 1;
                if n == 1 {
                    host.add_objective(
                        "spawner_healthy_write",
                        "dummy",
                        Text::literal("Healthy"),
                        ObjectiveRenderType::Integer,
                    )
                    .unwrap();
                }
            }
        }

        struct PanicMod {
            panicking_ticks: Arc<AtomicUsize>,
            healthy_ticks: Arc<AtomicUsize>,
        }
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for PanicMod {
            fn modify_custom_spawners(
                &self,
                _: &dyn mod_api::Host,
                event: &mut mod_api::ModifyCustomSpawnersEvent<'_>,
            ) {
                let panic = event.dimension == Dim::Overworld.id();
                let ticks = if panic {
                    &self.panicking_ticks
                } else {
                    &self.healthy_ticks
                };
                event.spawners.add(
                    "same-type",
                    MaybePanickingSpawner {
                        ticks: ticks.clone(),
                        panic,
                    },
                );
            }
        }

        struct HealthyMod {
            refusal_ticks: Arc<AtomicUsize>,
            healthy_ticks: Arc<AtomicUsize>,
        }
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for HealthyMod {
            fn modify_custom_spawners(
                &self,
                _: &dyn mod_api::Host,
                event: &mut mod_api::ModifyCustomSpawnersEvent<'_>,
            ) {
                if event.dimension == Dim::Overworld.id() {
                    event.spawners.add(
                        "refusing",
                        RefusingOnceSpawner {
                            ticks: self.refusal_ticks.clone(),
                            should_refuse: true,
                        },
                    );
                    event
                        .spawners
                        .add("healthy", HealthySpawner(self.healthy_ticks.clone()));
                }
            }
        }

        // Clear any leftover notices before asserting on this test's notice.
        let _ = mod_api::__private::game::take_callback_panic_notices();

        let healthy_same_type_ticks = Arc::new(AtomicUsize::new(0));
        let mods = vec![
            mod_api::ModEntry::new(
                PANIC_MOD,
                Box::new(PanicMod {
                    panicking_ticks: panic_ticks.clone(),
                    healthy_ticks: healthy_same_type_ticks.clone(),
                }),
            ),
            mod_api::ModEntry::new(
                HEALTHY_MOD,
                Box::new(HealthyMod {
                    refusal_ticks: refusal_ticks.clone(),
                    healthy_ticks: healthy_ticks.clone(),
                }),
            ),
        ];
        mod_platform::testing::with_mods(mods.into(), || {
            with_game(|game| {
                game.tick_mobs().unwrap();
                assert_eq!(panic_ticks.load(Ordering::Relaxed), 1);
                assert_eq!(refusal_ticks.load(Ordering::Relaxed), 1);
                assert_eq!(healthy_ticks.load(Ordering::Relaxed), 1);
                assert!(
                    game.objective("spawner_panic_write").is_none(),
                    "panicking spawner writes must roll back"
                );
                assert!(
                    game.objective("spawner_refusal_write").is_none(),
                    "phase-refused spawner writes must roll back"
                );
                assert!(
                    game.objective("spawner_healthy_write").is_some(),
                    "unaffected spawner commits normally"
                );

                // Second tick: the panicked spawner stays disabled for the session, while
                // the phase-refused spawner was not disabled and succeeds now.
                game.tick_mobs().unwrap();
                assert_eq!(panic_ticks.load(Ordering::Relaxed), 1);
                assert_eq!(refusal_ticks.load(Ordering::Relaxed), 2);
                assert_eq!(healthy_ticks.load(Ordering::Relaxed), 2);
                assert!(
                    game.objective("spawner_refusal_write").is_some(),
                    "phase-refused spawner commits once the refusal stops"
                );

                // The panicking instance does not poison its registering mod's type,
                // including a new instance registered after the first one's panic.
                game.native_level_opened(Dim::Nether);
                game.dim = Dim::Nether;
                game.tick_mobs().unwrap();
                assert_eq!(healthy_same_type_ticks.load(Ordering::Relaxed), 1);
            });
        });

        let notices = mod_api::__private::game::take_callback_panic_notices();
        assert!(
            notices.iter().any(|n| n.mod_id == PANIC_MOD
                && n.kind == "custom spawner"
                && n.operation == "CustomSpawner::tick"),
            "panic must queue a client error toast notice naming the mod: {notices:?}"
        );
    }
}
