struct SpawnerListStub;
impl mod_api::mod_api::CustomSpawners for SpawnerListStub {}

struct SpawnerRegistrationListener {
    saw: Arc<Mutex<Vec<&'static str>>>,
    name: &'static str,
    panics: bool,
}

#[mod_api::__subscriptions]
impl Hooks for SpawnerRegistrationListener {
    fn modify_custom_spawners(&self, _: &dyn Host, _: &mut ModifyCustomSpawnersEvent<'_>) {
        self.saw.lock().unwrap().push(self.name);
        assert!(!self.panics, "registration fixture panic");
    }
}

#[test]
fn a_panicking_spawner_registration_does_not_stop_other_listeners() {
    let saw = Arc::new(Mutex::new(Vec::new()));
    let mods = [("broken", true), ("healthy", false)].map(|(name, panics)| {
        ModEntry::new(
            name,
            Box::new(SpawnerRegistrationListener {
                saw: saw.clone(),
                name,
                panics,
            }),
        )
    });
    install(mods.into());
    for _ in 0..2 {
        mod_api::dispatch_modify_custom_spawners(
            &TestHost,
            mod_api::mod_api::DimensionKey,
            &mut SpawnerListStub,
        );
    }
    assert_eq!(*saw.lock().unwrap(), ["broken", "healthy", "healthy"]);
}

mod api {
    #[derive(Clone, Copy)]
    pub struct DimensionKey;

    pub trait CustomSpawners {}
}
