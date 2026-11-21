use crate::game::native_entity_refs::tests::{prepare_entity_ticking_neighborhood, with_game};
use crate::game::{Dim, Game};
use mod_api::{
    EntityLeaveLevelEvent, EntityTickListenedEvent, EntityTickPostEvent, EntityTickPreEvent,
    HostSide, HostTransaction, ItemExpireEvent, ModEntry, Registrar, RegistrationContext,
    ServerHost,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI32, AtomicU32, Ordering},
};
use sunrise_client::items::ItemStack;
use sunrise_client::motion::PlayerMotion;
use sunrise_core::{geometry::Vec3, position::ChunkPos};

const AT: [f64; 3] = [2.5, 64.0, 2.5];

fn expiry_level(game: &mut Game<'_>) {
    game.enter_level(Dim::Overworld).unwrap();
    game.options.dedicated = Some(Vec::new());
    prepare_entity_ticking_neighborhood(game, ChunkPos::new(0, 0));
    game.player = Some(PlayerMotion::new(game.motion, Vec3::new(12.5, 64.0, 12.5)).unwrap());
    game.update_chunk_tickets();
    assert!(game.entity_ticking_area()(2, 2));
}

fn drop_at_age(game: &mut Game<'_>, name: &str, age: i32) -> u32 {
    game.push_item_entity(ItemStack::new(name, 1), AT, Some([0.0; 3]), 0, None);
    let item = game.dropped.last_mut().unwrap();
    item.age = age;
    let id = item.id;
    game.native_joins_flush(false, 0);
    game.ticked_entities_through = id;
    id
}

#[derive(Default)]
struct ExpireLog {
    seen: Vec<(u32, Option<[f64; 3]>, Option<i32>)>,
    leaves: Vec<(u32, String)>,
}

struct ExpireListener {
    extra: Arc<AtomicI32>,
    log: Arc<Mutex<ExpireLog>>,
    record_leave: bool,
}

#[mod_api::__subscriptions]
impl mod_api::Hooks for ExpireListener {
    fn item_expire(&self, host: &mut dyn ServerHost, event: &mut ItemExpireEvent<'_>) {
        let entity = host.entity(event.entity);
        let pos = entity.and_then(|e| e.position(host));
        let age = entity
            .and_then(|e| host.item_entity(e))
            .and_then(|item| item.age(host));
        self.log.lock().unwrap().seen.push((event.entity, pos, age));
        event.extra_life = event
            .extra_life
            .wrapping_add(self.extra.load(Ordering::Relaxed));
    }

    fn entity_leave_level(&self, _: &mut dyn ServerHost, event: &mut EntityLeaveLevelEvent<'_>) {
        if self.record_leave {
            self.log
                .lock()
                .unwrap()
                .leaves
                .push((event.entity, event.kind.to_owned()));
        }
    }
}

