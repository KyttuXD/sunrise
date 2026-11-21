//! The handlers and their codecs, against the fixture host: literal amounts on writes, the
//! hostile saved shapes refused, and the journal's nested snapshots.
use super::*;
use crate::{HostTransaction, ItemKey, resource_util, testing::FakeHost};
use std::cell::RefCell;

thread_local! {
    /// The one item `accepts_the_chosen` lets in: a `fn` pointer cannot capture it.
    static CHOSEN: RefCell<Option<ItemKey>> = const { RefCell::new(None) };
}
/// A `valid` the tests can point a handler at: `true` for the item `CHOSEN` holds.
fn accepts_the_chosen(resource: &ItemResource) -> bool {
    CHOSEN.with_borrow(|chosen| {
        let Some(chosen) = chosen else {
            return false;
        };
        resource
            .to_stack(1)
            .is_some_and(|stack| stack.item() == *chosen)
    })
}

static ITEMS: AttachmentType<StackList<ItemResource>> = AttachmentType {
    namespace: "attached_storage_test",
    name: "items",
    default: StackList::new,
    codec: Some(ITEM_STACKS),
    copy_on_death: false,
    sync: false,
};
static VALID_ITEMS: AttachmentType<StackList<ItemResource>> = AttachmentType {
    namespace: "attached_storage_test",
    name: "valid_items",
    default: StackList::new,
    codec: Some(ITEM_STACKS),
    copy_on_death: false,
    sync: false,
};
static FLUIDS: AttachmentType<StackList<FluidResource>> = AttachmentType {
    namespace: "attached_storage_test",
    name: "fluids",
    default: StackList::new,
    codec: Some(FLUID_STACKS),
    copy_on_death: false,
    sync: false,
};
static ENERGY: AttachmentType<u32> = AttachmentType {
    namespace: "attached_storage_test",
    name: "energy",
    default: || 0,
    codec: None,
    copy_on_death: false,
    sync: false,
};
const CHUNK: Holder = Holder::Chunk([2, 3]);

