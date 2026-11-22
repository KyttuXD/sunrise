use super::*;
use sunrise_world::loot::{
    LootToolAccess,
    global_modifiers::{Generated, LootCandidateAccess as _, LootListAccess, LootStackAccess},
};

impl LootToolAccess for ItemStack {
    fn item_id(&self) -> &str {
        &self.item
    }

    fn enchantment_at(&self, index: usize) -> Option<(&str, i32)> {
        self.enchantments()
            .get(index)
            .map(|(id, level)| (id.as_ref(), *level as i32))
    }
}

impl sunrise_world::loot::LootAbilityTool for ItemStack {
    fn item_id(&self) -> &str {
        &self.item
    }

    fn is_empty(&self) -> bool {
        self.count == 0 || &*self.item == "minecraft:air"
    }

    fn resident_scalars(&self) -> Option<(std::num::NonZeroU32, Option<u32>)> {
        let count = std::num::NonZeroU32::new(self.count)?;
        let damage = self
            .carries_component(None, "minecraft:damage")
            .then_some(self.damage);
        Some((count, damage))
    }

    fn encode_components(&self) -> Option<sunrise_core::nbt::Tag> {
        crate::item_data::encode_extra_components(self)
    }
}

impl<R: RandomSource> LootContext<'_, R> {
    pub(super) fn stock_condition(&self, c: &Value) -> Option<bool> {
        sunrise_world::loot::query::condition(
            c,
            &self.query,
            self.held
                .map(|tool| tool as &dyn sunrise_world::loot::LootAbilityTool),
            || self.roll_facts(),
            self.items,
            |item, ability| self.items.vanilla_can_perform(item, ability),
        )
    }
    /// Non-raw completion splits after table functions, retaining the caller's
    /// context and random. Nested entries evaluate raw and never complete a query.
    pub(super) fn roll_root(
        &self,
        id: Option<&str>,
        table: &Value,
        random: &mut R,
    ) -> Vec<ItemStack> {
        let raw = self.evaluate_candidates(table, random);
        self.complete_root(id, raw, random, None)
    }
    pub(super) fn complete_root(
        &self,
        id: Option<&str>,
        raw: Vec<Candidate>,
        random: &mut R,
        dynamic: Option<&[ItemStack]>,
    ) -> Vec<ItemStack> {
        let mut stacks = self.items.split_loot_stacks(raw);
        self.query.finish(id);
        let generation = &self.items.global_modifiers;
        let dynamic = dynamic.unwrap_or(&[]);
        stacks = self.expand_dynamic(stacks, dynamic, true);
        let mut generated = Generated(stacks);
        if !generation.is_empty() {
            let mut scope = ModifierScope {
                context: self,
                random,
                dynamic,
            };
            generation.apply(&mut scope, &mut generated);
        }
        generated
            .0
            .into_iter()
            .map(|stack| stack.into_stack(self.items))
            .collect()
    }
    fn expand_dynamic(
        &self,
        stacks: Vec<Candidate>,
        dynamic: &[ItemStack],
        split: bool,
    ) -> Vec<Candidate> {
        if !stacks
            .iter()
            .any(|stack| stack.item_id().starts_with(DYNAMIC))
        {
            return stacks;
        }
        let mut out = Vec::with_capacity(stacks.len());
        for stack in stacks {
            if stack.item_id().starts_with(DYNAMIC) {
                let drops = dynamic.iter().cloned().map(Candidate::from);
                if split {
                    out.extend(self.items.split_loot_stacks(drops.collect()));
                } else {
                    out.extend(drops);
                }
            } else {
                out.push(stack);
            }
        }
        out
    }
    /// Borrow the parameters this evaluator has; random is lent separately.
    pub(super) fn roll_facts(&self) -> sunrise_world::loot::LootContext<'_> {
        sunrise_world::loot::LootContext {
            queried_table_id: self.query.id(),
            queried_stack: self.query.queried_stack.as_ref(),
            entity: self.target.map(|(entity, _)| entity),
            interacting_entity: self.interacting_entity.map(|(entity, _)| entity),
            enchantment_level: None,
            damage: None,
            player: None,
            block: (!self.block.is_empty()).then_some(self.block),
            tool: self.held.map(|tool| tool as &dyn LootToolAccess),
            luck: 0.0,
            biome: None,
            place: self.place.as_deref(),
        }
    }
}

