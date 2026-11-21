//! The send routes report an invalid update tag, rather than sending it as absent.
use super::*;
use crate::{
    block_entity_types::BlockEntityTypes,
    containers::{BlockEntity, ModBlockEntity, ModBlockEntityFields},
    lan::Out,
};
use std::sync::mpsc::Receiver;
use sunrise_core::nbt::{Compound, Limits};
use sunrise_protocol::{buf::Reader, registry::Builtin};

const POS: (i32, i32, i32) = (0, 63, 0);

fn invalid_update() -> (Hub, World, Receiver<Out>) {
    let (mut hub, rx) = hub_with_a_guest();
    let builtin = Builtin::parse(
        r#"{"minecraft:block_entity_type":{"entries":{"test:machine":{"protocol_id":49}}}}"#,
    )
    .unwrap();
    let types = BlockEntityTypes::new(
        [("test:machine".into(), "test:machine".into())].into(),
        builtin
            .get("minecraft:block_entity_type")
            .unwrap()
            .names
            .clone(),
    );
    let states = ["minecraft:air", "test:machine"].map(|n| (Arc::<str>::from(n), false, None));
    hub.encoder =
        ChunkEncoder::with_types(states, &builtin, &[], &[], Some(&types), Default::default());
    hub.encoder.mod_update_tag = Some(|_, entity| Some(entity.nbt.clone()));
    let mut nested = Tag::Byte(1);
    for _ in 0..=Limits::network().max_depth {
        nested = Tag::compound([("child", nested)]);
    }
    let mut world = floor_at(0);
    Arc::make_mut(world.chunks.get_mut(&(0, 0)).unwrap()).set(POS.0, POS.1, POS.2, 1);
    world.block_entities.insert(
        POS,
        BlockEntity::Mod(Box::new(ModBlockEntity::new(
            "test:machine".into(),
            ModBlockEntityFields {
                nbt: Compound::from([("nested".into(), nested)]),
                ..Default::default()
            },
        ))),
    );
    (hub, world, rx)
}

fn assert_encoding_disconnect(rx: Receiver<Out>, refused_id: i32) {
    let mut errors = Vec::new();
    for out in rx.try_iter() {
        match out {
            Out::Packet(body) => assert_ne!(Reader::new(&body).varint().unwrap(), refused_id),
            Out::Disconnect(reason) => errors.push(reason),
            _ => panic!("unexpected output from a packet send"),
        }
    }
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].get("translate"),
        Some(&Tag::String("disconnect.genericReason".into()))
    );
    let Some(Tag::List(args)) = errors[0].get("with") else {
        panic!("the disconnect did not report the encoder failure");
    };
    assert!(
        matches!(&args[0], Tag::String(text) if text.to_string_lossy().contains("depth limit exceeded"))
    );
}

#[test]
fn chunk_stream_does_not_send_a_block_entity_with_an_unencodable_tag() {
    let (mut hub, mut world, rx) = invalid_update();
    send_chunks(&mut hub, &mut world);
    assert!(hub.guests[0].disconnect_pending);
    assert!(!hub.guests[0].sent_chunks.contains(&(0, 0)));
    assert_encoding_disconnect(rx, ids::LEVEL_CHUNK_WITH_LIGHT);
}

#[test]
fn block_change_does_not_send_a_block_entity_with_an_unencodable_tag() {
    let (mut hub, mut world, rx) = invalid_update();
    hub.guests.update(0, |g| g.sent_chunks.insert((0, 0)));
    world.block_log = Some(vec![POS]);
    hub.broadcast_block_changes(&mut world);
    assert_encoding_disconnect(rx, ids::BLOCK_ENTITY_DATA);
}

#[test]
fn bulk_block_change_does_not_send_a_chunk_with_an_unencodable_tag() {
    let (mut hub, mut world, rx) = invalid_update();
    hub.guests.update(0, |g| g.sent_chunks.insert((0, 0)));
    world.bulk_log.insert((0, 0));
    hub.broadcast_block_changes(&mut world);
    assert_encoding_disconnect(rx, ids::LEVEL_CHUNK_WITH_LIGHT);
}

#[test]
fn support_resend_does_not_send_a_block_entity_with_an_unencodable_tag() {
    let (mut hub, mut world, rx) = invalid_update();
    hub.ticks = hub.guests[0].joined_tick + 200;
    let mut next_entity = 2;
    let border = crate::worldborder::WorldBorder::default();
    let ctx = TickCtx {
        sea_level: 63,
        world: &mut world,
        entities: &[],
        next_entity: &mut next_entity,
        spawn: [0.5, 64.0, 0.5],
        spawn_angle: [0.0; 2],
        spawn_dimension: "minecraft:overworld",
        difficulty: 2,
        difficulty_locked: false,
        clock_rate: 1.0,
        tick_rate: 20.0,
        tick_frozen: false,
        tick_steps: Default::default(),
        collision: None,
        border: &border,
        streaming: None,
    };
    hub.force_send_player_support_blocks(0, &ctx);
    assert_encoding_disconnect(rx, ids::BLOCK_ENTITY_DATA);
}
