//! Moved here from the game so mods can name it.
#[allow(unused_imports)]
use crate::*;
/// `Mth.lerp(float, float, float)`.
pub fn lerp(alpha: f32, p0: f32, p1: f32) -> f32 {
    p0 + alpha * (p1 - p0)
}
/// `Mth.wrapDegrees(float)`.
pub fn wrap_degrees(angle: f32) -> f32 {
    let mut normalized = angle % 360.0;
    if normalized >= 180.0 {
        normalized -= 360.0;
    }
    if normalized < -180.0 {
        normalized += 360.0;
    }
    normalized
}
/// What `AbstractClientPlayer.getFieldOfViewModifier` reads.
#[derive(Clone, Copy, Debug, PartialEq)]
// exhaustive: the game builds it with a literal; mods only read it
#[derive(sunrise_mod_api_macros::Wire)]
pub struct FovPlayer {
    /// `Abilities.flying`.
    pub flying: bool,
    /// `Abilities.getWalkingSpeed()`.
    pub walking_speed: f32,
    /// `getAttributeValue(MOVEMENT_SPEED)`.
    pub movement_speed: f64,
    /// The item in use and its use ticks, when using one.
    pub using: Option<(UseItem, i32)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// exhaustive: a closed set the game matches without a wildcard
#[derive(sunrise_mod_api_macros::Wire)]
pub enum UseItem {
    Bow,
    Spyglass,
    Other,
}
/// Why `ClientHost::set_local_rotation` changed nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// exhaustive: a closed set the game matches without a wildcard
#[derive(sunrise_mod_api_macros::Wire)]
pub enum LocalRotationError {
    /// No client world is open.
    NoWorld,
    /// The yaw or the pitch is NaN or infinite.
    NotFinite,
}
/// `CameraType`: where the camera sits for the local player. `key.togglePerspective` (F5) cycles
/// it first person, third person behind, third person in front.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
// exhaustive: a closed set the game matches without a wildcard
#[derive(sunrise_mod_api_macros::Wire)]
pub enum CameraType {
    #[default]
    FirstPerson,
    ThirdPersonBack,
    ThirdPersonFront,
}
impl CameraType {
    /// `CameraType.isFirstPerson`.
    pub fn is_first_person(self) -> bool {
        self == Self::FirstPerson
    }
    /// `CameraType.isMirrored`: third person in front.
    pub fn is_mirrored(self) -> bool {
        self == Self::ThirdPersonFront
    }
    /// `CameraType.cycle` (F5).
    pub fn cycle(self) -> Self {
        match self {
            Self::FirstPerson => Self::ThirdPersonBack,
            Self::ThirdPersonBack => Self::ThirdPersonFront,
            Self::ThirdPersonFront => Self::FirstPerson,
        }
    }
}
/// The camera entity as `Camera.alignWithEntity` and the field-of-view
/// death and hurt effects see it.
#[derive(Clone, Copy, Debug, PartialEq)]
// exhaustive: the game builds it with a literal; mods only read it
#[derive(sunrise_mod_api_macros::Wire)]
pub struct CameraEntity {
    pub position: [f64; 3],
    /// `xo`, `yo`, `zo`.
    pub previous: [f64; 3],
    pub x_rot: f32,
    pub y_rot: f32,
    pub x_rot_o: f32,
    pub y_rot_o: f32,
    /// `Entity.getEyeHeight()`.
    pub eye_height: f32,
    /// A living entity's `getScale()` and camera distance attribute.
    pub living: Option<LivingCamera>,
    /// The vehicle's scale and camera distance when it is living.
    pub mount: Option<(f32, f64)>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
// exhaustive: the game builds it with a literal; mods only read it
#[derive(sunrise_mod_api_macros::Wire)]
pub struct LivingCamera {
    pub scale: f32,
    pub camera_distance: f64,
    /// `isSleeping()` with `getBedOrientation()` as its `toYRot()`.
    pub sleeping: Option<Option<f32>>,
    /// `isDeadOrDying()` and `deathTime`.
    pub dead: Option<i32>,
}
impl CameraEntity {
    /// `Entity.getXRot(partialTicks)`.
    pub fn view_x_rot(&self, partial: f32) -> f32 {
        if partial == 1.0 {
            self.x_rot
        } else {
            lerp(partial, self.x_rot_o, self.x_rot)
        }
    }
    /// `LocalPlayer.getViewYRot` (`getYRot(partialTicks)`, a `Mth.rotLerp`).
    pub fn view_y_rot(&self, partial: f32) -> f32 {
        if partial == 1.0 {
            self.y_rot
        } else {
            self.y_rot_o + partial * wrap_degrees(self.y_rot - self.y_rot_o)
        }
    }
}

pub(crate) fn validate_camera_angles(
    yaw: &f32,
    pitch: &f32,
    roll: &f32,
) -> Result<(), &'static str> {
    if !yaw.is_finite() {
        Err("non-finite yaw")
    } else if !pitch.is_finite() {
        Err("non-finite pitch")
    } else if !roll.is_finite() {
        Err("non-finite roll")
    } else {
        Ok(())
    }
}

pub(crate) fn validate_camera_position(position: &[f64; 3]) -> Result<(), &'static str> {
    if position.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err("non-finite position")
    }
}

