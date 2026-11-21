use super::*;

impl Game<'_> {
    pub(in crate::game) fn native_register_emissive_sprites(
        context: &mod_api::RegistrationContext,
        layers: &mut Vec<mod_api::Answered<(mod_api::Identifier, mod_api::Identifier)>>,
    ) {
        Self::fire_register_emissive_sprites(context, layers);
    }

    pub(in crate::game) fn native_register_texture_atlases(
        context: &mod_api::RegistrationContext,
        atlases: &mut Vec<mod_api::Answered<TextureAtlasRegistration>>,
    ) {
        Self::fire_register_texture_atlases(context, atlases);
    }
}
