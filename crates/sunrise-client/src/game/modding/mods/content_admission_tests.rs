use super::{Fixture, registration_registry_names};
use crate::game::part_admission::admit_parts;
use mod_api::{
    Dist, Hooks, InstanceDirectory, MobEffectPart, MobEffectPartError, ModEntry, Registrar,
    RegistrationContext, RuntimeEnvironment,
};
use mod_platform::{__private::with_test_registration_data, testing::with_mods};
use std::fs;

struct EffectPart;
#[mod_api::sunrise_part]
impl MobEffectPart for EffectPart {}

#[test]
fn a_builtin_effect_missing_from_the_prepared_report_still_refuses_a_part() {
    struct Attaches;
    #[mod_api::__subscriptions]
    impl Hooks for Attaches {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let effect = r.effect_key("minecraft:speed");
            assert_eq!(
                r.mob_effect_part(&effect, EffectPart),
                Err(MobEffectPartError::VanillaEffect { effect }),
            );
        }
    }
    let fixture = Fixture::new("builtin-effect-inventory");
    fs::write(
        fixture.0.join("registries.json"),
        r#"{"minecraft:mob_effect":{"entries":{"minecraft:slowness":{"protocol_id":1}}}}"#,
    )
    .unwrap();
    let data = mod_api::__private::RegistrationData::new(
        registration_registry_names(&fixture.0).unwrap(),
        Default::default(),
        InstanceDirectory::from_path(&fixture.0).unwrap(),
        RuntimeEnvironment::new(Dist::Client, "26.3".into(), "test".into(), false),
    );
    with_test_registration_data(data, || {
        let mods: mod_api::Mods = vec![ModEntry::new("addon", Box::new(Attaches))].into();
        with_mods(mods.clone(), || admit_parts(&mods).unwrap());
    });
}
