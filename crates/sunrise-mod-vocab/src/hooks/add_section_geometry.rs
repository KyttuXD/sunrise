//! The `add_section_geometry` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Extra geometry in chunk section meshes (NeoForge's `AddSectionGeometryEvent`):
    /// `register(renderer)` adds the mod's own `SectionGeometry`, which the mesher asks to add its
    /// quads every time it meshes a section, on its worker threads. The quads are part of that
    /// section's mesh: they follow its rebuilds, are never drawn twice, and go when it unloads.
    /// Asked once at start on the client, before resources load.
    #[notes = r#"## Section geometry

Section geometry (NeoForge's `AddSectionGeometryEvent`) is quads a mod adds to a chunk section's own mesh, for
geometry that belongs to the terrain but no block model draws: a mod's wires between blocks, markings on the ground.
`Hooks::add_section_geometry` registers the mod's `SectionGeometry` once at start, and each time a section meshes
the mesher asks its `add(origin, context)` to add its quads, after the section's blocks:

```rust,ignore
struct Marker;
impl SectionGeometry for Marker {
    fn add(&self, origin: [i32; 3], context: &mut dyn SectionGeometryContext) {
        if origin != [0, 64, 0] {
            return;
        }
        let light = [context.level().brightness(LightLayer::Sky, [8, 65, 8]) as u8, 0];
        context.add("minecraft:block/gold_block", SectionQuad {
            positions: [[7.0, 1.01, 7.0], [7.0, 1.01, 9.0], [9.0, 1.01, 9.0], [9.0, 1.01, 7.0]],
            uvs: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            color: 0xFFFF_FFFF,
            light,
            translucent: false,
        });
    }
}

fn add_section_geometry(&self, _context: &RegistrationContext, event: &mut AddSectionGeometryEvent<'_>) {
    event.renderers.register(Arc::new(Marker));
}
```

Positions are in blocks from the section's lowest corner, UVs are within the sprite, and the sprite is a block
atlas sprite's name. Matching vanilla's `BufferBuilder` and `TextureAtlas.getSprite`, any finite positions and UVs
are accepted, and an unknown sprite falls back to `minecraft:missingno`. A quad with a non-finite position or UV,
or one that would exceed vanilla's `BufferBuilder.MAX_VERTEX_COUNT` (16,777,215 vertices per render pass), is
rejected individually with a warning; `context.add` returns `false` for that quad without discarding earlier
accepted quads or muting later calls. A renderer that panics rolls back only its own quads for that section, logs
an error and shows an error toast naming the mod, and is disabled for the session; the section's blocks and other
renderers are unaffected. `context.level()` reads the section's chunk and its eight neighbours, as a block tint
source's `level` does. The renderer runs on the meshing workers, so it keeps no mutable state and gets no `Host`;
it is asked for every section the client meshes, also one that holds only air, and returns quickly when it has
nothing there. Its quads are shaded only by the colour and light it gives them. A section meshes again when its
blocks change, when the resources reload, and when a mod calls `ClientHost::refresh_render_visibility` and the
hidden blocks change."#]
    #[hook(
        receiver = "associated",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        context = "registration",
        neoforge = "AddSectionGeometryEvent",
        version = 2
    )]
    fn add_section_geometry(renderers: &mut dyn mod_api::SectionGeometryRenderers);
}
