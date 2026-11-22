use crate::game::native_entity_refs::tests::{run_fresh_cases, with_client_game};
use mod_api::prelude::NetworkClientHost;
use mod_api::{ContainerClickRequest, PacketBuildError, PlayPacketRequest};
use sunrise_core::nbt::{Compound, Tag};
use sunrise_protocol::{
    buf::Reader,
    connection::Pipe,
    item::{Component, Hashed, NoNames, Stack},
    packets::play::{ContainerClick, Serverbound},
    registry::Builtin,
};

#[test]
#[ignore = "daily"]
fn play_packet_building_works_in_startup_reload_and_world() {
    use mod_api::{AddClientReloadListenersEvent, ClientHost, Hooks, ModEntry};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    if run_fresh_cases(
        "game::native_packet_builders::tests::play_packet_building_works_in_startup_reload_and_world",
        "SUNRISE_STATELESS_PACKET_BUILDER_CHILD",
        &["startup-and-world"],
    ) {
        return;
    }

    struct Probe(Arc<AtomicBool>);
    #[mod_api::__subscriptions]
    impl Hooks for Probe {
        fn add_client_reload_listeners(
            &self,
            host: &dyn ClientHost,
            _: &mut AddClientReloadListenersEvent<'_>,
        ) {
            let packet = host
                .build_play_packet(&PlayPacketRequest::Attack { entity: 300 })
                .unwrap();
            assert_eq!(packet.id.to_string(), "minecraft:attack");
            assert_eq!(packet.body, [0xac, 0x02]);
            assert_eq!(
                host.container_click(&ContainerClickRequest::default()),
                Err(PacketBuildError::Unavailable {
                    packet: "play_packet".into(),
                    field: "host".into()
                }),
            );
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let ran = Arc::new(AtomicBool::new(false));
    let mods = vec![ModEntry::new("packet_probe", Box::new(Probe(ran.clone())))].into();
    mod_platform::testing::with_mods(mods, || {
        let _ = crate::game::client_reload::register_startup_client_reload_listeners();
    });
    assert!(ran.load(Ordering::SeqCst));

    with_client_game(|game| {
        use mod_api::__private::{ModPhase, PhaseOwner};
        let request = PlayPacketRequest::Attack { entity: 300 };
        let click = ContainerClickRequest::default();
        {
            let _phase = PhaseOwner::scope(ModPhase::Setup);
            assert_eq!(
                NetworkClientHost::container_click(game, &click),
                Err(PacketBuildError::Unavailable {
                    packet: "play_packet".into(),
                    field: "host".into()
                }),
            );
        }
        let _phase = PhaseOwner::scope(ModPhase::World);
        let packet = NetworkClientHost::build_play_packet(game, &request).unwrap();
        assert_eq!(packet.id.to_string(), "minecraft:attack");
        assert_eq!(packet.body, [0xac, 0x02]);
    });
}

#[test]
#[ignore = "daily"]
fn client_host_hashes_plain_stacks_with_added_and_removed_components() {
    if run_fresh_cases(
        "game::native_packet_builders::tests::client_host_hashes_plain_stacks_with_added_and_removed_components",
        "SUNRISE_CONTAINER_BUILDER_CHILD",
        &["build"],
    ) {
        return;
    }
    with_client_game(|game| {
        let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
        let mut stack =
            mod_api::ItemStack::new(mod_api::__private::item_key("minecraft:stick").unwrap(), 3)
                .unwrap();
        stack
            .set_components(Compound::from([
                ("minecraft:damage".into(), Tag::Int(7)),
                ("!minecraft:lore".into(), Tag::Compound(Compound::new())),
            ]))
            .unwrap();
        let mut request = ContainerClickRequest {
            container_id: 2,
            state_id: 5,
            slot: 36,
            changed_slots: vec![(36, Some(stack.clone())), (37, None)],
            carried: Some(stack),
            ..Default::default()
        };
        let (client, _server) = Pipe::pair();
        game.connect_local(client, "PacketBuilder").unwrap();
        game.remote.as_mut().unwrap().builtin = std::sync::Arc::new(Builtin::parse(r#"{"minecraft:item":{"entries":{"minecraft:stick":{"protocol_id":4}}},"minecraft:data_component_type":{"entries":{"minecraft:damage":{"protocol_id":3},"minecraft:lore":{"protocol_id":11}}}}"#).unwrap());
        let got = NetworkClientHost::container_click(game, &request).unwrap();
        let network = Stack {
            added: vec![(3, Component::Int(7))],
            removed: vec![11],
            ..Stack::new(4, 3)
        };
        let hashed = Hashed::create(&network, &NoNames).unwrap();
        let expected = Serverbound::ContainerClick(ContainerClick {
            container: 2,
            state: 5,
            slot: 36,
            button: 0,
            input: 0,
            changed: vec![(36, Some(hashed.clone())), (37, None)],
            carried: Some(hashed),
        })
        .encode();
        let mut reader = Reader::new(&expected);
        reader.varint().unwrap();
        assert_eq!(got.body, reader.rest());
        request.changed_slots = vec![(36, None); 129];
        assert_eq!(
            NetworkClientHost::container_click(game, &request),
            Err(PacketBuildError::TooManyChangedSlots {
                packet: "container_click".into(),
                field: "changed_slots".into()
            })
        );
    });
}