/// NeoForge `EventHooks.onItemExpire` / `ItemEntity.tick`: listeners see the live
/// resident item, add to `extra_life`, extend `lifespan` without rewinding `age`,
/// clamp `lifespan` to `Short.MAX_VALUE - 1` (32766), and fire `entity_leave_level`
/// once at the removal site when discarded.
#[test]
#[ignore = "daily"]
fn item_expire_extends_lifespan_without_rewinding_age_and_caps_at_32766() {
    let log = Arc::new(Mutex::new(ExpireLog::default()));
    let extra_a = Arc::new(AtomicI32::new(60));
    let extra_b = Arc::new(AtomicI32::new(40));
    let mods: mod_api::Mods = vec![
        ModEntry::new(
            "item-expire-a",
            Box::new(ExpireListener {
                extra: extra_a.clone(),
                log: log.clone(),
                record_leave: true,
            }),
        ),
        ModEntry::new(
            "item-expire-b",
            Box::new(ExpireListener {
                extra: extra_b.clone(),
                log: log.clone(),
                record_leave: false,
            }),
        ),
    ]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            expiry_level(game);
            let id = drop_at_age(game, "minecraft:diamond", 5999);

            // First expiry at age 6000: listeners add 60 + 40 -> lifespan 6100, age stays 6000.
            game.tick_items();
            let item = game
                .dropped
                .iter()
                .find(|item| item.id == id)
                .expect("extended item survives");
            assert_eq!(item.age, 6000);
            assert_eq!(item.lifespan, 6100);
            {
                let guard = log.lock().unwrap();
                assert_eq!(guard.seen.len(), 2);
                for &(seen_id, pos, age) in &guard.seen {
                    assert_eq!(seen_id, id);
                    assert!(pos.is_some(), "item entity is resident during item_expire");
                    assert_eq!(age, Some(6000));
                }
                assert!(guard.leaves.is_empty());
            }

            // The sum 6100 + 30000 fits a Java int but exceeds Short.MAX_VALUE - 1.
            extra_a.store(30000, Ordering::Relaxed);
            extra_b.store(0, Ordering::Relaxed);
            game.dropped.last_mut().unwrap().age = 6099;
            game.tick_items();
            let item = game
                .dropped
                .iter()
                .find(|item| item.id == id)
                .expect("clamped item survives at age 6100 < 32766");
            assert_eq!(item.age, 6100);
            assert_eq!(item.lifespan, 32766);

            // Reaching 32766 discards even when listeners still request extra life;
            // entity_leave_level fires immediately at removal and does not duplicate on flush.
            game.dropped.last_mut().unwrap().age = 32765;
            game.tick_items();
            assert!(!game.dropped.iter().any(|item| item.id == id));
            assert_eq!(
                log.lock().unwrap().leaves.as_slice(),
                &[(id, "minecraft:item".to_owned())]
            );
            game.native_leaves_flush();
            assert_eq!(
                log.lock().unwrap().leaves.len(),
                1,
                "native_leaves_flush does not duplicate entity_leave_level"
            );
        });
    });
}

#[test]
#[ignore = "daily"]
fn item_expire_discards_after_insufficient_negative_or_wrapping_extensions() {
    let log = Arc::new(Mutex::new(ExpireLog::default()));
    let extra = Arc::new(AtomicI32::new(0));
    let mods: mod_api::Mods = vec![ModEntry::new(
        "item-expire-discard",
        Box::new(ExpireListener {
            extra: extra.clone(),
            log: log.clone(),
            record_leave: true,
        }),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            expiry_level(game);
            for (age, lifespan, extension) in [
                (6499, 6000, 100),      // Age 6500 still exceeds the extended lifespan of 6100.
                (5999, 6000, -500),     // Age 6000 exceeds the shortened lifespan of 5500.
                (6099, 6100, i32::MAX), // Java int sum -2147477549 clamps to zero.
            ] {
                extra.store(extension, Ordering::Relaxed);
                let id = drop_at_age(game, "minecraft:stone", age);
                game.dropped.last_mut().unwrap().lifespan = lifespan;
                game.tick_items();
                assert!(game.dropped.is_empty(), "extension {extension}");
                assert_eq!(
                    std::mem::take(&mut log.lock().unwrap().leaves),
                    [(id, "minecraft:item".to_owned())]
                );
                game.native_leaves_flush();
                assert!(log.lock().unwrap().leaves.is_empty());
            }
        });
    });
}

#[derive(Default)]
struct LeaveObservation {
    drops: [u32; 2],
    kill_later: bool,
    heard: Vec<(u32, Option<i32>)>,
    pre: Vec<u32>,
    post: Vec<u32>,
    killed: bool,
}

struct LeaveObserver(Arc<Mutex<LeaveObservation>>);

#[mod_api::__subscriptions]
impl mod_api::Hooks for LeaveObserver {
    fn entity_tick_listened(&self, _: HostSide<'_>, event: &mut EntityTickListenedEvent) {
        event.listened = true;
    }

    fn entity_tick_pre(&self, _: HostSide<'_>, event: &mut EntityTickPreEvent<'_>) {
        self.0.lock().unwrap().pre.extend_from_slice(event.entities);
    }

    fn entity_tick_post(&self, _: HostSide<'_>, event: &mut EntityTickPostEvent<'_>) {
        self.0
            .lock()
            .unwrap()
            .post
            .extend_from_slice(event.entities);
    }

