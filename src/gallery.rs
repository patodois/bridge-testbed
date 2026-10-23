//! The gallery: items loaded one at a time, laid out in rows on the ground, each with its label,
//! front arrow and world AABB. `place` is also what the shots use.

use std::sync::Arc;

use bevy::prelude::*;
use rewrite_bridge_bevy::{Bridge, BridgeAnimations, NoSource};

use crate::analyze::{Aabb, Stats};
use crate::config::{Config, ItemConfig, ItemKind};
use crate::hud::WorldLabel;
use crate::loader::{self, ClipInfo, Loaded, Payload, Pending};
use crate::providers::ProviderSet;

/// Frames to wait before the first load, so the window and the renderer are up
pub const WARM_UP_FRAMES: u32 = 10;
/// Width of the slot of an item that failed to load, metres
const FAILED_SLOT: Vec3 = Vec3::new(1.5, 0.0, 1.5);

#[derive(Clone, Debug)]
pub enum Status {
    Pending,
    Loading,
    Ready,
    Failed(String),
}

/// Playback of the clips of one item
#[derive(Clone, Debug, Default)]
pub struct ClipState {
    pub current: usize,
    pub paused: bool,
    /// Play each clip once and go on to the next
    pub auto: bool,
    /// The clip that was last started on the player
    pub applied: Option<usize>,
}

pub struct Item {
    pub cfg: ItemConfig,
    pub status: Status,
    pub root: Option<Entity>,
    /// World position of the item's origin
    pub origin: Vec3,
    /// World AABB
    pub aabb: Option<Aabb>,
    pub stats: Stats,
    pub warnings: Vec<String>,
    pub clips: Vec<ClipInfo>,
    pub clip: ClipState,
    pub collision_note: String,
    pub load_seconds: f32,
    /// Hull height below the origin, for dropping (physics)
    pub hull_min_y: f32,
    pub has_hull: bool,
    pub label: Option<Entity>,
}

impl Item {
    fn new(cfg: ItemConfig) -> Self {
        Self {
            cfg,
            status: Status::Pending,
            root: None,
            origin: Vec3::ZERO,
            aabb: None,
            stats: Stats::default(),
            warnings: Vec::new(),
            clips: Vec::new(),
            clip: ClipState::default(),
            collision_note: String::new(),
            load_seconds: 0.0,
            hull_min_y: 0.0,
            has_hull: false,
            label: None,
        }
    }
}

/// A hidden extra item: the block the stack key spawns
pub struct StackBlock {
    pub model: rewrite_bridge::Model,
    pub hull: Vec<Vec3>,
    pub size: Vec3,
    pub min_y: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Item(usize),
    Stack,
}

#[derive(Default)]
struct Layout {
    column: usize,
    x: f32,
    row_z: f32,
    row_depth: f32,
}

#[derive(Resource)]
pub struct Gallery {
    pub config: Config,
    pub providers: Arc<ProviderSet>,
    pub items: Vec<Item>,
    pub selected: usize,
    pub stack: Option<Result<StackBlock, String>>,
    loading: Option<(Slot, Pending)>,
    next: usize,
    stack_started: bool,
    layout: Layout,
    /// Set once every item has been tried
    pub done: bool,
    /// The frame counter, for the warm-up
    frames: u32,
}

impl Gallery {
    pub fn new(config: Config) -> Self {
        let providers = Arc::new(ProviderSet::new(&config.providers));
        let items = config.items.iter().cloned().map(Item::new).collect();
        Self {
            providers,
            items,
            selected: 0,
            stack: None,
            loading: None,
            next: 0,
            stack_started: false,
            layout: Layout {
                x: 1.5,
                ..Layout::default()
            },
            done: config.items.is_empty() && config.stack.is_none(),
            frames: 0,
            config,
        }
    }

    pub fn loading_label(&self) -> Option<&str> {
        match self.loading.as_ref()?.0 {
            Slot::Item(i) => Some(&self.items[i].cfg.label),
            Slot::Stack => Some("stack block"),
        }
    }

    /// Picks the world position of an item whose item-space bounds are `local` and records the slot
    fn slot(&mut self, idx: usize, local: Option<Aabb>) -> Vec3 {
        let gap = self.config.gallery.gap;
        let columns = self.config.gallery.columns;
        let cfg = &self.items[idx].cfg;
        let (lo, hi) = match local {
            Some(b) => (b.min(), b.max()),
            None => (Vec3::ZERO, FAILED_SLOT),
        };
        let size = hi - lo;
        if let Some([x, z]) = cfg.at {
            let y = if self.config.gallery.snap_to_ground {
                -lo.y
            } else {
                0.0
            };
            return Vec3::new(x, y, z);
        }
        let l = &mut self.layout;
        if columns > 0 && l.column >= columns {
            // Next row, behind this one (items face +Z, the camera starts on that side)
            l.column = 0;
            l.x = 1.5;
            l.row_z -= l.row_depth + gap + 1.0;
            l.row_depth = 0.0;
        }
        // Fronts (the +Z side) of a row line up on row_z; depth goes back
        let origin = Vec3::new(
            l.x - lo.x,
            if self.config.gallery.snap_to_ground {
                -lo.y
            } else {
                0.0
            },
            l.row_z - hi.z,
        );
        l.x += size.x + gap;
        l.row_depth = l.row_depth.max(size.z);
        l.column += 1;
        origin
    }
}

