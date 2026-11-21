//! Section geometry checked collector and mesher tests (`add_section_geometry`, NeoForge's
//! `AddSectionGeometryEvent` / vanilla `BufferBuilder` and `TextureAtlas.getSprite` semantics).
use super::mod_geometry::{
    MAX_LAYER_VERTICES, MISSING_SPRITE, Renderer, check_quad, run_renderers,
};
use super::mod_tint_tests::{resources, tint_values};
use super::*;
use mod_api::{
    __private::game::take_callback_panic_notices, BlockAndTintGetter, ColorResolver, Direction,
    LightLayer, SectionGeometry, SectionGeometryContext, SectionQuad,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use sunrise_assets::resource_source::ResourceRoot;

const GOLD: &str = "minecraft:block/gold_block";
const GLASS: &str = "minecraft:block/glass";

fn quad(translucent: bool) -> SectionQuad {
    SectionQuad {
        positions: [
            [2.0, 8.0, 2.0],
            [2.0, 8.0, 3.0],
            [3.0, 8.0, 3.0],
            [3.0, 8.0, 2.0],
        ],
        uvs: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        color: 0xFF20_4060,
        light: [15, 0],
        translucent,
    }
}

struct EmptyLevel;

impl BlockAndTintGetter for EmptyLevel {
    fn block_state(&self, _: [i32; 3]) -> mod_api::BlockState {
        unimplemented!("unused by unit tests")
    }
    fn fluid_state(&self, _: [i32; 3]) -> Option<mod_api::FluidState> {
        None
    }
    fn brightness(&self, _: LightLayer, _: [i32; 3]) -> i32 {
        15
    }
    fn raw_brightness(&self, _: [i32; 3], _: i32) -> i32 {
        15
    }
    fn can_see_sky(&self, _: [i32; 3]) -> bool {
        true
    }
    fn shade(&self, _: Direction) -> f32 {
        1.0
    }
    fn block_tint(&self, _: [i32; 3], _: ColorResolver) -> u32 {
        0xFFFF_FFFF
    }
    fn min_y(&self) -> i32 {
        MIN_Y
    }
    fn height(&self) -> i32 {
        384
    }
}

/// One decorative gold quad in every section, lit by the sky light the `level` reads there.
#[derive(Default)]
struct Decorative {
    calls: AtomicUsize,
}
impl SectionGeometry for Decorative {
    fn add(&self, origin: [i32; 3], quads: &mut dyn SectionGeometryContext) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut quad = quad(false);
        let above = [origin[0] + 2, origin[1] + 8, origin[2] + 2];
        quad.light = [quads.level().brightness(LightLayer::Sky, above) as u8, 3];
        assert!(quads.add(GOLD, quad));
    }
}

/// Submits valid, non-finite, out-of-section, extended-UV, and unknown-sprite quads together.
#[derive(Default)]
struct MixedSubmission {
    calls: AtomicUsize,
}
impl SectionGeometry for MixedSubmission {
    fn add(&self, _: [i32; 3], quads: &mut dyn SectionGeometryContext) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let base = quad(false);
        assert!(quads.add(GOLD, base));

        let mut nan_pos = base;
        nan_pos.positions[1][0] = f32::NAN;
        assert!(!quads.add(GOLD, nan_pos));

        let mut inf_uv = base;
        inf_uv.uvs[2][1] = f32::INFINITY;
        assert!(!quads.add(GOLD, inf_uv));

        let mut out_of_section = base;
        out_of_section.positions = [
            [-4.0, 18.5, -2.0],
            [-4.0, 18.5, 20.0],
            [19.25, 18.5, 20.0],
            [19.25, 18.5, -2.0],
        ];
        out_of_section.uvs = [[-0.5, 0.0], [-0.5, 1.75], [1.5, 1.75], [1.5, 0.0]];
        assert!(quads.add(GOLD, out_of_section));

        assert!(quads.add("missing:block/nothing", base));
    }
}

#[derive(Default)]
struct Panics {
    calls: AtomicUsize,
}
impl SectionGeometry for Panics {
    fn add(&self, _: [i32; 3], quads: &mut dyn SectionGeometryContext) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert!(quads.add(GOLD, quad(false)));
        assert!(quads.add(GLASS, quad(true)));
        panic!("a broken renderer");
    }
}

