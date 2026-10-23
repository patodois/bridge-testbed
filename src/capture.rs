//! Scripted gallery captures: the gallery without a window, driven by a short script, writing PNGs.
//! It is how the gallery gets checked without a person at the keyboard (and in the README's pictures).
//!
//! `--script "wait 2; frame all; shot overview; select Trork; drop; wait 2; shot after"`
//!
//! | command | does |
//! |---|---|
//! | `wait S` | simulates S seconds (60 frames each) |
//! | `view ex,ey,ez,tx,ty,tz` | puts the camera at e looking at t |
//! | `frame LABEL [H]` / `frame all` | frames an item's AABB (H extra metres above it), or everything |
//! | `select LABEL` | the HUD's item |
//! | `front LABEL` | frames an item from straight in front of it (+Z) |
//! | `clip LABEL N` | sets item's clip (1-based) |
//! | `drop [LABEL]` / `stack` / `reset` | the physics keys |
//! | `set help\|labels\|aabb\|grid on\|off` | the view toggles |
//! | `seek LABEL S` | pauses the item's clip at S seconds |
//! | `key SPACE` / `key shift+c` | presses keys for a frame, so the real key handlers run |
//! | `shot NAME` | writes `NAME.png` into the output folder |

use std::path::PathBuf;

use anyhow::{bail, Result};
use avian3d::prelude::*;
use bevy::prelude::*;
use rewrite_bridge_bevy::{Bridge, BridgeAnimations};

use crate::camera::{FrameRequest, Orbit};
use crate::gallery::Gallery;
use crate::hud::View;
use crate::offscreen::Target;
use crate::physics::StackBody;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Wait(f32),
    View(Vec3, Vec3),
    /// An item (or everything), with this many extra metres above it
    Frame(Option<String>, f32),
    Select(String),
    /// Looks at an item from straight in front of it (from +Z)
    Front(String),
    Clip(String, usize),
    /// Pauses the item's clip at this time, seconds
    Seek(String, f32),
    Drop(Option<String>),
    Stack,
    Reset,
    Set(String, bool),
    Shot(String),
    /// Presses keys (`+` joins a chord: `shift+c`) for one frame, so the real key handlers run
    Key(Vec<KeyCode>),
}

pub fn parse(script: &str) -> Result<Vec<Command>> {
    let mut out = Vec::new();
    for raw in script.split(';') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let (word, rest) = raw
            .split_once(' ')
            .map_or((raw, ""), |(w, r)| (w, r.trim()));
        out.push(match word {
            "wait" => Command::Wait(rest.parse()?),
            "view" => {
                let v: Vec<f32> = rest
                    .split(',')
                    .map(|n| n.trim().parse())
                    .collect::<Result<_, _>>()?;
                if v.len() != 6 {
                    bail!("view takes six numbers: {raw}");
                }
                Command::View(Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5]))
            }
            "frame" => {
                // `frame LABEL [EXTRA_HEIGHT]`
                let (label, extra) = match rest.rsplit_once(' ') {
                    Some((l, n)) => match n.parse::<f32>() {
                        Ok(n) => (l.trim(), n),
                        Err(_) => (rest, 0.0),
                    },
                    None => (rest, 0.0),
                };
                Command::Frame((label != "all").then(|| label.to_string()), extra)
            }
            "select" => Command::Select(rest.to_string()),
            "front" => Command::Front(rest.to_string()),
            "clip" => {
                let (l, n) = rest
                    .rsplit_once(' ')
                    .ok_or_else(|| anyhow::anyhow!("clip LABEL N: {raw}"))?;
                Command::Clip(l.trim().to_string(), n.trim().parse()?)
            }
            "seek" => {
                let (l, t) = rest
                    .rsplit_once(' ')
                    .ok_or_else(|| anyhow::anyhow!("seek LABEL SECONDS: {raw}"))?;
                Command::Seek(l.trim().to_string(), t.trim().parse()?)
            }
            "drop" => Command::Drop((!rest.is_empty()).then(|| rest.to_string())),
            "stack" => Command::Stack,
            "reset" => Command::Reset,
            "set" => {
                let (what, on) = rest
                    .rsplit_once(' ')
                    .ok_or_else(|| anyhow::anyhow!("set WHAT on|off: {raw}"))?;
                Command::Set(what.trim().to_string(), on.trim() == "on")
            }
            "shot" => Command::Shot(rest.to_string()),
            "key" => Command::Key(
                rest.split('+')
                    .map(|k| key_code(k.trim()).ok_or_else(|| anyhow::anyhow!("unknown key {k:?}")))
                    .collect::<Result<_>>()?,
            ),
            other => bail!("unknown script command {other:?}"),
        });
    }
    Ok(out)
}

#[derive(Resource)]
pub struct Script {
    pub commands: Vec<Command>,
    pub out: PathBuf,
    next: usize,
    wait_frames: u32,
    /// Frames to wait after a shot for the file
    settle: u32,
}

impl Script {
    pub fn new(commands: Vec<Command>, out: PathBuf) -> Self {
        Self {
            commands,
            out,
            next: 0,
            wait_frames: 0,
            settle: 0,
        }
    }
}

