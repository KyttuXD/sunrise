use super::*;
fn id(value: &str) -> Identifier {
    Identifier::parse(value).unwrap()
}
fn files() -> FileToIdConverter {
    FileToIdConverter::json("machines").unwrap()
}

#[test]
fn registry_views_and_explicit_order_before_vanilla() {
    let mut listeners = ClientListenerRegistry::new();
    let [first, model_manager, last, mod_first, mod_second] = [
        "minecraft:first",
        "minecraft:model_manager",
        "minecraft:last",
        "test_mod:first",
        "test_mod:second",
    ]
    .map(id);
    let registry: &mut dyn ClientReloadListeners = &mut listeners;
    registry
        .add_listener(mod_first.clone(), files(), Arc::new(Boundary))
        .unwrap();
    registry
        .add_listener(mod_second.clone(), files(), Arc::new(Boundary))
        .unwrap();

    let anchor = registry.get_last_vanilla_listener();
    assert_eq!(anchor, &model_manager);
    let names = registry.get_registry().iter();
    assert!(names.eq([&first, &model_manager, &last, &mod_first, &mod_second]));
    let anchor = anchor.clone();
    registry.add_dependency(mod_second.clone(), anchor).unwrap();
    let edges = registry.get_graph().iter();
    assert!(edges.eq([
        (&first, &model_manager),
        (&model_manager, &last),
        (&first, &mod_first),
        (&mod_first, &last),
        (&first, &mod_second),
        (&mod_second, &last),
        (&mod_second, &model_manager),
    ]));

    let sorted = listeners.finish().unwrap();
    assert_eq!(
        sorted.into_iter().map(|entry| entry.id).collect::<Vec<_>>(),
        [first, mod_second, model_manager, mod_first, last]
    );
}

#[test]
fn failed_registration_is_not_silenced_by_ignoring_the_result() {
    let mut listeners = ClientListenerRegistry::new();
    assert_eq!(
        listeners
            .add_dependency(
                id("mistyped:listener"),
                VanillaClientListeners::model_manager()
            )
            .unwrap_err(),
        ReloadOrderError::UnknownListener {
            id: id("mistyped:listener")
        }
    );
    assert!(matches!(
        listeners.finish(),
        Err(ReloadOrderError::UnknownListener { .. })
    ));
}

#[test]
fn duplicate_objects_cannot_make_the_reverse_lookup_ambiguous() {
    let mut listeners = ClientListenerRegistry::new();
    let listener = Arc::new(Boundary);
    listeners
        .add_listener(id("one:listener"), files(), listener.clone())
        .unwrap();
    assert_eq!(
        listeners.add_listener(id("two:listener"), files(), listener),
        Err(ReloadOrderError::DuplicateObject {
            id: id("two:listener"),
            registered_as: id("one:listener"),
        })
    );
}
