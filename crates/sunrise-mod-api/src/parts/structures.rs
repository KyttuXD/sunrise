//! Concrete authoring extracted verbatim from the vocabulary layer.
use super::*;

/// Fixtures that tests of other crates use to install Parts.
#[cfg(any(test, feature = "testing"))]
pub mod tests {
    #[cfg(test)]
    use super::*;
    #[cfg(test)]
    use crate::*;

    /// Explicit namespaces succeed while a structure type twice, a built-in structure type and
    /// one without a name refuse at startup, naming the mod.
    #[test]
    pub(crate) fn duplicate_and_builtin_structure_types_refuse_registration() {
        struct Tower;
        impl mod_api::StructureType for Tower {
            fn find_generation_point(
                &self,
                _: &mut dyn mod_api::GenerationContext,
            ) -> Option<mod_api::FoundStructure> {
                None
            }
            fn load_piece(
                &self,
                _: &dyn mod_api::BlockMetadata,
                _: &mod_api::Compound,
            ) -> Result<std::sync::Arc<dyn mod_api::StructurePiece>, mod_api::StructureError>
            {
                Err(mod_api::StructureError::new(std::io::Error::other(
                    "fixture has no pieces",
                )))
            }
        }
        struct Valid;
        #[mod_api::__subscriptions]
        impl Hooks for Valid {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                r.register_structure_type("tower", Tower);
                r.register_structure_type("other:tower", Tower);
                r.register_structure_type("minecraft:custom_tower", Tower);
            }
        }
        struct Invalid;
        #[mod_api::__subscriptions]
        impl Hooks for Invalid {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                r.register_structure_type("tower", Tower);
                r.register_structure_type("tower", Tower);
                r.register_structure_type("minecraft:jigsaw", Tower);
                r.register_structure_type("", Tower);
            }
        }
        let data = mod_vocab::__private::RegistrationData::new(
            [(
                "minecraft:worldgen/structure_type".into(),
                vec!["minecraft:jigsaw".into()],
            )]
            .into(),
            Default::default(),
            crate::InstanceDirectory::from_path(std::path::Path::new("unused")).unwrap(),
            crate::RuntimeEnvironment::new(
                crate::Dist::Client,
                "26.3".into(),
                "test".into(),
                false,
            ),
        );
        crate::load_state::with_test_registration_data(data, || {
            crate::parts::tests::install_parts_fixture(&[ModEntry::new("test", Box::new(Valid))])
                .unwrap();
            let types = crate::__private::structure_type_declarations();
            assert_eq!(
                types
                    .iter()
                    .map(|(id, owner, _)| (id.as_str(), *owner))
                    .collect::<Vec<_>>(),
                [
                    ("test:tower", "test"),
                    ("other:tower", "test"),
                    ("minecraft:custom_tower", "test")
                ]
            );

            let errors = install_parts(
                &[ModEntry::new("test", Box::new(Invalid))],
                &crate::parts::tests::rule_schema(),
                |_| false,
            )
            .unwrap_err();
            assert_eq!(errors.len(), 3, "{errors:?}");
            assert!(errors[0].contains("test registered structure type test:tower twice"));
            assert!(errors[1].contains("test registered built-in structure type minecraft:jigsaw"));
            assert!(errors[2].contains("invalid identifier \"test:\""));
        });
    }
}

impl StructuresRegistrar for PartRegistrar {
    fn add_structure_type(&mut self, name: &str, ty: Arc<dyn crate::StructureType>) {
        let id = match self.declaration_id(name) {
            Ok(id) => id,
            Err(reason) => {
                self.refused.push(reason);
                return;
            }
        };
        let refusal = if self.structure_types.iter().any(|t| *t.id == *id) {
            Some(format!(
                "{} registered structure type {id} twice",
                self.mod_id
            ))
        } else if self.has_builtin_entry("minecraft:worldgen/structure_type", &id) {
            Some(format!(
                "{} registered built-in structure type {id}",
                self.mod_id
            ))
        } else {
            None
        };
        if let Some(refusal) = refusal {
            self.refused.push(refusal);
            return;
        }
        self.structure_types.push(StructureTypeInfo {
            id: id.into(),
            mod_id: self.mod_id,
            ty,
        });
    }
}
