struct SpawnerRegistrationListener {
    name: &'static str,
    panics: bool,
}

#[mod_api::__subscriptions]
impl Hooks for SpawnerRegistrationListener {
    fn modify_custom_spawners(&self, _: &dyn Host, event: &mut ModifyCustomSpawnersEvent<'_>) {
        event.spawners.mark(self.name);
        assert!(!self.panics, "registration fixture panic");
    }
}

#[test]
fn a_panicking_spawner_registration_discards_its_edits_and_disables_only_that_listener() {
    let mods = [("broken", true), ("healthy", false)].map(|(name, panics)| {
        ModEntry::new(name, Box::new(SpawnerRegistrationListener { name, panics }))
    });
    install(mods.into());
    let mut list = mod_api::mod_api::__private::LevelCustomSpawners::default();
    for _ in 0..2 {
        mod_api::dispatch_modify_custom_spawners(&TestHost, mod_api::mod_api::LevelRef, &mut list);
    }
    assert_eq!(list.0, ["healthy", "healthy"]);
    assert_eq!(
        ENTERED.with(|entries| entries.borrow().clone()),
        [
            (0, "modify_custom_spawners"),
            (1, "modify_custom_spawners"),
            (1, "modify_custom_spawners"),
        ]
    );
}

mod api {
    #[derive(Clone, Copy)]
    pub struct LevelRef;

    #[derive(Default)]
    pub struct LevelCustomSpawners(pub Vec<&'static str>);

    pub struct CustomSpawners<'a> {
        target: &'a mut LevelCustomSpawners,
        mark: Option<&'static str>,
    }

    impl LevelCustomSpawners {
        pub fn edit(&mut self, _: &'static str, _: LevelRef) -> CustomSpawners<'_> {
            CustomSpawners {
                target: self,
                mark: None,
            }
        }
    }

    impl CustomSpawners<'_> {
        pub fn mark(&mut self, mark: &'static str) {
            self.mark = Some(mark);
        }

        pub fn commit(self) {
            self.target.0.extend(self.mark);
        }
    }
}
