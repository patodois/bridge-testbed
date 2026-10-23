//! Rendering without a window: the shots and the scripted gallery captures both draw into an image
//! and write it as a PNG. macOS windows that are covered render black, and CI has no display, so
//! there is no window at all: no primary window and no winit.

use std::path::PathBuf;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::time::TimeUpdateStrategy;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;

/// The simulated time of one frame: offscreen runs step time by hand so they do not depend on how fast
/// the machine renders
pub const FRAME: Duration = Duration::from_micros(16_667);

/// The image the camera draws into
#[derive(Resource, Clone)]
pub struct Target(pub Handle<Image>);

/// Adds Bevy's plugins without a window, a runner loop and a fixed time step
pub fn add_plugins(app: &mut App) {
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>()
            .set(bevy::render::RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            }),
        ScheduleRunnerPlugin::run_loop(Duration::ZERO),
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
}

/// A render target of this size (sRGB, 8 bits)
pub fn make_target(images: &mut Assets<Image>, size: UVec2) -> Handle<Image> {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    images.add(image)
}

pub fn render_target(handle: Handle<Image>) -> RenderTarget {
    RenderTarget::from(handle)
}

/// Asks for the target to be saved; the file shows up a few frames later
pub fn save(commands: &mut Commands, target: &Handle<Image>, path: PathBuf) {
    commands
        .spawn(Screenshot::image(target.clone()))
        .observe(save_to_disk(path));
}
