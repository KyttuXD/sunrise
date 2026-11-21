use super::super::*;
use crate::*;
use std::sync::Mutex;

type Trace = Arc<Mutex<Vec<(&'static str, f32)>>>;
#[derive(Clone)]
struct Cadence<const PERIOD: i32> {
    package: &'static str,
    trace: Trace,
}
#[mod_api::sunrise_part]
impl<const PERIOD: i32> MobEffectPart for Cadence<PERIOD> {
    fn should_apply_effect_tick_this_tick(
        &self,
        _: &dyn Host,
        event: &mut ShouldApplyEffectTickThisTickEvent,
    ) {
        assert!(!event.apply);
        event.apply = event.tick_count % PERIOD == 0;
    }
    fn apply_effect_tick(&self, _: &dyn Host, event: &mut ApplyEffectTickEvent<'_>) {
        assert!(event.has_effect(event.effect));
        self.trace
            .lock()
            .unwrap()
            .push((self.package, event.health));
        event.heal(PERIOD as f32);
        if matches!(
            (self.package, event.amplifier),
            ("owner", 99) | ("other", 100)
        ) {
            event.keep = false;
        }
    }
}
#[mod_api::__subscriptions]
impl<const PERIOD: i32> Hooks for Cadence<PERIOD> {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        let effect = r.effect_key("owner:comfort");
        r.mob_effect_part(&effect, self.clone()).unwrap();
    }
}

fn package(id: &str, dependency: Option<DependencyKind>) -> PackageInfo {
    PackageInfo::new(
        id.into(),
        "1".into(),
        PackageKind::Code,
        Side::Both,
        vec![id.into()],
        dependency
            .into_iter()
            .map(|kind| {
                PackageDependency::new(
                    "owner".into(),
                    "*".into(),
                    kind,
                    DependencyOrder::After,
                    Side::Both,
                    None,
                    None,
                )
            })
            .collect(),
    )
}

#[test]
fn an_unguarded_optional_effect_is_reported_missing() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let mods: Mods = vec![ModEntry::new(
        "behaviour",
        Box::new(Cadence::<3> {
            package: "behaviour",
            trace,
        }),
    )]
    .into();
    crate::load_state::with_test_package_inventory(
        vec![package("behaviour", Some(DependencyKind::Optional))],
        || {
            crate::testing::with_mods(mods.clone(), || {
                install_parts(&mods, &crate::parts::tests::rule_schema(), |_| false).unwrap();
                assert_eq!(
                    mod_vocab::__private::resolve_effects([], |_| None).unwrap_err(),
                    ["mod behaviour requested unknown effect owner:comfort"]
                );
            });
        },
    );
}

#[test]
fn registration_refuses_vanilla_and_duplicate_effect_parts() {
    struct Registration;
    #[mod_api::__subscriptions]
    impl Hooks for Registration {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let part = || Cadence::<2> {
                package: "owner",
                trace: Arc::new(Mutex::new(Vec::new())),
            };
            let vanilla = r.effect_key("minecraft:poison");
            assert_eq!(
                r.mob_effect_part(&vanilla, part()),
                Err(MobEffectPartError::VanillaEffect { effect: vanilla })
            );
            let custom = r.effect_key("minecraft:comfort");
            r.mob_effect_part(&custom, part()).unwrap();
            assert_eq!(
                r.mob_effect_part(&custom, part()),
                Err(MobEffectPartError::Duplicate {
                    effect: custom,
                    part: std::any::type_name::<Cadence<2>>().into(),
                })
            );
        }
    }
    let errors = super::super::shared::tests::with_builtin_part_registry(
        "minecraft:mob_effect",
        "minecraft:poison",
        || {
            install_parts(
                &[ModEntry::new("owner", Box::new(Registration))],
                &crate::parts::tests::rule_schema(),
                |_| false,
            )
            .unwrap_err()
        },
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("minecraft:comfort"))
    );
}

