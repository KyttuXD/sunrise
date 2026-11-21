use super::{Fixture, ModsState, admit_file_content, registration_registry_names, run_in_child};
use crate::game::{api_values::TryIntoApi, part_admission::admit_parts};
use mod_api::{
    Dist, Hooks, MobEffectPart, MobEffectPartError, ModEntry, Registrar, RegistrationContext,
    RuntimeEnvironment, Side,
};
use mod_platform::__private::{with_test_package_inventory, with_test_registration_data};
use mod_platform::testing::with_mods;
use std::{fs, sync::Arc};
use sunrise_mod_package::Package;

type Files<'a> = &'a [(&'a str, &'a [u8])];

struct EffectPart;
#[mod_api::sunrise_part]
impl MobEffectPart for EffectPart {}

#[test]
#[ignore = "daily"]
fn optional_effect_targets_use_admitted_packages_and_effective_dependency_rows() {
    const MODE: &str = "SUNRISE_TEST_OPTIONAL_EFFECT_ADMISSION";
    if std::env::var_os(MODE).is_none() {
        // Reading package content also records its verdict for this process.
        run_in_child(
            "game::mods::tests::content_admission::optional_effect_targets_use_admitted_packages_and_effective_dependency_rows",
            MODE,
        );
        return;
    }
    struct Integration;
    #[mod_api::__subscriptions]
    impl Hooks for Integration {
        fn register(&self, r: &mut dyn Registrar, context: &RegistrationContext) {
            for (package, target) in [
                ("with_effect", "effect_alias:missing"),
                ("missing_jar", "missing_jar:comfort"),
                ("refused_content", "refused_content:missing"),
                ("server_only_row", "server_only_row:missing"),
                ("dropped_row", "dropped_row:missing"),
            ] {
                assert!(context.mods().get(package).is_some());
                if context.has_optional_dependency(package) {
                    let effect = r.effect_key(target);
                    r.mob_effect_part(&effect, EffectPart).unwrap();
                }
            }
        }
    }
    let fixture = Fixture::new("optional-effect-admission");
    let root = &fixture.0;
    let native = format!(
        "id='machine_sample'\nname='Sample'\nversion='0.3.0'\napi='0.2'\nside='both'\n\
         [dependencies]\n\
         with_effect={{version='^1',optional=true,order='none'}}\n\
         missing_jar={{version='^1',optional=true}}\n\
         refused_content={{version='^1',optional=true}}\n\
         server_only_row={{version='^1',optional=true,side='server'}}\n\
         dropped_row={{version='^1',optional=true}}\n\
         [native]\ngame='26.3'\nsunrise='{}'\nmixins='{}'\n",
        "a".repeat(40),
        "b".repeat(40),
    );
    let native_files: Files<'_> = &[
        ("Cargo.toml", b"[package]\nname='sample'\nversion='1'\n"),
        ("src/lib.rs", b"pub fn hooks() {}"),
    ];
    fs::write(
        root.join("machine_sample.smod"),
        Package::write_files(&native, native_files).unwrap(),
    )
    .unwrap();
    let effect = br#"{"category":"beneficial","color":16711680}"#;
    let missing_jar = format!(
        "[[jar]]\nid='original'\nversion='1'\nsha512='{}'\n",
        "9".repeat(128)
    );
    let packages: [(&str, &str, Files<'_>); 5] = [
        (
            "with_effect",
            "namespaces=['effect_alias']\n",
            &[("data/effect_alias/sunrise/mob_effect/comfort.json", effect)],
        ),
        (
            "missing_jar",
            &missing_jar,
            &[("data/missing_jar/sunrise/mob_effect/comfort.json", effect)],
        ),
        (
            "refused_content",
            "",
            &[(
                "data/refused_content/sunrise/item/token.json",
                br#"{"template":"minecraft:stick","components":{"refused_content:unknown":1}}"#,
            )],
        ),
        ("server_only_row", "", &[]),
        ("dropped_row", "", &[]),
    ];
    let dev = packages
        .into_iter()
        .map(|(id, extra, files)| {
            let manifest = format!("id='{id}'\nname='{id}'\nversion='1'\napi='0.2'\n{extra}");
            let path = root.join(format!("{id}.smod"));
            fs::write(&path, Package::write_files(&manifest, files).unwrap()).unwrap();
            path
        })
        .collect();
    fs::write(
        root.join(sunrise_mod_package::dependencies::OVERRIDES),
        "[machine_sample]\ndropped_row='drop'\n",
    )
    .unwrap();
    fs::write(root.join("registries.json"), "{}").unwrap();
    let mut sources = ModsState {
        native: Arc::new(fixture.runtime()),
        dev,
        overrides_dir: root.clone(),
        installation_side: Side::Client,
        ..Default::default()
    };
    sources.select(root, &Default::default());
    let metadata = sources
        .selected
        .iter()
        .map(|source| source.package.try_into_api().unwrap())
        .chain(
            sources
                .native
                .packages()
                .iter()
                .map(|package| package.try_into_api().unwrap()),
        )
        .collect();
    let files = admit_file_content(root, &sources, &root.join("jars")).unwrap();
    for (id, admitted) in [
        ("with_effect", true),
        ("missing_jar", false),
        ("refused_content", false),
        ("server_only_row", true),
        ("dropped_row", true),
    ] {
        assert_eq!(
            files.mods.iter().any(|package| package.id == id),
            admitted,
            "{id}"
        );
    }
    let data = mod_api::__private::RegistrationData::new(
        registration_registry_names(root).unwrap(),
        Default::default(),
        root.clone(),
        RuntimeEnvironment::new(Dist::Client, "26.3".into(), "test".into(), false),
    )
    .with_optional_dependencies(files.resolution.optional_dependencies);
    with_test_package_inventory(metadata, || {
        with_test_registration_data(data, || {
            let mods: mod_api::Mods =
                vec![ModEntry::new("machine_sample", Box::new(Integration))].into();
            with_mods(mods.clone(), || {
                admit_parts(&mods).unwrap();
                let names = files
                    .mods
                    .iter()
                    .flat_map(|package| &package.content.effects)
                    .map(|effect| effect.id.as_str());
                assert_eq!(
                    mod_api::__private::resolve_effects(names, |name| {
                        (name == "effect_alias:comfort").then_some(0)
                    })
                    .unwrap_err(),
                    ["mod machine_sample requested unknown effect effect_alias:missing"],
                );
            });
        });
    });
}

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
        fixture.0.clone(),
        RuntimeEnvironment::new(Dist::Client, "26.3".into(), "test".into(), false),
    );
    with_test_registration_data(data, || {
        let mods: mod_api::Mods = vec![ModEntry::new("addon", Box::new(Attaches))].into();
        with_mods(mods.clone(), || admit_parts(&mods).unwrap());
    });
}