impl ItemStack {
    /// Convert loot facts using the same component write path as native loot functions.
    /// Caller consumption: signed empty counts become zero without removing their list position.
    pub fn from_loot(value: &sunrise_world::loot::LootStack) -> Self {
        let mut stack = Self {
            item: Arc::from(value.item.as_str()),
            count: 1,
            damage: 0,
            extra: None,
        };
        let before = loot_of(&stack);
        apply_loot(&before, value, &mut stack, |_, _| None);
        if let Some(search) = &value.exploration {
            stack.defer_exploration_map(search);
        }
        stack
    }

    /// Retain a loot map's deferred search until the world consumer can resolve it.
    pub(crate) fn defer_exploration_map(&mut self, map: &sunrise_world::loot::ExplorationMap) {
        use sunrise_core::nbt::Tag;
        let search = Tag::compound([
            ("destination", Tag::String(map.destination.as_str().into())),
            ("decoration", Tag::String(map.decoration.as_str().into())),
            ("zoom", Tag::Byte(map.zoom as i8)),
            ("search_radius", Tag::Int(map.search_radius)),
            (
                "skip_existing_chunks",
                Tag::Byte(i8::from(map.skip_existing_chunks)),
            ),
        ]);
        let mut custom = self.custom_data().cloned().unwrap_or_default();
        custom.insert("sunrise:exploration_map".into(), search);
        self.data_mut()
            .set_loot_component("minecraft:custom_data", &Some(Tag::Compound(custom)));
    }
}

impl Items {
    /// `LootTable.createStackSplitter`, also used after resolving dynamic drops.
    pub(super) fn split_loot_stacks(&self, mut stacks: Vec<Candidate>) -> Vec<Candidate> {
        let Some(first) = stacks.iter_mut().position(|stack| {
            stack.count() > 1
                && !stack.item_id().starts_with(DYNAMIC)
                && stack.count() > crate::bundle::max_stack(self, stack.stock(self).0) as i32
        }) else {
            return stacks;
        };
        let mut out = Vec::with_capacity(stacks.len());
        let mut stacks = stacks.into_iter();
        out.extend(stacks.by_ref().take(first));
        for mut stack in stacks {
            // Empty positions and dynamic markers are not normalized here.
            if stack.count() > 1 && !stack.item_id().starts_with(DYNAMIC) {
                let max = crate::bundle::max_stack(self, stack.stock(self).0) as i32;
                let mut count = stack.count();
                while count > max {
                    let mut part = stack.clone();
                    part.set_count(max);
                    out.push(part);
                    count -= max;
                }
                stack.set_count(count);
            }
            out.push(stack);
        }
        out
    }

    pub(super) fn scoped_block_roll(
        &self,
        registry: &BlockRegistry,
        state: BlockStateId,
        roll: BlockLootRoll<'_>,
        random: &mut impl RandomSource,
        sequences: Option<(&mut crate::random_sequences::RandomSequences, i64)>,
    ) -> Vec<ItemStack> {
        let Some(block) = registry.state(state) else {
            return vec![];
        };
        let table = self.block_loot_table(registry, state);
        let Some(id) = table else {
            return vec![];
        };
        let Some(table) = self.loot.get(&id) else {
            return vec![];
        };
        sunrise_world::loot::query::with_query(sunrise_world::loot::query::current_query(), || {
            // LootTable.getRandomItems draws from the table's random sequence.
            if let (Some(seq), Some(sequences)) = (
                table.get("random_sequence").and_then(Value::as_str),
                sequences,
            ) {
                let random = sequences.0.get(seq, sequences.1);
                return self.roll_block_table(
                    &id,
                    table,
                    &block.name,
                    &block.properties,
                    roll,
                    random,
                );
            }
            self.roll_block_table(&id, table, &block.name, &block.properties, roll, random)
        })
    }
    /// `Block.dropFromBlockInteractLootTable`: the `block_interact` table `id`
    /// for a right click on `state` (its `match_block` conditions see the
    /// state), from the table's random sequence when it has one.
    /// `interacting` is `INTERACTING_ENTITY`, the player or mob that clicked;
    /// `location` reads the world around `ORIGIN`, the block's centre.
    pub fn interact_loot(
        &self,
        id: &str,
        registry: &BlockRegistry,
        state: BlockStateId,
        params: InteractionLootParams<'_>,
        random: &mut impl RandomSource,
        sequences: &mut crate::random_sequences::RandomSequences,
        seed: i64,
    ) -> Vec<ItemStack> {
        let (Some(table), Some(block)) = (self.loot.get(id), registry.state(state)) else {
            return vec![];
        };
        sunrise_world::loot::query::with_query(sunrise_world::loot::query::current_query(), || {
            match table.get("random_sequence").and_then(Value::as_str) {
                Some(sequence) => {
                    let random = sequences.get(sequence, seed);
                    self.interact_roll(id, table, &block.name, &block.properties, params, random)
                }
                None => {
                    self.interact_roll(id, table, &block.name, &block.properties, params, random)
                }
            }
        })
    }
}

