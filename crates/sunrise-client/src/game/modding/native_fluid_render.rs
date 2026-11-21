//! Still-fluid boxes under a block entity's pose (`BlockEntityDrawContext::fluid_box`), drawn
//! with their own sorted translucent material (`FLUID_OVERLAY`).
use super::{Game, native_block_entity_draw::BlockEntityDraw};
use mod_api::{FluidBoxOptions, FluidStack};
use sunrise_client::{
    fluid_render::{NativeFluidRenderOptions, fluid_box, fluid_light},
    gfx::{FLUID_OVERLAY, QuadRefusal},
    mesher::Vertex,
};

/// What a fluid draws as in a `fluid_box`: its sprite's layer, its `0xAARRGGBB` tint and its
/// block light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FluidBoxLook {
    layer: u32,
    tint: u32,
    luminosity: u8,
}

impl Game<'_> {
    /// Draws `stack` as a still fluid filling `bounds` under `draw`'s pose, sampling the fluid's
    /// still sprite from the block sprite arrays. See `BlockEntityDrawContext::fluid_box`.
    pub(super) fn native_draw_fluid_box(
        &self,
        draw: &mut BlockEntityDraw,
        stack: &FluidStack,
        bounds: [f32; 6],
        options: FluidBoxOptions,
    ) {
        if stack.amount == 0 {
            return;
        }
        let Some(key) = stack.key() else {
            return;
        };
        let Some(look) = self.standalone_geometry.fluid_look(
            mod_api::__private::parts_installation::fluid_key_slot(key),
            || self.resolve_fluid_look(key),
        ) else {
            return;
        };
        draw_fluid_box(draw, look, bounds, options);
    }

    /// The look of `key`, once per resource generation: water and lava keep their own sprite,
    /// tint and light, any other fluid reads its look (`data/<mod>/sunrise/fluid/`).
    fn resolve_fluid_look(&self, key: mod_api::FluidKey) -> Option<FluidBoxLook> {
        let id = mod_api::__private::fluid_id(key)?;
        let luminosity = super::native_block_tables::BlockTables::new(&self.res, &self.pack)
            .fluid_type_properties(key)
            .map_or(0, |properties| properties.light_level);
        fluid_box_look(
            id,
            sunrise_client::mod_content::fluid_look(id).as_ref(),
            luminosity,
            |sprite| self.pack.render.block_sprite_layer(sprite),
        )
    }
}

/// The look of fluid `id`, whose declared look is `look` and whose block light is `luminosity`:
/// vanilla's water and lava by their own sprites and tints, any other fluid by its look (a look
/// with no sprite draws its tint flat, and a registered fluid with no look falls back to
/// `minecraft:missingno`, matching vanilla `FluidStateModelSet.get`). A missing sprite
/// falls back to `minecraft:missingno` (matching `TextureAtlas.getSprite`). Returns `None` for
/// `minecraft:empty` or if neither the sprite nor `minecraft:missingno` is loaded.
fn fluid_box_look(
    id: &str,
    look: Option<&sunrise_mod_package::content::FluidLook>,
    luminosity: u8,
    sprite_layer: impl Fn(&str) -> Option<u32>,
) -> Option<FluidBoxLook> {
    let (sprite, tint) = match id {
        "minecraft:empty" => return None,
        "minecraft:water" => ("minecraft:block/water_still", 0xFF3F_76E4),
        "minecraft:lava" => ("minecraft:block/lava_still", 0xFFFF_FFFF),
        _ => match look {
            Some(look) => (
                look.sprite
                    .as_deref()
                    .unwrap_or(sunrise_client::fluid_models::FLAT_SPRITE),
                super::native_panel_recipes::fluid_tint(id, look.tint),
            ),
            None => ("minecraft:missingno", 0xFFFF_FFFF),
        },
    };
    Some(FluidBoxLook {
        layer: sprite_layer(sprite).or_else(|| sprite_layer("minecraft:missingno"))?,
        tint,
        luminosity,
    })
}

