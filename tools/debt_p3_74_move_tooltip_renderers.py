#!/usr/bin/env python3
"""Move the existing ItemStack detail renderers into component dispatch arms."""

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
GAME = ROOT / "crates/sunrise-client/src/game/blocks/stations.rs"
START = '        details.start("minecraft:tropical_fish/pattern", lines.len());\n'
END = '        // ItemStack.addDetailsToTooltipTail:'


def arm(variant, body):
    return f"                {variant} => {{\n{body}                }}\n"


def main():
    source = GAME.read_text()
    start = source.index(START)
    end = source.index(END, start)
    old = source[start:end]
    markers = list(re.finditer(r'^        details\.start\("minecraft:([a-z_/]+)", lines\.len\(\)\);\n', old, re.M))
    assert len(markers) == 21, len(markers)
    arms = []
    for index, mark in enumerate(markers):
        key = mark[1]
        stop = markers[index + 1].start() if index + 1 < len(markers) else len(old)
        body = old[mark.end():stop]
        variant = ''.join(word.title() for word in re.split(r'[_/]', key))
        if key == 'trim':
            body, enchantments = body.split('        for component in [EnchantmentComponent::Stored, EnchantmentComponent::Applied] {\n')
            enchantments = enchantments.replace('            details.start(component.id(), lines.len());\n', '')
            assert enchantments.endswith('        }\n')
            arms.append(arm(variant, body))
            arms.append(arm('StoredEnchantments | Enchantments',
                            '        let component = if vanilla == StoredEnchantments {\n'
                            '            EnchantmentComponent::Stored\n'
                            '        } else {\n'
                            '            EnchantmentComponent::Applied\n'
                            '        };\n' + enchantments.removesuffix('        }\n')))
        elif key == 'block_entity_data':
            body, adventure = body.split('        // AdventureModePredicate.addToTooltip under its header, for\n')
            adventure, damage = adventure.split('        details.end(lines.len());\n')
            arms.append(arm(variant, body))
            arms.append(arm('CanBreak | CanPlaceOn',
                            '        // AdventureModePredicate.addToTooltip under its header.\n'
                            + adventure[adventure.index('        for (text, c)'):]
                            .replace('            &display,\n', '            &display,\n            id,\n')))
            arms.append(arm('Damage', damage))
        else:
            if key == 'fireworks':
                variant = 'Fireworks | FireworkExplosion'
                body = body.replace('firework_lines(stack, &translate)',
                                    'firework_component_lines(stack, &display, id, &translate)')
            elif key == 'entity_data':
                body = body.replace('        let entity_data = block_entity_item::item_entity_data(stack, self.items());\n', '')
            arms.append(arm(variant, body))

    rendered = '''        use mod_api::__private::{VanillaTooltipAppender::*, VANILLA_TOOLTIP_APPENDERS};
        let entity_data = block_entity_item::item_entity_data(stack, self.items());
        let mut appenders = appenders.iter().peekable();
        let mut projected_stack = None;
        for (slot, vanilla) in VANILLA_TOOLTIP_APPENDERS.iter().copied().map(Some).chain([None]).enumerate() {
            while let Some(appender) = appenders.next_if(|a| a.slot() == slot) {
                if !appender.enabled()
                    || !display.shows(appender.component())
                    || !stack.carries_component(item_info, appender.component())
                {
                    continue;
                }
                let stack = match projected_stack.get_or_insert_with(|| stack.try_into_api()) {
                    Ok(stack) => stack,
                    Err(error) => {
                        log::error!("mod {}: tooltip appender for {} could not read the stack: {error}", appender.mod_id(), appender.component());
                        continue;
                    }
                };
                if let Some(added) = appender.append(self, stack, &context) {
                    for (text, color) in added {
                        match text.try_into_engine(&()) {
                            Ok(text) => lines.push((text, color)),
                            Err(error) => log::error!("mod {}: tooltip appender for {} returned invalid text: {error}", appender.mod_id(), appender.component()),
                        }
                    }
                }
            }
            let Some((vanilla, id)) = vanilla else { break };
            match vanilla {
'''
    rendered += ''.join(arms)
    rendered += '''                // These registered vanilla components do not yet emit hover lines in Sunrise.
                ContainerLoot | PotDecorations | Profile | SulfurCubeContent
                | IntangibleProjectile | SignTextFront | SignTextBack => {}
            }
        }
'''
    source = source[:start] + rendered + source[end:]
    source = source.replace('        let mut details = sunrise_client::item_tooltip::DetailMarks::new(!appenders.is_empty());\n', '')
    source = source.replace('        // The component details, marked for the placements of the registered appenders.\n', '        // The frozen component appender order, after the item\'s own lines.\n')
    start = source.index('        // RegisterTooltipAppendersEvent: each registered appender\'s lines, where its placement puts them.')
    end = source.index('        // ItemTooltipEvent, once the tooltip vanilla shows is complete.', start)
    GAME.write_text(source[:start] + source[end:])


if __name__ == '__main__':
    main()