#[test]
fn registration_refuses_malformed_effect_key_distinctly_from_duplicate() {
    struct Registration;
    #[mod_api::__subscriptions]
    impl Hooks for Registration {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let part = Cadence::<2> {
                package: "owner",
                trace: Arc::new(Mutex::new(Vec::new())),
            };
            let invalid = r.effect_key("Bad Id");
            assert_eq!(
                r.mob_effect_part(&invalid, part),
                Err(MobEffectPartError::InvalidKey { effect: invalid })
            );
        }
    }
    let errors = install_parts(
        &[ModEntry::new("owner", Box::new(Registration))],
        &crate::parts::tests::rule_schema(),
        |_| false,
    )
    .unwrap_err();
    assert_eq!(
        errors,
        [
            "mod owner: invalid registry key \"minecraft:Bad Id\"",
            &format!(
                "mod owner attached {} to an invalid effect key",
                std::any::type_name::<Cadence<2>>()
            ),
        ]
    );
}

struct World;
impl Host for World {}
crate::impl_host_areas!(World);

fn install(mods: &[ModEntry]) -> EffectKey {
    install_parts(mods, &crate::parts::tests::rule_schema(), |_| false).unwrap();
    mod_vocab::__private::resolve_effects(["owner:comfort"], |_| Some(0)).unwrap();
    mod_vocab::__private::effect_key_by_index(0).unwrap()
}

#[test]
fn registered_foreign_effect_parts_run_without_dependencies_and_owner_runs_first() {
    let trace = Arc::new(Mutex::new(Vec::new()));
    let mods: Mods = vec![
        // The package resolver can put dependents first (order = "before").
        ModEntry::new(
            "other",
            Box::new(Cadence::<6> {
                package: "other",
                trace: trace.clone(),
            }),
        ),
        ModEntry::new(
            "behaviour",
            Box::new(Cadence::<3> {
                package: "behaviour",
                trace: trace.clone(),
            }),
        ),
        ModEntry::new(
            "owner",
            Box::new(Cadence::<2> {
                package: "owner",
                trace: trace.clone(),
            }),
        ),
    ]
    .into();
    crate::load_state::with_test_package_inventory(
        vec![
            package("owner", None),
            package("behaviour", None),
            package("other", None),
        ],
        || {
            crate::testing::with_mods(mods.clone(), || {
                let effect = install(&mods);
                let active_effects = [effect];
                mod_vocab::__private::freeze_effect_part_order(|_| Some("owner"));
                for (ticks, amplifier, expected, expected_trace, expected_keep) in [
                    (1, 0, vec![], vec![], true),
                    (
                        2,
                        0,
                        vec![EffectTickChange::Heal(2.0)],
                        vec![("owner", 10.0)],
                        true,
                    ),
                    (
                        3,
                        0,
                        vec![EffectTickChange::Heal(3.0)],
                        vec![("behaviour", 10.0)],
                        true,
                    ),
                    (
                        6,
                        0,
                        vec![
                            EffectTickChange::Heal(2.0),
                            EffectTickChange::Heal(6.0),
                            EffectTickChange::Heal(3.0),
                        ],
                        vec![("owner", 10.0), ("other", 10.0), ("behaviour", 10.0)],
                        true,
                    ),
                    (
                        6,
                        99,
                        vec![EffectTickChange::Heal(2.0)],
                        vec![("owner", 10.0)],
                        false,
                    ),
                    (
                        6,
                        100,
                        vec![EffectTickChange::Heal(2.0), EffectTickChange::Heal(6.0)],
                        vec![("owner", 10.0), ("other", 10.0)],
                        false,
                    ),
                ] {
                    trace.lock().unwrap().clear();
                    let mut changes = Vec::new();
                    let mut keep = true;
                    let prepared = std::cell::Cell::new(false);
                    mod_vocab::__private::dispatch_mob_effect_tick(
                        &World,
                        ticks,
                        effect,
                        amplifier,
                        |effect, amplifier| {
                            prepared.set(true);
                            mod_vocab::__private::ApplyEffectTickCall {
                                effect,
                                dimension: "minecraft:overworld",
                                entity: Some(7),
                                amplifier,
                                holder: LivingRef::new(EntityRef::new(WorldEpoch::new(1), 7)),
                                health: 10.0,
                                max_health: 20.0,
                                food: None,
                                active_effects: &active_effects,
                                changes: &mut changes,
                                keep: &mut keep,
                            }
                        },
                    );
                    assert_eq!(prepared.get(), !expected_trace.is_empty());
                    assert_eq!(changes, expected);
                    assert_eq!(*trace.lock().unwrap(), expected_trace);
                    assert_eq!(keep, expected_keep);
                }
            });
        },
    );
}
