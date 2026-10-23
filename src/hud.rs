//! Text on screen: labels projected from world positions, the help and the selected item's panel.

use bevy::prelude::*;
use rewrite_bridge_bevy::BridgeAnimations;

use crate::camera::{FrameRequest, Orbit};
use crate::gallery::{item_label, Gallery, Status};

#[derive(Resource)]
pub struct View {
    pub aabb: bool,
    pub grid: bool,
    pub labels: bool,
    pub help: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            aabb: true,
            grid: true,
            labels: true,
            help: true,
        }
    }
}

/// Text that follows a point in the world
#[derive(Component)]
pub struct WorldLabel {
    pub world: Vec3,
    pub item: Option<usize>,
}

impl WorldLabel {
    pub fn fixed(world: Vec3, text: &str) -> impl Bundle {
        (
            WorldLabel { world, item: None },
            label_text(text.to_string(), Color::srgb(0.85, 0.85, 0.85)),
        )
    }

    pub fn item(idx: usize, world: Vec3, text: String) -> impl Bundle {
        (
            WorldLabel {
                world,
                item: Some(idx),
            },
            label_text(text, Color::WHITE),
        )
    }
}

fn label_text(text: String, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: 14.0.into(),
            ..default()
        },
        TextColor(color),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::all(Val::Px(3.0)),
            max_width: Val::Px(320.0),
            ..default()
        },
        // The point is the middle of the label's bottom edge
        UiTransform::from_translation(Val2::new(Val::Percent(-50.0), Val::Percent(-100.0))),
        Visibility::Hidden,
    )
}

pub fn update_labels(
    view: Res<View>,
    gallery: Res<Gallery>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut labels: Query<(&WorldLabel, &mut Node, &mut Visibility, &mut TextColor)>,
) {
    let Ok((camera, transform)) = camera.single() else {
        return;
    };
    for (label, mut node, mut visibility, mut color) in &mut labels {
        let point = camera.world_to_viewport(transform, label.world);
        match point {
            Ok(p) if view.labels => {
                node.left = Val::Px(p.x);
                node.top = Val::Px(p.y);
                *visibility = Visibility::Inherited;
            }
            _ => *visibility = Visibility::Hidden,
        }
        if let Some(i) = label.item {
            let failed = matches!(gallery.items[i].status, Status::Failed(_));
            color.0 = if failed {
                Color::srgb(1.0, 0.4, 0.4)
            } else if i == gallery.selected {
                Color::srgb(1.0, 0.92, 0.3)
            } else {
                Color::WHITE
            };
        }
    }
}

#[derive(Component)]
pub struct HudText;
#[derive(Component)]
pub struct PhysicsNote;

pub const PHYSICS_NOTE: &str = "PHYSICS: the bridge carries no physics data yet, so masses, gravity, friction and restitution are testbed assumptions. \
Avian 0.8; gravity 9.81 m/s2; mass = density x hull volume (1 kg per m3 by default); friction and restitution are Avian's defaults; \
dynamic props are convex hulls of the render mesh at rest. Static colliders are the game's own collision shapes where the provider \
offers them outside the bridge (Warframe, War Thunder) and otherwise a trimesh of the render meshes (an assumption: Hytale, Nocturne).";

pub fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        HudText,
        Text::new(""),
        TextFont {
            font_size: 14.0.into(),
            ..default()
        },
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(8.0),
            top: Val::Px(8.0),
            max_width: Val::Px(760.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
    ));
    commands.spawn((
        PhysicsNote,
        Text::new(PHYSICS_NOTE),
        TextFont {
            font_size: 12.0.into(),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.8, 0.5)),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(8.0),
            bottom: Val::Px(8.0),
            max_width: Val::Px(900.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
    ));
}

const HELP: &str = "LMB orbit | RMB/MMB pan | wheel zoom | WASD/QE move | Tab/Shift+Tab item | F frame item | Home frame all\n\
C/Shift+C clip | P pause | Z auto-advance clips | Space drop prop from 3 m | B stack | R reset | V AABBs | N grid | L labels | H help";

