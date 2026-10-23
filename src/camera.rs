//! An orbit camera: drag to orbit, right or middle drag to pan, wheel to zoom, WASD and Q/E to move the
//! target. `frame` points it at a box; the shots use `shot_camera` for a fixed 3/4 view of one.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;

use crate::analyze::Aabb;
use crate::gallery::Gallery;

/// Vertical field of view of every camera, radians (Bevy's default, 45 degrees)
pub const FOV: f32 = std::f32::consts::FRAC_PI_4;

#[derive(Resource, Clone, Copy)]
pub struct Orbit {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            target: Vec3::new(3.0, 1.0, 0.0),
            yaw: 0.45,
            pitch: 0.35,
            dist: 14.0,
        }
    }
}

impl Orbit {
    pub fn transform(&self) -> Transform {
        let offset = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        ) * self.dist;
        Transform::from_translation(self.target + offset).looking_at(self.target, Vec3::Y)
    }

    /// Looks at the middle of `b` from far enough to see all of it
    pub fn frame(&mut self, b: &Aabb) {
        self.target = b.center();
        // Far enough that all eight corners are inside a 16:9 view from the current angles
        let view = Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(-self.pitch);
        let (right, up, back) = (view * Vec3::X, view * Vec3::Y, view * Vec3::Z);
        let tan_v = (FOV * 0.5).tan();
        let tan_h = tan_v * 16.0 / 9.0;
        let (lo, hi) = (b.min() - self.target, b.max() - self.target);
        let mut dist = 0.5f32;
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            // The corner's depth in front of the camera is dist - c.back
            let depth = -c.dot(back);
            dist = dist
                .max(c.dot(right).abs() / tan_h - depth)
                .max(c.dot(up).abs() / tan_v - depth);
        }
        self.dist = dist * 1.08;
    }
}

/// Requests the camera to frame a box (set by the keys)
#[derive(Resource, Default)]
pub struct FrameRequest(pub Option<Aabb>);

#[allow(clippy::too_many_arguments)]
pub fn orbit_camera(
    mut orbit: ResMut<Orbit>,
    mut request: ResMut<FrameRequest>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
) {
    if let Some(b) = request.0.take() {
        orbit.frame(&b);
    }
    let right = Quat::from_rotation_y(orbit.yaw) * Vec3::X;
    let forward = -(Quat::from_rotation_y(orbit.yaw) * Vec3::Z);
    if buttons.pressed(MouseButton::Left) {
        orbit.yaw -= motion.delta.x * 0.005;
        orbit.pitch = (orbit.pitch + motion.delta.y * 0.005).clamp(-0.1, 1.5);
    }
    if buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle) {
        let k = orbit.dist * 0.0012;
        orbit.target += right * -motion.delta.x * k + Vec3::Y * motion.delta.y * k;
    }
    orbit.dist = (orbit.dist * (1.0 - scroll.delta.y * 0.1)).clamp(0.3, 400.0);
    let speed = orbit.dist * 0.6 * time.delta_secs();
    let mut mv = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        mv += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        mv -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        mv += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        mv -= right;
    }
    if keys.pressed(KeyCode::KeyE) {
        mv += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        mv -= Vec3::Y;
    }
    orbit.target += mv * speed;
    for mut t in &mut cameras {
        *t = orbit.transform();
    }
}

/// Frames everything once when the last item has been placed
pub fn frame_all_when_done(
    gallery: Res<Gallery>,
    mut request: ResMut<FrameRequest>,
    mut done: Local<bool>,
) {
    if *done || !gallery.done {
        return;
    }
    *done = true;
    request.0 = all_bounds(&gallery);
}

pub fn all_bounds(gallery: &Gallery) -> Option<Aabb> {
    gallery
        .items
        .iter()
        .filter_map(|i| i.aabb)
        .chain(std::iter::once(Aabb::new(
            Vec3::new(-3.5, 0.0, -0.5),
            Vec3::new(-1.0, 2.0, 0.5),
        )))
        .reduce(Aabb::union)
}

/// The shots' camera: a fixed 3/4 view (35 degrees to the right of the item's front, 22 degrees
/// above the horizon) from far enough to fit the AABB's bounding sphere
pub fn shot_camera(b: &Aabb) -> Transform {
    const YAW: f32 = 35.0 * std::f32::consts::PI / 180.0;
    const PITCH: f32 = 22.0 * std::f32::consts::PI / 180.0;
    let radius = (b.size().length() * 0.5).max(0.05);
    let dist = radius / (FOV * 0.5).sin();
    Orbit {
        target: b.center(),
        yaw: YAW,
        pitch: PITCH,
        dist,
    }
    .transform()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shot_camera_looks_at_the_box_from_the_front_right() {
        let b = Aabb::new(Vec3::new(-1.0, 0.0, -2.0), Vec3::new(1.0, 2.0, 2.0));
        let t = shot_camera(&b);
        assert!(t.translation.z > b.center().z);
        assert!(t.translation.x > b.center().x);
        assert!(t.translation.y > b.center().y);
        let fwd = t.forward();
        let to_center = (b.center() - t.translation).normalize();
        assert!(fwd.dot(to_center) > 0.999);
        // the bounding sphere fits the 45 degree field of view
        let r = b.size().length() * 0.5;
        assert!((b.center() - t.translation).length() >= r / (FOV * 0.5).sin() - 1e-3);
    }
}
