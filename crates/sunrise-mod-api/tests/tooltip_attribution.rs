#![cfg(feature = "testing")]
use mod_api::prelude::ItemsHost;
use mod_api::*;
use std::sync::Arc;

#[test]
fn a_panicking_tooltip_appender_restores_owner_and_disables_only_itself() {
    mod_api::__private::initialize().unwrap();
    struct Check(bool);
    impl TooltipAppender for Check {
        fn append(
            &self,
            _: &dyn ClientHost,
            _: &ItemStack,
            _: &TooltipAppendContext,
            lines: &mut Vec<(Text, u32)>,
        ) {
            assert_eq!(mod_api::content::crash::current_native_mod(), Some("inner"));
            lines.push((Text::literal("an appender line"), u32::MAX));
            assert!(!self.0, "test unwind");
        }
    }
    struct Inner;
    #[mod_api::__subscriptions]
    impl Hooks for Inner {
        fn register(&self, registrar: &mut dyn Registrar, _: &RegistrationContext) {
            registrar.register_tooltip_appender(
                "minecraft:repair_cost",
                TooltipPlacement::AfterAll,
                Arc::new(Check(true)),
            );
            registrar.register_tooltip_appender(
                "minecraft:max_stack_size",
                TooltipPlacement::AfterAll,
                Arc::new(Check(false)),
            );
        }
    }
    let mods = [ModEntry::new("inner", Box::new(Inner))];
    mod_api::__private::part_tests::install_parts_fixture(&mods).unwrap();
    let context = TooltipAppendContext {
        player: 0,
        player_entity_id: 0,
        tick_rate: 20.0,
        advanced: false,
        creative: false,
        nested: false,
    };
    let mut host = mod_api::testing::FakeHost::new();
    let id = Identifier::parse("minecraft:stick").unwrap();
    host.registries.resolve_items([id.clone()]).unwrap();
    let stack = ItemStack::of(host.item_key(&id).unwrap());
    let outer = mod_api::content::crash::enter_native_mod(mod_api::content::crash::ModCall {
        id: "outer",
        hook: "outer",
        part: None,
    });
    let [broken, healthy] = mod_api::__private::tooltip_appenders() else {
        panic!("expected both registered appenders");
    };
    assert!(broken.append(&host, &stack, &context).is_none());
    assert_eq!(mod_api::content::crash::current_native_mod(), Some("outer"));
    assert!(!broken.enabled());
    assert!(broken.append(&host, &stack, &context).is_none());
    assert_eq!(
        healthy.append(&host, &stack, &context),
        Some(vec![(Text::literal("an appender line"), u32::MAX)]),
    );
    assert_eq!(mod_api::content::crash::current_native_mod(), Some("outer"));
    let notices = mod_api::__private::game::take_callback_panic_notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].mod_id, "inner");
    assert!(notices[0].message.contains("test unwind"));
    drop(outer);
    assert_eq!(mod_api::content::crash::current_native_mod(), None);
}