struct Translucent;
impl SectionGeometry for Translucent {
    fn add(&self, _: [i32; 3], quads: &mut dyn SectionGeometryContext) {
        assert!(quads.add(GLASS, quad(true)));
    }
}

fn sprites() -> BTreeMap<String, u32> {
    BTreeMap::from([
        (MISSING_SPRITE.to_owned(), 1),
        (GOLD.to_owned(), 7),
        (GLASS.to_owned(), 12),
    ])
}

#[test]
fn vanilla_vertex_and_sprite_semantics_are_accepted() {
    let sprites = sprites();
    let mut wide = quad(false);
    wide.positions = [
        [-12.5, -3.0, 24.0],
        [-12.5, -3.0, 32.0],
        [20.0, 17.5, 32.0],
        [20.0, 17.5, 24.0],
    ];
    wide.uvs = [[-1.0, -0.25], [-1.0, 2.5], [3.0, 2.5], [3.0, -0.25]];
    assert_eq!(check_quad(&sprites, GOLD, &wide, 0), Ok(7));
    assert_eq!(
        check_quad(&sprites, "unknown:block/does_not_exist", &wide, 0),
        Ok(1),
        "unknown sprite resolves to minecraft:missingno"
    );
}

#[test]
fn non_finite_quad_is_isolated_without_discarding_accepted_quads_or_muting() {
    let sprites = sprites();
    let mixed = Arc::new(MixedSubmission::default());
    let renderers = [Renderer::new(Some("mixed_mod"), mixed.clone())];
    for pass in 0..2 {
        let mut out = SectionMesh::default();
        run_renderers(&renderers, &sprites, &[], &EmptyLevel, [0, 0, 0], &mut out);
        assert_eq!(
            quads(&out.solid),
            3,
            "pass {pass}: accepted quads before and after non-finite rejections remain"
        );
        assert_eq!(out.solid[0].layer, 7);
        assert_eq!(out.solid[4].pos, [-4.0, 18.5, -2.0]);
        assert_eq!(out.solid[8].layer, 1, "unknown sprite uses missingno layer");
    }
    assert_eq!(mixed.calls.load(Ordering::Relaxed), 2);
    assert!(renderers[0].enabled());
}

#[test]
fn per_pass_vertex_limit_matches_vanilla_buffer_builder() {
    let sprites = sprites();
    let q = quad(false);
    assert_eq!(
        check_quad(&sprites, GOLD, &q, MAX_LAYER_VERTICES - 4),
        Ok(7),
        "last quad fitting within BufferBuilder.MAX_VERTEX_COUNT is accepted"
    );
    let err = check_quad(&sprites, GOLD, &q, MAX_LAYER_VERTICES - 3).unwrap_err();
    assert!(err.contains("BufferBuilder.MAX_VERTEX_COUNT"));

    let mut nan_pos = q;
    nan_pos.positions[0][2] = f32::NEG_INFINITY;
    assert!(check_quad(&sprites, GOLD, &nan_pos, 0).is_err());

    let mut nan_uv = q;
    nan_uv.uvs[3][0] = f32::NAN;
    assert!(check_quad(&sprites, GOLD, &nan_uv, 0).is_err());
}

