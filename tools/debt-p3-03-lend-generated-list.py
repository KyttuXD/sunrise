#!/usr/bin/env python3
"""Move the P3-03 engine modifier boundary and its fixtures onto borrowed lists."""
from pathlib import Path
import re


def edit(relative, replace):
    path = Path(relative)
    old = path.read_text()
    new = replace(old)
    assert new != old, path
    path.write_text(new)


world = 'crates/sunrise-world/src/'
items = 'crates/sunrise-items/src/'
client = 'crates/sunrise-client/src/game/modding/'

edit(world + 'loot/global_modifiers.rs', lambda s: s.replace(
    'use super::{LootContext, LootStack};',
    'use super::LootContext;\nuse mod_api::__private::{LootListAccess, LootStackAccess};',
).replace('FnMut(&LootStack)', 'FnMut(&dyn LootStackAccess)')
 .replace('&mut Vec<LootStack>', '&mut dyn LootListAccess'))
edit(world + 'loot/global_modifiers/tests.rs', lambda s: s.replace(
    '&mut Vec<LootStack>', '&mut dyn LootListAccess',
))


def fixture(s):
    s = s.replace('&mut Vec<LootStack>', '&mut dyn LootListAccess')
    s = re.sub(r'generated\s*\.iter\(\)', '(0..generated.len())\n                    .map(|i| generated.get(i).unwrap())', s)
    s = re.sub(r'generated\[(\d+)\]\.count = (-?\d+);', r'generated.get_mut(\1).unwrap().set_count(\2);', s)
    s = s.replace('stack.count)', 'stack.count())').replace('stack.item.as_str(), stack.count()', 'stack.item_id(), stack.count()')
    s = s.replace('stack.item.clone(), stack.count()', 'stack.item_id().to_owned(), stack.count()')
    return s.replace('generated[0]\n                .components\n                .push(("minecraft:custom_data".into(), Some(Tag::Int(42))));', 'generated.get_mut(0).unwrap().push_component("minecraft:custom_data".into(), Some(Tag::Int(42)));').replace('generated[1].components,', 'generated.get(1).unwrap().components(),')


for path in [world + 'loot_query_tests.rs', items + 'loot_query_tests.rs']:
    def migrate(s):
        def scoped(m):
            return 'use sunrise_world::loot::global_modifiers::LootListAccess;\n    ' + fixture(m[0]) if path.startswith(items) else fixture(m[0])
        return re.sub(r'impl ModifierApply for \w+ \{.*?^    \}', scoped, s, flags=re.M | re.S) if not path.startswith(items) else re.sub(r'(impl ModifierApply for \w+ \{.*?^    \})', lambda m: fixture(m[0]), s, flags=re.M | re.S)
    edit(path, lambda s: ('use sunrise_world::loot::global_modifiers::LootListAccess;\n' if path.startswith(items) else 'use mod_api::__private::LootListAccess;\n') + migrate(s))

edit(client + 'api_values/loot_values.rs', lambda s: s.replace(
    'fn loot_stack_view(stack: &engine::LootStack)', 'fn loot_stack_view(stack: &dyn LootStackAccess)',
).replace(
    'fn loot_list(stacks: &mut Vec<engine::LootStack>)',
    'fn loot_list(stacks: &mut dyn mod_api::__private::LootListAccess)',
))
edit(client + 'global_loot_modifiers.rs', lambda s: s.replace(
    'generated: &mut Vec<loot::LootStack>', 'generated: &mut dyn mod_api::__private::LootListAccess',
))
