//! Loading one item from its provider, off the main thread, and measuring it.
//!
//! One item loads at a time (the caller starts the next after the previous arrived and was spawned),
//! which keeps the peak memory near that of the largest single item.

use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::math::Vec3;
use rewrite_bridge as rb;
use rewrite_bridge_bevy::{ModelSource, Providers};

use crate::analyze::{self, Aabb, CollisionSource, Stats, Trimesh};
use crate::config::{ItemConfig, ItemKind};
use crate::providers::ProviderSet;

pub enum Payload {
    Model(Box<rb::AnimatedModel>),
    Scene(Box<rb::Scene>),
}

#[derive(Clone, Debug)]
pub struct ClipInfo {
    pub name: String,
    pub duration_s: f32,
    /// Distinct keyframe times, seconds
    pub keys: Vec<f32>,
}

#[derive(Default)]
pub struct Collision {
    /// Convex hull input (item space), for dynamic items
    pub hull: Option<Vec<Vec3>>,
    /// Static trimesh (item space)
    pub trimesh: Option<Trimesh>,
    /// Why a collider is missing or what it was made from, for the HUD
    pub note: Option<String>,
}

pub struct Loaded {
    pub payload: Payload,
    pub stats: Stats,
    /// Item space: the origin is the item's root, as the provider placed it
    pub bounds: Option<Aabb>,
    /// Things worth a look that do not stop the item from showing
    pub warnings: Vec<String>,
    pub clips: Vec<ClipInfo>,
    pub collision: Collision,
    pub seconds: f32,
}

/// A load in flight: the thread sends exactly one message
pub struct Pending(Mutex<Receiver<Result<Loaded, String>>>);

impl Pending {
    /// `None` while the load is still running
    pub fn poll(&self) -> Option<Result<Loaded, String>> {
        match self.0.lock().expect("lock").try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the loader thread died".into())),
        }
    }
}

/// Starts loading `item` on a thread
pub fn spawn_load(providers: Arc<ProviderSet>, item: ItemConfig) -> Pending {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name(format!("load {}", item.label))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                load_item(&providers, &item)
            }))
            .unwrap_or_else(|_| Err("the reader panicked while loading".to_string()));
            let _ = tx.send(result);
        })
        .expect("a thread");
    Pending(Mutex::new(rx))
}