/// Column-major 4x4 presentation-only view transform composed at vanilla's
/// `GameRenderer.bobHurt` / `bobView` stage (`render_view_transform`).
///
/// Operations post-multiply onto the current matrix (`self = self * m`), matching vanilla's
/// `PoseStack` multiplication order. Every mutating method rejects non-finite inputs without
/// changing the stored matrix.
// no wire: client-local per-frame render transform at the GameRenderer bob stage.
// exhaustive: its field is private, so mods compose through its transform methods
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewTransform {
    matrix: [f32; 16],
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl ViewTransform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        matrix: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    /// Returns the column-major 4x4 matrix.
    pub const fn matrix(&self) -> [f32; 16] {
        self.matrix
    }

    /// Replaces the transform with `matrix`; reports and returns `false` for non-finite entries.
    pub fn set_matrix(&mut self, matrix: [f32; 16]) -> bool {
        if !matrix.iter().all(|v| v.is_finite()) {
            if let Some(call) = crate::content::crash::current_native_call() {
                log::warn!(
                    "mod {} hook {} refused a non-finite view transform",
                    call.id,
                    call.hook
                );
            }
            return false;
        }
        self.matrix = matrix;
        true
    }

    /// Post-multiplies by a column-major 4x4 matrix; refuses a non-finite result.
    pub fn append_matrix(&mut self, rhs: [f32; 16]) -> bool {
        let lhs = self.matrix;
        self.set_matrix(std::array::from_fn(|index| {
            let (col, row) = (index / 4 * 4, index % 4);
            lhs[row] * rhs[col]
                + lhs[4 + row] * rhs[col + 1]
                + lhs[8 + row] * rhs[col + 2]
                + lhs[12 + row] * rhs[col + 3]
        }))
    }

    /// Post-multiplies a translation `(x, y, z)` in blocks along camera-local axes.
    pub fn translate(&mut self, x: f32, y: f32, z: f32) -> bool {
        self.append_matrix([
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            x, y, z, 1.0,
        ])
    }

    /// Post-multiplies a rotation about the local X axis by `radians`.
    pub fn rotate_x(&mut self, radians: f32) -> bool {
        let (s, c) = rotation_components(radians);
        self.append_matrix([
            1.0, 0.0, 0.0, 0.0, //
            0.0, c, s, 0.0, //
            0.0, -s, c, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// Post-multiplies a rotation about the local Y axis by `radians`.
    pub fn rotate_y(&mut self, radians: f32) -> bool {
        let (s, c) = rotation_components(radians);
        self.append_matrix([
            c, 0.0, -s, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            s, 0.0, c, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// Post-multiplies a rotation about the local Z axis by `radians`.
    pub fn rotate_z(&mut self, radians: f32) -> bool {
        let (s, c) = rotation_components(radians);
        self.append_matrix([
            c, s, 0.0, 0.0, //
            -s, c, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }

    /// Post-multiplies a scale `(x, y, z)`.
    pub fn scale(&mut self, x: f32, y: f32, z: f32) -> bool {
        self.append_matrix([
            x, 0.0, 0.0, 0.0, //
            0.0, y, 0.0, 0.0, //
            0.0, 0.0, z, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ])
    }
}

fn rotation_components(radians: f32) -> (f32, f32) {
    let angle = f64::from(radians);
    (
        sunrise_core::jmath::sin(angle) as f32,
        sunrise_core::jmath::cos(angle) as f32,
    )
}

#[cfg(test)]
mod view_transform_tests {
    use super::ViewTransform;

    #[test]
    fn post_multiplies_translation_and_rejects_non_finite_inputs() {
        let mut t = ViewTransform::default();
        assert_eq!(t, ViewTransform::IDENTITY);
        assert!(t.translate(1.0, -2.0, 3.0));
        assert!(t.scale(2.0, 2.0, 2.0));
        assert_eq!(
            t.matrix(),
            [
                2.0, 0.0, 0.0, 0.0, //
                0.0, 2.0, 0.0, 0.0, //
                0.0, 0.0, 2.0, 0.0, //
                1.0, -2.0, 3.0, 1.0,
            ]
        );
        let before = t;
        assert!(!t.translate(f32::NAN, 0.0, 0.0));
        assert!(!t.rotate_z(f32::INFINITY));
        assert!(!t.scale(1.0, f32::NEG_INFINITY, 1.0));
        assert!(!t.set_matrix([f32::NAN; 16]));
        assert_eq!(t, before);
        assert!(!t.scale(f32::MAX, 1.0, 1.0));
        assert_eq!(t, before);
    }

    #[test]
    fn rotations_match_right_handed_axes() {
        let mut rz = ViewTransform::IDENTITY;
        assert!(rz.rotate_z(core::f32::consts::FRAC_PI_2));
        let m = rz.matrix();
        assert_eq!([m[1], m[4]], [1.0, -1.0]);
    }
}
