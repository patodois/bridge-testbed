//! `bridge-testbed physics --out FILE`: the gallery's physics scenes run without a window or a person, and
//! every body's rest pose written as JSON, so that another implementation of the same scenes can be
//! compared with it.
//!
//! The scenes are the ones the keys run, one after the other, each from the same state:
//!
//! - **drop**: for every `dynamic = true` item with a convex hull, in config order, the item is put with its
//!   lowest point `drop_height` above the ground and let go (the Space key), the world runs for `--seconds`,
//!   and its pose is read; then everything is put back (R);
//! - **stack**: the `[stack]` pyramid (the B key), run for the same time, one pose per block.
//!
//! Time is stepped by hand: every frame is exactly one physics step of [`offscreen::FRAME`] (1/60 s), no
//! matter how fast the machine is, so the same config on the same machine gives the same file. The
//! numbers are the testbed's assumptions (see the README), not the games'.

use std::path::PathBuf;
use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use rewrite_bridge_bevy::Bridge;
use serde::Serialize;

use crate::gallery::{Gallery, Status};
use crate::offscreen;
use crate::physics::StackBody;

/// Frames after the last item is placed before the first scene, for uploads and the first collider builds
const SETTLE_FRAMES: u32 = 30;
/// Frames after a reset, for the poses to be written back before the next scene
const RESET_FRAMES: u32 = 4;
/// Default simulated seconds per scene
pub const DEFAULT_SECONDS: f32 = 6.0;

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Report {
    pub tool: String,
    /// Always says that every number is a testbed assumption
    pub note: String,
    pub step_s: f32,
    pub seconds_per_scene: f32,
    pub gravity_m_s2: f32,
    pub density_kg_m3: f32,
    pub drop_height_m: f32,
    pub scenes: Vec<Scene>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Scene {
    /// "drop" or "stack"
    pub scene: String,
    pub bodies: Vec<Body>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Body {
    /// The item's label, or `stack block N`
    pub name: String,
    pub provider: String,
    pub id: String,
    /// World position of the body's own origin when the scene began (a drop only: a stack's blocks are
    /// placed by the testbed's layout)
    pub start_position: Option<[f32; 3]>,
    /// World position of the body's own origin at the end, metres
    pub position: [f32; 3],
    /// World orientation at the end, a unit quaternion `[x, y, z, w]` with `w >= 0`
    pub rotation: [f32; 4],
    /// Whether the solver had put the body to sleep (at rest) by the end
    pub sleeping: bool,
    /// Speed at the end, m/s: near zero means the pose is a rest pose
    pub speed_m_s: f32,
}

#[derive(Resource)]
pub struct RestRun {
    out: PathBuf,
    seconds: f32,
    state: State,
    /// Dynamic items still to drop, by index into the gallery
    queue: Vec<usize>,
    frames: u32,
    report: Report,
}

enum State {
    Waiting,
    Dropping {
        item: usize,
        start: Duration,
    },
    Stacking {
        start: Duration,
    },
    /// Frames left until the next scene starts
    Resetting(u32, Next),
    Finished,
}

#[derive(Clone, Copy)]
enum Next {
    Item,
    Stack,
    End,
}

impl RestRun {
    pub fn new(out: PathBuf, seconds: f32) -> Self {
        Self {
            out,
            seconds,
            state: State::Waiting,
            queue: Vec::new(),
            frames: 0,
            report: Report {
                tool: format!("bridge-testbed {} physics", env!("CARGO_PKG_VERSION")),
                note: "Every physics value is a testbed assumption: the bridge carries no physics data yet. Positions are of each body's own origin in metres (Y up, items face +Z); the ground is the plane y = 0.".into(),
                step_s: offscreen::FRAME.as_secs_f32(),
                seconds_per_scene: seconds,
                gravity_m_s2: 0.0,
                density_kg_m3: 0.0,
                drop_height_m: 0.0,
                scenes: Vec::new(),
            },
        }
    }
}

/// `-0.0` written as `0.0`, so that a file does not differ between two runs for a sign that means nothing
fn clean<const N: usize>(v: [f32; N]) -> [f32; N] {
    v.map(|x| x + 0.0)
}

/// A unit quaternion in `[x, y, z, w]` order, signed so that `w >= 0` (q and -q are one rotation)
fn rotation_of(q: Quat) -> [f32; 4] {
    let q = if q.w < 0.0 { -q } else { q };
    clean(q.normalize().to_array())
}

#[allow(clippy::too_many_arguments)]
pub fn step(
    mut run: ResMut<RestRun>,
    mut gallery: ResMut<Gallery>,
    time: Res<Time<Physics>>,
    bodies: Query<(&Transform, Has<Sleeping>, &LinearVelocity)>,
    stack: Query<(Entity, &StackBody)>,
    stack_ids: Query<Entity, With<StackBody>>,
    mut bridge: Bridge,
    mut exit: MessageWriter<AppExit>,
) {
    if !gallery.done {
        return;
    }
    run.frames += 1;
    if run.frames < SETTLE_FRAMES {
        return;
    }
    let seconds = Duration::from_secs_f32(run.seconds);
    let now = time.elapsed();
    let state = std::mem::replace(&mut run.state, State::Finished);
    run.state = match state {
        State::Waiting => {
            run.report.gravity_m_s2 = gallery.config.physics.gravity;
            run.report.density_kg_m3 = gallery.config.physics.density;
            run.report.drop_height_m = gallery.config.physics.drop_height;
            run.queue = gallery
                .items
                .iter()
                .enumerate()
                .filter(|(_, i)| matches!(i.status, Status::Ready) && i.has_hull && i.cfg.dynamic)
                .map(|(i, _)| i)
                .collect();
            run.queue.reverse();
            State::Resetting(0, Next::Item)
        }
        State::Resetting(0, next) => match next {
            Next::Item => match run.queue.pop() {
                Some(item) => {
                    gallery.selected = item;
                    crate::physics::drop_selected(&gallery, &mut bridge.commands);
                    State::Dropping { item, start: now }
                }
                None => State::Resetting(0, Next::Stack),
            },
            Next::Stack => {
                if matches!(gallery.stack, Some(Ok(_))) {
                    crate::physics::spawn_stack(&gallery, &mut bridge);
                    State::Stacking { start: now }
                } else {
                    State::Resetting(0, Next::End)
                }
            }
            Next::End => {
                finish(&run);
                exit.write(AppExit::Success);
                State::Finished
            }
        },
        State::Resetting(n, next) => State::Resetting(n - 1, next),
        State::Dropping { item, start } => {
            if now - start < seconds {
                State::Dropping { item, start }
            } else {
                let it = &gallery.items[item];
                if let Some((t, sleeping, v)) = it.root.and_then(|r| bodies.get(r).ok()) {
                    let start_position = Vec3::new(
                        it.origin.x,
                        gallery.config.physics.drop_height - it.hull_min_y,
                        it.origin.z,
                    );
                    let body = Body {
                        name: it.cfg.label.clone(),
                        provider: it.cfg.provider.clone(),
                        id: it.cfg.id.clone(),
                        start_position: Some(clean(start_position.to_array())),
                        position: clean(t.translation.to_array()),
                        rotation: rotation_of(t.rotation),
                        sleeping,
                        speed_m_s: v.length(),
                    };
                    run.report.scenes.push(Scene {
                        scene: "drop".into(),
                        bodies: vec![body],
                    });
                }
                crate::physics::reset_all(&gallery, &stack_ids, &mut bridge.commands);
                State::Resetting(RESET_FRAMES, Next::Item)
            }
        }
        State::Stacking { start } => {
            if now - start < seconds {
                State::Stacking { start }
            } else {
                let block = gallery.config.stack.clone().expect("a stack ran");
                let mut found: Vec<(usize, Body)> = stack
                    .iter()
                    .filter_map(|(e, n)| {
                        let (t, sleeping, v) = bodies.get(e).ok()?;
                        Some((
                            n.0,
                            Body {
                                name: format!("stack block {}", n.0),
                                provider: block.provider.clone(),
                                id: block.id.clone(),
                                start_position: None,
                                position: clean(t.translation.to_array()),
                                rotation: rotation_of(t.rotation),
                                sleeping,
                                speed_m_s: v.length(),
                            },
                        ))
                    })
                    .collect();
                found.sort_by_key(|(n, _)| *n);
                run.report.scenes.push(Scene {
                    scene: "stack".into(),
                    bodies: found.into_iter().map(|(_, b)| b).collect(),
                });
                crate::physics::reset_all(&gallery, &stack_ids, &mut bridge.commands);
                State::Resetting(RESET_FRAMES, Next::End)
            }
        }
        State::Finished => State::Finished,
    };
}

fn finish(run: &RestRun) {
    let text = serde_json::to_string_pretty(&run.report).expect("json");
    match std::fs::write(&run.out, text + "\n") {
        Ok(()) => println!(
            "{} scenes ({} bodies) in {}",
            run.report.scenes.len(),
            run.report
                .scenes
                .iter()
                .map(|s| s.bodies.len())
                .sum::<usize>(),
            run.out.display()
        ),
        Err(e) => error!("cannot write {}: {e}", run.out.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_zero_is_written_as_zero() {
        let v = clean([-0.0, 1.0, -0.0]);
        assert!(v.iter().all(|x| x.is_sign_positive()));
    }

    #[test]
    fn rotation_is_signed_so_w_is_not_negative() {
        let q = Quat::from_rotation_y(2.5);
        assert_eq!(rotation_of(q), rotation_of(-q));
        assert!(rotation_of(-q)[3] >= 0.0);
    }
}