    fn entity_leave_level(&self, host: &mut dyn ServerHost, event: &mut EntityLeaveLevelEvent<'_>) {
        let (drops, kill_later) = {
            let observed = self.0.lock().unwrap();
            (observed.drops, observed.kill_later)
        };
        let next_age = host
            .entity(drops[1])
            .and_then(|entity| host.item_entity(entity))
            .and_then(|item| item.age(host));
        self.0.lock().unwrap().heard.push((event.entity, next_age));
        if kill_later && event.entity == drops[0] {
            // This removes the later drop immediately, not through the handle removal sweep.
            let killed = host.commands().run_command("kill @e[type=minecraft:item]");
            self.0.lock().unwrap().killed = killed;
        }
    }
}

#[test]
#[ignore = "daily"]
fn expiry_leave_runs_before_the_next_due_drop_ticks() {
    let observed = Arc::new(Mutex::new(LeaveObservation::default()));
    // With no item_expire or individual tick listener, it is the removal that restores the list.
    let mods: mod_api::Mods = vec![ModEntry::new(
        "expiry-leave-order",
        Box::new(LeaveObserver(observed.clone())),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            expiry_level(game);
            game.build_registries();
            for kill_later in [false, true] {
                let first = drop_at_age(game, "minecraft:stone", 5999);
                let next = drop_at_age(game, "minecraft:diamond", 5999);
                *observed.lock().unwrap() = LeaveObservation {
                    drops: [first, next],
                    kill_later,
                    ..LeaveObservation::default()
                };

                game.tick_items();
                game.native_leaves_flush();
                assert!(game.dropped.is_empty());
                let observed = observed.lock().unwrap();
                assert_eq!(observed.killed, kill_later);
                assert_eq!(observed.heard, [(first, Some(5999)), (next, None)]);
                assert_eq!(observed.pre, [first, next]);
                assert_eq!(
                    observed.post.as_slice(),
                    &[first, next][..if kill_later { 1 } else { 2 }],
                    "only drops that reached their turns appear in Post"
                );
            }
        });
    });
}

#[derive(Default)]
struct DiscardLog {
    drops: [u32; 2],
    commit: Option<bool>,
    discarded: Option<[bool; 2]>,
    expires: Vec<u32>,
    leaves: Vec<u32>,
    posts: Vec<(u32, usize)>,
}

struct DiscardOnExpiry(Arc<Mutex<DiscardLog>>);

#[mod_api::__subscriptions]
impl mod_api::Hooks for DiscardOnExpiry {
    fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
        registrar.entity_tick_each("minecraft:item");
    }

    fn entity_tick_listened(&self, _: HostSide<'_>, event: &mut EntityTickListenedEvent) {
        event.listened = true;
    }

    fn entity_tick_post(&self, _: HostSide<'_>, event: &mut EntityTickPostEvent<'_>) {
        let mut log = self.0.lock().unwrap();
        let leaves = log.leaves.len();
        log.posts
            .extend(event.entities.iter().map(|&id| (id, leaves)));
    }

    fn item_expire(&self, host: &mut dyn ServerHost, event: &mut ItemExpireEvent<'_>) {
        let (drops, commit) = {
            let mut log = self.0.lock().unwrap();
            log.expires.push(event.entity);
            (log.drops, log.commit)
        };
        // Without the discard both items live long enough to reach pickup.
        event.extra_life = 20;
        if event.entity != drops[0] {
            return;
        }
        let mut discarded = [false; 2];
        let mut discard = |host: &mut dyn ServerHost| {
            discarded = drops.map(|id| host.entity(id).is_some_and(|entity| entity.discard(host)));
        };
        if let Some(commit) = commit {
            let _ = host.transaction(|host, _| {
                discard(host);
                if commit { Ok(()) } else { Err(()) }
            });
        } else {
            discard(host);
        }
        self.0.lock().unwrap().discarded = Some(discarded);
    }

    fn entity_leave_level(&self, _: &mut dyn ServerHost, event: &mut EntityLeaveLevelEvent<'_>) {
        self.0.lock().unwrap().leaves.push(event.entity);
    }
}

