//! A player's break for native mods (`PlayerRef::destroy_block`): what a vein miner needs to
//! break the blocks around the one its player broke as that player breaks them, taking their
//! drops and experience in `block_drops`.
use super::Game;
use mod_api::{BlockDropDestination, PlayerRef};

impl Game<'_> {
    /// `ServerPlayerGameMode.destroyBlock` at `pos` as `player` breaks it. Inventory routing
    /// takes just the item drops `block_drops` leaves; what doesn't fit drops from the player
    /// as in `Inventory.placeItemBackInInventory`.
    pub(super) fn player_destroy_block(
        &mut self,
        player: PlayerRef,
        pos: [i32; 3],
        destination: BlockDropDestination,
    ) -> bool {
        if !self.native_authoritative() {
            return false;
        }
        let mut broke = false;
        self.native_as_player_handle(player, |game| {
            if !game.acting_in_level_in_place() {
                return;
            }
            // Only this break owns these drops. Other breaks, including ones made by
            // callbacks, cannot inherit this destination, even if this call unwinds.
            let mut drops = (destination == BlockDropDestination::Inventory).then(Vec::new);
            let result = game.player_break_block(pos, drops.as_mut());
            for stack in drops.into_iter().flatten() {
                game.give_or_drop(stack);
            }
            broke = result.unwrap_or_else(|e| {
                log::warn!("a mod's break at {pos:?} failed: {e}");
                false
            });
        });
        broke
    }
}

