//! `bridge-testbed shots`: every item rendered alone, 512 x 512, from a fixed 3/4 camera framed on its
//! AABB, under the stage's fixed lighting, into `DIR/<label>.png` and `DIR/manifest.json`.
//! The point is regression: the same config on the same machine must give the same pixels.

use std::path::PathBuf;

use bevy::camera::ClearColorConfig;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::light::{GeneratedEnvironmentMapLight, Skybox};
use bevy::prelude::*;
use rewrite_bridge_bevy::{Bridge, BridgeAnimations};
use serde::{Deserialize, Serialize};

use crate::analyze::Aabb;
use crate::config::Config;
use crate::gallery::{fail, place, Gallery, PlaceOpts, Status};
use crate::loader::{self, Pending};
use crate::offscreen::{self, Target};

pub const SIZE: u32 = 512;
/// Frames after the item is placed before its clip is frozen
const FREEZE_AT: u32 = 20;
/// Frames after the item is placed before the picture is taken, for uploads, mipmaps and shadows
const SHOOT_AT: u32 = 45;
/// Fraction of the first clip at which an animated item is frozen
pub const CLIP_FRACTION: f32 = 0.0;
/// The manifest's lighting line for `--unlit`
pub const UNLIT_NOTE: &str = "unlit: base colour (times its texture and vertex colours) only, no lights, no shadows, no tonemapping, no anti-aliasing, transparent background";

