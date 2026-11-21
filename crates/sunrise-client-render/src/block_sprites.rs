//! Where a block's textures are in the block sprite arrays, side by side, for drawing the block
//! somewhere the mesher does not: a native mod's own pass drawing blocks as voxels
//! (`visit_block_sprites`, `WorldDrawn::block_sprites`; docs/modding/src/how-to/native-mods.md). The answer is the
//! model's own faces, as the mesher bakes them: each face's sprite (its array and layer), where on
//! the block's side it lies and how its texture runs across it, and its tint source.
//!
//! Each sprite is a whole layer of a square texture array (`sprites::Sprites`), so a face's UVs run
//! 0 to 1 within its layer; the renderer's split (`Renderer::sprite_split`) layers to an array.
use crate::{
    baked_model_views::direction, mesher::TintSource, models::catalog::BakedFace,
    streaming::WorkerContext,
};
use mod_api::{__private::built, BlockSpriteInfo, FaceSprite, SpriteRef, TextureSprite};
use sunrise_assets::model::Direction;
use sunrise_core::{position::BlockPos, random::position_seed};
use sunrise_world::registry::{BlockRegistry, BlockStateId};

/// The two axes a side's `rect` and `uv` count along, as indices into (x, y, z): down and up across x then z; north
/// and south across x then y; west and east across z then y. (The block's corner at 0, 0, 0 is 0 on each.)
pub fn side_axes(side: Direction) -> (usize, usize) {
    match side {
        Direction::Down | Direction::Up => (0, 2),
        Direction::North | Direction::South => (0, 1),
        Direction::West | Direction::East => (2, 1),
    }
}

/// The axis a side faces along, and where its plane is (0 or 1).
fn side_plane(side: Direction) -> (usize, f32) {
    match side {
        Direction::Down => (1, 0.0),
        Direction::Up => (1, 1.0),
        Direction::North => (2, 0.0),
        Direction::South => (2, 1.0),
        Direction::West => (0, 0.0),
        Direction::East => (0, 1.0),
    }
}

/// A block state from `name[property=value,...]`, the namespace `minecraft:` when it has none; properties left
/// out take the block's default. None for a block or a property the game does not know.
pub fn parse_state(registry: &BlockRegistry, text: &str) -> Option<BlockStateId> {
    let (name, props) = match text.trim().split_once('[') {
        Some((n, rest)) => (n, rest.strip_suffix(']')?),
        None => (text.trim(), ""),
    };
    let name = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    let default = registry.default_state(&name)?;
    let mut state = registry.state(default)?.clone();
    for pair in props.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=')?;
        let slot = state.properties.get_mut(k.trim())?;
        *slot = v.trim().to_owned();
    }
    registry.resolve(&state).ok()
}

/// What a fluid block (`minecraft:water`, a bubble column, lava) shows on every side instead of a model:
/// its still texture, its tint source (water's is the biome's, `MeshTables::water_tint`, at tint index 0)
/// and whether it is translucent. None for any other block.
pub fn fluid_faces(block: &str) -> Option<(&'static str, Option<TintSource>, bool)> {
    match block {
        "minecraft:water" | "minecraft:bubble_column" => {
            Some(("minecraft:block/water_still", Some(TintSource::Water), true))
        }
        "minecraft:lava" => Some(("minecraft:block/lava_still", None, false)),
        _ => None,
    }
}

/// Each sprite layer's texture name, as `native_draw_cube` and `native_draw_quad` take it: vanilla's
/// without its namespace (`block/stone`), a mod's with it (`mymod:block/gear`).
pub fn texture_names(
    sprite_layers: &std::collections::BTreeMap<String, u32>,
) -> std::collections::HashMap<u32, String> {
    sprite_layers
        .iter()
        .map(|(name, layer)| (*layer, name.trim_start_matches("minecraft:").to_owned()))
        .collect()
}

/// The sprites `context`'s resources hold, `split` layers to an array (`Renderer::sprite_split`), and
/// their names (`texture_names`).
pub struct Lookup<'a> {
    context: &'a WorkerContext,
    split: u32,
    names: &'a std::collections::HashMap<u32, String>,
}

impl<'a> Lookup<'a> {
    pub fn new(
        context: &'a WorkerContext,
        split: u32,
        names: &'a std::collections::HashMap<u32, String>,
    ) -> Self {
        Self {
            context,
            split: split.max(1),
            names,
        }
    }
    /// The sprite at `layer` of all the layers.
    fn sprite(&self, layer: u32) -> SpriteRef {
        let render = &self.context.render;
        built::sprite_ref(
            render.generation,
            layer / self.split,
            layer % self.split,
            render.animations.iter().any(|a| a.layer == layer),
            render
                .sprite_mips
                .get(layer as usize)
                .is_some_and(|m| m.has_transparent),
        )
    }

