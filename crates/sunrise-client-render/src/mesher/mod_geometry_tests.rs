//! The section geometry design's acceptance cases (docs/design/mod-api/section-geometry.md)
//! that the mesher alone decides.
use super::mod_geometry::Renderer;
use super::mod_tint_tests::resources;
use super::*;
use mod_api::{LightLayer, SectionGeometry, SectionGeometryContext, SectionQuad, SectionQuadError};
use std::sync::atomic::{AtomicUsize, Ordering};
use sunrise_assets::resource_source::ResourceRoot;

const GOLD: &str = "minecraft:block/gold_block";

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
        quads.add(GOLD, quad).unwrap();
    }
}

/// Submits valid geometry on both sides of individual refusals.
struct Mixed;
impl SectionGeometry for Mixed {
    fn add(&self, _: [i32; 3], quads: &mut dyn SectionGeometryContext) {
        quads.add(GOLD, quad(false)).unwrap();
        let mut bad = quad(false);
        bad.positions[1][0] = f32::INFINITY;
        assert_eq!(
            quads.add(GOLD, bad),
            Err(SectionQuadError::NonFinitePosition)
        );
        bad = quad(false);
        bad.uvs[2][1] = f32::NAN;
        assert_eq!(quads.add(GOLD, bad), Err(SectionQuadError::NonFiniteUv));
        let mut outside = quad(false);
        outside.positions[1][0] = 17.25;
        outside.uvs[0] = [-0.250_001, 2.75];
        quads.add("missing:block/nothing", outside).unwrap();
    }
}

#[derive(Default)]
struct Panics(AtomicUsize);
impl SectionGeometry for Panics {
    fn add(&self, _: [i32; 3], quads: &mut dyn SectionGeometryContext) {
        self.0.fetch_add(1, Ordering::Relaxed);
        quads.add(GOLD, quad(false)).unwrap();
        panic!("a broken renderer");
    }
}

struct Translucent;
impl SectionGeometry for Translucent {
    fn add(&self, _: [i32; 3], quads: &mut dyn SectionGeometryContext) {
        quads.add("minecraft:block/glass", quad(true)).unwrap();
    }
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
    fn tables(&self, renderers: impl IntoIterator<Item = Renderer>) -> MeshTables {
        let mut tables = MeshTables::new(
            &self.assets,
            &self.render,
            &self.facts,
            self.visibility(),
            None,
        )
        .unwrap();
        tables.section_geometry = renderers.into_iter().collect();
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
#[ignore = "daily"]
fn a_decorative_quad_joins_its_section_once_per_compilation() {
    let world = World::load().expect("prepared resources");
    let decorative = Arc::new(Decorative::default());
    let tables = world.tables([Renderer::new("decorative", decorative.clone())]);
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
#[ignore = "daily"]
fn refusal_only_skips_its_quad_and_missing_sprites_draw() {
    let world = World::load().expect("prepared resources");
    let tables = world.tables([Renderer::new("mixed", Arc::new(Mixed))]);
    let mut mesher = world.mesher(&tables);
    let chunk = world.cube();
    let mut region = [[None; 3]; 3];
    region[1][1] = Some(&chunk);
    for _ in 0..2 {
        let mesh = mesher.mesh(&region, 0);
        assert_eq!(quads(&mesh.solid), 8, "cube plus the two finite quads");
        let gold = world.render.sprite_layers[GOLD] as u16;
        let missing = world.render.sprite_layers["minecraft:missingno"] as u16;
        assert_ne!(gold, missing);
        assert_eq!(
            mesh.solid[24..28]
                .iter()
                .map(|v| v.layer)
                .collect::<Vec<_>>(),
            [gold; 4]
        );
        assert_eq!(
            mesh.solid[28..32]
                .iter()
                .map(|v| v.layer)
                .collect::<Vec<_>>(),
            [missing; 4]
        );
        assert_eq!(mesh.solid[29].pos, [17.25, 8.0, 3.0]);
        assert_eq!(
            mesh.solid[28].uv().map(f32::to_bits),
            [-0.250_001_f32, 2.75].map(f32::to_bits),
            "sprite-relative UVs reach the mesh without rounding or clamping"
        );
        assert_eq!(
            mesh.compute_bounds().unwrap()[1].x,
            17.25 + SectionMesh::BOUNDS_PAD
        );
    }
}

#[test]
#[ignore = "daily"]
fn panic_keeps_completed_quads_and_disables_only_its_callback() {
    use mod_api::__private::game::{
        RegistrationContextNew, SectionGeometryRegistry, take_callback_panic_notices,
    };

    // The client toast queue is process-wide. Check this callback's notices without draining
    // notices from other tests running on their own threads.
    const CHILD: &str = "SUNRISE_SECTION_GEOMETRY_PANIC_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "mesher::mod_geometry_tests::panic_keeps_completed_quads_and_disables_only_its_callback",
            ])
            .env(CHILD, "1")
            .output()
            .expect("run the isolated callback test");
        let stdout = String::from_utf8_lossy(&child.stdout);
        assert!(
            child.status.success() && stdout.contains("test result: ok. 1 passed;"),
            "isolated section callback test failed:\n{stdout}{}",
            String::from_utf8_lossy(&child.stderr),
        );
        return;
    }

    struct Registrant(Arc<Panics>);
    #[mod_api::__subscriptions]
    impl mod_api::Hooks for Registrant {
        fn add_section_geometry(
            &self,
            _: &mod_api::RegistrationContext,
            event: &mut mod_api::AddSectionGeometryEvent<'_>,
        ) {
            event.renderers.register(self.0.clone());
        }
    }

    let panics = Arc::new(Panics::default());
    let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
        "broken_geometry",
        Box::new(Registrant(panics.clone())),
    )]
    .into();
    let mut registry = SectionGeometryRegistry::default();
    mod_platform::testing::with_mods(mods, || {
        mod_api::dispatch_add_section_geometry(&mod_api::RegistrationContext::new(), &mut registry);
    });
    let world = World::load().expect("prepared resources");
    let registered = registry.into_renderers();
    let tables = world.tables(
        registered
            .into_iter()
            .map(|(id, renderer)| Renderer::new(id, renderer))
            .chain([Renderer::new("other_geometry", Arc::new(Translucent))]),
    );
    let mut mesher = world.mesher(&tables);
    let chunk = world.cube();
    let mut region = [[None; 3]; 3];
    region[1][1] = Some(&chunk);
    for solid in [7, 6] {
        let mesh = mesher.mesh(&region, 0);
        assert_eq!(quads(&mesh.solid), solid);
        assert_eq!(quads(&mesh.translucent), 1, "the other callback still runs");
    }
    assert_eq!(panics.0.load(Ordering::Relaxed), 1);
    let notices = take_callback_panic_notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].mod_id, "broken_geometry");
    assert_eq!(notices[0].operation, "add_section_geometry");
    assert!(notices[0].message.contains("a broken renderer"));
}