/// Loads and measures an item (blocking)
pub fn load_item(providers: &ProviderSet, item: &ItemConfig) -> Result<Loaded, String> {
    let started = Instant::now();
    let provider = providers.get(&item.provider)?;
    let mut warnings = Vec::new();
    let mut loaded = match item.kind {
        ItemKind::Model | ItemKind::Animated => {
            let am = if item.kind == ItemKind::Model {
                provider.load(&item.id).map(rb::AnimatedModel::from)
            } else {
                provider.load_animated(&item.id)
            }
            .map_err(|e| format!("{e}"))?;
            am.validate().map_err(|e| format!("invalid: {e}"))?;
            let stats = analyze::model_stats(&am);
            let bounds = analyze::model_bounds(&am.model);
            if bounds.is_none() {
                warnings.push("no vertices".to_string());
            }
            let clips = am
                .animations
                .iter()
                .map(|a| ClipInfo {
                    name: a.name.clone(),
                    duration_s: a.duration_s,
                    keys: analyze::keyframe_times(a),
                })
                .collect();
            let mut collision = Collision::default();
            if item.dynamic {
                let pts = analyze::hull_points(&am.model);
                if pts.len() < 4 {
                    collision.note = Some("too few points for a convex hull".into());
                } else {
                    collision.hull = Some(pts);
                }
            }
            if item.collide {
                #[cfg(feature = "wt")]
                wt_object_collision(providers, item, &am.model.name, &mut collision);
            }
            if item.collide && collision.trimesh.is_none() {
                let g = analyze::rest_geometry(&am.model);
                if g.triangles.len() > analyze::MAX_TRIMESH_TRIANGLES {
                    collision.note = Some(format!(
                        "{} triangles is over the trimesh limit",
                        g.triangles.len()
                    ));
                } else {
                    collision.trimesh = Some(Trimesh {
                        vertices: g.vertices,
                        triangles: g.triangles,
                        capsules: Vec::new(),
                        source: CollisionSource::RenderMeshes,
                        detail: String::new(),
                    });
                }
            }
            Loaded {
                payload: Payload::Model(Box::new(am)),
                stats,
                bounds,
                warnings: Vec::new(),
                clips,
                collision,
                seconds: 0.0,
            }
        }
        ItemKind::Scene => {
            let scene = provider.load_scene(&item.id).map_err(|e| format!("{e}"))?;
            let scene = resolve(scene, provider.as_ref(), providers, &mut warnings);
            scene.validate().map_err(|e| format!("invalid: {e}"))?;
            let (stats, bounds) = analyze::scene_stats(&scene);
            if bounds.is_none() {
                warnings.push("no placed geometry".to_string());
            }
            let mut collision = Collision::default();
            if let Some(cid) = &item.collision_id {
                match provider.load_scene(cid) {
                    Ok(cs) => {
                        let cs = resolve(cs, provider.as_ref(), providers, &mut warnings);
                        match analyze::scene_trimesh(&cs, CollisionSource::ProviderCollision) {
                            Ok(t) => collision.trimesh = Some(t),
                            Err(e) => {
                                collision.note = Some(format!("provider collision {cid}: {e}"))
                            }
                        }
                    }
                    Err(e) => {
                        collision.note = Some(format!("provider collision {cid} did not load: {e}"))
                    }
                }
            }
            #[cfg(feature = "wt")]
            if collision.trimesh.is_none() {
                wt_level_collision(providers, item, &scene, &mut collision);
            }
            if collision.trimesh.is_none() {
                match analyze::scene_trimesh(&scene, CollisionSource::RenderMeshes) {
                    Ok(t) => collision.trimesh = Some(t),
                    Err(e) => {
                        collision.note = Some(match collision.note.take() {
                            Some(n) => format!("{n}; render meshes: {e}"),
                            None => format!("render meshes: {e}"),
                        })
                    }
                }
            }
            Loaded {
                payload: Payload::Scene(Box::new(scene)),
                stats,
                bounds,
                warnings: Vec::new(),
                clips: Vec::new(),
                collision,
                seconds: 0.0,
            }
        }
    };
    if let (Some(n), true) = (
        no_collision_note(&item.provider),
        item.dynamic || item.collide || item.kind == ItemKind::Scene,
    ) {
        loaded.collision.note = Some(match loaded.collision.note.take() {
            Some(x) => format!("{x}; {n}"),
            None => n.to_string(),
        });
    }
    // Hints, not verdicts: a person looks at the picture
    let s = &loaded.stats;
    if !s.stray_models.is_empty() {
        warnings.push(format!(
            "{} models have vertices beyond {} km (provider data), left out of the bounds: {}",
            s.stray_models.len(),
            analyze::MAX_PLACEMENT_M / 1000.0,
            s.stray_models.join(", ")
        ));
    }
    if s.stray_placements > 0 {
        warnings.push(format!(
            "{} placements are farther than {} km from the origin; they are left out of the bounds and the colliders",
            s.stray_placements,
            analyze::MAX_PLACEMENT_M / 1000.0
        ));
    }
    if s.normal_checked_triangles > 0 && s.flipped_triangles * 100 > s.normal_checked_triangles {
        warnings.push(format!(
            "{} of {} triangles wind against their normals",
            s.flipped_triangles, s.normal_checked_triangles
        ));
    }
    if let Some(a) = s.mean_albedo {
        if a < 0.03 {
            warnings.push(format!(
                "very dark base colour (mean linear luminance {a:.3})"
            ));
        } else if a > 0.8 {
            warnings.push(format!(
                "very bright base colour (mean linear luminance {a:.3})"
            ));
        }
    }
    loaded.warnings = warnings;
    loaded.seconds = started.elapsed().as_secs_f32();
    Ok(loaded)
}

/// Providers whose bridge carries no collision data: the static collider of their items is a trimesh of the
/// render meshes (a testbed assumption), and the panel says so
fn no_collision_note(provider: &str) -> Option<&'static str> {
    match provider {
        "nocturne" => Some("Nocturne has no collision data in the bridge"),
        "mc" => Some("Minecraft has no collision data in this provider (block shapes are in the game's code)"),
        _ => None,
    }
}

