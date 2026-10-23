//! Physics with Avian 3D. Every value here is an assumption of the testbed: the bridge carries no
//! physics data yet (no masses, no friction, no collision shapes). The HUD and the README say so.
//!
//! - static trimesh colliders for scenes (the provider's own collision where it gave some, else the
//!   render meshes) and for models flagged `collide`;
//! - convex-hull rigid bodies for models flagged `dynamic`, mass = density x hull volume, 1 kg per m3
//!   by default; they wait, disabled, at their slot until dropped;
//! - Space drops the selected prop from `drop_height` metres, B spawns a pyramid of the stack block
//!   (a Hytale block in the example config), R puts everything back.

use avian3d::prelude::*;
use bevy::prelude::*;
use rewrite_bridge_bevy::Bridge;

use crate::analyze::CollisionSource;
use crate::gallery::{Gallery, Status};
use crate::loader::Collision;

/// Where the stack goes: x, z (testbed choice)
pub const STACK_AT: [f32; 2] = [0.0, 4.0];
/// Gap between the ground, or a layer, and the block above at the start, metres (testbed choice)
const STACK_GAP: f32 = 0.02;

#[derive(Component)]
pub struct StackBody;

/// A rigid body made from a flagged item
#[derive(Component)]
pub struct Prop;

pub fn plugins() -> impl PluginGroup {
    PhysicsPlugins::default()
}

pub fn setup_ground(mut commands: Commands, gallery: Res<Gallery>) {
    commands.insert_resource(Gravity(Vec3::NEG_Y * gallery.config.physics.gravity));
    commands.spawn((
        Name::new("ground collider (testbed)"),
        RigidBody::Static,
        Collider::cuboid(crate::stage::GROUND_SIZE, 2.0, crate::stage::GROUND_SIZE),
        Transform::from_xyz(0.0, -1.0, 0.0),
    ));
}

/// Adds the colliders of a placed item; returns the sentence the HUD shows for it
pub fn attach(
    commands: &mut Commands,
    root: Entity,
    collision: Collision,
    dynamic: bool,
    density: f32,
) -> String {
    let Collision {
        hull,
        trimesh,
        note,
    } = collision;
    let mut notes: Vec<String> = Vec::new();
    if dynamic {
        match hull.and_then(|h| Collider::convex_hull(&h)) {
            Some(collider) => {
                commands.entity(root).insert((
                    RigidBody::Dynamic,
                    collider,
                    ColliderDensity(density),
                    RigidBodyDisabled,
                    Prop,
                ));
                notes.push(format!(
                    "dynamic convex hull of the render mesh (testbed assumption), {density} kg/m3; Space drops it"
                ));
            }
            None => notes.push("dynamic, but no convex hull could be built".into()),
        }
    } else if let Some(t) = trimesh {
        let what = match t.source {
            CollisionSource::ProviderCollision => {
                "static trimesh of the provider's own collision shapes"
            }
            CollisionSource::RenderMeshes => {
                "static trimesh of the render meshes (testbed assumption)"
            }
        };
        notes.push(format!("{what}, {} triangles", t.triangles.len()));
        commands.entity(root).insert((
            RigidBody::Static,
            Collider::trimesh(t.vertices, t.triangles),
        ));
    }
    if let Some(n) = note {
        notes.push(n);
    }
    notes.join("; ")
}

/// Puts the selected prop `drop_height` above the ground and lets go
pub fn drop_selected(gallery: &Gallery, commands: &mut Commands) {
    let Some(item) = gallery.items.get(gallery.selected) else {
        return;
    };
    let (Status::Ready, Some(root), true) = (&item.status, item.root, item.has_hull) else {
        return;
    };
    let y = gallery.config.physics.drop_height - item.hull_min_y;
    commands.entity(root).insert((
        Transform::from_xyz(item.origin.x, y, item.origin.z),
        LinearVelocity::ZERO,
        AngularVelocity::ZERO,
    ));
    commands.entity(root).remove::<RigidBodyDisabled>();
}

/// Props back at their slots and disabled, the stack gone
pub fn reset_all(
    gallery: &Gallery,
    stack: &Query<Entity, With<StackBody>>,
    commands: &mut Commands,
) {
    for e in stack {
        commands.entity(e).despawn();
    }
    for item in &gallery.items {
        if let (Some(root), true) = (item.root, item.has_hull) {
            commands.entity(root).insert((
                Transform::from_translation(item.origin),
                LinearVelocity::ZERO,
                AngularVelocity::ZERO,
                RigidBodyDisabled,
            ));
        }
    }
}

/// A pyramid of the stack block (3 + 2 + 1 for six), each layer centred on the one below
pub fn spawn_stack(gallery: &Gallery, bridge: &mut Bridge) {
    let Some(Ok(block)) = &gallery.stack else {
        return;
    };
    let count = gallery.config.stack.as_ref().map_or(6, |s| s.count);
    // Layers of n, n-1, ... blocks, as many as `count` needs
    let mut n = 1;
    while n * (n + 1) / 2 < count {
        n += 1;
    }
    let mut placed = 0;
    let mut layer = 0;
    while placed < count {
        let per = n - layer;
        for i in 0..per {
            if placed == count {
                break;
            }
            let x =
                STACK_AT[0] + (i as f32 - (per as f32 - 1.0) * 0.5) * (block.size.x + STACK_GAP);
            let y = layer as f32 * (block.size.y + STACK_GAP) - block.min_y + STACK_GAP;
            let root = bridge.spawn_model(block.model.clone()).root;
            if let Some(collider) = Collider::convex_hull(&block.hull) {
                bridge.commands.entity(root).insert((
                    Transform::from_xyz(x, y, STACK_AT[1]),
                    RigidBody::Dynamic,
                    collider,
                    ColliderDensity(gallery.config.physics.density),
                    StackBody,
                ));
            }
            placed += 1;
        }
        layer += 1;
    }
}

pub fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    gallery: Res<Gallery>,
    stack: Query<Entity, With<StackBody>>,
    mut bridge: Bridge,
) {
    if keys.just_pressed(KeyCode::Space) {
        drop_selected(&gallery, &mut bridge.commands);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        reset_all(&gallery, &stack, &mut bridge.commands);
    }
    if keys.just_pressed(KeyCode::KeyB) {
        spawn_stack(&gallery, &mut bridge);
    }
}
