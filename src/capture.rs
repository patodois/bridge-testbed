//! Scripted gallery captures: the gallery without a window, driven by a short script, writing PNGs.
//! It is how the gallery gets checked without a person at the keyboard (and in the README's pictures).
//!
//! `--script "wait 2; frame all; shot overview; select Trork; drop; wait 2; shot after"`
//!
//! | command | does |
//! |---|---|
//! | `wait S` | simulates S seconds (60 frames each) |
//! | `view ex,ey,ez,tx,ty,tz` | puts the camera at e looking at t |
//! | `frame LABEL [H]` / `frame all [H]` | frames an item's AABB (H extra metres above it), or everything; a LABEL that ends in a number, such as `Tank 2`, is that item when one has that label |
//! | `select LABEL` | the HUD's item |
//! | `front LABEL` | frames an item from straight in front of it (+Z) |
//! | `clip LABEL N` | sets item's clip (1-based) |
//! | `drop [LABEL]` / `stack` / `reset` | the physics keys |
//! | `set help\|labels\|aabb\|grid on\|off` | the view toggles |
//! | `seek LABEL S` | pauses the item's clip at S seconds |
//! | `key SPACE` / `key shift+c` | presses keys for a frame, so the real key handlers run |
//! | `shot NAME` | writes `NAME.png` into the output folder, and waits until it is there |
//!
//! A `shot` first removes a `NAME.png` an earlier run left, then waits for the new file (up to 600
//! frames). A shot that never lands is reported, and the program then exits with status 1.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use avian3d::prelude::*;
use bevy::prelude::*;
use rewrite_bridge_bevy::{Bridge, BridgeAnimations};

use crate::camera::{FrameRequest, Orbit};
use crate::gallery::Gallery;
use crate::hud::View;
use crate::offscreen::Target;
use crate::physics::StackBody;
use crate::shots::{advance_shot, Shot, SHOOT_AT};

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Wait(f32),
    View(Vec3, Vec3),
    /// `LABEL [H]` or `all [H]`: an item (or everything), with H extra metres above it. It is split
    /// when the script runs, when the item labels are known (see `frame_target`)
    Frame(String),
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
            "frame" => Command::Frame(rest.to_string()),
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
    /// The capture being waited for
    shot: Option<PendingShot>,
    /// The shots that never landed
    failed: Vec<String>,
}

/// A capture in progress, run through the same states as the `shots` command's
struct PendingShot {
    /// The frame in the states of `shots::advance_shot`
    n: u32,
    path: Option<PathBuf>,
    target: PathBuf,
}

impl Script {
    pub fn new(commands: Vec<Command>, out: PathBuf) -> Self {
        Self {
            commands,
            out,
            next: 0,
            wait_frames: 0,
            shot: None,
            failed: Vec::new(),
        }
    }

    /// Starts a capture into `NAME.png`; the next `step_shot` removes an earlier file and asks for it
    fn begin_shot(&mut self, name: &str) {
        self.shot = Some(PendingShot {
            n: SHOOT_AT - 1,
            path: None,
            target: self.out.join(format!("{name}.png")),
        });
    }

    /// One frame of the capture in progress: `true` while it is still waited for. `request` asks for
    /// the image to be written to the path it is given.
    fn step_shot(&mut self, request: impl FnOnce(&Path)) -> bool {
        let Some(mut shot) = self.shot.take() else {
            return false;
        };
        shot.n += 1;
        match advance_shot(shot.n, shot.path.take(), &shot.target, request) {
            Shot::Wait(path) => {
                shot.path = path;
                self.shot = Some(shot);
                true
            }
            Shot::Saved => {
                info!("script: shot {} saved", shot.target.display());
                false
            }
            Shot::Failed(why) => {
                error!("script: shot {} failed: {why}", shot.target.display());
                self.failed
                    .push(format!("{}: {why}", shot.target.display()));
                false
            }
        }
    }