pub fn update_hud(
    view: Res<View>,
    gallery: Res<Gallery>,
    players: Query<(&BridgeAnimations, &AnimationPlayer)>,
    bodies: Query<(&avian3d::prelude::ComputedMass, &Transform)>,
    mut text: Query<&mut Text, With<HudText>>,
    mut note: Query<&mut Visibility, With<PhysicsNote>>,
) {
    for mut v in &mut note {
        *v = if view.help {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let mut s = String::new();
    if view.help {
        s.push_str(HELP);
        s.push('\n');
    }
    if let Some(l) = gallery.loading_label() {
        s.push_str(&format!("loading {l} ...\n"));
    } else if !gallery.done {
        s.push_str("starting ...\n");
    }
    s.push_str(&format!(
        "lighting (testbed): {}\n",
        crate::stage::LIGHTING_NOTE
    ));
    if let Some(item) = gallery.items.get(gallery.selected) {
        s.push_str(&format!(
            "\n[{}/{}] {}   {} {} \"{}\"\n",
            gallery.selected + 1,
            gallery.items.len(),
            item.cfg.label,
            item.cfg.provider,
            item.cfg.kind.as_str(),
            item.cfg.id
        ));
        match &item.status {
            Status::Pending => s.push_str("waiting to load\n"),
            Status::Loading => s.push_str("loading ...\n"),
            Status::Failed(e) => s.push_str(&format!("ERROR: {e}\n")),
            Status::Ready => {
                let st = &item.stats;
                s.push_str(&item_label(item).replace('\n', "   "));
                s.push('\n');
                s.push_str(&format!(
                    "origin ({:.2}, {:.2}, {:.2}) | {} nodes, {} meshes ({} skinned), {} materials, {} vertices, {} triangles, {:.1} MB of images | loaded in {:.1} s\n",
                    item.origin.x, item.origin.y, item.origin.z, st.nodes, st.meshes, st.skinned_meshes,
                    st.materials, st.vertices, st.triangles, st.image_megabytes, item.load_seconds
                ));
                if st.instances > 1 {
                    s.push_str(&format!(
                        "{} placements = {} triangles drawn\n",
                        st.instances, st.instanced_triangles
                    ));
                }
                if let (Some(a), Some(m), Some(r)) =
                    (st.mean_albedo, st.mean_metallic, st.mean_roughness)
                {
                    s.push_str(&format!(
                        "mean linear albedo {a:.3} | metallic factor {m:.2} | roughness factor {r:.2} | max emissive {:.2} | flipped {}/{}\n",
                        st.max_emissive, st.flipped_triangles, st.normal_checked_triangles
                    ));
                }
                if !item.clips.is_empty() {
                    let c = item.clip.current.min(item.clips.len() - 1);
                    let clip = &item.clips[c];
                    let (t, key) = item
                        .root
                        .and_then(|r| players.get(r).ok())
                        .and_then(|(clips, player)| {
                            let node = clips.clips().get(c)?.node;
                            let t = player.animation(node)?.seek_time();
                            let key = clip.keys.iter().filter(|k| **k <= t + 1e-4).count();
                            Some((t, key))
                        })
                        .unwrap_or((0.0, 0));
                    s.push_str(&format!(
                        "clip {}/{}: {} | {:.2} s long | t {:.2} s | keyframe {}/{} | {}\n",
                        c + 1,
                        item.clips.len(),
                        clip.name,
                        clip.duration_s,
                        t,
                        key,
                        clip.keys.len(),
                        if item.clip.paused {
                            "paused"
                        } else if item.clip.auto {
                            "playing once, then next"
                        } else {
                            "looping"
                        }
                    ));
                } else if item.cfg.kind == ItemKindAnimated {
                    s.push_str("no clips from this provider\n");
                }
                if let Some((mass, t)) = item
                    .root
                    .filter(|_| item.cfg.dynamic)
                    .and_then(|r| bodies.get(r).ok())
                {
                    s.push_str(&format!(
                        "body: mass {:.2} kg (density x hull volume, assumption) | position ({:.2}, {:.2}, {:.2})\n",
                        mass.value(), t.translation.x, t.translation.y, t.translation.z
                    ));
                }
                if !item.collision_note.is_empty() {
                    s.push_str(&format!("collision: {}\n", item.collision_note));
                }
                for w in &item.warnings {
                    s.push_str(&format!("! {w}\n"));
                }
            }
        }
    }
    text.0 = s;
}

#[allow(non_upper_case_globals)]
const ItemKindAnimated: crate::config::ItemKind = crate::config::ItemKind::Animated;

/// Keyboard: selection, view toggles, clips, framing
pub fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut gallery: ResMut<Gallery>,
    mut view: ResMut<View>,
    mut request: ResMut<FrameRequest>,
    mut orbit: ResMut<Orbit>,
) {
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let n = gallery.items.len();
    if n > 0 && keys.just_pressed(KeyCode::Tab) {
        gallery.selected = if shift {
            (gallery.selected + n - 1) % n
        } else {
            (gallery.selected + 1) % n
        };
    }
    if keys.just_pressed(KeyCode::KeyF) {
        if let Some(b) = gallery.items.get(gallery.selected).and_then(|i| i.aabb) {
            request.0 = Some(b);
        }
    }
    if keys.just_pressed(KeyCode::Home) {
        request.0 = crate::camera::all_bounds(&gallery);
        orbit.yaw = 0.45;
        orbit.pitch = 0.35;
    }
    if keys.just_pressed(KeyCode::KeyV) {
        view.aabb = !view.aabb;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        view.grid = !view.grid;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        view.labels = !view.labels;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        view.help = !view.help;
    }
    let sel = gallery.selected;
    if let Some(item) = gallery.items.get_mut(sel) {
        let nc = item.clips.len();
        if nc > 0 {
            if keys.just_pressed(KeyCode::KeyC) {
                item.clip.current = if shift {
                    (item.clip.current + nc - 1) % nc
                } else {
                    (item.clip.current + 1) % nc
                };
            }
            if keys.just_pressed(KeyCode::KeyP) {
                item.clip.paused = !item.clip.paused;
            }
            if keys.just_pressed(KeyCode::KeyZ) {
                item.clip.auto = !item.clip.auto;
                item.clip.applied = None;
            }
        }
    }
}
