# Breaking blocks as a player

`PlayerRef::destroy_block(host, pos, destination)` breaks a block as that player breaks it,
through vanilla's `ServerPlayerGameMode.destroyBlock`, as FTB Ultimine does for each block of a
vein. This is not a dig packet: there is no check for spawn protection, reach or mining progress.
For example, a survival player can directly destroy bedrock.

`break_block` fires on a live target at NeoForge's `CommonHooks.fireBlockBreak` site, before
removal. The main-hand item, game mode or a game-master permission check can start it cancelled.
Use `#[listener(receive_cancelled)]` on the hook method to observe those attempts. A listener can
veto a break or clear a cancellation, but clearing it does not bypass game-mode or game-master
restrictions.

`player_destroyed_block` hears a successful removal. Loot is rolled for the main-hand item and
its enchantments; `block_drops` sees the player as `breaker`; and item wear, statistics and
exhaustion follow the player's break path. Pass `BlockDropDestination::World` to spawn the
remaining item drops at the block or `BlockDropDestination::Inventory` to put them in the player's
inventory. Overflow drops from the player. Experience and indirect drops still spawn in the world.

The result is `true` only if this call removed the block. It is `false` for a player who is dead
or not present in the hook's level, an absent block, a refusal or veto, or a subject already
removed by a nested break, even if it left water or another block behind.

## A vein miner

A vein miner keeps the drops of its own breaks with `block_drops`: it takes the item list and the
experience during the breaks, without canceling, so the other effects of each break still happen.
Then it merges the stacks and sets them down at the block the player mined, with
`LevelRef::pop_resource` and `LevelRef::award_experience`. Nothing spawns at the other blocks.

```rust,ignore
struct VeinMiner {
    busy: AtomicBool,
    vein: Mutex<Vec<[i32; 3]>>,
    kept: Mutex<(Vec<ItemStack>, u32)>,
}

impl Hooks for VeinMiner {
    fn break_block(&self, host: &mut dyn ServerHost, event: &mut BreakBlockEvent<'_>) -> Flow {
        // Our own breaks come through here too.
        if self.busy.swap(true, Ordering::Relaxed) {
            return Flow::Continue;
        }
        let player = host.players().acting_player().unwrap();
        let origin = event.pos;
        let vein = find_vein(host, origin); // the origin first
        *self.vein.lock().unwrap() = vein.clone();
        for pos in vein {
            player.destroy_block(host, pos, BlockDropDestination::World);
        }
        self.busy.store(false, Ordering::Relaxed);
        let (stacks, experience) = std::mem::take(&mut *self.kept.lock().unwrap());
        let level = player.level(host).unwrap();
        for stack in merge(host, stacks) {
            let _ = level.pop_resource(host, origin, stack);
        }
        let [x, y, z] = origin.map(|v| f64::from(v) + 0.5);
        level.award_experience(host, x, y, z, experience);
        // The origin is already broken, by the loop.
        Flow::Cancel
    }

    fn block_drops(&self, _: &dyn LevelHost, event: &mut BlockDropsEvent<'_>) -> Flow {
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        if self.busy.load(Ordering::Relaxed) && self.vein.lock().unwrap().contains(&pos) {
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
        Flow::Continue
    }
}
```

`merge` folds each stack into the earlier ones it `is_stackable_with`, up to
`host.max_stack_size(stack.item())`, in the order they came. Filter `block_drops` on the
breaker too when other players can break the same blocks while the vein runs. What else a break
sets loose (a torch losing its support, a container's contents) is not the block's own drop and
still drops where it was.

Keep a guard like `busy` when a listener starts breaks of its own. Chained calls have no fixed
nesting limit, and the origin is still present when `break_block` first fires. Without a guard,
a listener that asks to break the same live block can call itself indefinitely.
