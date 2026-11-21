#![cfg(test)]

#[test]
fn replacing_mod_block_entity_payload_keeps_its_type_and_saved_identity() {
    use sunrise_client::containers::{BlockEntity, ModBlockEntity, ModBlockEntityFields};
    use sunrise_client::save::{block_entity_encode, decode_block_entity};
    use sunrise_core::nbt::{Compound, Tag};

    let pos = (1, 64, 1);
    let mut machine = ModBlockEntity::new("test:machine".into(), Default::default());
    let made = machine.revision();
    *machine = ModBlockEntityFields {
        data: vec![7],
        nbt: Compound::from([("progress".into(), Tag::Int(42))]),
        ..Default::default()
    };
    assert_eq!(machine.id(), "test:machine");
    assert_eq!(machine.data, [7]);
    assert_ne!(machine.revision(), made);
    let entity = BlockEntity::Mod(Box::new(machine));
    let saved = block_entity_encode::encode(pos, &entity, "minecraft:stone", &Default::default());
    let (saved_pos, loaded) = decode_block_entity(&saved).unwrap();
    assert_eq!(saved_pos, pos);
    assert_eq!(loaded.id(), "test:machine");
    let BlockEntity::Mod(loaded) = loaded else {
        panic!("not a mod block entity");
    };
    assert_eq!(loaded.nbt.get(&"progress".into()), Some(&Tag::Int(42)));
    assert!(loaded.data.is_empty(), "menu data stays transient");
}
