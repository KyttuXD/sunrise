//! Non-raw loot table completion, distinct from nested raw evaluation.
use super::*;

impl<R: RandomSource + Clone> Context<'_, R> {
    /// Every non-raw entry point completes here with its original context and
    /// random still available. Nested entries call `table_raw` instead: their
    /// functions run before the root splitter and they never complete a query.
    pub(super) fn roll_root(&mut self, id: Option<&str>, table: &Value) -> Vec<LootStack> {
        let mut items = Vec::new();
        self.table_raw(id.unwrap_or("<root>"), table, &mut |context, stack| {
            context.data.split(stack, &mut items)
        });
        // Every non-raw roll completes after the splitter with its own
        // context and random, before the caller consumes the list.
        self.query.finish(id);
        let generation = &self.data.global_modifiers;
        if generation.is_empty() {
            return items;
        }
        generation.apply(self, &mut items);
        items
    }
}

impl<R: RandomSource + Clone> global_modifiers::ModifierRoll for Context<'_, R> {
    fn facts(&self) -> LootContext<'_> {
        query_context::roll_facts(
            &self.query,
            self.place.as_deref(),
            self.entity,
            self.biome,
            self.luck,
        )
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
    fn condition(&mut self, condition: &types::PreparedCondition) -> bool {
        self.condition_with_bindings(condition.value(), Some(condition.rules()))
    }
    fn roll_raw(
        &mut self,
        id: &str,
        visit: &mut dyn FnMut(&dyn LootStackAccess),
    ) -> Result<(), String> {
        let mut output = Vec::new();
        self.append_raw(id, &mut output)?;
        for stack in &output {
            visit(stack);
        }
        Ok(())
    }
    fn append_raw(&mut self, id: &str, generated: &mut dyn LootListAccess) -> Result<(), String> {
        let table = self
            .data
            .tables
            .get(id)
            .ok_or_else(|| format!("table: missing loot table {id}"))?;
        if let Some(vec) = generated.as_any_mut().downcast_mut::<Vec<LootStack>>() {
            self.table_raw(id, table, &mut |_, stack| vec.push(stack));
        } else {
            self.table_raw(id, table, &mut |_, stack| push_loot_stack(generated, stack));
        }
        Ok(())
    }
}

fn push_loot_stack(generated: &mut dyn LootListAccess, stack: LootStack) {
    let slot = generated.push_empty(&stack.item);
    slot.set_count(stack.count);
    slot.set_damage(stack.damage);
    slot.set_enchantments(stack.enchantments);
    slot.set_potion_id(stack.potion);
    slot.set_map(stack.map);
    slot.set_exploration(stack.exploration.map(|map| ApiExplorationMap {
        destination: map.destination,
        decoration: map.decoration,
        zoom: map.zoom,
        search_radius: map.search_radius,
        skip_existing_chunks: map.skip_existing_chunks,
    }));
    slot.set_ominous_amplifier(stack.ominous_amplifier);
    slot.set_stew_effects(stack.stew_effects);
    slot.set_names_values(stack.names);
    slot.set_components(stack.components);
}