/// Marks an item's root entity
#[derive(Component)]
pub struct ItemRoot;

/// Loads the next item when the previous one is placed, and places arrivals
pub fn pump(mut gallery: ResMut<Gallery>, mut bridge: Bridge) {
    gallery.frames += 1;
    if gallery.frames < WARM_UP_FRAMES || gallery.done {
        return;
    }
    if let Some((slot, pending)) = gallery.loading.take() {
        match pending.poll() {
            None => gallery.loading = Some((slot, pending)),
            Some(result) => arrive(&mut gallery, &mut bridge, slot, result),
        }
        // One placement per frame; the next load starts on the next
        return;
    }
    let g = &mut *gallery;
    if g.next < g.items.len() {
        let idx = g.next;
        g.next += 1;
        g.items[idx].status = Status::Loading;
        let rx = loader::spawn_load(g.providers.clone(), g.items[idx].cfg.clone());
        g.loading = Some((Slot::Item(idx), rx));
    } else if !g.stack_started && g.config.stack.is_some() {
        g.stack_started = true;
        let s = g.config.stack.clone().expect("stack");
        let cfg = ItemConfig {
            provider: s.provider,
            id: s.id,
            kind: ItemKind::Model,
            label: "stack block".into(),
            dynamic: true,
            collide: false,
            collision_id: None,
            at: None,
        };
        let rx = loader::spawn_load(g.providers.clone(), cfg);
        g.loading = Some((Slot::Stack, rx));
    } else {
        g.done = true;
    }
}

fn arrive(gallery: &mut Gallery, bridge: &mut Bridge, slot: Slot, result: Result<Loaded, String>) {
    match slot {
        Slot::Item(idx) => match result {
            Ok(loaded) => place(
                gallery,
                bridge,
                idx,
                loaded,
                PlaceOpts {
                    labels: true,
                    physics: true,
                },
            ),
            Err(e) => fail(gallery, bridge, idx, e, true),
        },
        Slot::Stack => {
            gallery.stack = Some(result.and_then(|l| match l.payload {
                Payload::Model(am) => {
                    let hull = l
                        .collision
                        .hull
                        .ok_or_else(|| "the stack block has no convex hull".to_string())?;
                    let b = l.bounds.ok_or("the stack block has no vertices")?;
                    Ok(StackBlock {
                        model: am.model,
                        hull,
                        size: b.size(),
                        min_y: b.min[1],
                    })
                }
                Payload::Scene(_) => Err("the stack block is a scene".to_string()),
            }));
        }
    }
}

/// Spawns a failed item's slot: an error label and no model
pub fn fail(gallery: &mut Gallery, bridge: &mut Bridge, idx: usize, error: String, labels: bool) {
    let origin = gallery.slot(idx, None);
    let item = &mut gallery.items[idx];
    item.origin = origin;
    item.status = Status::Failed(error.clone());
    item.aabb = Some(Aabb::new(origin, origin + FAILED_SLOT));
    if labels {
        let e = bridge
            .commands
            .spawn(WorldLabel::item(
                idx,
                origin + Vec3::new(0.0, 0.4, 0.0),
                format!("{}\nERROR: {}", item.cfg.label, error),
            ))
            .id();
        item.label = Some(e);
    }
}

/// What `place` adds besides the model
#[derive(Clone, Copy)]
pub struct PlaceOpts {
    pub labels: bool,
    pub physics: bool,
}