fn key_code(name: &str) -> Option<KeyCode> {
    Some(match name.to_ascii_lowercase().as_str() {
        "space" => KeyCode::Space,
        "tab" => KeyCode::Tab,
        "home" => KeyCode::Home,
        "shift" => KeyCode::ShiftLeft,
        "a" => KeyCode::KeyA,
        "b" => KeyCode::KeyB,
        "c" => KeyCode::KeyC,
        "f" => KeyCode::KeyF,
        "h" => KeyCode::KeyH,
        "l" => KeyCode::KeyL,
        "n" => KeyCode::KeyN,
        "p" => KeyCode::KeyP,
        "r" => KeyCode::KeyR,
        "v" => KeyCode::KeyV,
        "z" => KeyCode::KeyZ,
        _ => return None,
    })
}

fn find(gallery: &Gallery, label: &str) -> Option<usize> {
    gallery.items.iter().position(|i| i.cfg.label == label)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    mut script: ResMut<Script>,
    mut gallery: ResMut<Gallery>,
    mut orbit: ResMut<Orbit>,
    mut request: ResMut<FrameRequest>,
    mut view: ResMut<View>,
    target: Res<Target>,
    stack: Query<Entity, With<StackBody>>,
    mut bridge: Bridge,
    mut exit: MessageWriter<AppExit>,
    mut ready_frames: Local<u32>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut held: Local<Vec<KeyCode>>,
    mut players: Query<(&BridgeAnimations, &mut AnimationPlayer)>,
) {
    // Keys pressed by the previous command are let go
    for k in held.drain(..) {
        keys.release(k);
    }
    if !gallery.done {
        return;
    }
    // A few frames after the last item, for uploads and shadows
    if *ready_frames < 30 {
        *ready_frames += 1;
        return;
    }
    if script.settle > 0 {
        script.settle -= 1;
        return;
    }
    if script.wait_frames > 0 {
        script.wait_frames -= 1;
        return;
    }
    let Some(command) = script.commands.get(script.next).cloned() else {
        info!("script finished");
        exit.write(AppExit::Success);
        return;
    };
    script.next += 1;
    match command {
        Command::Wait(s) => script.wait_frames = (s * 60.0) as u32,
        Command::View(eye, at) => {
            let d = eye - at;
            orbit.target = at;
            orbit.dist = d.length().max(0.01);
            orbit.yaw = d.x.atan2(d.z);
            orbit.pitch = (d.y / orbit.dist).clamp(-1.0, 1.0).asin();
            script.wait_frames = 4;
        }
        Command::Frame(label, extra) => {
            let bounds = match label {
                None => crate::camera::all_bounds(&gallery),
                Some(l) => {
                    let b = find(&gallery, &l).and_then(|i| gallery.items[i].aabb);
                    if b.is_none() {
                        warn!("script: no item {l:?} to frame");
                    }
                    b
                }
            };
            request.0 =
                bounds.map(|b| crate::analyze::Aabb::new(b.min(), b.max() + Vec3::Y * extra));
            script.wait_frames = 4;
        }
        Command::Front(l) => {
            match find(&gallery, &l).and_then(|i| gallery.items[i].aabb) {
                Some(b) => {
                    orbit.yaw = 0.0;
                    orbit.pitch = 0.08;
                    request.0 = Some(b);
                }
                None => warn!("script: no item {l:?} to look at"),
            }
            script.wait_frames = 4;
        }
        Command::Select(l) => match find(&gallery, &l) {
            Some(i) => gallery.selected = i,
            None => warn!("script: no item {l:?}"),
        },
        Command::Clip(l, n) => {
            if let Some(i) = find(&gallery, &l) {
                let nc = gallery.items[i].clips.len();
                if nc > 0 {
                    gallery.items[i].clip.current = (n.max(1) - 1).min(nc - 1);
                }
            }
        }
        Command::Seek(l, t) => {
            if let Some(i) = find(&gallery, &l) {
                let item = &mut gallery.items[i];
                item.clip.paused = true;
                if let (Some(root), Some(c)) = (item.root, item.clip.applied) {
                    if let Ok((clips, mut player)) = players.get_mut(root) {
                        if let Some(clip) = clips.clips().get(c) {
                            if let Some(a) = player.animation_mut(clip.node) {
                                a.seek_to(t).pause();
                            }
                        }
                    }
                }
            }
        }
        Command::Drop(l) => {
            if let Some(l) = l {
                if let Some(i) = find(&gallery, &l) {
                    gallery.selected = i;
                }
            }
            crate::physics::drop_selected(&gallery, &mut bridge.commands);
        }
        Command::Stack => crate::physics::spawn_stack(&gallery, &mut bridge),
        Command::Reset => crate::physics::reset_all(&gallery, &stack, &mut bridge.commands),
        Command::Set(what, on) => match what.as_str() {
            "help" => view.help = on,
            "labels" => view.labels = on,
            "aabb" => view.aabb = on,
            "grid" => view.grid = on,
            other => warn!("script: cannot set {other:?}"),
        },
        Command::Key(codes) => {
            for k in codes {
                keys.press(k);
                held.push(k);
            }
            script.wait_frames = 2;
        }
        Command::Shot(name) => {
            let path = script.out.join(format!("{name}.png"));
            crate::offscreen::save(&mut bridge.commands, &target.0, path.clone());
            info!("script: shot {}", path.display());
            script.settle = 20;
        }
    }
}

/// Stops physics from running until there is something to simulate
pub fn physics_time(mut time: ResMut<Time<Physics>>, gallery: Res<Gallery>) {
    // Props wait disabled at their slots, so the simulation may run from the start; only pause it
    // while items are still being placed, so that scenes do not settle before the camera sees them
    if gallery.done {
        time.unpause();
    } else {
        time.pause();
    }
}