fn draw_fluid_box(
    draw: &mut BlockEntityDraw,
    FluidBoxLook {
        layer,
        tint,
        luminosity,
    }: FluidBoxLook,
    bounds: [f32; 6],
    options: FluidBoxOptions,
) {
    let pose = draw.pose();
    let light = fluid_light(draw.light(), luminosity);
    let argb = options.argb.unwrap_or(tint);
    // The vertex colour is ABGR.
    let color = (argb & 0xFF00_FF00) | ((argb >> 16) & 0xFF) | ((argb & 0xFF) << 16);
    let options = NativeFluidRenderOptions {
        render_bottom: options.render_bottom,
        uv_shrink_ratio: options.uv_shrink_ratio,
    };
    fluid_box(bounds, pose, options, |quad| {
        match draw.submit_quad(quad.map(|v| Vertex {
            pos: v.pos,
            uv: v.uv,
            layer: FLUID_OVERLAY,
            color,
            light,
            // The TRANSLUCENT shader does not use normals for lighting.
            normal: [0; 4],
            glint_uv: [layer as f32, 0.0],
        })) {
            Err(QuadRefusal::Full) => {
                log::warn!("Fluid box: remaining quads do not fit the scoped entity buffers");
                false
            }
            Err(QuadRefusal::Invalid) => {
                log::warn!("Fluid box: skipped a quad with invalid vertex coordinates or UVs");
                true
            }
            Ok(()) => true,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Mat4, Vec3};
    use sunrise_client::block_entity_render::diffuse_lights;
    use sunrise_client::gfx::EntityQuadCapacity;

    fn stack(fluid: &str, amount: u32) -> FluidStack {
        mod_platform::__private::part_tests::game_thread();
        FluidStack::new(mod_api::__private::fluid_key(fluid).unwrap(), amount)
    }

    /// A frame a block east of the camera, lit by sky 12 and block 3.
    fn frame() -> BlockEntityDraw {
        BlockEntityDraw::posed(
            0.0,
            0,
            0x30C0,
            [0; 3],
            Mat4::from_translation(Vec3::X),
            diffuse_lights(false),
            Vec::new(),
        )
    }

    fn drawn(stack: &FluidStack, bounds: [f32; 6], options: FluidBoxOptions) -> Vec<Vertex> {
        let mut draw = frame();
        if stack.amount != 0
            && let Some(look) = look(stack, None, |sprite| {
                Some(if sprite == "minecraft:missingno" {
                    1
                } else {
                    7
                })
            })
        {
            draw_fluid_box(&mut draw, look, bounds, options);
        }
        draw.into_vertices()
    }

    /// The look of `stack`'s fluid, lava's block light 15 and the others' 0.
    fn look(
        stack: &FluidStack,
        declared: Option<&sunrise_mod_package::content::FluidLook>,
        sprite_layer: impl Fn(&str) -> Option<u32>,
    ) -> Option<FluidBoxLook> {
        let id = mod_api::__private::fluid_id(stack.key().unwrap()).unwrap();
        let light = if id == "minecraft:lava" { 15 } else { 0 };
        fluid_box_look(id, declared, light, sprite_layer)
    }

    fn honey() -> sunrise_mod_package::content::FluidLook {
        sunrise_mod_package::content::FluidLook {
            id: "test:honey".into(),
            sprite: Some("test:block/honey".into()),
            tint: Some(0xC0FF_AA00),
            world: None,
        }
    }

    #[test]
    fn water_tiles_under_the_pose_in_its_own_material_with_the_stack_tint() {
        let vertices = drawn(
            &stack("minecraft:water", 1000),
            [0.0, 0.0, 0.0, 2.0, 1.0, 1.0],
            FluidBoxOptions::new(false, 0.0),
        );
        // Two tiles on top, front, back; one on each end; the bottom left out.
        assert_eq!(vertices.len(), (2 + 2 + 2 + 1 + 1) * 4);
        assert!(vertices.iter().all(|v| v.layer == FLUID_OVERLAY
            && v.glint_uv == [7.0, 0.0]
            && v.color == 0xFFE4_763F
            && v.light == 0x30C0));
        // The pose moved the box a block east: x spans 1..=3.
        let xs = vertices.iter().map(|v| v.pos[0]);
        assert_eq!(xs.clone().fold(f32::MAX, f32::min), 1.0);
        assert_eq!(xs.fold(f32::MIN, f32::max), 3.0);
    }

    #[test]
    fn lava_raises_block_light_and_an_explicit_tint_replaces_the_stack_tint() {
        let vertices = drawn(
            &stack("minecraft:lava", 1),
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            FluidBoxOptions::new(true, 0.0).with_argb(0x8011_2233),
        );
        assert_eq!(vertices.len(), 6 * 4);
        // Sky 12 kept, block light 3 raised to lava's 15.
        assert!(
            vertices
                .iter()
                .all(|v| v.light == 0xF0C0 && v.color == 0x8033_2211)
        );
    }

    #[test]
    fn empty_invalid_and_missing_sprite_fallback_behaviour() {
        let unit = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let options = FluidBoxOptions::new(true, 0.0);
        // Empty fluid, empty stack, reversed bounds and non-finite bounds draw nothing.
        for (stack, bounds, options) in [
            (
                stack("minecraft:water", 1000),
                [1.0, 0.0, 0.0, 0.0, 1.0, 1.0],
                options,
            ),
            (
                stack("minecraft:water", 1000),
                [0.0, 0.0, 0.0, f32::NAN, 1.0, 1.0],
                options,
            ),
            (stack("minecraft:water", 0), [f32::NAN; 6], options),
            (
                stack("minecraft:water", 1000),
                unit,
                FluidBoxOptions::new(true, f32::NAN),
            ),
        ] {
            let vertices = drawn(&stack, bounds, options);
            assert!(vertices.is_empty(), "{bounds:?} {options:?}");
        }
        assert_eq!(
            fluid_box_look("minecraft:empty", None, 0, |_| Some(7)),
            None
        );

        // A registered fluid with no look draws with `minecraft:missingno` and white tint.
        let vertices = drawn(&stack("test:potion", 1000), unit, options);
        assert_eq!(vertices.len(), 6 * 4);
        assert!(
            vertices
                .iter()
                .all(|v| v.glint_uv == [1.0, 0.0] && v.color == 0xFFFF_FFFF)
        );

        // A missing sprite falls back to `minecraft:missingno`.
        assert_eq!(
            look(&stack("minecraft:lava", 1000), None, |sprite| {
                (sprite == "minecraft:missingno").then_some(1)
            }),
            Some(FluidBoxLook {
                layer: 1,
                tint: 0xFFFF_FFFF,
                luminosity: 15,
            })
        );
        // When even `minecraft:missingno` is absent, look resolution returns `None`.
        assert_eq!(look(&stack("minecraft:lava", 1000), None, |_| None), None);

        // Unclamped shrink ratio and large coordinates emit without invented caps.
        let vertices = drawn(
            &stack("minecraft:water", 1000),
            [2_000_000.0, 0.0, 0.0, 2_000_001.0, 1.0, 1.0],
            FluidBoxOptions::new(true, 1.5),
        );
        assert_eq!(vertices.len(), 6 * 4);

        // An earlier source uses one quad; the byte limits leave only one more complete quad.
        let mut draw = frame();
        draw.set_quad_capacity(EntityQuadCapacity::from_buffer_sizes(
            (10 * size_of::<Vertex>()) as u64,
            (18 * size_of::<u32>()) as u64,
        ));
        let look = look(&stack("minecraft:water", 1000), None, |_| Some(7)).unwrap();
        draw_fluid_box(
            &mut draw,
            look,
            [0.0, 0.0, 0.0, 0.5, 0.0, 0.5],
            FluidBoxOptions::new(false, 0.0),
        );
        draw_fluid_box(&mut draw, look, [0.0, 0.0, 0.0, 10.0, 1.0, 10.0], options);
        assert_eq!(draw.into_vertices().len(), 8);
    }

    #[test]
    fn a_mod_fluid_draws_its_look_and_a_stack_with_components_draws_the_same() {
        let layers = |sprite: &str| (sprite == "test:block/honey").then_some(9);
        let honey = honey();
        let plain = stack("test:potion", 1000);
        let with_components = {
            use sunrise_core::nbt::{Compound, Tag};
            let mut components = Compound::new();
            components.insert(
                "minecraft:custom_data".into(),
                Tag::Compound(Compound::new()),
            );
            FluidStack {
                components: Some(Box::new(components.into())),
                ..stack("test:potion", 1000)
            }
        };
        let drawn_with = |stack: &FluidStack, options| {
            let mut draw = frame();
            let look = look(stack, Some(&honey), layers).unwrap();
            draw_fluid_box(
                &mut draw,
                FluidBoxLook {
                    luminosity: 15,
                    ..look
                },
                [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                options,
            );
            draw.into_vertices()
        };
        let options = FluidBoxOptions::new(true, 0.0);
        let vertices = drawn_with(&plain, options);
        assert_eq!(vertices.len(), 6 * 4);
        // The look's sprite layer and its 0xAARRGGBB tint as ABGR; block light raised from the
        // draw's to the look's own light level 15.
        let lit = fluid_light(0x30C0, 15);
        assert_ne!(lit, 0x30C0);
        assert!(
            vertices
                .iter()
                .all(|v| v.glint_uv == [9.0, 0.0] && v.color == 0xC000_AAFF && v.light == lit)
        );
        // Components change nothing, and `argb` still replaces the look's tint.
        let same = drawn_with(&with_components, options);
        assert_eq!(same.len(), vertices.len());
        assert!(same.iter().zip(&vertices).all(|(a, b)| {
            (a.pos, a.color, a.light, a.glint_uv) == (b.pos, b.color, b.light, b.glint_uv)
        }));
        let argb = drawn_with(&plain, options.with_argb(0x8011_2233));
        assert!(argb.iter().all(|v| v.color == 0x8033_2211));
    }

    #[test]
    fn a_look_with_no_sprite_draws_its_tint_flat() {
        let flat = sunrise_mod_package::content::FluidLook {
            sprite: None,
            ..honey()
        };
        let layers = |sprite: &str| (sprite == "sunrise:flat_color").then_some(4);
        assert_eq!(
            fluid_box_look("test:honey", Some(&flat), 0, layers),
            Some(FluidBoxLook {
                layer: 4,
                tint: 0xC0FF_AA00,
                luminosity: 0
            })
        );
        // No tint is the colour `DrawContext::fluid` draws: an opaque mid grey.
        let plain = sunrise_mod_package::content::FluidLook { tint: None, ..flat };
        assert_eq!(
            fluid_box_look("test:honey", Some(&plain), 0, layers).map(|look| look.tint),
            Some(0xFF9A_9A9A)
        );
    }
}
