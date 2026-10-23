//! Reference objects. They are the testbed's own (not from any game) and are labelled as such:
//! a 1 m cube, a vertical 2 m ruler with 10 cm ticks and the axes at the origin. Each item also
//! gets a front arrow and its world AABB, drawn by `gallery`.

use bevy::prelude::*;

use crate::hud::WorldLabel;

/// Where the cube and the ruler stand, left of the first item
pub const CUBE_AT: Vec3 = Vec3::new(-3.0, 0.5, 0.0);
pub const RULER_AT: Vec3 = Vec3::new(-1.5, 0.0, 0.0);
pub const RULER_HEIGHT: f32 = 2.0;
pub const TICK_STEP: f32 = 0.1;

#[derive(Component)]
pub struct Reference;

pub fn spawn_references(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cube = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.35, 0.25),
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.spawn((
        Name::new("reference cube 1 m"),
        Reference,
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(cube),
        Transform::from_translation(CUBE_AT),
    ));
    commands.spawn(WorldLabel::fixed(
        CUBE_AT + Vec3::Y * 0.7,
        "1 m cube\n(testbed reference)",
    ));

    // A white bar, and a black tick every 10 cm: a long one every 50 cm, longer at each metre
    let white = materials.add(StandardMaterial {
        base_color: Color::srgb(0.92, 0.92, 0.9),
        perceptual_roughness: 0.8,
        ..default()
    });
    let black = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.02, 0.02),
        perceptual_roughness: 0.8,
        ..default()
    });
    let bar = Cuboid::new(0.14, RULER_HEIGHT, 0.02);
    commands.spawn((
        Name::new("reference ruler 2 m"),
        Reference,
        Mesh3d(meshes.add(bar)),
        MeshMaterial3d(white),
        Transform::from_translation(RULER_AT + Vec3::Y * RULER_HEIGHT / 2.0),
    ));
    let ticks = (RULER_HEIGHT / TICK_STEP).round() as usize;
    let short = meshes.add(Cuboid::new(0.06, 0.008, 0.026));
    let mid = meshes.add(Cuboid::new(0.10, 0.012, 0.026));
    let long = meshes.add(Cuboid::new(0.14, 0.016, 0.026));
    for i in 0..=ticks {
        let mesh = if i % 10 == 0 {
            &long
        } else if i % 5 == 0 {
            &mid
        } else {
            &short
        };
        commands.spawn((
            Reference,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(black.clone()),
            Transform::from_translation(RULER_AT + Vec3::new(0.0, i as f32 * TICK_STEP, 0.0)),
        ));
    }
    commands.spawn(WorldLabel::fixed(
        RULER_AT + Vec3::Y * (RULER_HEIGHT + 0.25),
        "2 m ruler, 10 cm ticks\n(testbed reference)",
    ));
    commands.spawn(WorldLabel::fixed(
        Vec3::new(0.0, 1.15, 0.0),
        "origin: X red, Y green, Z blue",
    ));
}
