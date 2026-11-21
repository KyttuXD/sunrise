use super::*;

impl Game<'_> {
    pub(in crate::game) fn native_compute_camera_angles(
        &self,
        partial: f32,
        angles: &mut [f32; 3],
    ) {
        self.fire_compute_camera_angles(partial, angles);
    }

    pub(in crate::game) fn native_camera_position(&self, partial: f32, position: &mut [f64; 3]) {
        self.fire_camera_position(partial, position);
    }

    pub(in crate::game) fn native_view_transform(&self, partial: f32, transform: &mut [f32; 16]) {
        self.fire_view_transform(partial, transform);
    }
}