    /// Lend a texture by its namespaced atlas id; none if the atlas lacks it.
    pub fn texture(&self, name: &str) -> Option<TextureSprite<'a>> {
        let (name, &layer) = self.context.render.sprite_layers.get_key_value(name)?;
        Some(TextureSprite {
            name,
            sprite: self.sprite(layer),
        })
    }

    /// Lend what `state` draws on its sides. A fluid has its still texture on all six; a block drawn
    /// only as a block entity or not at all has no side faces, and its particle sprite if any.
    pub fn visit_state(
        &self,
        state: BlockStateId,
        visit: &mut dyn FnMut(mod_api::Direction, FaceSprite<'a>),
    ) -> Option<BlockSpriteInfo> {
        let block = self.context.assets.registry.state(state)?;
        if let Some((texture, tint, translucent)) = fluid_faces(&block.name) {
            let particle = self.texture(texture).map(|view| view.sprite);
            if let Some(sprite) = particle {
                let face = FaceSprite {
                    sprite,
                    texture: texture.strip_prefix("minecraft:").unwrap_or(texture),
                    tint_index: if tint.is_some() { 0 } else { -1 },
                    rect: [0.0, 0.0, 1.0, 1.0],
                    inset: 0.0,
                    uv: [0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
                    tint,
                    translucent,
                };
                for side in mod_api::Direction::ALL {
                    visit(side, face);
                }
            }
            return Some(BlockSpriteInfo {
                particle,
                cube: particle.is_some(),
            });
        }

        let catalog = &self.context.render.model_catalog;
        let mut first_layer = None;
        let mut sides = [false; 6];
        let mut whole = true;
        // Match the original model selection at the block origin, including multipart order.
        let _ = catalog.each_seeded(
            state,
            position_seed(BlockPos::new(0, 0, 0)),
            |(_, model)| {
                for face in &model.faces {
                    first_layer.get_or_insert(face.layer);
                    let Some((side, face)) = self.face(state, face) else {
                        whole = false;
                        continue;
                    };
                    whole &= face.rect == [0.0, 0.0, 1.0, 1.0] && face.inset == 0.0;
                    sides[side as usize] = true;
                    visit(direction(side), face);
                }
            },
        );
        let particle = catalog
            .particle(state)
            .or(first_layer)
            .map(|layer| self.sprite(layer));
        Some(BlockSpriteInfo {
            particle,
            cube: whole && sides.into_iter().all(|side| side),
        })
    }

    /// A model face as it lies on its side; none when it does not lie flat on one.
    fn face(&self, state: BlockStateId, face: &BakedFace) -> Option<(Direction, FaceSprite<'a>)> {
        let q = &face.quad;
        let side = q.facing;
        let (normal, plane) = side_plane(side);
        let depth = q.positions[0][normal];
        if q.positions.iter().any(|p| (p[normal] - depth).abs() > 1e-4) {
            return None;
        }
        let (a, b) = side_axes(side);
        let ab: [[f32; 2]; 4] = q.positions.map(|p| [p[a], p[b]]);
        let uv = affine(&ab, &q.uv)?;
        let (mut a0, mut b0, mut a1, mut b1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for [x, y] in ab {
            a0 = a0.min(x);
            b0 = b0.min(y);
            a1 = a1.max(x);
            b1 = b1.max(y);
        }
        let snap = |v: f32| {
            let r = (v * 4096.0).round() / 4096.0;
            if r == 0.0 { 0.0 } else { r }
        };
        Some((
            side,
            FaceSprite {
                sprite: self.sprite(face.layer),
                texture: self.names.get(&face.layer).map_or("", String::as_str),
                tint_index: q.tint_index,
                rect: [a0, b0, a1, b1].map(snap),
                inset: snap((depth - plane).abs()),
                uv: uv.map(snap),
                tint: self.context.tables.tint_source(state, q.tint_index),
                translucent: face.translucent,
            },
        ))
    }
}

/// The affine map from a face's points on its side to its UVs, `[u0, v0, du_u, du_v, dv_u, dv_v]`, from three of
/// its corners that are not in a line (a model's faces are parallelograms on their side).
fn affine(ab: &[[f32; 2]; 4], uv: &[[f32; 2]; 4]) -> Option<[f32; 6]> {
    for [i, j, k] in [[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]] {
        let d1 = [ab[j][0] - ab[i][0], ab[j][1] - ab[i][1]];
        let d2 = [ab[k][0] - ab[i][0], ab[k][1] - ab[i][1]];
        let det = d1[0] * d2[1] - d1[1] * d2[0];
        if det.abs() < 1e-6 {
            continue;
        }
        let e1 = [uv[j][0] - uv[i][0], uv[j][1] - uv[i][1]];
        let e2 = [uv[k][0] - uv[i][0], uv[k][1] - uv[i][1]];
        // e1 = d1.a * du + d1.b * dv, e2 = d2.a * du + d2.b * dv
        let du = [
            (e1[0] * d2[1] - e2[0] * d1[1]) / det,
            (e1[1] * d2[1] - e2[1] * d1[1]) / det,
        ];
        let dv = [
            (e2[0] * d1[0] - e1[0] * d2[0]) / det,
            (e2[1] * d1[0] - e1[1] * d2[0]) / det,
        ];
        let o = [
            uv[i][0] - ab[i][0] * du[0] - ab[i][1] * dv[0],
            uv[i][1] - ab[i][0] * du[1] - ab[i][1] * dv[1],
        ];
        return Some([o[0], o[1], du[0], du[1], dv[0], dv[1]]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_faces_texture_runs_across_its_side() {
        // a whole face, the texture upright: u along a, v along b
        let ab = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert_eq!(
            affine(&ab, &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
            Some([0.0, 0.0, 1.0, 0.0, 0.0, 1.0])
        );
        // turned a quarter (a log on its side): u runs down b, v back along a
        let turned = affine(&ab, &[[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]).unwrap();
        assert_eq!(turned, [0.0, 1.0, 0.0, -1.0, 1.0, 0.0]);
        // a slab's side: the lower half of the block, the lower half of the texture (v counts down)
        let half = [[0.0, 0.0], [1.0, 0.0], [1.0, 0.5], [0.0, 0.5]];
        let m = affine(&half, &[[0.0, 1.0], [1.0, 1.0], [1.0, 0.5], [0.0, 0.5]]).unwrap();
        assert_eq!(m, [0.0, 1.0, 1.0, 0.0, 0.0, -1.0]);
        // a face seen edge on has no map
        assert_eq!(affine(&[[0.0, 0.0]; 4], &[[0.0, 0.0]; 4]), None);
    }

    /// The game's own blocks from real prepared resources: a cube, a grass block's tinted top and overlaid sides, a
    /// log on its side (its texture turned), leaves (cut out, foliage), water, a slab and a flower.
    #[test]
    #[ignore = "daily"]
    fn real_blocks_name_their_sprites() -> crate::Result<()> {
        use crate::{
            assets::Assets, biome::Biomes, blocks::BlockFacts, light::LightTables,
            mesher::MeshTables,
        };
        use std::sync::Arc;
        let root = std::path::PathBuf::from(
            std::env::var_os("SUNRISE_TEST_RESOURCES").expect("SUNRISE_TEST_RESOURCES"),
        );
        let assets = Arc::new(Assets::load(&root)?);
        let render = Arc::new(assets.reload(
            &sunrise_assets::resource_source::ResourceRoot::directory(root.to_path_buf()),
        )?);
        let facts = BlockFacts::load(&root, assets.registry)?;
        let tables = Arc::new(MeshTables::new(
            &assets,
            &render,
            &facts,
            crate::models::load_visibility(&root, assets.registry)?,
            None,
        )?);
        let context = WorkerContext {
            light: Arc::new(LightTables::load(&root, assets.registry, &facts)?),
            tables,
            assets: assets.clone(),
            render,
            biomes: Arc::new(Biomes::load(
                &sunrise_assets::resource_source::ResourceRoot::directory(root.to_path_buf()),
            )?),
            hidden_states: Arc::from([]),
            part_tables: Default::default(),
        };
        let names = texture_names(&context.render.sprite_layers);
        let lookup = Lookup::new(&context, 2048, &names);
        let whole = [0.0, 0.0, 1.0, 1.0];
        let upright = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];

        type Sides<'a> = [Vec<FaceSprite<'a>>; 6];
        let query = |name: &str| {
            let state = parse_state(context.assets.registry, name)?;
            let mut sides: Sides<'_> = Default::default();
            let info = lookup.visit_state(state, &mut |direction, face| {
                sides[direction as usize].push(face);
            })?;
            Some((info, sides))
        };
        let (stone, sides) = query("minecraft:stone").unwrap();
        assert!(stone.cube);
        let top = &sides[Direction::Up as usize][0];
        let (array, layer) = top.sprite.location(context.render.generation).unwrap();
        assert_eq!(
            (top.rect, top.inset, top.tint, array),
            (whole, 0.0, None, 0)
        );
        assert_eq!(
            layer,
            context.render.sprite_layers["minecraft:block/stone"] % 2048
        );
        // By the name the drawing helpers take, untinted.
        assert_eq!((top.texture, top.tint_index), ("block/stone", -1));
        let named = lookup.texture("minecraft:block/stone").unwrap();
        assert_eq!(named.name, "minecraft:block/stone");
        assert_eq!(named.sprite, top.sprite);
        assert!(lookup.texture("mymod:block/nonexistent").is_none());
        assert!(!top.sprite.animated && !top.sprite.cutout);

        let (grass, sides) = query("grass_block").unwrap();
        assert!(grass.cube);
        assert_eq!(
            sides[Direction::Up as usize][0].tint,
            Some(TintSource::Grass)
        );
        assert_eq!(sides[Direction::Down as usize][0].tint, None);
        let side = &sides[Direction::North as usize];
        assert_eq!(side.len(), 2, "the side and its overlay");
        assert_eq!(
            (side[0].tint, side[1].tint),
            (None, Some(TintSource::Grass))
        );
        assert_eq!(
            lookup
                .texture("minecraft:block/grass_block_side_overlay")
                .unwrap()
                .sprite,
            side[1].sprite
        );

        // seen from outside, a side's texture runs from its top left: on the north side (looking south, east on
        // the left) u falls with x and v with y; on top, u and v run with x and z (stone's first variant is turned)
        let (_, planks) = query("oak_planks").unwrap();
        let uv = |sides: &Sides<'_>, d: Direction| sides[d as usize][0].uv;
        let north = uv(&planks, Direction::North);
        assert_eq!(north, [1.0, 1.0, -1.0, 0.0, 0.0, -1.0]);
        assert_eq!(uv(&planks, Direction::Up), upright);
        assert_eq!(
            uv(&planks, Direction::South),
            [0.0, 1.0, 1.0, 0.0, 0.0, -1.0]
        );
        assert_eq!(
            uv(&planks, Direction::Down),
            [0.0, 1.0, 1.0, 0.0, 0.0, -1.0]
        );
        // an upright log's side runs as stone's; on its side (axis x) the texture is turned a quarter
        let (_, log) = query("oak_log").unwrap();
        let (_, log_x) = query("minecraft:oak_log[axis=x]").unwrap();
        assert_eq!(log[Direction::North as usize][0].uv, north);
        let turned = log_x[Direction::North as usize][0].uv;
        assert_eq!(
            (turned[2], turned[5]),
            (0.0, 0.0),
            "u runs along y, v along x: {turned:?}"
        );
        assert_eq!(
            log_x[Direction::West as usize][0].sprite,
            log[Direction::Up as usize][0].sprite,
            "its end faces west"
        );

        let (_, leaves) = query("oak_leaves").unwrap();
        let leaf = &leaves[Direction::Up as usize][0];
        assert!(leaf.sprite.cutout);
        assert_eq!(leaf.tint, Some(TintSource::Foliage));
        assert_eq!((leaf.texture, leaf.tint_index), ("block/oak_leaves", 0));
        assert!(matches!(
            query("spruce_leaves").unwrap().1[0][0].tint,
            Some(TintSource::Fixed(0x61_9961))
        ));

        let (_, water) = query("water").unwrap();
        let w = &water[Direction::Up as usize][0];
        assert!(w.sprite.animated && w.translucent);
        assert_eq!(w.tint, Some(TintSource::Water));
        assert_eq!(w.texture, "block/water_still");

        let (slab, sides) = query("stone_slab").unwrap();
        assert!(!slab.cube);
        assert_eq!(sides[Direction::Up as usize][0].inset, 0.5);
        assert_eq!(
            sides[Direction::North as usize][0].rect,
            [0.0, 0.0, 1.0, 0.5]
        );

        let (flower, sides) = query("dandelion").unwrap();
        assert!(!flower.cube && sides.iter().all(Vec::is_empty));
        assert!(flower.particle.is_some_and(|p| p.cutout));

        assert_eq!(query("minecraft:no_such_block"), None);
        assert_eq!(query("stone[no_such=1]"), None);
        assert!(query("air").unwrap().1.iter().all(Vec::is_empty));
        Ok(())
    }
}
