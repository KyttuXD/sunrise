//! Concrete authoring extracted verbatim from the vocabulary layer.
use super::*;

/// Fixtures that tests of other crates use to install Parts.
#[cfg(any(test, feature = "testing"))]
pub mod tests {
    #[cfg(test)]
    use crate::*;

    #[test]
    pub(crate) fn settings_are_owned_and_bad_or_duplicate_ones_are_refused() {
        let mut registrar = super::PartRegistrar::new("first");
        let first = registrar
            .register_setting(SettingSpec::toggle("on", "first.on", true))
            .unwrap();
        assert_eq!(
            registrar
                .register_setting(SettingSpec::toggle("on", "first.on", false))
                .unwrap_err(),
            SettingError::Duplicate("on".into())
        );
        assert_eq!(
            registrar
                .register_setting(SettingSpec::range(
                    "speed",
                    "first.speed",
                    1.0,
                    0.0,
                    0.1,
                    0.5
                ))
                .unwrap_err(),
            SettingError::EmptyRange
        );
        registrar.mod_id = "second";
        let second = registrar
            .register_setting(SettingSpec::toggle("on", "second.on", false))
            .unwrap();
        let owners = [&first, &second].map(|key| {
            let setting = crate::__private::setting_registration(key);
            (setting.owner, setting.index)
        });
        assert_eq!(owners, [("first", 0), ("second", 1)]);
        assert!(
            registrar.refused.is_empty(),
            "a refused setting is the mod's error, not startup's"
        );
    }
}

impl SettingsRegistrar for PartRegistrar {
    fn register_setting(
        &mut self,
        setting: crate::SettingSpec,
    ) -> Result<crate::SettingKey, crate::SettingError> {
        setting.check()?;
        if self.parts.settings.iter().any(|key| {
            let registered = crate::__private::setting_registration(key);
            registered.owner == self.mod_id && registered.spec.key == setting.key
        }) {
            return Err(crate::SettingError::Duplicate(setting.key));
        }
        let key =
            crate::__private::make_setting_key(self.mod_id, self.parts.settings.len(), setting);
        self.parts.settings.push(key.clone());
        Ok(key)
    }
}