/// Turns a scene's asset references into inline models, loading each distinct reference once (a
/// reference that fails stays a reference and is reported; the spawn then skips its instances)
fn resolve(
    mut scene: rb::Scene,
    local: &dyn rb::Provider,
    providers: &ProviderSet,
    warnings: &mut Vec<String>,
) -> rb::Scene {
    let opened = providers.opened();
    let mut source = Providers::new(local);
    for p in opened {
        if p.game() != local.game() {
            source = source.with(p);
        }
    }
    let mut loaded: Vec<(rb::AssetRef, rb::Model)> = Vec::new();
    for sm in &mut scene.models {
        let rb::SceneModel::Asset(r) = sm else {
            continue;
        };
        if let Some((_, m)) = loaded.iter().find(|(k, _)| k == r) {
            *sm = rb::SceneModel::Inline(m.clone());
            continue;
        }
        match source.load_model(r) {
            Ok(m) => {
                loaded.push((r.clone(), m.clone()));
                *sm = rb::SceneModel::Inline(m);
            }
            Err(e) => warnings.push(format!("model {:?} did not load: {e}", r.asset)),
        }
    }
    scene
}

/// A War Thunder object's own collision for a model item: `collision_id` names the object, else the model's
/// name is tried. Vehicles have none, and the item then falls back to its render meshes.
#[cfg(feature = "wt")]
fn wt_object_collision(
    providers: &ProviderSet,
    item: &ItemConfig,
    model_name: &str,
    collision: &mut Collision,
) {
    let Some(wt) = providers.wt(&item.provider) else {
        return;
    };
    let name = item
        .collision_id
        .clone()
        .unwrap_or_else(|| item.id.rsplit(':').next().unwrap_or(model_name).to_string());
    match wt.load_object_collision(&name) {
        Ok(Some(o)) => match crate::wt_collision::object_trimesh(&o) {
            Ok(t) => collision.trimesh = Some(t),
            Err(e) => collision.note = Some(e),
        },
        Ok(None) => {
            collision.note = Some(format!(
            "the game has no collision resource {name}_collision (War Thunder vehicles have none)"
        ))
        }
        Err(e) => collision.note = Some(format!("collision of {name}: {e:#}")),
    }
}

