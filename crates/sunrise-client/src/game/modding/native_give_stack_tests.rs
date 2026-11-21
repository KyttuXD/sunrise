//! `player_give_stack` against a real LAN guest and `item_from_snbt` on the public host.

use super::{
    api_values::TryIntoApi, native_entity_refs::tests::with_game,
    native_guest_turn_tests::with_guests,
};
use mod_api::{Host, ItemStack, PlayerDataRefusal, prelude::*};
use sunrise_core::nbt::parse_snbt;
use sunrise_items::item_nbt;

fn api_item(game: &super::Game<'_>, snbt: &str) -> Option<ItemStack> {
    let host: &dyn Host = game;
    host.item_from_snbt(snbt)
}

/// What a mod reads from SNBT and writes back is the compound `ItemStack.CODEC` started
/// from, through the API's own stack and back.
#[test]
#[ignore = "daily"]
fn an_snbt_item_goes_through_the_api_unchanged() {
    with_guests(|game| {
        for snbt in [
            r#"{id:"minecraft:diamond_sword",count:1,components:{"minecraft:damage":5,"minecraft:enchantments":{"minecraft:sharpness":3},"minecraft:custom_name":"Blade","minecraft:lore":["one","two"],"minecraft:custom_data":{husksync:{v:1b}}}}"#,
            r#"{id:"minecraft:stone",count:64}"#,
            r#"{id:"minecraft:potion",count:1,components:{"minecraft:potion_contents":{potion:"minecraft:strong_healing"}}}"#,
            r#"{id:"minecraft:written_book",count:1,components:{"minecraft:written_book_content":{title:{raw:"Log"},author:"guest13",pages:[{raw:"one"}]}}}"#,
        ] {
            let engine =
                item_nbt::item_from_snbt(snbt, |_| false).unwrap_or_else(|error| panic!("{error}"));
            let api = api_item(game, snbt).unwrap();
            assert_eq!(
                Some(&api),
                engine.clone().try_into_api().ok().as_ref(),
                "{snbt}"
            );
            let (tag, _) = parse_snbt(snbt).unwrap();
            assert_eq!(item_nbt::item_to_nbt(&engine, game.items()), tag, "{snbt}");
        }
    });
}

