//! Field of view (`Camera.tickFov`, `calculateFov`,
//! `AbstractClientPlayer.getFieldOfViewModifier`), view and hurt bobbing
//! (`GameRenderer.bobHurt`, `bobView`), the portal and Nausea spin
//! (`GameRenderer.renderLevel`) and the spyglass (`SpyglassItem`,
//! `Hud.extractSpyglassOverlay`). The arithmetic is `sunrise_client::camera`.
#[cfg(test)]
#[path = "tests/fov_camera_tests.rs"]
mod camera_tests;

use super::{Game, remote_camera::SpectatedEntity};
use mod_api::client::camera::FovProjection;
use sunrise_core::position::BlockPos;

pub(super) fn camera_effects_enabled(title: bool, studio_freecam: bool) -> bool {
    !title && !studio_freecam
}

pub(super) fn studio_draws_hand(studio_freecam: bool, ordinary_hand_visible: bool) -> bool {
    !studio_freecam && ordinary_hand_visible
}
use sunrise_client::{
    camera::{
        BobState, CameraFluid, FovPlayer, HurtBob, UseItem, bob_matrix, lerp,
        raw_field_of_view_modifier, spinning_effect_matrix,
    },
    fog::{FogType, fog_type},
    hud::HudBatch,
    hud_layout, hud_overlays,
};

fn fov_use_item(item: &str) -> UseItem {
    match item {
        "minecraft:bow" => UseItem::Bow,
        "minecraft:spyglass" => UseItem::Spyglass,
        _ => UseItem::Other,
    }
}

fn remote_fov_item(player: &sunrise_client::mobs::Mob) -> Option<(UseItem, i32)> {
    if player.extra.using_item <= 0 {
        return None;
    }
    let hand = if player.extra.using_off_hand {
        sunrise_items::equipment::OFFHAND
    } else {
        sunrise_items::equipment::MAINHAND
    };
    let stack = player.extra.slot_stack(hand)?;
    Some((fov_use_item(&stack.item), player.extra.using_item - 1))
}