#[test]
fn panicking_renderer_rolls_back_only_its_own_contribution_and_disables_only_itself() {
    let _notice_lock = crate::mod_model_loaders::PANIC_NOTICE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _ = take_callback_panic_notices();

    let sprites = sprites();
    let mut animated = vec![false; 16];
    animated[7] = true;

    let before = Arc::new(Decorative::default());
    let broken = Arc::new(Panics::default());
    let sibling = Arc::new(Decorative::default());
    let renderers = [
        Renderer::new(Some("good_mod"), before.clone()),
        Renderer::new(Some("broken_mod"), broken.clone()),
        Renderer::new(Some("broken_mod"), sibling.clone()),
    ];

    let mut out = SectionMesh::default();
    let block_vertex = TerrainVertex::new([0.0; 3], [0.0; 2], 2, 0xFFFF_FFFF, 0);
    out.solid.extend([block_vertex; 4]);
    out.solid_tints.push(0xFF_FFFF);

    run_renderers(
        &renderers,
        &sprites,
        &animated,
        &EmptyLevel,
        [0, 0, 0],
        &mut out,
    );

    // Block quad + `before` + `sibling`; `broken`'s solid and translucent quads rolled back.
    assert_eq!(quads(&out.solid), 3);
    assert_eq!(out.solid_tints.len(), 3);
    assert!(out.translucent.is_empty());
    assert!(renderers[0].enabled());
    assert!(!renderers[1].enabled());
    assert!(
        renderers[2].enabled(),
        "sibling registration from the same mod stays enabled"
    );

    let notices = take_callback_panic_notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].mod_id, "broken_mod");
    assert_eq!(notices[0].kind, "section geometry");
    assert_eq!(notices[0].operation, "add_section_geometry");
    assert!(notices[0].message.contains("a broken renderer"));

    // Second pass skips the disabled renderer and emits no second panic notice.
    let mut second = SectionMesh::default();
    run_renderers(
        &renderers,
        &sprites,
        &animated,
        &EmptyLevel,
        [16, 0, 0],
        &mut second,
    );
    assert_eq!(quads(&second.solid), 2);
    assert_eq!(broken.calls.load(Ordering::Relaxed), 1);
    assert_eq!(before.calls.load(Ordering::Relaxed), 2);
    assert_eq!(sibling.calls.load(Ordering::Relaxed), 2);
    assert!(take_callback_panic_notices().is_empty());
}

#[test]
fn panicking_renderer_rolls_back_animated_sprite_layers() {
    let _notice_lock = crate::mod_model_loaders::PANIC_NOTICE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _ = take_callback_panic_notices();

    let sprites = sprites();
    let mut animated = vec![false; 16];
    animated[7] = true;
    let renderers = [Renderer::new(
        Some("broken_mod"),
        Arc::new(Panics::default()),
    )];

    let mut out = SectionMesh::default();
    run_renderers(
        &renderers,
        &sprites,
        &animated,
        &EmptyLevel,
        [0, 0, 0],
        &mut out,
    );
    assert!(out.solid.is_empty());
    assert!(out.solid_tints.is_empty());
    assert!(out.translucent.is_empty());
    assert!(out.sprite_layers.is_empty());
    let _ = take_callback_panic_notices();
}

struct World {
    assets: Assets,
    render: RenderAssets,
    facts: BlockFacts,
    biomes: Biomes,
    root: std::path::PathBuf,
}

impl World {
    fn load() -> Option<Self> {
        let root = resources()?;
        let assets = Assets::load(&root).unwrap();
        let facts = BlockFacts::load(&root, assets.registry).unwrap();
        let biomes = Biomes::load(&ResourceRoot::directory(root.clone())).unwrap();
        let render = assets
            .reload(&ResourceRoot::directory(root.clone()))
            .unwrap();
        Some(Self {
            assets,
            render,
            facts,
            biomes,
            root,
        })
    }

    fn visibility(&self) -> FaceVisibility {
        crate::models::load_visibility(&self.root, self.assets.registry).unwrap()
    }