    /// How the program ends once the script is done: an error when a shot never landed
    fn outcome(&self) -> AppExit {
        if self.failed.is_empty() {
            AppExit::Success
        } else {
            AppExit::error()
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

/// What `frame ARG` frames: the item labelled ARG when there is one (so a label that ends in a
/// number, such as `Tank 2`, is that item), else `LABEL H` with H extra metres above it; `all` is
/// everything (`None`)
fn frame_target(arg: &str, is_label: impl Fn(&str) -> bool) -> (Option<String>, f32) {
    let (label, extra) = if is_label(arg) {
        (arg, 0.0)
    } else {
        match arg.rsplit_once(' ') {
            Some((l, n)) => match n.parse::<f32>() {
                Ok(n) => (l.trim(), n),
                Err(_) => (arg, 0.0),
            },
            None => (arg, 0.0),
        }
    };
    ((label != "all").then(|| label.to_string()), extra)
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
    if script.shot.is_some() {
        let image = target.0.clone();
        let waiting = script.step_shot(|p| {
            crate::offscreen::save(&mut bridge.commands, &image, p.to_path_buf());
        });
        if waiting {
            return;
        }
    }
    if script.wait_frames > 0 {
        script.wait_frames -= 1;
        return;
    }
    let Some(command) = script.commands.get(script.next).cloned() else {
        info!("script finished");
        for f in &script.failed {
            error!("script: no capture: {f}");
        }
        exit.write(script.outcome());
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
        Command::Frame(arg) => {
            let (label, extra) = frame_target(&arg, |l| find(&gallery, l).is_some());
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
            info!("script: shot {name}");
            script.begin_shot(&name);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("testbed-capture-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Steps the shot frame by frame as `run` does; `write` is what the capture puts on disk
    fn shoot(script: &mut Script, name: &str, write: Option<&[u8]>) -> u32 {
        script.begin_shot(name);
        let mut frames = 0;
        while script.step_shot(|p| {
            if let Some(bytes) = write {
                std::fs::write(p, bytes).unwrap();
            }
        }) {
            frames += 1;
            assert!(frames < 10_000, "the shot never ended");
        }
        frames
    }

    #[test]
    fn frame_can_name_a_label_that_ends_in_a_number() {
        let labels = ["Tank 2", "Trork"];
        let is_label = |l: &str| labels.contains(&l);
        let frame = |script: &str| match parse(script).unwrap().remove(0) {
            Command::Frame(arg) => frame_target(&arg, is_label),
            other => panic!("{other:?}"),
        };
        assert_eq!(frame("frame Tank 2"), (Some("Tank 2".to_string()), 0.0));
        assert_eq!(frame("frame Tank 2 3.5"), (Some("Tank 2".to_string()), 3.5));
        assert_eq!(frame("frame Trork 3.5"), (Some("Trork".to_string()), 3.5));
        assert_eq!(frame("frame Trork"), (Some("Trork".to_string()), 0.0));
        assert_eq!(frame("frame all"), (None, 0.0));
        assert_eq!(frame("frame all 2"), (None, 2.0));
    }

    #[test]
    fn a_shot_that_never_lands_is_a_failure_not_the_old_png() {
        let dir = scratch("stale");
        let old = dir.join("overview.png");
        std::fs::write(&old, b"png of an earlier run").unwrap();
        let mut script = Script::new(Vec::new(), dir.clone());
        shoot(&mut script, "overview", None);
        assert_eq!(script.failed.len(), 1, "{:?}", script.failed);
        assert!(!old.exists(), "the earlier run's PNG is still there");
        assert_eq!(script.outcome(), AppExit::error());
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn a_shot_that_lands_is_waited_for_and_succeeds() {
        let dir = scratch("lands");
        let old = dir.join("after.png");
        std::fs::write(&old, b"png of an earlier run").unwrap();
        let mut script = Script::new(Vec::new(), dir.clone());
        let frames = shoot(&mut script, "after", Some(b"new png"));
        assert!(script.failed.is_empty(), "{:?}", script.failed);
        assert!(frames > 0, "the script did not wait for the file");
        assert_eq!(std::fs::read(&old).unwrap(), b"new png");
        assert_eq!(script.outcome(), AppExit::Success);
        std::fs::remove_file(&old).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