impl Game<'_> {
    /// `Player.isScoping`: using a spyglass.
    pub(super) fn scoping(&self) -> bool {
        self.using
            .and_then(|(slot, _)| self.stack_in(slot))
            .is_some_and(|s| &*s.item == "minecraft:spyglass")
    }
    /// `options.fovEffectScale`.
    pub(super) fn fov_effect_scale(&self) -> f32 {
        self.settings.file.double("fovEffectScale") as f32
    }
    /// `options.screenEffectScale` (Distortion Effects).
    pub(super) fn screen_effect_scale(&self) -> f32 {
        self.settings.file.double("screenEffectScale") as f32
    }
    /// `LocalPlayer.getEffectBlendFactor(NAUSEA, partial)`: 0 without the effect.
    pub(super) fn nausea_blend(&self, partial: f32) -> f32 {
        if self.potions.effects.get("nausea").is_none() {
            return 0.0;
        }
        let fx = &self.screen_fx;
        fx.o_nausea + partial * (fx.nausea - fx.o_nausea)
    }
    /// The spin `GameRenderer.renderLevel` multiplies into the level's
    /// projection: the larger of the portal and Nausea intensities, scaled by
    /// Distortion Effects squared, turning by `LocalPlayer.getSpinningEffectAngle`.
    pub(super) fn spinning_matrix(&self, partial: f32) -> glam::Mat4 {
        let fx = &self.screen_fx;
        let portal = fx.o_portal + partial * (fx.portal - fx.o_portal);
        let scale = self.screen_effect_scale();
        let intensity = portal.max(self.nausea_blend(partial)) * (scale * scale);
        spinning_effect_matrix(intensity, fx.spinning_time + partial * fx.spinning_speed)
    }
    /// `Camera.tick`: ease toward the eye height and FOV modifier of the camera entity.
    pub(super) fn tick_fov(&mut self) {
        let (eye, player) = if let Some(camera) = self.spectated_camera() {
            let player = match camera {
                SpectatedEntity::Mob(mob) if mob.kind.id == "minecraft:player" => Some(FovPlayer {
                    // ClientPacketListener.handlePlayerAbilities only updates the local player.
                    // RemotePlayer keeps Abilities' defaults and uses its synced movement attribute.
                    flying: false,
                    walking_speed: 0.1,
                    movement_speed: mob.attrs.speed,
                    using: remote_fov_item(mob),
                }),
                _ => None,
            };
            (Some(camera.snapshot().eye_height), player)
        } else if let Some(p) = &self.player {
            let eye = if self.sleeping.is_some() {
                0.2
            } else {
                p.dimensions.eye_height
            };
            let using = self.using.and_then(|(slot, ticks)| {
                self.stack_in(slot)
                    .map(|stack| (fov_use_item(&stack.item), ticks as i32))
            });
            (
                Some(eye),
                Some(FovPlayer {
                    flying: p.flying,
                    walking_speed: self.remote_walking_speed(),
                    // Player.aiStep keeps `speed` as the float movement speed attribute.
                    movement_speed: f64::from(p.speed),
                    using,
                }),
            )
        } else {
            (None, None)
        };
        let target = player.map_or(1.0, |player| {
            let Some(base) =
                raw_field_of_view_modifier(&player, self.camera_type.is_first_person())
            else {
                // NeoForge patches the interpolated return, not the first-person spyglass return.
                return 0.1;
            };
            let effect_scale = self.fov_effect_scale();
            let mut modifier = lerp(effect_scale, 1.0, base);
            self.native_compute_fov_modifier(&player, base, effect_scale, &mut modifier);
            modifier
        });
        self.view.tick(eye, target);
    }
    /// The player's `deathTime` while dead or dying: whole ticks counted by
    /// `LivingEntity.tickDeath`, so `deathTime + partialTicks` in
    /// `Camera.extractRenderState` rises smoothly across frames.
    pub(super) fn death_time(&self) -> Option<i32> {
        let p = self.player.as_ref()?;
        p.health
            .dead_or_dying()
            .then(|| self.ui.death_time.min(i32::MAX as u32) as i32)
    }
    /// `Camera.getFluidInCamera` for the camera at `camera`, where the parts of each block it
    /// reads may answer the state the camera sees there (`state_at_viewpoint`), and the mods may
    /// then replace the answer (`camera_submersion`). Every client reader of the camera's fluid
    /// reads it.
    pub(super) fn camera_fog_type(&self, camera: [f64; 3]) -> FogType {
        let mut fog_type = self.camera_fog_type_seen(camera);
        self.native_camera_submersion(camera, &mut fog_type);
        fog_type
    }
    /// The fog type at `camera` as the blocks the camera sees there give it. Both paths go
    /// through the replaceable `fog_type`.
    fn camera_fog_type_seen(&self, camera: [f64; 3]) -> FogType {
        let (world, facts, near) = (&self.world, self.res.facts(), self.near_plane());
        if !mod_api::__private::has_scoped_hook(mod_api::HookId::StateAtViewpoint) {
            return fog_type(world, facts, camera, &near, &|state, _| state);
        }
        let seen = |state, [x, y, z]: [i32; 3]| {
            if !super::api_values::block_state::block_listens(
                mod_api::HookId::StateAtViewpoint,
                state,
            ) {
                return state;
            }
            let mut seen = state;
            self.native_state_at_viewpoint(state, BlockPos::new(x, y, z), camera, &mut seen);
            seen
        };
        fog_type(world, facts, camera, &near, &seen)
    }
    /// The fluid at the camera, as `Camera.getFluidInCamera` reports it.
    pub(super) fn camera_fluid(&self, camera: [f64; 3]) -> CameraFluid {
        match self.camera_fog_type(camera) {
            FogType::Water => CameraFluid::Water,
            FogType::Lava => CameraFluid::Lava,
            FogType::PowderSnow => CameraFluid::PowderSnow,
            FogType::Atmospheric => CameraFluid::None,
        }
    }
    /// `Camera.update` computes the world angle, then the hand angle. NeoForge's viewport
    /// event is at the end of `modifyFovBasedOnDeathOrFluid`, shared by both angles.
    pub(super) fn camera_fovs(&self, partial: f32, camera: [f64; 3]) -> (f32, f32) {
        let effect_scale = self.fov_effect_scale();
        if self.studio.flying() {
            return (
                self.studio.freecam.fov.clamp(1.0, 179.0),
                self.view.calculate_hud_fov(
                    partial,
                    effect_scale,
                    self.death_time(),
                    self.camera_fluid(camera),
                ),
            );
        }
        let spectator = self.spectated_camera();
        let death = match spectator {
            Some(SpectatedEntity::Mob(mob)) => (!mob.alive()).then_some(mob.death_time),
            Some(_) => None,
            None => self.death_time(),
        };
        let listens = mod_api::__private::listened(mod_api::HookId::ComputeFov);
        let scoping = listens
            && self.camera_type.is_first_person()
            && match spectator {
                Some(SpectatedEntity::Mob(mob)) if mob.kind.id == "minecraft:player" => {
                    remote_fov_item(mob).is_some_and(|(item, _)| item == UseItem::Spyglass)
                }
                Some(_) => false,
                None => self.scoping(),
            };
        let mut world = self.view.calculate_fov(
            partial,
            self.settings.fov as i32,
            effect_scale,
            death,
            self.camera_fluid(camera),
            false,
        );
        if listens {
            self.native_compute_fov(partial, camera, FovProjection::World, scoping, &mut world);
        }
        let mut hud =
            self.view
                .calculate_hud_fov(partial, effect_scale, death, self.camera_fluid(camera));
        if listens {
            self.native_compute_fov(partial, camera, FovProjection::Hud, scoping, &mut hud);
        }
        (world, hud)
    }
    /// The pose `GameRenderer.renderLevel` multiplies into the projection:
    /// `bobHurt`, then `bobView` with the View Bobbing option.
    pub(super) fn bob_matrix(&self, partial: f32) -> glam::Mat4 {
        let Some(p) = self.player.as_ref() else {
            return glam::Mat4::IDENTITY;
        };
        let hurt = &p.health.hurt;
        let state = BobState {
            living: Some(HurtBob {
                hurt_time: p.health.hurt_animation_time as f32 - partial,
                hurt_duration: hurt.hurt_duration,
                hurt_dir: p.health.hurt_dir,
                death_time: self.death_time().map(|t| t as f32 + partial),
            }),
            player: Some((
                self.avatar.backwards_interpolated_walk_distance(partial),
                self.avatar.interpolated_bob(partial),
            )),
        };
        let tilt = self.settings.file.double("damageTiltStrength");
        glam::Mat4::from_cols_array(&bob_matrix(&state, tilt, self.settings.view_bobbing).m)
            * self.native_shake_matrix(partial)
    }
    /// `AbstractClientPlayer.tick` and `aiStep` (`ClientAvatarState.tick`,
    /// `updateBob`), `rideTick`'s `resetBob` and `LocalPlayer.move`'s walked
    /// distance, after the player's movement this tick. `before` is the
    /// ground state, velocity and position at the start of the tick.
    pub(super) fn tick_avatar(&mut self, before: (bool, [f64; 3], [f64; 3]), riding: bool) {
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let (ground, velocity, start) = before;
        let swimming = p.pose == sunrise_client::pose::Pose::Swimming;
        let alive = !p.health.dead_or_dying();
        let (dx, dz) = (
            (p.position.x - start[0]) as f32,
            (p.position.z - start[2]) as f32,
        );
        self.avatar.tick(start);
        self.avatar
            .update_bob(ground && alive && !swimming, velocity);
        if riding {
            self.avatar.reset_bob();
        } else {
            self.avatar.add_walked(dx, dz);
        }
    }
    /// `SpyglassItem.use`: the sound, then start scoping (a mod's `Start`
    /// may refuse that, not the sound).
    pub(super) fn use_spyglass(&mut self) {
        // Player.playSound: at the feet (getY), not the eyes.
        let at = self
            .player
            .as_ref()
            .map(|p| [p.position.x, p.position.y, p.position.z]);
        self.audio.play("minecraft:item.spyglass.use", at, 1.0, 1.0);
        self.award_stat("used", "minecraft:spyglass", 1);
        self.start_using_item();
    }
    /// `SpyglassItem.releaseUsing` / `finishUsingItem` (after 1200 ticks).
    pub(super) fn stop_spyglass(&mut self) {
        let at = self.eye_position();
        self.audio
            .play("minecraft:item.spyglass.stop_using", at, 1.0, 1.0);
    }
    /// `Hud.extractCameraOverlays`: the spyglass scope, its size easing
    /// towards 1.125 by half the frame's game ticks and reset to 0.5 while
    /// not scoping in first person.
    pub(super) fn draw_spyglass(&mut self, batch: &mut HudBatch) {
        self.scope_scale += 0.5 * self.frame_ticks * (1.125 - self.scope_scale);
        if !self.camera_type.is_first_person() {
            return;
        }
        if !self.scoping() {
            self.scope_scale = 0.5;
            return;
        }
        let draws =
            hud_overlays::spyglass(batch.width as i32, batch.height as i32, self.scope_scale);
        hud_layout::paint(batch, &self.gui.atlas, &draws);
    }
}

#[cfg(test)]
mod tests {
    use super::{camera_effects_enabled, studio_draws_hand};

    #[test]
    fn freecam_uses_unshaken_camera_and_player_view_keeps_hands() {
        assert!(!camera_effects_enabled(false, true));
        assert!(camera_effects_enabled(false, false));
        assert!(!camera_effects_enabled(true, false));
        assert!(studio_draws_hand(false, true));
        assert!(!studio_draws_hand(true, true));
        assert!(!studio_draws_hand(false, false));
    }
}