#[cfg(test)]
#[path = "loot_query_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "loot_split_tests.rs"]
mod split_tests;

/// One capability lending the original evaluator and selected random through
/// the root pass. Raw sub-rolls retain all borrowed parameters and visited guards.
struct ModifierScope<'s, 'a, R> {
    context: &'s LootContext<'a, R>,
    random: &'s mut R,
    dynamic: &'s [ItemStack],
}
impl<R: RandomSource> ModifierScope<'_, '_, R> {
    fn eval_raw(&mut self, id: &str) -> std::result::Result<Vec<Candidate>, String> {
        let context = self.context;
        let table = context
            .items
            .loot
            .get(id)
            .ok_or_else(|| format!("table: missing loot table {id}"))?;
        let Some(_guard) = NestedRoll::enter(id) else {
            return Ok(Vec::new());
        };
        // Raw output retains emitted empties all the way through nested tables.
        // The original root's output policy does not filter a raw sub-roll.
        let raw = LootContext {
            query: context.query.clone(),
            place: context.place.clone(),
            items: context.items,
            local_bindings: None,
            block: context.block,
            properties: context.properties,
            held: context.held,
            killed_by_player: context.killed_by_player,
            explosion_radius: context.explosion_radius,
            this_entity: context.this_entity,
            entity_levels: context.entity_levels,
            block_levels: context.block_levels,
            target: context.target,
            target_entity: context.target_entity,
            interacting_entity: context.interacting_entity,
            location: context.location,
            keep_empty: true,
        }
        .evaluate_candidates(table, self.random);
        Ok(context.expand_dynamic(raw, self.dynamic, false))
    }
}
impl<R: RandomSource> sunrise_world::loot::global_modifiers::ModifierRoll
    for ModifierScope<'_, '_, R>
{
    fn facts(&self) -> sunrise_world::loot::LootContext<'_> {
        self.context.roll_facts()
    }
    fn next_int(&mut self) -> i32 {
        self.random.next_int()
    }
    fn next_int_bound(&mut self, bound: i32) -> i32 {
        self.random.next_int_bound(bound).unwrap_or(0)
    }
    fn next_long(&mut self) -> i64 {
        self.random.next_long()
    }
    fn next_float(&mut self) -> f32 {
        self.random.next_float()
    }
    fn next_double(&mut self) -> f64 {
        self.random.next_double()
    }
    fn next_boolean(&mut self) -> bool {
        self.random.next_boolean()
    }
    fn next_gaussian(&mut self) -> f64 {
        self.random.next_gaussian()
    }
    fn condition(&mut self, condition: &sunrise_world::loot::types::PreparedCondition) -> bool {
        self.context.condition_with_bindings(
            condition.value(),
            self.random,
            Some(condition.rules()),
        )
    }
    fn roll_raw(
        &mut self,
        id: &str,
        visit: &mut dyn FnMut(&dyn LootStackAccess),
    ) -> std::result::Result<(), String> {
        let raw = sunrise_world::loot::global_modifiers::in_engine(|| self.eval_raw(id))?;
        for stack in &raw {
            visit(stack.lend());
        }
        Ok(())
    }
    fn append_raw(
        &mut self,
        id: &str,
        generated: &mut dyn LootListAccess,
    ) -> std::result::Result<(), String> {
        let target = generated
            .as_any_mut()
            .downcast_mut::<Generated<Candidate>>()
            .ok_or("target list is not backed by the active loot evaluator")?;
        target.0.extend(self.eval_raw(id)?);
        Ok(())
    }
    fn append_split(
        &mut self,
        id: &str,
        generated: &mut dyn LootListAccess,
    ) -> std::result::Result<(), String> {
        let target = generated
            .as_any_mut()
            .downcast_mut::<Generated<Candidate>>()
            .ok_or("target list is not backed by the active loot evaluator")?;
        target
            .0
            .extend(self.context.items.split_loot_stacks(self.eval_raw(id)?));
        Ok(())
    }
}