impl Game<'_> {
    /// `ServerPlayerGameMode.destroyBlock`'s `Level.removeBlock`, then `Block.destroy`: the mods'
    /// `player_destroyed_block` hears the removal once it succeeded, before a creative player's
    /// return and the drops. It is told the block the removal took out, read here, after
    /// `playerWillDestroy` and the mods' `break_block` had their say, together with that block's
    /// block entity data. True when the block was removed.
    pub(in crate::game) fn remove_block_as_player(
        &mut self,
        pos: [i32; 3],
        creative: bool,
        correct_tool: bool,
    ) -> bool {
        let source = self
            .lan_acting_uuid
            .map(sunrise_world::level::LevelEventPlayer);
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let block_entity = self.removed_block_entity_data(pos, state);
        let removed = self.with_level(|b, level, ticks| {
            b.player_destroy_block(
                level,
                ticks,
                sunrise_core::position::BlockPos::new(pos[0], pos[1], pos[2]),
                creative,
                correct_tool,
                source,
            )
        });
        if removed {
            self.player_break_removed(pos, state, block_entity);
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use sunrise_client::{
        command::args::GameMode, first_person::Hand, items::ItemStack,
        lan::abilities::PlayerAbilities, motion::PlayerMotion,
    };
    use sunrise_core::geometry::Vec3;

    /// A server's game with its own player in survival standing at the
    /// origin chunk, holding `held`: their handle.
    fn with_player(game: &mut Game<'_>, held: ItemStack) -> PlayerRef {
        // The shared fixture does not open a server or register a ticking chunk.
        game.options.dedicated = Some(Vec::new());
        let _ = game
            .block_ticks
            .add_empty_chunk(sunrise_core::position::ChunkPos::new(0, 0));
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(4.5, 64.0, 4.5)).unwrap());
        game.lan_acting_uuid = Some(123);
        // The host acts, whose entity the hooks name as 0 while no guest acts.
        game.lan_acting_entity = Some(0);
        game.lan_acting_name = Some("Breaker".into());
        game.lan_acting_permission = Some(0);
        game.set_mode(GameMode::Survival);
        game.inventory[game.selected] = Some(held);
        game.native_acting_player().unwrap()
    }
    fn place(game: &mut Game<'_>, pos: [i32; 3], block: &str) {
        let state = game.pack.assets.registry.default_state(block).unwrap();
        game.world.set(pos[0], pos[1], pos[2], state).unwrap();
    }
    fn block(game: &Game<'_>, pos: [i32; 3]) -> String {
        let state = game.world.block(pos[0], pos[1], pos[2]);
        game.res.facts().get(state).name.to_string()
    }
    fn carried(game: &Game<'_>, item: &str) -> u32 {
        let held = game.inventory.iter().flatten();
        held.filter(|s| &*s.item == item).map(|s| s.count).sum()
    }
    fn popped(game: &Game<'_>, item: &str) -> bool {
        game.dropped.iter().any(|e| &*e.stack.item == item)
    }

    /// Stone broken with a diamond pickaxe drops cobblestone, wears the
    /// pickaxe by one and counts as mined (`Block.playerDestroy`,
    /// `ItemStack.mineBlock`), as a player's own break does.
    #[test]
    #[ignore = "daily"]
    fn a_mod_breaks_stone_as_the_player_with_their_pickaxe() {
        with_game(|game| {
            game.world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
            );
            let player = with_player(game, ItemStack::new("minecraft:diamond_pickaxe", 1));
            let pos = [6, 64, 4];
            place(game, pos, "minecraft:stone");
            // A break requested from an offhand interaction still uses the main hand.
            game.offhand = Some(ItemStack::new("minecraft:debug_stick", 1));
            game.hand = Hand::Off;
            assert!(game.player_destroy_block(player, pos, BlockDropDestination::World));
            assert!(matches!(game.hand, Hand::Off));
            assert_eq!(block(game, pos), "minecraft:air");
            assert!(popped(game, "minecraft:cobblestone"));
            assert_eq!(game.inventory[game.selected].as_ref().unwrap().damage, 1);
            assert_eq!(game.stats.get("mined", "minecraft:stone"), 1);
        });
    }

    /// Only the selected slot harvests a block that requires a tool. Iron doors do not
    /// require one in 26.3, so both hand arrangements harvest their lower-half drop.
    #[test]
    #[ignore = "daily"]
    fn harvesting_uses_the_selected_slot_and_iron_doors_need_no_tool() {
        with_game(|game| {
            game.world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
            );
            let pickaxe = ItemStack::new("minecraft:iron_pickaxe", 1);
            let player = with_player(game, pickaxe.clone());
            let iron = [8, 64, 4];
            place(game, iron, "minecraft:iron_block");
            assert!(game.player_destroy_block(player, iron, BlockDropDestination::World));
            assert!(
                popped(game, "minecraft:iron_block"),
                "selected pickaxe harvests"
            );
            game.dropped.clear();
            game.inventory[game.selected] = None;
            game.offhand = Some(pickaxe.clone());
            game.hand = sunrise_client::first_person::Hand::Off;
            place(game, iron, "minecraft:iron_block");
            assert!(game.player_destroy_block(player, iron, BlockDropDestination::World));
            assert!(
                !popped(game, "minecraft:iron_block"),
                "offhand cannot harvest"
            );
            game.inventory[game.selected] = Some(pickaxe.clone());
            game.offhand = None;
            game.hand = sunrise_client::first_person::Hand::Main;
            let (lower, upper) = ([6, 64, 4], [6, 65, 4]);
            let door = |game: &mut Game<'_>, half: &str, pos: [i32; 3]| {
                let state = game
                    .pack
                    .assets
                    .registry
                    .resolve(&sunrise_world::chunk::BlockState {
                        name: "minecraft:iron_door".into(),
                        properties: [
                            ("facing", "north"),
                            ("half", half),
                            ("hinge", "left"),
                            ("open", "false"),
                            ("powered", "false"),
                        ]
                        .into_iter()
                        .map(|(k, v)| (k.into(), v.into()))
                        .collect(),
                    })
                    .unwrap();
                game.world.set(pos[0], pos[1], pos[2], state).unwrap();
            };
            door(game, "lower", lower);
            door(game, "upper", upper);
            assert!(game.player_destroy_block(player, upper, BlockDropDestination::World));
            assert_eq!(block(game, upper), "minecraft:air");
            // Vanilla takes the lower half out in the same call, by the neighbour update.
            game.process_level_events().unwrap();
            assert_eq!(
                block(game, lower),
                "minecraft:air",
                "main hand: correct tool"
            );
            assert!(popped(game, "minecraft:iron_door"));

            game.dropped.clear();
            game.inventory[game.selected] = None;
            game.offhand = Some(pickaxe);
            game.hand = sunrise_client::first_person::Hand::Off;
            door(game, "lower", lower);
            door(game, "upper", upper);
            assert_eq!(block(game, lower), "minecraft:iron_door");
            assert!(game.player_destroy_block(player, upper, BlockDropDestination::World));
            assert_eq!(block(game, upper), "minecraft:air");
            assert_eq!(
                block(game, lower),
                "minecraft:air",
                "empty main hand: door needs no tool"
            );
            assert!(popped(game, "minecraft:iron_door"));
        });
    }

    /// Into the inventory: silk touch keeps the stone whole, and a diamond
    /// ore gives its diamond and still its experience (3 to 7 points).
    #[test]
    #[ignore = "daily"]
    fn drops_go_into_the_inventory_rolled_for_the_tools_enchantments() {
        with_game(|game| {
            game.world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
            );
            let mut silk = ItemStack::new("minecraft:diamond_pickaxe", 1);
            silk.enchant("minecraft:silk_touch", 1);
            let player = with_player(game, silk);
            let (stone, ore) = ([6, 64, 4], [7, 64, 4]);
            place(game, stone, "minecraft:stone");
            assert!(game.player_destroy_block(player, stone, BlockDropDestination::Inventory));
            assert_eq!(carried(game, "minecraft:stone"), 1);
            assert!(!popped(game, "minecraft:stone"));

            game.inventory[game.selected] = Some(ItemStack::new("minecraft:diamond_pickaxe", 1));
            place(game, ore, "minecraft:diamond_ore");
            assert!(game.player_destroy_block(player, ore, BlockDropDestination::Inventory));
            assert_eq!(carried(game, "minecraft:diamond"), 1);
            assert!(!game.orbs.is_empty(), "the ore's experience");
        });
    }

    /// The direct call still refuses invalid players and absent blocks, and uses
    /// the game mode, main-hand item and game-master permissions.
    #[test]
    #[ignore = "daily"]
    fn the_players_checks_hold() {
        with_game(|game| {
            game.world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
            );
            let player = with_player(game, ItemStack::new("minecraft:diamond_pickaxe", 1));
            let (stone, command, air) = ([6, 64, 4], [7, 64, 4], [8, 64, 4]);
            place(game, stone, "minecraft:stone");
            place(game, command, "minecraft:command_block");
            assert!(!game.player_destroy_block(player, air, BlockDropDestination::World));
            assert!(!game.player_destroy_block(player, [16, 64, 4], BlockDropDestination::World));
            let epoch = game.native_lifecycle.epoch().unwrap();
            let stranger =
                mod_api::__private::game::player_ref(mod_api::EntityRef::new(epoch, 21), 124);
            assert!(!game.player_destroy_block(stranger, stone, BlockDropDestination::World));
            game.set_mode(GameMode::Adventure);
            assert!(!game.player_destroy_block(player, stone, BlockDropDestination::World));
            game.set_mode(GameMode::Spectator);
            assert!(!game.player_destroy_block(player, stone, BlockDropDestination::World));
            assert_eq!(game.held().unwrap().damage, 0, "nothing wore the pickaxe");
            game.set_mode(GameMode::Creative);
            assert!(!game.player_destroy_block(player, command, BlockDropDestination::World));
            assert_eq!(block(game, command), "minecraft:command_block");
            game.inventory[game.selected] = Some(ItemStack::new("minecraft:diamond_sword", 1));
            assert!(!game.player_destroy_block(player, stone, BlockDropDestination::World));
            game.set_mode(GameMode::Survival);
            game.acting_abilities = Some(PlayerAbilities {
                instabuild: true,
                ..PlayerAbilities::default()
            });
            // Item.canDestroyBlock consults the ability, not the name of the game mode.
            assert!(!player.destroy_block(game, stone, BlockDropDestination::World));
            assert_eq!(block(game, stone), "minecraft:stone");
            assert!(game.dropped.is_empty());
        });
    }

    #[test]
    #[ignore = "daily"]
    fn a_direct_break_ignores_spawn_protection_range_and_mining_progress() {
        with_game(|game| {
            game.world.chunks.insert(
                (0, 0),
                std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
            );
            let player = with_player(game, ItemStack::new("minecraft:diamond_pickaxe", 1));
            let pos = [13, 64, 13];
            place(game, pos, "minecraft:bedrock");
            game.level.world_spawn = Some(pos);
            game.lan_acting_spawn_protection = Some(1);
            assert!(game.spawn_protects(1, game.dim.id(), pos));
            assert!(player.destroy_block(game, pos, BlockDropDestination::World));
            assert_eq!(block(game, pos), "minecraft:air");
        });
    }

    /// A vein miner as the issue's contract builds one: the player's break of the vein's
    /// origin starts it, the mod breaks every block of the vein as the player (the origin
    /// too, then cancels the outer break), takes each break's drops and experience in
    /// `block_drops`, and sets them down at the origin merged within stack sizes.
    struct VeinMiner {
        vein: Vec<[i32; 3]>,
        busy: std::sync::atomic::AtomicBool,
        kept: std::sync::Mutex<(Vec<mod_api::ItemStack>, u32)>,
    }

    #[mod_api::__subscriptions]
    impl mod_api::Hooks for VeinMiner {
        fn break_block(
            &self,
            host: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::BreakBlockEvent<'_>,
        ) -> mod_api::Flow {
            use std::sync::atomic::Ordering;
            let origin = self.vein[0];
            if event.pos != origin || self.busy.swap(true, Ordering::Relaxed) {
                return mod_api::Flow::Continue;
            }
            let player = host.players().acting_player().expect("the breaker acts");
            for &pos in &self.vein {
                player.destroy_block(host, pos, BlockDropDestination::World);
            }
            self.busy.store(false, Ordering::Relaxed);
            let (stacks, experience) = std::mem::take(&mut *self.kept.lock().unwrap());
            let mut merged: Vec<mod_api::ItemStack> = Vec::new();
            for stack in stacks {
                let max = host.max_stack_size(stack.item()).unwrap_or(64);
                let mut rest = stack.count();
                for into in merged.iter_mut().filter(|m| m.is_stackable_with(&stack)) {
                    let room = max.saturating_sub(into.count()).min(rest);
                    into.grow(room).unwrap();
                    rest -= room;
                }
                if rest > 0 {
                    let mut left = stack.clone();
                    left.set_count(rest).unwrap();
                    merged.push(left);
                }
            }
            let level = player.level(host).expect("the player's level");
            for stack in merged {
                level.pop_resource(host, origin, stack).unwrap();
            }
            let [x, y, z] = origin.map(|v| f64::from(v) + 0.5);
            level.award_experience(host, x, y, z, experience);
            mod_api::Flow::Cancel
        }
        fn block_drops(
            &self,
            _: &dyn mod_api::LevelHost,
            event: &mut mod_api::BlockDropsEvent<'_>,
        ) -> mod_api::Flow {
            let pos = [event.pos.x, event.pos.y, event.pos.z];
            if self.busy.load(std::sync::atomic::Ordering::Relaxed)
                && event.breaker == Some(123)
                && self.vein.contains(&pos)
            {
                let mut kept = self.kept.lock().unwrap();
                event.drops.retain(|drop| {
                    if let Some(stack) = drop.stack().and_then(|stack| stack.project()) {
                        kept.0.push(stack);
                        false
                    } else {
                        true
                    }
                });
                kept.1 += std::mem::take(&mut event.experience);
            }
            mod_api::Flow::Continue
        }
    }

    /// The issue's acceptance (#2953): a vein miner breaks N blocks as the player and gathers
    /// their drops, merged within stack sizes, and their experience at the origin, with no
    /// drop or orb anywhere else. The pickaxe wears once per block, as the player's own breaks.
    #[test]
    #[ignore = "daily"]
    fn a_vein_miner_sets_a_veins_drops_and_experience_down_at_the_origin() {
        // Five diamond ores in a row, then sixty-six stone: more cobblestone than one stack.
        let mut vein: Vec<[i32; 3]> = (0..5).map(|i| [2 + i, 64, 2]).collect();
        vein.extend((0..66).map(|i| [i % 11 + 2, 65 + i / 11, 2]));
        let ores = 5;
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "vein",
            Box::new(VeinMiner {
                vein: vein.clone(),
                busy: false.into(),
                kept: Default::default(),
            }),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                with_player(game, ItemStack::new("minecraft:diamond_pickaxe", 1));
                for (i, &pos) in vein.iter().enumerate() {
                    let block = if i < ores {
                        "minecraft:diamond_ore"
                    } else {
                        "minecraft:stone"
                    };
                    place(game, pos, block);
                }
                let origin = vein[0];
                // The player's own break of the origin: the mod broke it, and the vein.
                assert!(!game.player_break_block(origin, None).unwrap());
                assert!(vein.iter().all(|&pos| block(game, pos) == "minecraft:air"));
                assert_eq!(game.held().unwrap().damage, vein.len() as u32);
                assert_eq!(game.stats.get("mined", "minecraft:stone"), 66);

                let near = |at: [f64; 3], by: f64| {
                    (0..3).all(|i| (at[i] - (f64::from(origin[i]) + 0.5)).abs() <= by)
                };
                let mut stacks: Vec<(String, u32)> = Vec::new();
                for item in game.dropped.iter() {
                    let at = [item.position.x, item.position.y, item.position.z];
                    assert!(near(at, 0.5), "every drop pops at the origin: {at:?}");
                    stacks.push((item.stack.item.to_string(), item.stack.count));
                }
                stacks.sort();
                assert_eq!(
                    stacks,
                    [
                        ("minecraft:cobblestone".to_owned(), 2),
                        ("minecraft:cobblestone".to_owned(), 64),
                        ("minecraft:diamond".to_owned(), ores as u32),
                    ]
                );
                // Each diamond ore gives 3 to 7 points, all of them awarded at the origin.
                let points: u32 = game.orbs.iter().map(|o| o.points()).sum();
                assert!(
                    (3 * ores as u32..=7 * ores as u32).contains(&points),
                    "{points}"
                );
                assert!(game.orbs.iter().all(|o| {
                    let p = o.position;
                    near([p.x, p.y, p.z], 0.01)
                }));
            });
        });
    }

    struct ChainedBreaks {
        last: i32,
        vetoed: [i32; 3],
    }

    #[mod_api::__subscriptions]
    impl mod_api::Hooks for ChainedBreaks {
        fn break_block(
            &self,
            host: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::BreakBlockEvent<'_>,
        ) -> mod_api::Flow {
            if event.pos == self.vetoed {
                return mod_api::Flow::Cancel;
            }
            let [x, y, z] = event.pos;
            let (next, collect) = if x < self.last {
                ([x + 1, y, z], false)
            } else {
                (self.vetoed, true)
            };
            let player = host.players().acting_player().expect("the breaker acts");
            let destination = if collect {
                BlockDropDestination::Inventory
            } else {
                BlockDropDestination::World
            };
            assert_eq!(player.destroy_block(host, next, destination), !collect);
            mod_api::Flow::Continue
        }
    }

    #[test]
    #[ignore = "daily"]
    fn chained_breaks_keep_their_own_drops_without_a_depth_limit() {
        let (first, last, vetoed) = (2, 11, [12, 64, 4]);
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "chain",
            Box::new(ChainedBreaks { last, vetoed }),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                let player = with_player(game, ItemStack::new("minecraft:diamond_pickaxe", 1));
                for x in first..=last {
                    place(game, [x, 64, 4], "minecraft:stone");
                }
                place(game, vetoed, "minecraft:stone");
                assert!(game.player_destroy_block(
                    player,
                    [first, 64, 4],
                    BlockDropDestination::Inventory
                ));
                assert!((first..=last).all(|x| block(game, [x, 64, 4]) == "minecraft:air"));
                assert_eq!(block(game, vetoed), "minecraft:stone");
                assert_eq!(game.held().unwrap().damage, 10);
                assert_eq!(carried(game, "minecraft:cobblestone"), 1);
                assert_eq!(
                    game.dropped
                        .iter()
                        .filter(|item| &*item.stack.item == "minecraft:cobblestone")
                        .map(|item| item.stack.count)
                        .sum::<u32>(),
                    9
                );
            });
        });
    }

    struct FinishFirst(AtomicBool);

    #[mod_api::__subscriptions]
    impl mod_api::Hooks for FinishFirst {
        fn break_block(
            &self,
            host: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::BreakBlockEvent<'_>,
        ) -> mod_api::Flow {
            if !self.0.swap(true, Ordering::Relaxed) {
                let player = host.players().acting_player().expect("the breaker acts");
                assert!(player.destroy_block(host, event.pos, BlockDropDestination::World));
            }
            mod_api::Flow::Continue
        }
    }

    #[test]
    #[ignore = "daily"]
    fn a_break_does_not_disarm_tripwire_already_removed_by_its_listener() {
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "finish-first",
            Box::new(FinishFirst(false.into())),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                let player = with_player(game, ItemStack::new("minecraft:shears", 1));
                let pos = [6, 64, 4];
                place(game, pos, "minecraft:tripwire");
                // Disarming the original state in the outer break would write the wire back.
                assert!(!game.player_destroy_block(player, pos, BlockDropDestination::World));
                assert_eq!(block(game, pos), "minecraft:air");
                assert_eq!(game.held().unwrap().damage, 1);
            });
        });
    }

    #[test]
    #[ignore = "daily"]
    fn a_nested_break_of_turtle_eggs_removes_one_egg_and_wears_the_tool_once() {
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "finish-first",
            Box::new(FinishFirst(false.into())),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                let player = with_player(game, ItemStack::new("minecraft:diamond_pickaxe", 1));
                let [x, y, z] = [6, 64, 4];
                place(game, [x, y - 1, z], "minecraft:sand");
                let egg = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:turtle_egg")
                    .unwrap();
                let three_eggs = game.with_props(egg, &[("eggs", "3")]).unwrap();
                let two_eggs = game.with_props(egg, &[("eggs", "2")]).unwrap();
                game.world.set(x, y, z, three_eggs).unwrap();

                assert!(!player.destroy_block(game, [x, y, z], BlockDropDestination::World));
                assert_eq!(game.world.block(x, y, z), two_eggs);
                assert_eq!(game.held().unwrap().damage, 1);
                assert_eq!(game.stats.get("mined", "minecraft:turtle_egg"), 1);
            });
        });
    }

    struct RemoveDoorOnHarvest(Arc<AtomicUsize>);

    #[mod_api::__subscriptions]
    impl mod_api::Hooks for RemoveDoorOnHarvest {
        fn player_harvest_check(
            &self,
            host: mod_api::HostSide<'_>,
            event: &mut mod_api::PlayerHarvestCheckEvent,
        ) {
            let mod_api::HostSide::Server(host) = host else {
                return;
            };
            if self.0.fetch_add(1, Ordering::Relaxed) == 0 {
                let player = host.players().acting_player().expect("the breaker acts");
                let level = player.level(host).expect("the player's level");
                assert!(level.remove_block(host, event.pos, false));
            }
        }
    }

    #[test]
    #[ignore = "daily"]
    fn a_door_removed_by_the_harvest_hook_is_not_queried_again() {
        let queries = Arc::new(AtomicUsize::new(0));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "remove-door",
            Box::new(RemoveDoorOnHarvest(Arc::clone(&queries))),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
                let player = with_player(game, ItemStack::new("minecraft:diamond_axe", 1));
                let pos = [6, 64, 4];
                place(game, [6, 63, 4], "minecraft:stone");
                place(game, pos, "minecraft:oak_door");
                let door = game.world.block(6, 64, 4);
                let upper = game.with_props(door, &[("half", "upper")]).unwrap();
                game.world.set(6, 65, 4, upper).unwrap();

                assert!(!player.destroy_block(game, pos, BlockDropDestination::Inventory));
                assert_eq!(block(game, pos), "minecraft:air");
                assert_eq!(queries.load(Ordering::Relaxed), 1);
                assert_eq!(game.held().unwrap().damage, 0);
                assert_eq!(carried(game, "minecraft:oak_door"), 0);
            });
        });
    }

    struct UncancelBreak(Arc<Mutex<Vec<[i32; 3]>>>);

    #[mod_api::__subscriptions]
    impl mod_api::Hooks for UncancelBreak {
        #[listener(receive_cancelled)]
        fn break_block(
            &self,
            _: &mut dyn mod_api::ServerHost,
            event: &mut mod_api::BreakBlockEvent<'_>,
        ) -> mod_api::Flow {
            if event.cancelled {
                self.0.lock().unwrap().push(event.pos);
                event.cancelled = false;
            }
            mod_api::Flow::Continue
        }
    }

    #[test]
    #[ignore = "daily"]
    fn a_restricted_attempt_is_posted_before_enforcing_hard_block_break_checks() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "observe-restrictions",
            Box::new(UncancelBreak(Arc::clone(&heard))),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                let player = with_player(game, ItemStack::new("minecraft:diamond_pickaxe", 1));
                let command = [6, 64, 4];
                let stone = [7, 64, 4];
                game.ui.registries.permission = 0;
                place(game, command, "minecraft:command_block");
                place(game, stone, "minecraft:stone");

                game.set_mode(GameMode::Creative);
                assert!(!player.destroy_block(game, command, BlockDropDestination::World));
                assert_eq!(block(game, command), "minecraft:command_block");

                game.set_mode(GameMode::Spectator);
                assert!(!player.destroy_block(game, stone, BlockDropDestination::World));
                assert_eq!(block(game, stone), "minecraft:stone");

                // NeoForge removes the separate tool check: an explicit uncancel can override it.
                game.set_mode(GameMode::Creative);
                game.inventory[game.selected] = Some(ItemStack::new("minecraft:diamond_sword", 1));
                assert!(player.destroy_block(game, stone, BlockDropDestination::World));
                assert_eq!(block(game, stone), "minecraft:air");
                assert_eq!(*heard.lock().unwrap(), [command, stone, stone]);
            });
        });
    }
}