    /// Mesh tables whose section geometry is `renderers`.
    fn tables(
        &self,
        renderers: Vec<(Option<&'static str>, Arc<dyn SectionGeometry>)>,
    ) -> MeshTables {
        let mut tables = MeshTables::new(
            &self.assets,
            &self.render,
            &self.facts,
            self.visibility(),
            None,
        )
        .unwrap();
        tables.section_geometry = renderers
            .into_iter()
            .map(|(owner, renderer)| Renderer::new(owner, renderer))
            .collect();
        tables.mod_tints.values = Some(tint_values(self.assets.registry));
        tables
    }

    fn mesher<'a>(&'a self, tables: &'a MeshTables) -> Mesher<'a> {
        Mesher {
            nether_shading: false,
            shader_pack_lighting: false,
            separate_ao: false,
            shader_pack: false,
            assets: &self.assets,
            render: &self.render,
            tables,
            biomes: &self.biomes,
            visibility: self.visibility(),
            zoom: BiomeZoom::new(0),
            blend_radius: 0,
            smooth_lighting: false,
            cutout_leaves: true,
            opaque_ice: false,
            translucent_layers: &[],
            leaf_culling: LeafCullingMode::Off,
            hidden_states: &[],
            part_tables: Default::default(),
        }
    }

    /// A chunk with one stone block in section 0 and section 1 empty.
    fn cube(&self) -> Chunk {
        let stone = self
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        let mut chunk = Chunk::empty(0, 0);
        chunk.set(2, MIN_Y + 4, 2, stone.0);
        chunk
    }
}

fn quads(vertices: &[TerrainVertex]) -> usize {
    vertices.len() / 4
}

#[test]
fn a_decorative_quad_joins_its_section_once_per_compilation() {
    let Some(world) = World::load() else {
        return;
    };
    let decorative = Arc::new(Decorative::default());
    let tables = world.tables(vec![(Some("decorative"), decorative.clone())]);
    let mut mesher = world.mesher(&tables);
    let chunk = world.cube();
    let mut region = [[None; 3]; 3];
    region[1][1] = Some(&chunk);
    // Isolated cube plus one quad: 7 quads, the callback asked once.
    let cube = mesher.mesh(&region, 0);
    assert_eq!(quads(&cube.solid), 7);
    assert_eq!(decorative.calls.load(Ordering::Relaxed), 1);
    let gold = world.render.sprite_layers[GOLD] as u16;
    let added: Vec<_> = cube
        .solid
        .iter()
        .filter(|v| v.layer & !OPAQUE_LAYER == gold)
        .collect();
    assert_eq!(added.len(), 4);
    assert!(
        added.iter().all(|v| v.color == 0xFF60_4020),
        "ARGB packed as RGBA"
    );
    assert!(added.iter().all(|v| v.light == (15 * 16) | (3 * 16) << 8));
    // Empty section plus one quad: 1 quad after the build and after each of 3 remeshes into the
    // same buffers.
    let mut scratch = MeshScratch::default();
    let mut empty = mesher.mesh_reusing(&region, 1, SectionMesh::default(), &mut scratch);
    assert_eq!(quads(&empty.solid), 1);
    for _ in 0..3 {
        empty = mesher.mesh_reusing(&region, 1, empty, &mut scratch);
        assert_eq!(quads(&empty.solid), 1);
    }
    assert_eq!(decorative.calls.load(Ordering::Relaxed), 5);
}

#[test]
fn collector_isolates_failures_and_uses_vanilla_sprite_fallback_in_real_mesher() {
    let Some(world) = World::load() else {
        return;
    };
    let _notice_lock = crate::mod_model_loaders::PANIC_NOTICE_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _ = take_callback_panic_notices();

    assert!(
        world.render.sprite_layers.contains_key(MISSING_SPRITE),
        "block atlas always registers minecraft:missingno"
    );
    let missing_layer = world.render.sprite_layers[MISSING_SPRITE] as u16;

    let renderers: Vec<(Option<&'static str>, Arc<dyn SectionGeometry>)> = vec![
        (Some("panicking_mod"), Arc::new(Panics::default())),
        (Some("mixed_mod"), Arc::new(MixedSubmission::default())),
        (Some("glass_mod"), Arc::new(Translucent)),
    ];
    let tables = world.tables(renderers);
    let mut mesher = world.mesher(&tables);
    let chunk = world.cube();
    let mut region = [[None; 3]; 3];
    region[1][1] = Some(&chunk);
    for _ in 0..2 {
        let mesh = mesher.mesh(&region, 0);
        // 6 cube faces + 3 accepted quads from MixedSubmission (1 base + 1 out-of-section + 1 missingno).
        assert_eq!(quads(&mesh.solid), 9);
        assert!(
            mesh.solid
                .iter()
                .any(|v| v.layer & !OPAQUE_LAYER == missing_layer),
            "unknown sprite quad draws with minecraft:missingno"
        );
        assert_eq!(
            quads(&mesh.translucent),
            1,
            "the translucent renderer still adds its quad"
        );
    }
    assert!(
        !tables.section_geometry[0].enabled(),
        "the panicking renderer is disabled"
    );
    assert!(tables.section_geometry[1].enabled());
    assert!(tables.section_geometry[2].enabled());
    let notices = take_callback_panic_notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].mod_id, "panicking_mod");

    // No listeners: the mesh is the built-in one.
    let none = world.tables(Vec::new());
    let baseline = world.mesher(&none).mesh(&region, 0);
    assert_eq!(quads(&baseline.solid), 6);
    assert!(baseline.translucent.is_empty());
    assert!(world.mesher(&none).mesh(&region, 1).solid.is_empty());
}