/// Spawns a loaded item at its slot
pub fn place(
    gallery: &mut Gallery,
    bridge: &mut Bridge,
    idx: usize,
    loaded: Loaded,
    opts: PlaceOpts,
) {
    let Loaded {
        payload,
        stats,
        bounds,
        mut warnings,
        clips,
        mut collision,
        seconds,
    } = loaded;
    let origin = gallery.slot(idx, bounds);
    let kind = gallery.items[idx].cfg.kind;
    let dynamic = gallery.items[idx].cfg.dynamic;
    let density = gallery.config.physics.density;
    let scene_lights = gallery.config.gallery.scene_lights;
    let root = match payload {
        Payload::Model(am) => {
            if kind == ItemKind::Model {
                bridge.spawn_model(am.model).root
            } else {
                bridge.spawn_animated(*am).root
            }
        }
        Payload::Scene(mut scene) => {
            if !scene_lights && !scene.lights.is_empty() {
                warnings.push(format!(
                    "{} scene lights ignored: the testbed lights everything alike ([gallery] scene_lights = true keeps them)",
                    scene.lights.len()
                ));
                scene.lights.clear();
            }
            match bridge.spawn_scene(*scene, &NoSource) {
                Ok(spawned) => {
                    if !spawned.failed.is_empty() {
                        warnings.push(format!(
                            "{} scene models did not load (first: {})",
                            spawned.failed.len(),
                            spawned.failed[0].1
                        ));
                    }
                    spawned.root
                }
                Err(e) => {
                    let msg = format!("invalid scene: {e}");
                    fail(gallery, bridge, idx, msg, opts.labels);
                    return;
                }
            }
        }
    };
    bridge
        .commands
        .entity(root)
        .insert((Transform::from_translation(origin), ItemRoot));

    let hull_min_y = collision
        .hull
        .as_ref()
        .map(|h| h.iter().map(|p| p.y).fold(f32::MAX, f32::min));
    let collision_note = if opts.physics {
        crate::physics::attach(&mut bridge.commands, root, collision, dynamic, density)
    } else {
        collision.hull = None;
        String::new()
    };
    let item = &mut gallery.items[idx];
    item.root = Some(root);
    item.origin = origin;
    item.aabb = bounds.map(|b| b.translated(origin));
    item.stats = stats;
    item.warnings = warnings;
    item.clips = clips;
    item.load_seconds = seconds;
    item.status = Status::Ready;
    item.has_hull = opts.physics && hull_min_y.is_some();
    item.hull_min_y = hull_min_y.unwrap_or(0.0);
    item.collision_note = collision_note;
    if opts.labels {
        let top = item.aabb.map_or(origin + Vec3::Y, |b| {
            Vec3::new(b.center().x, b.max[1] + 0.2, b.center().z)
        });
        let e = bridge
            .commands
            .spawn(WorldLabel::item(idx, top, item_label(item)))
            .id();
        item.label = Some(e);
    }
}

/// The text over an item: its label and the size of its world AABB
pub fn item_label(item: &Item) -> String {
    match &item.aabb {
        Some(b) => {
            let s = b.size();
            format!("{}\n{:.2} x {:.2} x {:.2} m", item.cfg.label, s.x, s.y, s.z)
        }
        None => format!("{}\n(no geometry)", item.cfg.label),
    }
}

/// Front arrows, AABBs, the origin axes and the ground grid
pub fn draw_gizmos(mut gizmos: Gizmos, gallery: Res<Gallery>, view: Res<crate::hud::View>) {
    gizmos.axes(Transform::IDENTITY, 1.0);
    if view.grid {
        let half = 20u32;
        gizmos.grid(
            Isometry3d::new(
                Vec3::Y * 0.002,
                Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            ),
            UVec2::splat(half * 2),
            Vec2::splat(1.0),
            Color::srgba(1.0, 1.0, 1.0, 0.10),
        );
    }
    for (i, item) in gallery.items.iter().enumerate() {
        let Some(b) = item.aabb else { continue };
        let selected = i == gallery.selected;
        if matches!(item.status, Status::Failed(_)) {
            gizmos.cube(
                Transform::from_translation(b.center()).with_scale(b.size().max(Vec3::splat(0.05))),
                Color::srgb(0.9, 0.1, 0.1),
            );
            continue;
        }
        if view.aabb {
            gizmos.cube(
                Transform::from_translation(b.center())
                    .with_scale(b.size().max(Vec3::splat(0.001))),
                if selected {
                    Color::srgb(1.0, 0.9, 0.2)
                } else {
                    Color::srgb(0.3, 0.9, 0.9)
                },
            );
        }
        // The item's own origin, and an arrow on the ground toward its +Z front, starting just past
        // the front of its AABB (the bridge's front)
        gizmos.axes(Transform::from_translation(item.origin), 0.3);
        let start = Vec3::new(item.origin.x, 0.02, b.max[2] + 0.25);
        gizmos.arrow(start, start + Vec3::Z * 0.8, Color::srgb(0.2, 0.4, 1.0));
    }
}

/// Starts or stops the clip the item is set to, and moves the camera-independent clip state
pub fn drive_clips(
    mut gallery: ResMut<Gallery>,
    mut players: Query<(&BridgeAnimations, &mut AnimationPlayer)>,
) {
    for item in &mut gallery.items {
        let (Some(root), false) = (item.root, item.clips.is_empty()) else {
            continue;
        };
        let Ok((clips, mut player)) = players.get_mut(root) else {
            continue;
        };
        let n = item.clips.len();
        let state = &mut item.clip;
        state.current %= n;
        let node = clips.clips()[state.current].node;
        if state.applied != Some(state.current) {
            player.stop_all();
            let a = player.play(node);
            if state.auto {
                a.set_repeat(bevy::animation::RepeatAnimation::Never);
            } else {
                a.repeat();
            }
            state.applied = Some(state.current);
        }
        if let Some(a) = player.animation_mut(node) {
            if state.paused != a.is_paused() {
                if state.paused {
                    a.pause();
                } else {
                    a.resume();
                }
            }
            if state.auto && a.is_finished() {
                state.current = (state.current + 1) % n;
            }
        }
    }
}