#[test]
#[ignore = "daily"]
fn expiry_listener_discards_are_swept_before_more_ticks_or_pickup() {
    let log = Arc::new(Mutex::new(DiscardLog::default()));
    let mods: mod_api::Mods = vec![ModEntry::new(
        "discard-on-expiry",
        Box::new(DiscardOnExpiry(log.clone())),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            expiry_level(game);
            game.player =
                Some(PlayerMotion::new(game.motion, Vec3::new(AT[0], AT[1], AT[2])).unwrap());
            game.update_chunk_tickets();

            for commit in [None, Some(true), Some(false)] {
                game.inventory.fill(None);
                let drops = [
                    drop_at_age(game, "minecraft:apple", 5999),
                    drop_at_age(game, "minecraft:diamond", 5999),
                ];
                *log.lock().unwrap() = DiscardLog {
                    drops,
                    commit,
                    ..DiscardLog::default()
                };

                game.tick_items();
                assert!(game.dropped.is_empty(), "{commit:?}");
                let picked_up: Vec<&str> = game
                    .inventory
                    .iter()
                    .flatten()
                    .map(|s| s.item.as_ref())
                    .collect();
                {
                    let log = log.lock().unwrap();
                    assert_eq!(log.discarded, Some([true, true]), "{commit:?}");
                    if commit == Some(false) {
                        assert_eq!(log.expires, drops);
                        assert_eq!(log.posts, [(drops[0], 0), (drops[1], 0)]);
                        assert_eq!(picked_up, ["minecraft:apple", "minecraft:diamond"]);
                    } else {
                        assert_eq!(log.expires, [drops[0]], "{commit:?}");
                        assert_eq!(log.leaves, drops, "{commit:?}");
                        assert_eq!(log.posts, [(drops[0], 2)], "{commit:?}");
                        assert!(picked_up.is_empty(), "{commit:?}");
                    }
                }
                game.native_leaves_flush();
                assert_eq!(log.lock().unwrap().leaves, drops, "{commit:?}");
            }
        });
    });
}

#[derive(Default)]
struct PanicsAtExpiry {
    expire_calls: AtomicU32,
    leave_calls: AtomicU32,
}

struct PanickingListener(Arc<PanicsAtExpiry>);

#[mod_api::__subscriptions]
impl mod_api::Hooks for PanickingListener {
    fn item_expire(&self, _: &mut dyn ServerHost, event: &mut ItemExpireEvent<'_>) {
        self.0.expire_calls.fetch_add(1, Ordering::Relaxed);
        event.extra_life = i32::MIN;
        panic!("fixture expiry listener panicked");
    }

    fn entity_leave_level(&self, _: &mut dyn ServerHost, _: &mut EntityLeaveLevelEvent<'_>) {
        self.0.leave_calls.fetch_add(1, Ordering::Relaxed);
        panic!("fixture leave listener panicked");
    }
}

#[test]
#[ignore = "daily"]
fn panicking_expiry_and_leave_listeners_do_not_interrupt_other_listeners() {
    let panics = Arc::new(PanicsAtExpiry::default());
    let log = Arc::new(Mutex::new(ExpireLog::default()));
    let extra = Arc::new(AtomicI32::new(20));
    let mods: mod_api::Mods = vec![
        ModEntry::new(
            "expiry-a-panics",
            Box::new(PanickingListener(panics.clone())),
        ),
        ModEntry::new(
            "expiry-b-listens",
            Box::new(ExpireListener {
                extra: extra.clone(),
                log: log.clone(),
                record_leave: true,
            }),
        ),
    ]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            expiry_level(game);
            let first = drop_at_age(game, "minecraft:stone", 5999);
            game.tick_items();
            assert_eq!(game.dropped[0].lifespan, 6020);
            assert!(log.lock().unwrap().leaves.is_empty());

            let second = drop_at_age(game, "minecraft:diamond", 5999);
            extra.store(0, Ordering::Relaxed);
            game.tick_items();
            game.dropped[0].age = 6019;
            game.tick_items();
            let log = log.lock().unwrap();
            assert_eq!(
                log.seen.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
                [first, second, first]
            );
            assert_eq!(
                log.leaves,
                [
                    (second, "minecraft:item".to_owned()),
                    (first, "minecraft:item".to_owned())
                ]
            );
            assert!(game.dropped.is_empty());
            assert_eq!(panics.expire_calls.load(Ordering::Relaxed), 1);
            assert_eq!(panics.leave_calls.load(Ordering::Relaxed), 1);
        });
    });
}
