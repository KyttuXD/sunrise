use super::*;
use crate::game::commands::Target;
use sunrise_client::entities::ItemEntity;
use sunrise_client::join_seq::{JoinList, JoinSeq};
use sunrise_core::random::RandomSource;

impl Game<'_> {
    /// `ItemEntity.playerTouch` before the delay and target check. The resident item
    /// is available to listeners; `False` vetoes this attempt without setting a delay.
    pub(in crate::game) fn native_item_picked_up(
        &mut self,
        player: u128,
        item: ItemEntityRef,
        can_pickup: &mut TriState,
    ) {
        self.fire_item_picked_up(player, item, can_pickup);
    }

    /// `ItemEntity.playerTouch` after `Inventory.add` took all or part of the stack.
    pub(in crate::game) fn native_item_picked_up_post(
        &mut self,
        player: u128,
        item: ItemEntityRef,
        original: &ItemStack,
        remaining: &mut Option<ItemStack>,
    ) {
        self.fire_item_picked_up_post(player, item, original, remaining);
    }

    /// `ItemEntity.playerTouch` for the host's player. Restore the tick pass before
    /// touching its items, so Pre and Post see the same residents as on the guest path.
    pub(in crate::game) fn native_player_pickups(
        &mut self,
        items: JoinList<ItemEntity>,
        (min, max): ([f64; 3], [f64; 3]),
        feet: [f64; 3],
        own: [i32; 4],
    ) {
        // RestoreOrder::SurvivorsFirst keeps the ticks' spawn buffer out of this pass.
        let end = self
            .dropped
            .first()
            .map_or_else(JoinSeq::upcoming, |d| d.join);
        self.put_back_dropped(items);
        let mut i = 0;
        while let Some(item) = self.dropped.get(i).filter(|item| item.join < end) {
            let b = item.bounds();
            let touching = b.min_position().x < max[0]
                && b.max_position().x > min[0]
                && b.min_position().y < max[1]
                && b.max_position().y > min[1]
                && b.min_position().z < max[2]
                && b.max_position().z > min[2];
            if !touching {
                i += 1;
                continue;
            }
            let join = item.join;
            let next = self.dropped.get(i + 1).map(|d| d.join);
            let _ = self.native_player_touch_item(i, own, Some(feet));
            if self.dropped.get(i).is_some_and(|d| d.join == join) {
                i += 1;
            } else if self.dropped.get(i).map(|d| d.join) != next {
                // A hook moved more than the current row. Resume at the item's
                // successor, as the item tick pass does when a row goes away.
                i = self.dropped.locate(join).map_or_else(
                    || {
                        self.dropped
                            .iter()
                            .position(|d| d.join > join)
                            .unwrap_or(self.dropped.len())
                    },
                    |at| at + 1,
                );
            }
        }
    }

    /// Shared player touch on a resident item. The host draws its take animation
    /// locally; for a guest, return the count for `ClientboundTakeItemEntityPacket`.
    pub(in crate::game) fn native_player_touch_item(
        &mut self,
        index: usize,
        own: [i32; 4],
        host_feet: Option<[f64; 3]>,
    ) -> Option<u32> {
        let listened = mod_api::__private::listened;
        let item = &self.dropped[index];
        let (id, join) = (item.id, item.join);
        let live = |game: &Self, hint: usize| {
            if game.persist.removals.contains(&Target::Item(id)) {
                None
            } else if game.dropped.get(hint).is_some_and(|d| d.join == join) {
                Some(hint)
            } else {
                game.dropped.locate(join)
            }
        };
        if self.persist.removals.contains(&Target::Item(id)) {
            return None;
        }
        let pre = listened(mod_api::HookId::ItemPickedUp)
            .then(|| self.native_item_handle(id))
            .flatten();
        if pre.is_none()
            && !Self::pickup_decision(item.pickup_delay, item.target, own, TriState::Default)
        {
            return None;
        }
        // NeoForge captures getItem() before Pre, not after it. A setItem during
        // Pre replaces the resident stack, not the stack Inventory.add is given.
        let original = item.stack.clone();
        let revision = item.stack_revision;
        let mut can_pickup = TriState::Default;
        let index = if let Some(handle) = pre {
            let player = self.native_acting_id();
            self.native_item_picked_up(player, handle, &mut can_pickup);
            if can_pickup == TriState::False {
                return None;
            }
            let index = live(self, index)?;
            let item = &self.dropped[index];
            if !Self::pickup_decision(item.pickup_delay, item.target, own, can_pickup) {
                return None;
            }
            index
        } else {
            index
        };
        let before = original.count;
        if before == 0 || &*original.item == "minecraft:air" {
            return None;
        }
        let rest = self
            .add_stack_to_inventory(original.clone())
            .filter(|_| !self.creative);
        let inserted = before.saturating_sub(rest.as_ref().map_or(0, |s| s.count));
        if inserted == 0 {
            return None;
        }
        let post = listened(mod_api::HookId::ItemPickedUpPost)
            .then(|| self.native_item_handle(id))
            .flatten();
        let mut remaining = post.as_ref().and(rest.as_ref()).cloned();
        let item = &mut self.dropped[index];
        let thrower = item.thrower;
        let mut replaced = revision != item.stack_revision;
        let revision = item.stack_revision;
        if !replaced {
            match rest {
                Some(rest) => item.stack = rest,
                None => item.stack.count = 0,
            }
            item.needs_sync = true;
        }
        let mut after = Some(index);
        if let Some(handle) = post {
            let before_post = remaining.clone();
            let player = self.native_acting_id();
            self.native_item_picked_up_post(player, handle, &original, &mut remaining);
            after = live(self, index);
            if let Some(at) = after {
                if self.dropped[at].stack_revision != revision {
                    replaced = true;
                } else if remaining != before_post {
                    self.journal_set_item_entity(at, remaining);
                }
            }
        }
        // When DATA_ITEM was replaced, count the take from the captured stack,
        // not from an independent stack a listener placed on the ground.
        let left = after.map_or(0, |at| self.dropped[at].stack.count);
        let count = if replaced {
            inserted
        } else {
            before.saturating_sub(left)
        };
        if count == 0 {
            return None;
        }
        let live_revision = after.map(|at| self.dropped[at].stack_revision);
        // NeoForge discards when Inventory.add exhausted the captured stack,
        // even when a listener replaced DATA_ITEM in the meantime.
        let emptied = if replaced {
            inserted == before
        } else {
            left == 0
        };
        self.award_stat("picked_up", &original.item, count as i32);
        if let Some(thrower) = thrower {
            let picked = match after {
                Some(at) if !replaced && left > 0 => {
                    let at = live(self, at);
                    at.map(|at| self.dropped[at].stack.clone())
                }
                _ => None,
            };
            self.trigger_thrown_picked_up_by_player(picked.as_ref().unwrap_or(&original), thrower);
        }
        let at = live(self, after?)?;
        let animation = if emptied && Some(self.dropped[at].stack_revision) == live_revision {
            let item = self.remove_dropped(at);
            host_feet.map(|_| item)
        } else {
            host_feet.map(|_| self.dropped[at].clone())
        };
        if let (Some(feet), Some(mut item)) = (host_feet, animation) {
            item.stack = original;
            self.native_take_item(item, feet);
        }
        Some(count)
    }

    /// The take animation of `item` toward the host's player, and its sound at `feet`.
    fn native_take_item(&mut self, item: ItemEntity, feet: [f64; 3]) {
        self.add_pickup(
            crate::game::pickups::Taken::Item(Box::new(item)),
            crate::game::pickups::Collector::Player,
        );
        // `ClientPacketListener.handleTakeItemEntity`: the listener's random, never the level's.
        let mut random = sunrise_client::random_source::create();
        let pitch = ((random.next_float() - random.next_float()) * 0.7 + 1.0) * 2.0;
        self.audio
            .play("minecraft:entity.item.pickup", Some(feet), 0.2, pitch);
    }

    /// The item entity `id` as the hooks name it, even for a toss that has not joined.
    pub(in crate::game) fn native_item_handle(&self, id: u32) -> Option<ItemEntityRef> {
        let epoch = self.native_lifecycle.epoch()?;
        Some(ItemEntityRef::new(EntityRef::new(epoch, id)))
    }

    /// Vanilla's delay and target rule unless Pre forces the decision either way.
    pub(in crate::game) fn pickup_decision(
        pickup_delay: i32,
        target: Option<[i32; 4]>,
        player: [i32; 4],
        answer: TriState,
    ) -> bool {
        answer.or(pickup_delay == 0 && target.is_none_or(|t| t == player))
    }
}
