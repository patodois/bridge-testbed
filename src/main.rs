//! bridge-testbed: loads assets from every rewrite-bridge provider side by side, to check scale,
//! orientation, animation, materials and physics across games, and renders fixed shots for visual
//! regression. See README.md.

mod analyze;
mod camera;
mod capture;
mod compare;
mod config;
mod gallery;
mod hud;
mod inspect;
mod loader;
mod offscreen;
mod physics;
mod providers;
mod refs;
mod shots;
mod stage;
#[cfg(feature = "wt")]
mod wt_collision;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};
use bevy::prelude::*;
use clap::{Args, Parser, Subcommand};
use rewrite_bridge_bevy::{RewriteBridgePlugin, SpawnOptions};

use config::Config;

/// Marks the ground plane
#[derive(Component)]
pub struct Ground;

#[derive(Parser)]
#[command(name = "bridge-testbed", version, about)]
struct Cli {
    /// The testbed config (see testbed.example.toml)
    #[arg(long, global = true, default_value = "testbed.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    gallery: GalleryArgs,
}

#[derive(Args)]
struct GalleryArgs {
    /// Run the gallery without a window under a script and write PNGs (see capture.rs for the commands)
    #[arg(long)]
    script: Option<String>,
    /// Folder for the script's PNGs
    #[arg(long, default_value = ".")]
    out: PathBuf,
    /// Size of the scripted captures
    #[arg(long, default_value = "1600x900")]
    size: String,
    /// Quit after this many seconds (for measuring)
    #[arg(long)]
    exit_after: Option<f32>,
}

#[derive(Subcommand)]
enum Command {
    /// Render every item headless into DIR/<label>.png plus DIR/manifest.json
    Shots {
        #[arg(long)]
        out: PathBuf,
        /// Only the item with this label
        #[arg(long)]
        only: Option<String>,
    },
    /// Diff two shot folders per pixel; exits nonzero on regressions
    Compare {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        new: PathBuf,
        /// Allowed fraction of differing pixels
        #[arg(long, default_value_t = 0.01)]
        threshold: f32,
        /// Largest channel difference (of 255) that still counts as the same pixel
        #[arg(long, default_value_t = 4)]
        tolerance: u8,
        /// Where the diff images go (default NEW/diff)
        #[arg(long)]
        diff_out: Option<PathBuf>,
    },
    /// Print the numbers of every item (sizes, counts, materials, clips with their root motion) without rendering
    Inspect {
        #[arg(long)]
        only: Option<String>,
        /// Clips to list per animated item
        #[arg(long, default_value_t = 12)]
        clips: usize,
    },
    /// Print the ids a provider lists, to find items for the config
    List {
        #[arg(long)]
        provider: String,
        #[arg(long, default_value = "")]
        filter: String,
        #[arg(long, default_value_t = 40)]
        limit: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Compare {
            base,
            new,
            threshold,
            tolerance,
            diff_out,
        }) => {
            let diff_out = diff_out.unwrap_or_else(|| new.join("diff"));
            let regressions = compare::run(&compare::Args {
                base,
                new,
                threshold,
                tolerance,
                diff_out,
            })?;
            if regressions > 0 {
                std::process::exit(1);
            }
            Ok(())
        }
        Some(Command::Inspect { only, clips }) => {
            inspect::run(&read_config(&cli.config)?, only.as_deref(), clips)
        }
        Some(Command::List {
            provider,
            filter,
            limit,
        }) => {
            let config = read_config(&cli.config)?;
            let set = providers::ProviderSet::new(&config.providers);
            let p = set.get(&provider).map_err(anyhow::Error::msg)?;
            let all = p.list(&filter).map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("{} assets ({} shown)", all.len(), all.len().min(limit));
            for a in all.iter().take(limit) {
                println!("{:?}\t{}\t{}", a.kind, a.id, a.name);
            }
            Ok(())
        }
        Some(Command::Shots { out, only }) => {
            let mut config = read_config(&cli.config)?;
            shots::prepare_config(&mut config);
            if let Some(o) = &only {
                if !config.items.iter().any(|i| &i.label == o) {
                    bail!("no item labelled {o:?}");
                }
            }
            std::fs::create_dir_all(&out)?;
            run_shots(config, out, only);
            Ok(())
        }
        None => {
            let config = read_config(&cli.config)?;
            run_gallery(config, cli.gallery)
        }
    }
}