/// What `shots` leaves out so that a shot can be set beside a reference picture
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShotOptions {
    /// No ground plane: the item floats on the background
    pub no_ground: bool,
    /// Base colour only: no lights, no shadows, no tonemapping, a transparent background (RGBA PNG)
    pub unlit: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Manifest {
    pub size: [u32; 2],
    pub lighting: String,
    pub camera: String,
    /// `--no-ground` was given (absent in older manifests: the ground was there)
    #[serde(default)]
    pub no_ground: bool,
    /// `--unlit` was given (absent in older manifests: the shots are lit)
    #[serde(default)]
    pub unlit: bool,
    pub items: Vec<Entry>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub label: String,
    pub provider: String,
    pub id: String,
    pub kind: String,
    /// "ok" or "error"
    pub status: String,
    pub error: Option<String>,
    pub png: Option<String>,
    pub aabb_min: Option<[f32; 3]>,
    pub aabb_max: Option<[f32; 3]>,
    pub aabb_size: Option<[f32; 3]>,
    pub vertices: usize,
    pub triangles: usize,
    pub instances: usize,
    pub clips: usize,
    pub mean_albedo: Option<f32>,
    pub warnings: Vec<String>,
}

#[derive(Resource)]
pub struct ShotRun {
    pub out: PathBuf,
    pub only: Option<String>,
    next: usize,
    state: State,
    /// Write RGBA PNGs: the background is transparent
    unlit: bool,
    pub manifest: Manifest,
}

enum State {
    Idle,
    Loading(usize, Pending),
    /// item index, frames since placed, the PNG path (once the picture was asked for)
    Shoot(usize, u32, Option<PathBuf>),
    /// frames to let freed assets go before the next item
    Cleanup(u32),
    Finished,
}

impl ShotRun {
    pub fn new(out: PathBuf, only: Option<String>, options: ShotOptions) -> Self {
        Self {
            out,
            only,
            next: 0,
            state: State::Idle,
            unlit: options.unlit,
            manifest: Manifest {
                size: [SIZE, SIZE],
                lighting: if options.unlit {
                    UNLIT_NOTE
                } else {
                    crate::stage::LIGHTING_NOTE
                }
                .to_string(),
                no_ground: options.no_ground,
                unlit: options.unlit,
                camera: "3/4 view: 35 deg right of the +Z front, 22 deg above, bounding sphere of the AABB fitted to a 45 deg field of view".to_string(),
                items: Vec::new(),
            },
        }
    }
}

pub fn prepare_config(config: &mut Config) {
    // Each item alone at the origin, sitting where its own origin puts it
    for item in &mut config.items {
        item.at = Some([0.0, 0.0]);
    }
    config.gallery.snap_to_ground = false;
}

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, options: Res<ShotOptions>) {
    let target = offscreen::make_target(&mut images, UVec2::splat(SIZE));
    let mut camera = commands.spawn((
        Camera3d::default(),
        offscreen::render_target(target.clone()),
        Transform::from_xyz(0.0, 2.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    if options.unlit {
        // Pixels are the base colour as it is: nothing may tone, dither or smooth it, and the edge of the
        // item is a hard 0 / 255 alpha, which is what a silhouette needs
        camera.insert((Tonemapping::None, DebandDither::Disabled, Msaa::Off));
    }
    commands.insert_resource(Target(target));
}

/// The entities that carry a scene's sky
type WithSky = Or<(With<Skybox>, With<GeneratedEnvironmentMapLight>)>;

/// `--unlit`: every material shows its base colour only, and a scene's own sky or clear colour is
/// taken off the camera. Runs every frame, since materials arrive with each item.
pub fn unlit_pass(
    options: Res<ShotOptions>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cameras: Query<&mut Camera, With<Camera3d>>,
    sky: Query<Entity, WithSky>,
    mut commands: Commands,
) {
    if !options.unlit {
        return;
    }
    // Look first: `iter_mut` would mark every material as changed on every frame
    let lit: Vec<_> = materials
        .iter()
        .filter(|(_, m)| !m.unlit)
        .map(|(id, _)| id)
        .collect();
    for id in lit {
        if let Some(mut m) = materials.get_mut(id) {
            m.unlit = true;
        }
    }
    for mut camera in &mut cameras {
        if !matches!(camera.clear_color, ClearColorConfig::Custom(c) if c == Color::NONE) {
            camera.clear_color = ClearColorConfig::Custom(Color::NONE);
        }
    }
    for e in &sky {
        commands
            .entity(e)
            .remove::<(Skybox, GeneratedEnvironmentMapLight)>();
    }
}

fn entry_for(g: &Gallery, idx: usize) -> Entry {
    let item = &g.items[idx];
    let mut e = Entry {
        label: item.cfg.label.clone(),
        provider: item.cfg.provider.clone(),
        id: item.cfg.id.clone(),
        kind: item.cfg.kind.as_str().to_string(),
        ..Entry::default()
    };
    match &item.status {
        Status::Ready => {
            e.status = "ok".into();
            e.png = Some(format!("{}.png", item.cfg.label));
            if let Some(b) = item.aabb {
                e.aabb_min = Some(b.min);
                e.aabb_max = Some(b.max);
                e.aabb_size = Some(b.size().to_array());
            }
            e.vertices = item.stats.vertices;
            e.triangles = item.stats.triangles;
            e.instances = item.stats.instances;
            e.clips = item.clips.len();
            e.mean_albedo = item.stats.mean_albedo;
            e.warnings = item.warnings.clone();
        }
        Status::Failed(err) => {
            e.status = "error".into();
            e.error = Some(err.clone());
        }
        _ => {
            e.status = "error".into();
            e.error = Some("not loaded".into());
        }
    }
    e
}

#[allow(clippy::too_many_arguments)]
pub fn step(
    mut run: ResMut<ShotRun>,
    mut gallery: ResMut<Gallery>,
    mut bridge: Bridge,
    target: Res<Target>,
    mut camera: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
    mut players: Query<(&BridgeAnimations, &mut AnimationPlayer)>,
    mut exit: MessageWriter<AppExit>,
    mut frame: Local<u32>,
) {
    *frame += 1;
    if *frame < crate::gallery::WARM_UP_FRAMES {
        return;
    }
    let state = std::mem::replace(&mut run.state, State::Idle);
    run.state = match state {
        State::Idle => {
            // Next item that is wanted
            let wanted = |i: usize, g: &Gallery, only: &Option<String>| {
                only.as_ref().is_none_or(|o| &g.items[i].cfg.label == o)
            };
            let mut next = run.next;
            while next < gallery.items.len() && !wanted(next, &gallery, &run.only) {
                next += 1;
            }
            if next >= gallery.items.len() {
                State::Finished
            } else {
                run.next = next + 1;
                gallery.items[next].status = Status::Loading;
                info!("shot: loading {}", gallery.items[next].cfg.label);
                let rx =
                    loader::spawn_load(gallery.providers.clone(), gallery.items[next].cfg.clone());
                State::Loading(next, rx)
            }
        }
        State::Loading(idx, pending) => match pending.poll() {
            None => State::Loading(idx, pending),
            Some(result) => {
                match result {
                    Ok(loaded) => place(
                        &mut gallery,
                        &mut bridge,
                        idx,
                        loaded,
                        PlaceOpts {
                            labels: false,
                            physics: false,
                        },
                    ),
                    Err(e) => fail(&mut gallery, &mut bridge, idx, e, false),
                }
                match &gallery.items[idx].status {
                    Status::Ready => {
                        let b = gallery.items[idx]
                            .aabb
                            .unwrap_or(Aabb::new(Vec3::splat(-0.5), Vec3::splat(0.5)));
                        if let Ok((mut t, mut p)) = camera.single_mut() {
                            *t = crate::camera::shot_camera(&b);
                            let dist = (t.translation - b.center()).length();
                            *p = Projection::Perspective(PerspectiveProjection {
                                fov: crate::camera::FOV,
                                near: dist * 0.01,
                                far: dist * 20.0,
                                ..default()
                            });
                        }
                        State::Shoot(idx, 0, None)
                    }
                    _ => {
                        let e = entry_for(&gallery, idx);
                        warn!(
                            "shot: {} failed: {}",
                            e.label,
                            e.error.clone().unwrap_or_default()
                        );
                        run.manifest.items.push(e);
                        State::Cleanup(0)
                    }
                }
            }
        },
        State::Shoot(idx, n, path) => {
            let n = n + 1;
            if n == FREEZE_AT {
                // Freeze the first clip at a fixed fraction of its length
                let item = &gallery.items[idx];
                if let (Some(root), Some(clip)) = (item.root, item.clips.first()) {
                    if let Ok((clips, mut player)) = players.get_mut(root) {
                        if let Some(c) = clips.clips().first() {
                            player.stop_all();
                            player
                                .play(c.node)
                                .seek_to(clip.duration_s * CLIP_FRACTION)
                                .pause();
                        }
                    }
                }
            }
            match path {
                None if n == SHOOT_AT => {
                    let path = run
                        .out
                        .join(format!("{}.png", gallery.items[idx].cfg.label));
                    if run.unlit {
                        offscreen::save_rgba(&mut bridge.commands, &target.0, path.clone());
                    } else {
                        offscreen::save(&mut bridge.commands, &target.0, path.clone());
                    }
                    State::Shoot(idx, n, Some(path))
                }
                Some(path) if path.exists() && n > SHOOT_AT + 4 => {
                    let e = entry_for(&gallery, idx);
                    info!("shot: {} saved", e.label);
                    run.manifest.items.push(e);
                    if let Some(root) = gallery.items[idx].root.take() {
                        bridge.commands.entity(root).despawn();
                    }
                    State::Cleanup(0)
                }
                path => {
                    if n > SHOOT_AT + 600 {
                        // The file never appeared
                        let mut e = entry_for(&gallery, idx);
                        e.status = "error".into();
                        e.error = Some("the PNG was not written".into());
                        run.manifest.items.push(e);
                        State::Cleanup(0)
                    } else {
                        State::Shoot(idx, n, path)
                    }
                }
            }
        }
        State::Cleanup(n) => {
            if n >= 10 {
                State::Idle
            } else {
                State::Cleanup(n + 1)
            }
        }
        State::Finished => {
            let path = run.out.join("manifest.json");
            let text = serde_json::to_string_pretty(&run.manifest).expect("json");
            if let Err(e) = std::fs::write(&path, text + "\n") {
                error!("cannot write {}: {e}", path.display());
            }
            let failed = run
                .manifest
                .items
                .iter()
                .filter(|e| e.status != "ok")
                .count();
            println!(
                "{} shots in {}, {} failed",
                run.manifest.items.len() - failed,
                run.out.display(),
                failed
            );
            exit.write(AppExit::Success);
            State::Finished
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_from_before_the_flags_reads_as_lit_with_ground() {
        let old = r#"{"size":[512,512],"lighting":"x","camera":"y","items":[]}"#;
        let m: Manifest = serde_json::from_str(old).unwrap();
        assert!(!m.unlit && !m.no_ground);
    }

    #[test]
    fn the_flags_are_in_the_manifest() {
        let run = ShotRun::new(
            PathBuf::from("out"),
            None,
            ShotOptions {
                no_ground: true,
                unlit: true,
            },
        );
        assert!(run.manifest.no_ground && run.manifest.unlit);
        assert_eq!(run.manifest.lighting, UNLIT_NOTE);
    }
}
