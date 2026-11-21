use mod_api::{
    ClientHost, Hooks, ModEntry, Mods, Registrar, RegistrationContext, Text, TooltipAppendContext,
    TooltipAppender, TooltipPlacement,
};
use std::sync::Arc;
use sunrise_client::{items::ItemStack, text::Text as EngineText};
use sunrise_core::nbt::Tag;

struct Line(&'static str);
impl TooltipAppender for Line {
    fn append(
        &self,
        _: &dyn ClientHost,
        _: &mod_api::ItemStack,
        _: &TooltipAppendContext,
        lines: &mut Vec<(Text, u32)>,
    ) {
        lines.push((Text::literal(self.0), u32::MAX));
    }
}

struct First;
#[mod_api::__subscriptions]
impl Hooks for First {
    fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
        registrar.register_tooltip_appender(
            "minecraft:repair_cost",
            TooltipPlacement::Before("minecraft:map_post_processing".into()),
            Arc::new(Line("before mod anchor")),
        );
        registrar.register_tooltip_appender(
            "minecraft:max_stack_size",
            TooltipPlacement::After("minecraft:lore".into()),
            Arc::new(Line("after lore")),
        );
        registrar.register_tooltip_appender(
            "minecraft:note_block_sound",
            TooltipPlacement::AfterAll,
            Arc::new(Line("absent component")),
        );
    }
}

struct Second;
#[mod_api::__subscriptions]
impl Hooks for Second {
    fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
        registrar.register_tooltip_appender(
            "minecraft:map_post_processing",
            TooltipPlacement::Before("minecraft:lore".into()),
            Arc::new(Line("mod anchor")),
        );
        registrar.register_tooltip_appender(
            "minecraft:max_damage",
            TooltipPlacement::Before("minecraft:damage".into()),
            Arc::new(Line("before damage")),
        );
        registrar.register_tooltip_appender(
            "minecraft:rarity",
            TooltipPlacement::AfterAll,
            Arc::new(Line("after all vanilla")),
        );
    }
}

#[test]
#[ignore = "daily"]
fn component_tooltip_graph_orders_visible_lines_and_keeps_hidden_anchors() {
    let mods: Mods = vec![
        ModEntry::new("first", Box::new(First)),
        ModEntry::new("second", Box::new(Second)),
    ]
    .into();
    mod_platform::testing::with_mods(mods, || {
        crate::game::native_entity_refs::tests::with_client_game(|game| {
            let mut stack = ItemStack::new("minecraft:iron_sword", 1);
            stack.damage = 1;
            let data = stack.data_mut();
            data.set_other("minecraft:repair_cost", Some(Tag::Int(1)));
            data.set_other("minecraft:max_stack_size", Some(Tag::Int(1)));
            data.set_other("minecraft:rarity", Some(Tag::String("common".into())));
            data.set_other(
                "minecraft:lore",
                Some(Tag::List(vec![Tag::compound([(
                    "text",
                    Tag::String("lore".into()),
                )])])),
            );
            // A transient component never written to the owned stack's saved patch.
            data.map_post_processing = Some(1);
            let durability = EngineText::translate(
                "item.durability",
                &[EngineText::literal("249"), EngineText::literal("250")],
            )
            .plain();
            let expected = [
                "before mod anchor",
                "mod anchor",
                "lore",
                "after lore",
                "before damage",
                durability.as_str(),
                "after all vanilla",
                "minecraft:iron_sword",
            ];
            let witness = |stack: &ItemStack| {
                game.build_stack_tooltip_lines(stack, false, false, true, true)
                    .into_iter()
                    .map(|(text, _)| text.plain())
                    .filter(|line| expected.contains(&line.as_str()) || line == "absent component")
                    .collect::<Vec<_>>()
            };
            assert_eq!(witness(&stack), expected);

            stack.data_mut().set_other(
                "minecraft:tooltip_display",
                Some(Tag::compound([(
                    "hidden_components",
                    Tag::List(vec![Tag::String("minecraft:map_post_processing".into())]),
                )])),
            );
            assert_eq!(
                witness(&stack),
                [
                    "before mod anchor",
                    "lore",
                    "after lore",
                    "before damage",
                    durability.as_str(),
                    "after all vanilla",
                    "minecraft:iron_sword",
                ],
            );
        });
    });
}
