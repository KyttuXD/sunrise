//! Client emissive sprite registrations, resolved during the resource bake.
// catalog-order: 0002

pub trait Hooks {
    /// Register terrain sprite overlays at startup and each client resource reload.
    /// The base is a terrain sprite id; the overlay is a texture id without `.png`.
    /// Per-pixel overlay alpha marks full-bright material without changing world light.
    /// Call `event.register((base, overlay))` with two owned [`Identifier`]s.
    /// A later usable registration for the same base replaces an earlier usable one.
    /// Missing or invalid textures are reported at bake time. Overlay layers that do not
    /// fit the device's terrain arrays are reported without dropping the base sprites.
    #[hook(
        target = "sunrise_client::game::native_hooks::register_texture_atlases::Game<'_>::native_register_emissive_sprites",
        receiver = "associated",
        dispatch = "flat",
        side = "client",
        contract = "collect",
        out = "layers",
        registration_append = "register",
        context = "registration",
        answered_by = "mod",
        owned_answer_batch = true,
        neoforge = "none (Sunrise-only; NeoForge has no counterpart)",
        version = 2
    )]
    fn register_emissive_sprites(layers: &mut Vec<Answered<(Identifier, Identifier)>>);
}