/// Complete the fixture load's tables with `items` and `fluids`, so the stacks below resolve.
fn resolve(items: &[&str], fluids: &[&str]) {
    let mut host = FakeHost::new();
    if !items.is_empty() {
        host.registries
            .resolve_items(
                items
                    .iter()
                    .map(|id| Identifier::parse(id).unwrap())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    if !fluids.is_empty() {
        host.registries
            .resolve_fluids(fluids.iter().copied())
            .unwrap();
    }
}

/// The stack of the resolved item `id`, one of it.
fn item(id: &str) -> ItemResource {
    let key = item_key_by_identifier(&Identifier::parse(id).unwrap()).unwrap();
    ItemResource::of(&ItemStack::new(key, 1).unwrap())
}

/// One of the resolved fluid `id`.
fn fluid(id: &str) -> FluidResource {
    let key = crate::content::registry_keys::fluid_key(id).unwrap();
    FluidResource::of(&FluidStack {
        fluid: FluidIdentity::Resolved(key),
        amount: 1,
        components: None,
    })
}

/// The writes run in the World phase: the boundary's access checks refuse a call outside one.
fn in_world(test: impl FnOnce()) {
    let _world = crate::world::phase::PhaseOwner::scope(crate::world::phase::ModPhase::World);
    test();
}

/// The water amount the fluids codec saves and reads back, with its index.
#[test]
fn a_fluid_tank_round_trips_through_its_codec_and_moves() {
    in_world(|| {
        resolve(&[], &["minecraft:water"]);
        let mut host = FakeHost::new();
        let water = fluid("minecraft:water");
        let tanks = AttachedFluids::new(&FLUIDS, CHUNK, 5001, 1000, |_| true);
        let moved =
            host.transaction(|host, tx| Ok::<_, ()>(tanks.insert_at(host, tx, 5000, &water, 250)));
        assert_eq!(moved, Ok(250));
        assert_eq!(tanks.amount(&mut host, 5000), 250);
        // The exact old on-disk layout still holds, including the signed Int slot and amount.
        let expected: Compound = [(
            "stacks".into(),
            Tag::List(vec![Tag::Compound(
                [
                    ("Slot".into(), Tag::Int(5000)),
                    ("id".into(), Tag::String("minecraft:water".into())),
                    ("amount".into(), Tag::Int(250)),
                ]
                .into(),
            )]),
        )]
        .into();
        let server: &mut dyn ServerHost = &mut host;
        let held = FLUIDS.get_existing(server, CHUNK).unwrap().unwrap();
        assert_eq!((FLUID_STACKS.encode)(&held), Ok(expected.clone()));
        assert_eq!(
            (FLUID_STACKS.decode)(&expected),
            Ok(StackList::from([(5000, (water.clone(), 250))]))
        );
        let came =
            host.transaction(|host, tx| Ok::<_, ()>(tanks.extract_at(host, tx, 5000, &water, 250)));
        assert_eq!(came, Ok(250));
        assert_eq!(tanks.amount(&mut host, 5000), 0);
    });
}

/// Extraction of a part of what an index holds, and none of another resource.
#[test]
fn extraction_takes_part_and_none_of_another_resource() {
    in_world(|| {
        resolve(&["minecraft:dirt", "minecraft:cobblestone"], &[]);
        let mut host = FakeHost::new();
        let dirt = item("minecraft:dirt");
        let cobble = item("minecraft:cobblestone");
        let slots = AttachedItems::new(&ITEMS, CHUNK, 9, 64, |_| true);
        let went =
            host.transaction(|host, tx| Ok::<_, ()>(slots.insert_at(host, tx, 4, &dirt, 30)));
        assert_eq!(went, Ok(30));
        let none =
            host.transaction(|host, tx| Ok::<_, ()>(slots.extract_at(host, tx, 4, &cobble, 30)));
        assert_eq!(none, Ok(0), "none of another resource");
        let came =
            host.transaction(|host, tx| Ok::<_, ()>(slots.extract_at(host, tx, 4, &dirt, 12)));
        assert_eq!(came, Ok(12));
        assert_eq!(slots.amount(&mut host, 4), 18);
        let rest =
            host.transaction(|host, tx| Ok::<_, ()>(slots.extract_at(host, tx, 4, &dirt, 50)));
        assert_eq!(rest, Ok(18), "at most what remains");
        let empty =
            host.transaction(|host, tx| Ok::<_, ()>(slots.extract_at(host, tx, 4, &dirt, 1)));
        assert_eq!(empty, Ok(0));
        assert_eq!(slots.amount(&mut host, 4), 0);
    });
}

/// `capacity` is the physical limit of an in-range index, `is_valid` the permission, and
/// extraction independent of validity.
#[test]
fn capacity_stays_physical_while_validity_refuses_inserts_only() {
    in_world(|| {
        resolve(&["minecraft:dirt", "minecraft:cobblestone"], &[]);
        let mut host = FakeHost::new();
        let dirt = item("minecraft:dirt");
        let cobble = item("minecraft:cobblestone");
        CHOSEN.with_borrow_mut(|chosen| *chosen = Some(dirt.to_stack(1).unwrap().item()));
        let only_dirt = AttachedItems::new(&VALID_ITEMS, CHUNK, 4, 500, accepts_the_chosen);
        assert_eq!(
            only_dirt.capacity(&mut host, 2, Some(&cobble)),
            64,
            "the physical limit (the item's stack size), though cobblestone is not valid here"
        );
        assert!(!only_dirt.is_valid(&mut host, 2, &cobble));
        let refused =
            host.transaction(|host, tx| Ok::<_, ()>(only_dirt.insert_at(host, tx, 2, &cobble, 10)));
        assert_eq!(refused, Ok(0));
        let went =
            host.transaction(|host, tx| Ok::<_, ()>(only_dirt.insert_at(host, tx, 2, &dirt, 120)));
        assert_eq!(went, Ok(64), "at most the item's stack size per index");
        // A cobblestone another (permissive) handler's write stored reads out through this one.
        let anything = AttachedItems::new(&VALID_ITEMS, CHUNK, 4, 500, |_| true);
        let _ =
            host.transaction(|host, tx| Ok::<_, ()>(anything.insert_at(host, tx, 3, &cobble, 7)));
        let came =
            host.transaction(|host, tx| Ok::<_, ()>(only_dirt.extract_at(host, tx, 3, &cobble, 7)));
        assert_eq!(came, Ok(7), "extraction is independent of validity");
    });
}

/// The per-call limits and a nested abort: the inner transaction's write is undone, the
/// outer's stays.
#[test]
fn energy_limits_and_a_nested_abort() {
    in_world(|| {
        resolve(&[], &[]);
        let mut host = FakeHost::new();
        let battery = AttachedEnergy::new(&ENERGY, CHUNK, 100, 30, 20);
        let past_insert = host.transaction(|host, tx| Ok::<_, ()>(battery.insert(host, tx, 150)));
        assert_eq!(past_insert, Ok(30), "at most max_insert per call");
        let past_extract = host.transaction(|host, tx| Ok::<_, ()>(battery.extract(host, tx, 25)));
        assert_eq!(past_extract, Ok(20), "at most max_extract per call");
        let nested = host.transaction(|outer_host, outer_tx| {
            let aborted = outer_host.transaction(|host, tx| {
                battery.insert(host, tx, 10);
                Err::<(), _>(())
            });
            assert_eq!(aborted, Err(()));
            let came = battery.extract(&mut *outer_host, outer_tx, 5);
            assert_eq!(came, 5);
            Ok::<_, ()>(came)
        });
        assert_eq!(nested, Ok(5));
        assert_eq!(
            battery.amount(&mut host),
            5,
            "the inner write was undone, the outer's stayed"
        );
    });
}

/// A write through a smaller view keeps the higher indices a bigger one stored.
#[test]
fn a_smaller_view_keeps_higher_indices() {
    in_world(|| {
        resolve(&["minecraft:dirt"], &[]);
        let mut host = FakeHost::new();
        let dirt = item("minecraft:dirt");
        let big = AttachedItems::new(&ITEMS, CHUNK, 9, 64, |_| true);
        let went = host.transaction(|host, tx| Ok::<_, ()>(big.insert_at(host, tx, 7, &dirt, 12)));
        assert_eq!(went, Ok(12));
        let small = AttachedItems::new(&ITEMS, CHUNK, 3, 64, |_| true);
        let went = host.transaction(|host, tx| Ok::<_, ()>(small.insert_at(host, tx, 0, &dirt, 5)));
        assert_eq!(went, Ok(5));
        assert_eq!(big.amount(&mut host, 7), 12, "the higher index survived");
        assert_eq!(big.amount(&mut host, 0), 5);
        assert_eq!(
            small.amount(&mut host, 7),
            0,
            "past the smaller view's size"
        );
    });
}

#[test]
fn a_saved_slot_uses_the_full_nonnegative_nbt_int_range() {
    in_world(|| {
        resolve(&["minecraft:dirt"], &[]);
        let dirt = item("minecraft:dirt");
        let saved = |slot: i32| -> Compound {
            [(
                "stacks".into(),
                Tag::List(vec![Tag::Compound(
                    [
                        ("Slot".into(), Tag::Int(slot)),
                        ("id".into(), Tag::String("minecraft:dirt".into())),
                        ("count".into(), Tag::Int(1)),
                    ]
                    .into(),
                )]),
            )]
            .into()
        };
        let last = i32::MAX as usize;
        let held = StackList::from([(last, (dirt.clone(), 1))]);
        assert_eq!((ITEM_STACKS.decode)(&saved(i32::MAX)), Ok(held.clone()));
        assert_eq!((ITEM_STACKS.encode)(&held), Ok(saved(i32::MAX)));
        assert_eq!(
            (ITEM_STACKS.decode)(&saved(-1)),
            Err(AttachmentError::Decode)
        );
        assert_eq!(
            (ITEM_STACKS.encode)(&StackList::from([(last + 1, (dirt, 1))])),
            Err(AttachmentError::Encode)
        );
    });
}

#[test]
fn the_codec_round_trips_more_than_4096_occupied_slots() {
    let stacks: StackList<_> = (0..5000).map(|slot| (slot, ((), 1))).collect();
    let encoded = encode_stacks(&stacks, |_, _| Some(Compound::new())).unwrap();
    let Some(Tag::List(entries)) = encoded.get(&"stacks".into()) else {
        panic!("encoded stack list");
    };
    assert_eq!(entries.len(), 5000);
    assert_eq!(decode_stacks(&encoded, |_| Some(((), 1))), Ok(stacks));
}

#[test]
fn slotless_transfers_skip_gaps_in_large_attached_handlers() {
    #[derive(Clone, PartialEq)]
    struct Charge(u8);
    impl StackedResource for Charge {}

    static CELLS: AttachmentType<StackList<Charge>> = AttachmentType {
        namespace: "attached_storage_test",
        name: "cells",
        default: StackList::new,
        codec: None,
        copy_on_death: false,
        sync: false,
    };

    in_world(|| {
        let host: &mut dyn ServerHost = &mut FakeHost::new();
        let charge = Charge(1);
        let last = usize::MAX - 1;
        let slots = AttachedStacks::new(&CELLS, CHUNK, usize::MAX, 64, |r| r.0 == 1);
        assert_eq!(
            host.transaction(|host, tx| slots.insert(host, tx, &Charge(0), 1)),
            Ok(0)
        );
        assert_eq!(
            host.transaction(|host, tx| Ok::<_, ()>(slots.insert_at(host, tx, last, &charge, 55))),
            Ok(55)
        );
        let ranged =
            resource_util::RangedResourceHandler::of(host, Box::new(slots), 1..usize::MAX).unwrap();
        let closed = AttachedStacks::single(&CELLS, CHUNK, 0, |_| false);
        let slots = resource_util::CombinedResourceHandler::new(
            host,
            vec![Box::new(closed), Box::new(ranged)],
        );
        assert_eq!(resource_util::index_of(host, &slots, &charge), Some(last));
        assert_eq!(
            host.transaction(|host, tx| slots.extract(host, tx, &charge, 23)),
            Ok(23)
        );
        assert_eq!(slots.amount(host, last), 32);
        assert_eq!(
            resource_util::insert_stacking(host, &slots, &charge, 37, None),
            Ok(37)
        );
        assert_eq!(
            slots.amount(host, last),
            64,
            "top up before a lower empty slot"
        );
        assert_eq!(
            slots.amount(host, 1),
            5,
            "the rest goes in the first accepting empty slot"
        );
        assert_eq!(
            host.transaction(|host, tx| slots.extract(host, tx, &charge, 38)),
            Ok(38)
        );
        assert_eq!(
            slots.amount(host, 1),
            0,
            "extraction takes the lower slot first"
        );
        assert_eq!(
            host.transaction(|host, tx| slots.extract(host, tx, &charge, 32)),
            Ok(31)
        );
        assert!(resource_util::is_empty(host, &slots));
    });
}
