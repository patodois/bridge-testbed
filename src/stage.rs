//! The neutral stage every item stands on: one sun with shadows, an ambient light, a ground plane.
//! These values are the testbed's, not any game's, and are identical in the gallery and in the shots,
//! so that two games' assets are lit the same way.

use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::prelude::*;

/// Sun illuminance, lux (Bevy's `AMBIENT_DAYLIGHT`). With the camera's default exposure
/// (EV100 9.7) a white diffuse surface facing the sun reads as bright but not clipped.
pub const SUN_LUX: f32 = 10_000.0;
/// Ambient light, cd/m^2, white, from every direction
pub const AMBIENT_CD_M2: f32 = 400.0;
/// The direction the sun is in (to the light), unnormalised: above, to the right and to the front
/// (+Y, +X, +Z), so a model's front face (+Z) is lit and shadows fall behind it to the left
pub const SUN_TOWARD: Vec3 = Vec3::new(0.45, 0.8, 0.55);
/// Background colour: a mid grey, linear
pub const CLEAR: Color = Color::LinearRgba(LinearRgba::new(0.16, 0.17, 0.19, 1.0));
/// Ground plane albedo (sRGB) and size (metres per side)
pub const GROUND_SRGB: f32 = 0.42;
pub const GROUND_SIZE: f32 = 400.0;
/// Shadow cascades reach this far from the camera, metres
pub const SHADOW_DISTANCE: f32 = 90.0;

/// The descriptions the README and the HUD print
pub const LIGHTING_NOTE: &str = "sun 10000 lux from (0.45, 0.8, 0.55), shadows, ambient 400 cd/m2, default exposure, grey ground";

pub fn sun() -> impl Bundle {
    (
        DirectionalLight {
            illuminance: SUN_LUX,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::IDENTITY.looking_to(-SUN_TOWARD.normalize(), Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            first_cascade_far_bound: 6.0,
            maximum_distance: SHADOW_DISTANCE,
            ..default()
        }
        .build(),
    )
}

/// Adds the lighting resources to an app
pub fn add_resources(app: &mut App) {
    app.insert_resource(ClearColor(CLEAR))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: AMBIENT_CD_M2,
            affects_lightmapped_meshes: true,
        })
        .insert_resource(DirectionalLightShadowMap { size: 4096 });
}

/// Spawns the sun and the ground
pub fn spawn_stage(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    shot: Option<Res<crate::shots::ShotOptions>>,
) {
    let shot = shot.as_deref().copied().unwrap_or_default();
    // Unlit pictures need no sun, and its shadows would only cost time
    if !shot.unlit {
        commands.spawn(sun());
    }
    if shot.no_ground {
        return;
    }
    commands.spawn((
        Name::new("ground"),
        Mesh3d(meshes.add(Plane3d::default().mesh().size(GROUND_SIZE, GROUND_SIZE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(GROUND_SRGB, GROUND_SRGB, GROUND_SRGB),
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        })),
        Transform::IDENTITY,
        crate::Ground,
    ));
}