fn read_config(path: &Path) -> Result<Config> {
    if !path.exists() {
        bail!(
            "{} does not exist: copy testbed.example.toml to it and set your game paths",
            path.display()
        );
    }
    Config::load(path)
}

/// The conversions: Bevy's default exposure, so the sky/ambient scale of the plugin matches
fn bridge_plugin() -> RewriteBridgePlugin {
    RewriteBridgePlugin::with_options(SpawnOptions {
        light_shadows: false,
        ..Default::default()
    })
}

fn run_shots(config: Config, out: PathBuf, only: Option<String>) {
    let mut app = App::new();
    offscreen::add_plugins(&mut app);
    stage::add_resources(&mut app);
    app.add_plugins(bridge_plugin())
        .insert_resource(gallery::Gallery::new(config))
        .insert_resource(shots::ShotRun::new(out, only))
        .add_systems(Startup, (stage::spawn_stage, shots::setup))
        .add_systems(Update, shots::step)
        .run();
}

fn run_gallery(config: Config, args: GalleryArgs) -> Result<()> {
    let mut app = App::new();
    let scripted = args.script.is_some();
    if scripted {
        let (w, h) = args
            .size
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
            .ok_or_else(|| anyhow::anyhow!("--size takes WxH"))?;
        let commands = capture::parse(args.script.as_deref().unwrap_or(""))?;
        std::fs::create_dir_all(&args.out)?;
        offscreen::add_plugins(&mut app);
        app.insert_resource(capture::Script::new(commands, args.out.clone()))
            .insert_resource(CaptureSize(UVec2::new(w, h)))
            .add_systems(
                Update,
                (
                    capture::run.before(hud::keys).before(physics::keys),
                    capture::physics_time,
                ),
            );
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "bridge-testbed".into(),
                resolution: (1600, 900).into(),
                ..default()
            }),
            ..default()
        }));
    }
    if let Some(s) = args.exit_after {
        app.add_systems(
            Update,
            move |time: Res<Time<Real>>, mut exit: MessageWriter<AppExit>| {
                if time.elapsed() > Duration::from_secs_f32(s) {
                    exit.write(AppExit::Success);
                }
            },
        );
    }
    stage::add_resources(&mut app);
    app.add_plugins((bridge_plugin(), physics::plugins()))
        .insert_resource(gallery::Gallery::new(config))
        .init_resource::<camera::Orbit>()
        .init_resource::<camera::FrameRequest>()
        .init_resource::<hud::View>()
        .add_systems(
            Startup,
            (
                stage::spawn_stage,
                refs::spawn_references,
                hud::spawn_hud,
                spawn_camera,
                physics::setup_ground,
            ),
        )
        .add_systems(
            Update,
            (
                gallery::pump,
                hud::keys,
                physics::keys,
                camera::frame_all_when_done,
                camera::orbit_camera,
                gallery::drive_clips,
                gallery::draw_gizmos,
                hud::update_hud,
                hud::update_labels.after(camera::orbit_camera),
            ),
        )
        .run();
    Ok(())
}

#[derive(Resource)]
struct CaptureSize(UVec2);

fn spawn_camera(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    size: Option<Res<CaptureSize>>,
    orbit: Res<camera::Orbit>,
) {
    let mut camera = commands.spawn((
        Camera3d::default(),
        IsDefaultUiCamera,
        orbit.transform(),
        Projection::Perspective(PerspectiveProjection {
            far: 2000.0,
            ..default()
        }),
    ));
    if let Some(size) = size {
        let target = offscreen::make_target(&mut images, size.0);
        camera.insert(offscreen::render_target(target.clone()));
        commands.insert_resource(offscreen::Target(target));
    }
}