/// A War Thunder level's own collision (`load_level_collision`) for a scene item
#[cfg(feature = "wt")]
fn wt_level_collision(
    providers: &ProviderSet,
    item: &ItemConfig,
    scene: &rb::Scene,
    collision: &mut Collision,
) {
    let Some(wt) = providers.wt(&item.provider) else {
        return;
    };
    match wt
        .load_level_collision(&item.id)
        .map_err(|e| format!("{e:#}"))
        .and_then(|lc| crate::wt_collision::level_trimesh(scene, &lc))
    {
        Ok(t) => collision.trimesh = Some(t),
        Err(e) => collision.note = Some(format!("the level's own collision: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ItemConfig;
    use rb::{AssetInfo, AssetKind, Instance, Node, Provider, Transform};

    fn cube(name: &str, size: f32) -> rb::Model {
        let h = size / 2.0;
        let corners = [
            [-h, 0.0, -h],
            [h, 0.0, -h],
            [h, size, -h],
            [-h, size, -h],
            [-h, 0.0, h],
            [h, 0.0, h],
            [h, size, h],
            [-h, size, h],
        ];
        let indices = vec![
            4, 5, 6, 4, 6, 7, 1, 0, 3, 1, 3, 2, 5, 1, 2, 5, 2, 6, 0, 4, 7, 0, 7, 3, 7, 6, 2, 7, 2,
            3, 0, 1, 5, 0, 5, 4,
        ];
        rb::Model {
            name: name.into(),
            nodes: vec![Node::new("root", None)],
            meshes: vec![rb::Mesh {
                name: "cube".into(),
                positions: corners.to_vec(),
                indices,
                ..rb::Mesh::default()
            }],
            materials: vec![rb::Material {
                metallic: 0.0,
                ..rb::Material::default()
            }],
            skins: vec![],
        }
    }

    /// A game with a crate, a waving crate, a yard of two crates and one asset that is broken
    struct Fake;

    impl Provider for Fake {
        fn game(&self) -> &str {
            "Fake"
        }
        fn list(&self, _: &str) -> Result<Vec<AssetInfo>, rb::BoxError> {
            Ok(vec![AssetInfo {
                id: "crate".into(),
                name: "crate".into(),
                kind: AssetKind::Prop,
            }])
        }
        fn load(&self, id: &str) -> Result<rb::Model, rb::BoxError> {
            match id {
                "crate" | "wave" => Ok(cube("crate", 1.0)),
                "broken" => {
                    let mut m = cube("broken", 1.0);
                    m.meshes[0].indices[0] = 99;
                    Ok(m)
                }
                other => Err(format!("no asset {other}").into()),
            }
        }
        fn load_animated(&self, id: &str) -> Result<rb::AnimatedModel, rb::BoxError> {
            let mut am = rb::AnimatedModel::from(self.load(id)?);
            if id == "wave" {
                am.animations.push(rb::Animation {
                    name: "bob".into(),
                    duration_s: 1.0,
                    channels: vec![rb::Channel::new(
                        0,
                        rb::Target::Translation,
                        rb::Interpolation::Linear,
                        vec![0.0, 0.5, 1.0],
                        vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.0, 0.0, 0.0, 0.0],
                    )],
                });
            }
            Ok(am)
        }
        fn load_scene(&self, _: &str) -> Result<rb::Scene, rb::BoxError> {
            Ok(rb::Scene {
                name: "yard".into(),
                models: vec![
                    rb::SceneModel::Asset(rb::AssetRef::local("crate")),
                    rb::SceneModel::Asset(rb::AssetRef::local("nope")),
                ],
                instances: vec![
                    Instance::new(0, Transform::IDENTITY),
                    Instance::new(0, Transform::from_translation([3.0, 0.0, 0.0])),
                ],
                ..rb::Scene::default()
            })
        }
    }

    fn set() -> ProviderSet {
        ProviderSet::with_providers(vec![("wt", std::sync::Arc::new(Fake))])
    }

    fn item(kind: ItemKind, id: &str, dynamic: bool) -> ItemConfig {
        ItemConfig {
            provider: "wt".into(),
            id: id.into(),
            kind,
            label: id.into(),
            dynamic,
            collide: false,
            collision_id: None,
            at: None,
        }
    }

    #[test]
    fn a_model_gets_bounds_stats_and_a_hull() {
        let l = load_item(&set(), &item(ItemKind::Model, "crate", true))
            .ok()
            .unwrap();
        let b = l.bounds.unwrap();
        assert_eq!(b.size(), Vec3::ONE);
        assert_eq!(b.min[1], 0.0);
        assert_eq!((l.stats.vertices, l.stats.triangles), (8, 12));
        assert_eq!(l.collision.hull.as_ref().map(|h| h.len()), Some(8));
        assert!(l.collision.trimesh.is_none());
    }

    #[test]
    fn clips_come_with_keyframe_times() {
        let l = load_item(&set(), &item(ItemKind::Animated, "wave", false))
            .ok()
            .unwrap();
        assert_eq!(l.clips.len(), 1);
        assert_eq!(l.clips[0].keys, vec![0.0, 0.5, 1.0]);
        assert_eq!(l.stats.clips, 1);
    }

    #[test]
    fn a_scene_resolves_its_references_and_reports_the_one_that_fails() {
        let l = load_item(&set(), &item(ItemKind::Scene, "yard", false))
            .ok()
            .unwrap();
        assert_eq!(l.stats.instances, 2);
        assert_eq!(l.bounds.unwrap().size().x, 4.0);
        assert!(l.warnings.iter().any(|w| w.contains("nope")));
        // scenes always get a static trimesh of their render meshes
        let t = l.collision.trimesh.unwrap();
        assert_eq!(t.triangles.len(), 24);
        assert_eq!(t.source, CollisionSource::RenderMeshes);
    }

    #[test]
    fn failures_are_text_not_panics() {
        let s = set();
        let missing = |l: Result<Loaded, String>| l.err().unwrap();
        assert!(
            missing(load_item(&s, &item(ItemKind::Model, "nope", false))).contains("no asset nope")
        );
        assert!(
            missing(load_item(&s, &item(ItemKind::Model, "broken", false))).starts_with("invalid:")
        );
        let mut other = item(ItemKind::Model, "crate", false);
        other.provider = "hytale".into();
        assert!(missing(load_item(&s, &other)).contains("no [providers.hytale]"));
    }

    #[test]
    fn providers_without_collision_data_say_so() {
        assert!(no_collision_note("nocturne")
            .unwrap()
            .contains("no collision data"));
        assert!(no_collision_note("mc")
            .unwrap()
            .contains("no collision data"));
        assert!(no_collision_note("hytale").is_none());
        assert!(no_collision_note("wt").is_none());
    }

    #[test]
    fn a_provider_that_cannot_open_says_why() {
        let mut config = std::collections::BTreeMap::new();
        config.insert("wt".to_string(), crate::config::ProviderConfig::default());
        let s = ProviderSet::new(&config);
        let e = s.get("wt").err().unwrap();
        assert!(
            e.contains("failed to open") || e.contains("no `wt` provider"),
            "{e}"
        );
    }
}