/// This API refuses with None when the grammar or saved-item codec refuses the input.
#[test]
#[ignore = "daily"]
fn item_from_snbt_refuses_what_is_not_one_item_compound() {
    with_guests(|game| {
        let host: &dyn Host = game;
        let count = |snbt: &str| host.item_from_snbt(snbt).map(|s| s.count());
        assert_eq!(
            count(r#"{id:"stone"}"#),
            Some(1),
            "stone is minecraft:stone"
        );
        assert_eq!(count(r#"{id:"minecraft:stone",count:99}"#), Some(99));
        assert_eq!(count(r#"{id:"minecraft:stone",count:1}"#), Some(1));
        assert_eq!(
            count(r#"{id:"minecraft:stone",count:4294967297L}"#),
            Some(1),
            "Codec.INT uses Number.intValue before checking the range"
        );
        assert_eq!(
            count(r#"{id:"minecraft:stone",count:100}"#),
            None,
            "a count past 99 is refused, not clamped"
        );
        assert_eq!(count(r#"{id:"minecraft:stone",count:0}"#), None);
        assert_eq!(count(r#"{id:"minecraft:stone",count:-1}"#), None);
        assert_eq!(count(r#"{id:"minecraft:stone",count:"5"}"#), None);
        assert_eq!(count("  {id:\"minecraft:stone\"} \n\u{3000}"), Some(1));
        assert_eq!(count("stone"), None, "no compound");
        assert_eq!(count("-"), None, "no item");
        assert_eq!(count(r#"[{id:"minecraft:stone"}]"#), None);
        assert_eq!(
            count(r#"{id:"minecraft:stone"} x"#),
            None,
            "text after the compound"
        );
        assert_eq!(
            count(r#"{id:"minecraft:stone"}{id:"minecraft:stone"}"#),
            None
        );
        assert_eq!(count(r#"{id:"minecraft:air",count:1}"#), None);
        assert_eq!(count(r#"{id:"nomod:thing",count:1}"#), None);
        assert_eq!(count(r#"{count:1}"#), None);
        assert_eq!(count(r#"{id:"stone",components:1}"#), None);
        assert_eq!(
            count(r#"{id:"diamond_sword",components:{"minecraft:damage":-1}}"#),
            None,
            "negative damage is a codec error, not zero damage"
        );
        assert_eq!(
            count(r#"{id:"stone",components:{"missing:component":{}}}"#),
            None,
            "unregistered components are not opaque payloads"
        );
        assert_eq!(
            count(r#"{id:"stone",components:{"minecraft:creative_slot_lock":{}}}"#),
            None
        );
        let nested = |levels: usize| {
            format!(
                r#"{{id:"minecraft:stone",components:{{"minecraft:custom_data":{{a:{}0{}}}}}}}"#,
                "[".repeat(levels),
                "]".repeat(levels)
            )
        };
        assert_eq!(count(&nested(8)), Some(1));
        assert_eq!(count(&nested(600)), None, "nested past 512 levels");
    });
}

#[test]
#[ignore = "daily"]
fn item_from_snbt_keeps_custom_data_larger_than_the_binary_quota() {
    with_game(|game| {
        let payload = "x".repeat(2 * 1024 * 1024 + 1);
        let snbt = format!(
            r#"{{id:"minecraft:stone",count:3,components:{{"minecraft:custom_data":{{data:"{payload}"}}}}}}"#
        );
        let stack = api_item(game, &snbt).expect("in-memory text may exceed the binary quota");
        assert_eq!(stack.count(), 3);
        let data = stack
            .components()
            .and_then(|patch| patch.as_nbt().get(&"minecraft:custom_data".into()))
            .and_then(|tag| tag.get("data"));
        let Some(mod_api::Tag::String(data)) = data else {
            panic!("the item must retain its custom data");
        };
        assert_eq!(data.0.len(), payload.len());
        assert!(data.0.iter().copied().eq(payload.encode_utf16()));
    });
}

/// A given stack goes into the inventory where it fits, components and all; the rest is
/// thrown for the player alone, with no pickup delay.
#[test]
#[ignore = "daily"]
fn a_given_stack_fills_the_inventory_then_drops_for_its_player() {
    with_guests(|game| {
        let b = game.players().server_players()[1];
        let rock = r#"{id:"minecraft:stone",count:10,components:{"minecraft:custom_name":"Rock"}}"#;
        let named = api_item(game, rock).unwrap();
        let engine = item_nbt::item_from_snbt(rock, |_| false).unwrap();
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .update_guest(13, |g| {
                let dirt = Some(sunrise_client::items::ItemStack::new("minecraft:dirt", 64));
                g.inventory.iter_mut().for_each(|s| *s = dirt.clone());
                g.inventory[5] = Some(sunrise_client::items::ItemStack {
                    count: 60,
                    ..engine.clone()
                });
            })
            .unwrap();
        let before = game.dropped.len();
        assert_eq!(b.give_stack(game, named.clone()), Ok(None));
        let g = game.lan.as_ref().unwrap().hub.guest(13).unwrap();
        assert_eq!(g.inventory[5].as_ref().map(|s| s.count), Some(64));
        assert_eq!(game.dropped.len(), before + 1);
        let dropped = game.dropped.last().unwrap();
        assert_eq!(dropped.stack.count, 6);
        assert_eq!(dropped.stack.extra, engine.extra);
        assert_eq!(dropped.pickup_delay, 0, "no pickup delay");
        assert_eq!(
            dropped.target,
            Some(sunrise_client::lan::playerdata::uuid_ints(13)),
            "only its player picks it up"
        );
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .update_guest(13, |g| g.inventory[7] = None)
            .unwrap();
        assert_eq!(b.give_stack(game, named), Ok(None));
        let g = game.lan.as_ref().unwrap().hub.guest(13).unwrap();
        assert_eq!(g.inventory[7], Some(engine));
        assert_eq!(game.dropped.len(), before + 1);
    });
}

/// This API refuses a dead player with its own name and gives nothing.
#[test]
#[ignore = "daily"]
fn a_dead_player_is_given_nothing() {
    with_guests(|game| {
        let b = game.players().server_players()[1];
        let rock = api_item(game, r#"{id:"minecraft:stone",count:3}"#).unwrap();
        game.lan
            .as_mut()
            .unwrap()
            .hub
            .guests
            .update_connection(13, |g| g.dead = true);
        let before = game.dropped.len();
        assert_eq!(b.give_stack(game, rock), Err(PlayerDataRefusal::Dead));
        assert_eq!(
            b.with_inventory(game, |view| view.iter().next().is_none()),
            Some(true)
        );
        assert_eq!(game.dropped.len(), before);
    });
}
