//! Concrete authoring extracted from the vocabulary layer.
use super::*;
use mod_vocab::__private::game::RegistrationContextNew;

impl GeneralTypesRegistrar for PartRegistrar {
    fn declare_general_entity(
        &mut self,
        id: crate::Identifier,
        category: GeneralEntityCategory,
        width: f64,
        height: f64,
        passenger_attachment: Option<[f64; 3]>,
        save_policy: GeneralSavePolicy,
        summonable: bool,
        pickable: bool,
        piston_push_reaction: Option<PushReaction>,
        tracking_chunks: i32,
        update_interval: i32,
        payload_version: u16,
        required_features: &[crate::FeatureFlag],
    ) -> Result<crate::EntityTypeKey, GeneralEntityRegistrationError> {
        use GeneralEntityRegistrationError as Error;
        // The registry freezes with the phase: once registration ends and any
        // later phase begins, no declaration is accepted anywhere.
        if crate::__private::current_phase() != crate::__private::ModPhase::Registration {
            self.refused.push(format!(
                "mod {} declares a general entity type after the registry froze",
                self.mod_id
            ));
            return Err(Error::Frozen);
        }
        let id_for_log = id.clone();
        let result = (|| {
            if id.namespace().is_empty() || id.path().is_empty() {
                return Err(Error::InvalidId);
            }
            if mod_vocab::__private::parts_installation::loaded_mods()
                .get(self.mod_id)
                .is_some_and(|package| package.side() != crate::Side::Both)
            {
                return Err(Error::WrongSide);
            }
            if self
                .general_entity_types
                .iter()
                .any(|declared| declared.id == id)
                || crate::RegistrationContext::new()
                    .registry("minecraft:entity_type")
                    .is_ok_and(|names| names.iter().any(|name| name == id.as_str()))
            {
                return Err(Error::Duplicate);
            }
            if ![width, height]
                .into_iter()
                .chain(passenger_attachment.into_iter().flatten())
                .all(f64::is_finite)
            {
                return Err(Error::NotFinite);
            }
            let mut features = Vec::with_capacity(required_features.len());
            for &flag in required_features {
                if !features.contains(&flag) {
                    features.push(flag);
                }
            }
            let slot = self
                .registry_keys
                .borrow_mut()
                .entity_types
                .request_identifier(self.mod_id, &id)
                .map_err(|_| Error::RegistrySlotsExhausted)?;
            self.general_entity_types
                .push(GeneralEntityTypeDeclaration {
                    mod_id: self.mod_id.to_owned(),
                    id,
                    category,
                    width,
                    height,
                    passenger_attachment,
                    save_policy,
                    summonable,
                    pickable,
                    piston_push_reaction: piston_push_reaction.unwrap_or_default(),
                    tracking_chunks,
                    update_interval,
                    payload_version,
                    required_features: features,
                });
            Ok(mod_vocab::__private::parts_installation::entity_type_key(
                slot,
            ))
        })();
        if let Err(error) = &result {
            self.refused.push(format!(
                "mod {} refuses general entity type {id_for_log:?}: {error:?}",
                self.mod_id
            ));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::__private::{ModPhase, PhaseOwner};
    use crate::GeneralEntityRegistrationError as Error;

    fn registrar() -> (impl Sized, PartRegistrar) {
        // Declarations are phase-owned; tests scope their thread to the
        // registration phase the way the loader does, restoring it after.
        let scope = PhaseOwner::scope(ModPhase::Registration);
        (scope, PartRegistrar::new("mymod"))
    }

    fn declare(
        registrar: &mut PartRegistrar,
        path: &str,
        width: f64,
        height: f64,
        tracking_chunks: i32,
        update_interval: i32,
        required: &[crate::FeatureFlag],
    ) -> Result<crate::EntityTypeKey, Error> {
        registrar.declare_general_entity(
            crate::Identifier::parse(path).unwrap(),
            GeneralEntityCategory::Misc,
            width,
            height,
            None,
            GeneralSavePolicy::Save,
            true,
            false,
            None,
            tracking_chunks,
            update_interval,
            1,
            required,
        )
    }

    #[test]
    pub(crate) fn general_entity_type_declarations_validate() {
        crate::__private::initialize().unwrap();
        let (_phase, mut registrar) = registrar();
        let robot = declare(&mut registrar, "mymod:robot", 0.8, 1.9, 8, 3, &[]).unwrap();
        assert_eq!(robot, registrar.entity_type_key("mymod:robot"));
        assert_eq!(
            declare(&mut registrar, "mymod:", 0.6, 1.7, 8, 3, &[]),
            Err(Error::InvalidId)
        );
        // A key can be valid in the registry without fitting into a network string.
        let long_name = format!("foreign:{}", "l".repeat(33_000));
        let long_key = declare(&mut registrar, &long_name, 0.6, 1.7, 8, 3, &[]).unwrap();
        assert_eq!(long_key, registrar.entity_type_key(&long_name));
        assert_eq!(registrar.general_entity_types[1].id.as_str(), long_name);
        // The same type twice, whatever declared it, is a duplicate.
        assert_eq!(
            declare(&mut registrar, "mymod:robot", 0.5, 1.0, 4, 2, &[]),
            Err(Error::Duplicate)
        );
        // Non-finite dimensions.
        assert_eq!(
            declare(&mut registrar, "wide", f64::NAN, 1.0, 8, 3, &[]),
            Err(Error::NotFinite)
        );
        assert_eq!(
            declare(&mut registrar, "wide", f64::INFINITY, 1.0, 8, 3, &[]),
            Err(Error::NotFinite)
        );
        // Real Minecraft feature flags are accepted without treating them as mod dependencies.
        let flagged = declare(
            &mut registrar,
            "mymod:experimental",
            1.0,
            1.0,
            8,
            3,
            &[
                crate::FeatureFlag::RedstoneExperiments,
                crate::FeatureFlag::MinecartImprovements,
                crate::FeatureFlag::RedstoneExperiments,
            ],
        )
        .unwrap();
        assert_eq!(flagged, registrar.entity_type_key("mymod:experimental"));
        // Every refusal was reported, and the good declarations stand.
        assert_eq!(registrar.general_entity_types.len(), 3);
        assert_eq!(registrar.general_entity_types[0].id.as_str(), "mymod:robot");
        assert_eq!(
            registrar.general_entity_types[2].required_features,
            vec![
                crate::FeatureFlag::RedstoneExperiments,
                crate::FeatureFlag::MinecartImprovements,
            ]
        );
        assert_eq!(registrar.refused.len(), 4);
    }

    #[test]
    fn an_existing_builtin_is_rejected_but_an_unused_key_in_its_namespace_is_not() {
        crate::__private::initialize().unwrap();
        let (_phase, mut registrar) = registrar();
        let data = crate::__private::RegistrationData::new(
            [(
                "minecraft:entity_type".to_owned(),
                vec!["minecraft:pig".to_owned()],
            )]
            .into(),
            Default::default(),
            Default::default(),
            crate::RuntimeEnvironment::new(
                crate::Dist::Client,
                "26.3".into(),
                "test".into(),
                false,
            ),
        );
        crate::__private::with_test_registration_data(data, || {
            assert_eq!(
                declare(&mut registrar, "minecraft:pig", 1.0, 1.0, 8, 3, &[]),
                Err(Error::Duplicate)
            );
            assert!(declare(&mut registrar, "minecraft:robot", 1.0, 1.0, 8, 3, &[]).is_ok());
        });
        assert_eq!(
            registrar.general_entity_types[0].id.as_str(),
            "minecraft:robot"
        );
    }

    #[test]
    fn a_non_finite_passenger_attachment_refuses_registration() {
        crate::__private::initialize().unwrap();
        let (_phase, mut registrar) = registrar();
        assert_eq!(
            registrar.declare_general_entity(
                crate::Identifier::parse("mymod:robot").unwrap(),
                GeneralEntityCategory::Misc,
                0.8,
                1.9,
                Some([1.0, f64::NAN, 0.0]),
                GeneralSavePolicy::Save,
                true,
                false,
                None,
                8,
                3,
                1,
                &[],
            ),
            Err(Error::NotFinite)
        );
        assert!(registrar.general_entity_types.is_empty());
    }

    #[test]
    fn vanilla_ranges_and_no_update_interval_are_accepted() {
        crate::__private::initialize().unwrap();
        let (_phase, mut registrar) = registrar();
        declare(
            &mut registrar,
            "robot",
            f64::from(f32::MAX),
            f64::from(f32::MAX),
            i32::MAX,
            i32::MAX,
            &[],
        )
        .unwrap();
        declare(&mut registrar, "signed", 0.0, -1.0, i32::MIN, i32::MIN, &[]).unwrap();
        assert_eq!(registrar.general_entity_types.len(), 2);
        assert!(registrar.refused.is_empty());
    }

    #[test]
    fn client_only_package_refuses_entity_types() {
        crate::__private::initialize().unwrap();
        use crate::load_state::with_test_package_inventory;
        use crate::{PackageInfo, PackageKind};
        let packages = vec![PackageInfo::new(
            "amod".to_owned(),
            "1".to_owned(),
            PackageKind::Code,
            crate::Side::Client,
            vec!["shared".to_owned()],
            vec![],
        )];
        let _scope = PhaseOwner::scope(ModPhase::Registration);
        with_test_package_inventory(packages, || {
            let mut registrar = PartRegistrar::new("amod");
            assert_eq!(
                declare(&mut registrar, "shared:x", 0.8, 1.9, 8, 3, &[]),
                Err(Error::WrongSide)
            );
            assert!(registrar.general_entity_types.is_empty());
        });
    }

    /// The registry is frozen outside the registration phase: a declaration
    /// then refuses with `Frozen`, whatever it names, and the frozen table is
    /// unchanged.
    #[test]
    pub(crate) fn general_entity_types_frozen_outside_registration() {
        crate::__private::initialize().unwrap();
        // Scoped, because the thread pool reuses threads across tests: this
        // test must see `World` even after another test's registration scope.
        let _world = PhaseOwner::scope(ModPhase::World);
        let mut registrar = PartRegistrar::new("mymod");
        assert_eq!(
            declare(&mut registrar, "robot", 0.8, 1.9, 8, 3, &[]),
            Err(Error::Frozen)
        );
        assert!(registrar.general_entity_types.is_empty());
        assert_eq!(registrar.refused.len(), 1);
    }

    /// A declaration from a mod the test inventory does not list stands: the
    /// side check refuses only a listed non-both-side package, exactly as the
    /// attribute registration's does.
    #[test]
    pub(crate) fn general_entity_types_unlisted_mod_declares() {
        crate::__private::initialize().unwrap();
        let (_phase, mut registrar) = registrar();
        declare(&mut registrar, "robot", 0.8, 1.9, 8, 3, &[]).unwrap();
        assert_eq!(registrar.general_entity_types.len(), 1);
        assert!(registrar.refused.is_empty());
    }

    /// A mod's `Hooks::register` receives `&mut dyn Registrar`, and the
    /// generated `Registrar` trait carries `GeneralTypesRegistrar` among its
    /// supertraits: the declaration reaches mods through the registrar they
    /// already hold, with no engine type leaking. Red if the supertrait
    /// wiring is lost: `declare_general_entity` would not resolve on
    /// `dyn Registrar`.
    /// Two mods may own one namespace (a manifest's `namespaces`), but their
    /// registrations run against the one registrar `register_all` drives
    /// (`parts/shared.rs`), so the second mod's declaration of the same key
    /// is a `Duplicate` at registration, with a refusal naming it — never a
    /// panic in the game thread at freeze. Red without the duplicate check:
    /// the second declaration would stand and the frozen registry would hit
    /// the unreachable.
    #[test]
    pub(crate) fn general_entity_types_two_mods_one_key_refuse_the_second() {
        crate::__private::initialize().unwrap();
        use crate::load_state::with_test_package_inventory;
        use crate::{PackageInfo, PackageKind};
        let packages = vec![
            PackageInfo::new(
                "amod".to_owned(),
                "1".to_owned(),
                PackageKind::Code,
                crate::Side::Both,
                vec!["shared".to_owned()],
                vec![],
            ),
            PackageInfo::new(
                "bmod".to_owned(),
                "1".to_owned(),
                PackageKind::Code,
                crate::Side::Both,
                vec!["shared".to_owned()],
                vec![],
            ),
        ];
        let _scope = PhaseOwner::scope(ModPhase::Registration);
        with_test_package_inventory(packages, || {
            let mut registrar = PartRegistrar::new("amod");
            declare(&mut registrar, "shared:x", 0.8, 1.9, 8, 3, &[]).unwrap();
            // What `register_all` does between mods: the same registrar, the
            // next mod's id.
            registrar.mod_id = "bmod";
            assert_eq!(
                declare(&mut registrar, "shared:x", 0.6, 1.2, 4, 2, &[]),
                Err(Error::Duplicate)
            );
            assert_eq!(registrar.general_entity_types.len(), 1);
            assert_eq!(registrar.general_entity_types[0].mod_id, "amod");
            assert_eq!(registrar.refused.len(), 1);
        });
    }

    #[test]
    pub(crate) fn declare_general_entity_reaches_mods_through_dyn_registrar() {
        crate::__private::initialize().unwrap();
        let _scope = PhaseOwner::scope(ModPhase::Registration);
        let mut registrar = PartRegistrar::new("mymod");
        let dynamic: &mut dyn crate::Registrar = &mut registrar;
        dynamic
            .declare_general_entity(
                crate::Identifier::parse("mymod:robot").unwrap(),
                GeneralEntityCategory::Misc,
                0.8,
                1.9,
                None,
                GeneralSavePolicy::Save,
                true,
                false,
                Some(PushReaction::IgnoreEntity),
                8,
                3,
                1,
                &[],
            )
            .unwrap();
        assert_eq!(registrar.general_entity_types.len(), 1);
        assert_eq!(
            registrar.general_entity_types[0].piston_push_reaction,
            PushReaction::IgnoreEntity
        );
    }
}
