//! Numbers measured on what a provider returned: bounds, counts, and a few checks that make
//! cross-game inconsistencies visible (albedo brightness, triangle winding against stored normals).
//! Nothing here changes a model.

use std::collections::HashSet;

use bevy::math::{Mat4, Vec3};
use rewrite_bridge as rb;
use rewrite_bridge_bevy::{rest_globals, rest_positions};
use serde::Serialize;

/// Most triangles a static trimesh collider may have. Over this the collider is skipped and the UI says so.
pub const MAX_TRIMESH_TRIANGLES: usize = 1_500_000;

/// World-space axis-aligned box in metres
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self {
            min: min.to_array(),
            max: max.to_array(),
        }
    }
    pub fn min(&self) -> Vec3 {
        Vec3::from(self.min)
    }
    pub fn max(&self) -> Vec3 {
        Vec3::from(self.max)
    }
    pub fn size(&self) -> Vec3 {
        self.max() - self.min()
    }
    pub fn center(&self) -> Vec3 {
        (self.min() + self.max()) * 0.5
    }
    pub fn translated(&self, by: Vec3) -> Self {
        Self::new(self.min() + by, self.max() + by)
    }
    pub fn union(self, other: Aabb) -> Aabb {
        Aabb::new(self.min().min(other.min()), self.max().max(other.max()))
    }
    /// The box of this box after `m` (the box of its eight corners)
    pub fn transformed(&self, m: &Mat4) -> Aabb {
        let (lo, hi) = (self.min(), self.max());
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let p = m.transform_point3(c);
            min = min.min(p);
            max = max.max(p);
        }
        Aabb::new(min, max)
    }
}

/// What the manifest and the HUD say about an item
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Stats {
    pub nodes: usize,
    pub meshes: usize,
    pub skinned_meshes: usize,
    pub materials: usize,
    pub skins: usize,
    /// Distinct vertices and triangles of the models (a scene's models once each)
    pub vertices: usize,
    pub triangles: usize,
    /// Scenes: placements, and the triangles they add up to
    pub instances: usize,
    pub instanced_triangles: usize,
    pub clips: usize,
    pub sockets: usize,
    pub image_megabytes: f32,
    /// Triangle-weighted mean of the linear luminance of the base colour (texture times factor);
    /// 0 is black, 1 is white. `None` when nothing has a colour to sample.
    pub mean_albedo: Option<f32>,
    pub mean_metallic: Option<f32>,
    pub mean_roughness: Option<f32>,
    /// Largest emissive factor times strength over the materials
    pub max_emissive: f32,
    /// Triangles whose winding normal points against the mean of their stored vertex normals, and the
    /// number of triangles that could be checked (those of meshes with normals)
    pub flipped_triangles: usize,
    pub normal_checked_triangles: usize,
    pub double_sided_materials: usize,
    pub blended_materials: usize,
}

impl Stats {
    pub fn add_model(&mut self, m: &rb::Model, weight_instances: usize) {
        self.nodes += m.nodes.len();
        self.meshes += m.meshes.len();
        self.skinned_meshes += m.meshes.iter().filter(|x| x.skin.is_some()).count();
        self.materials += m.materials.len();
        self.skins += m.skins.len();
        let tris: usize = m.meshes.iter().map(|x| x.indices.len() / 3).sum();
        self.vertices += m.meshes.iter().map(|x| x.positions.len()).sum::<usize>();
        self.triangles += tris;
        self.instanced_triangles += tris * weight_instances;
        let bytes: usize = m
            .materials
            .iter()
            .flat_map(|x| [&x.base_color, &x.metallic_roughness, &x.normal, &x.emissive])
            .flatten()
            .map(|i| i.rgba.len())
            .sum();
        self.image_megabytes += bytes as f32 / 1e6;
        self.double_sided_materials += m.materials.iter().filter(|x| x.double_sided).count();
        self.blended_materials += m
            .materials
            .iter()
            .filter(|x| x.alpha == rb::AlphaMode::Blend)
            .count();
        for (flipped, checked) in m.meshes.iter().map(flipped_triangles) {
            self.flipped_triangles += flipped;
            self.normal_checked_triangles += checked;
        }
    }
}

/// Triangle-weighted material averages of a model: (albedo luminance, metallic, roughness, weight)
fn material_means(m: &rb::Model, emissive_strength: &[f32]) -> (f64, f64, f64, f64, f32) {
    let mut weight_per_material = vec![0.0f64; m.materials.len()];
    for mesh in &m.meshes {
        if let Some(w) = weight_per_material.get_mut(mesh.material) {
            *w += (mesh.indices.len() / 3) as f64;
        }
    }
    let (mut albedo, mut metal, mut rough, mut total) = (0.0, 0.0, 0.0, 0.0);
    let mut max_emissive = 0.0f32;
    for (i, (mat, w)) in m.materials.iter().zip(&weight_per_material).enumerate() {
        let strength = emissive_strength.get(i).copied().unwrap_or(1.0);
        let e = mat.emissive_factor.iter().cloned().fold(0.0f32, f32::max) * strength;
        if mat.emissive.is_some() || e > 0.0 {
            max_emissive = max_emissive.max(e);
        }
        if *w == 0.0 {
            continue;
        }
        let tex = mat
            .base_color
            .as_ref()
            .map_or([1.0f64; 3], mean_linear_colour);
        let f = mat.base_color_factor;
        let lum = 0.2126 * tex[0] * f64::from(f[0])
            + 0.7152 * tex[1] * f64::from(f[1])
            + 0.0722 * tex[2] * f64::from(f[2]);
        albedo += lum * w;
        metal += f64::from(mat.metallic) * w;
        rough += f64::from(mat.roughness) * w;
        total += w;
    }
    (albedo, metal, rough, total, max_emissive)
}

fn srgb_to_linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Mean linear RGB of an image's opaque-ish texels, sampled on a grid of about 4096 texels
fn mean_linear_colour(img: &rb::Image) -> [f64; 3] {
    let n = (img.width as usize) * (img.height as usize);
    if n == 0 || img.rgba.len() < n * 4 {
        return [1.0; 3];
    }
    let step = (n / 4096).max(1);
    let mut sum = [0.0f64; 3];
    let mut count = 0.0;
    for i in (0..n).step_by(step) {
        let p = &img.rgba[i * 4..i * 4 + 4];
        // Transparent texels are not part of the surface that shows
        if p[3] < 128 {
            continue;
        }
        for k in 0..3 {
            sum[k] += srgb_to_linear(p[k]);
        }
        count += 1.0;
    }
    if count == 0.0 {
        return [1.0; 3];
    }
    sum.map(|s| s / count)
}

/// (triangles whose winding disagrees with their stored normals, triangles checked)
pub fn flipped_triangles(mesh: &rb::Mesh) -> (usize, usize) {
    if mesh.normals.is_empty() {
        return (0, 0);
    }
    let (mut flipped, mut checked) = (0, 0);
    for t in mesh.indices.as_chunks::<3>().0 {
        let p = |i: u32| Vec3::from(mesh.positions[i as usize]);
        let n = |i: u32| Vec3::from(mesh.normals[i as usize]);
        let face = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
        let stored = n(t[0]) + n(t[1]) + n(t[2]);
        if face.length_squared() < 1e-14 || stored.length_squared() < 1e-6 {
            continue;
        }
        checked += 1;
        if face.dot(stored) < 0.0 {
            flipped += 1;
        }
    }
    (flipped, checked)
}

/// Stats of one animated model
pub fn model_stats(am: &rb::AnimatedModel) -> Stats {
    let mut s = Stats::default();
    s.add_model(&am.model, 1);
    s.instances = 1;
    s.clips = am.animations.len();
    s.sockets = am.sockets.len();
    let (a, m, r, w, e) = material_means(&am.model, &am.emissive_strength);
    if w > 0.0 {
        s.mean_albedo = Some((a / w) as f32);
        s.mean_metallic = Some((m / w) as f32);
        s.mean_roughness = Some((r / w) as f32);
    }
    s.max_emissive = e;
    s
}

/// Model-space bounds at rest, in the bridge's axes
pub fn model_bounds(m: &rb::Model) -> Option<Aabb> {
    rewrite_bridge_bevy::rest_bounds(m).map(|(lo, hi)| Aabb::new(lo, hi))
}

pub fn instance_matrix(t: &rb::Transform) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::from(t.scale),
        bevy::math::Quat::from_array(t.rotation).normalize(),
        Vec3::from(t.translation),
    )
}

/// Stats and bounds of a scene whose models are all inline (asset references that did not load
/// count nothing). `None` bounds when no instance has geometry.
pub fn scene_stats(scene: &rb::Scene) -> (Stats, Option<Aabb>) {
    let mut s = Stats::default();
    let mut uses = vec![0usize; scene.models.len()];
    for i in &scene.instances {
        if let Some(u) = uses.get_mut(i.model) {
            *u += 1;
        }
    }
    let (mut a, mut m, mut r, mut w, mut e) = (0.0, 0.0, 0.0, 0.0, 0.0f32);
    let mut bounds: Option<Aabb> = None;
    let per_model: Vec<Option<Aabb>> = scene
        .models
        .iter()
        .map(|sm| match sm {
            rb::SceneModel::Inline(model) => model_bounds(model),
            _ => None,
        })
        .collect();
    for (idx, sm) in scene.models.iter().enumerate() {
        if let rb::SceneModel::Inline(model) = sm {
            s.add_model(model, uses[idx]);
            let (ma, mm, mr, mw, me) = material_means(model, &[]);
            // Weight by placements so a forest counts its trees
            let k = uses[idx] as f64;
            a += ma * k;
            m += mm * k;
            r += mr * k;
            w += mw * k;
            e = e.max(me);
        }
    }
    for inst in &scene.instances {
        s.instances += 1;
        if let Some(Some(b)) = per_model.get(inst.model) {
            let b = b.transformed(&instance_matrix(&inst.transform));
            bounds = Some(bounds.map_or(b, |x| x.union(b)));
        }
    }
    if w > 0.0 {
        s.mean_albedo = Some((a / w) as f32);
        s.mean_metallic = Some((m / w) as f32);
        s.mean_roughness = Some((r / w) as f32);
    }
    s.max_emissive = e;
    (s, bounds)
}

/// A model's triangles with every node at rest, in model space
pub struct Geometry {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
}

pub fn rest_geometry(model: &rb::Model) -> Geometry {
    let globals = rest_globals(model);
    let mut g = Geometry {
        vertices: Vec::new(),
        triangles: Vec::new(),
    };
    for mesh in &model.meshes {
        let base = g.vertices.len() as u32;
        g.vertices.extend(rest_positions(model, mesh, &globals));
        g.triangles.extend(
            mesh.indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| [base + t[0], base + t[1], base + t[2]]),
        );
    }
    g
}

/// The distinct points of a model at rest, snapped to a grid so a convex hull of a big mesh stays cheap
pub fn hull_points(model: &rb::Model) -> Vec<Vec3> {
    let geometry = rest_geometry(model);
    let mut grid = 0.01f32;
    loop {
        let mut seen = HashSet::new();
        let pts: Vec<Vec3> = geometry
            .vertices
            .iter()
            .filter(|p| seen.insert((*p / grid).round().as_ivec3()))
            .copied()
            .collect();
        if pts.len() <= 20_000 || grid > 1.0 {
            return pts;
        }
        grid *= 2.0;
    }
}

/// Where the static collider comes from
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionSource {
    /// The render meshes, as a trimesh: a testbed choice
    RenderMeshes,
    /// Geometry the provider gave as the game's own collision (a `collision_id` scene)
    ProviderCollision,
}

pub struct Trimesh {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub source: CollisionSource,
}

/// A scene's placed triangles as one trimesh, or the reason there is none
pub fn scene_trimesh(scene: &rb::Scene, source: CollisionSource) -> Result<Trimesh, String> {
    let geoms: Vec<Option<Geometry>> = scene
        .models
        .iter()
        .map(|sm| match sm {
            rb::SceneModel::Inline(m) => Some(rest_geometry(m)),
            _ => None,
        })
        .collect();
    let total: usize = scene
        .instances
        .iter()
        .filter_map(|i| geoms.get(i.model).and_then(|g| g.as_ref()))
        .map(|g| g.triangles.len())
        .sum();
    if total == 0 {
        return Err("no triangles to collide with".into());
    }
    if total > MAX_TRIMESH_TRIANGLES {
        return Err(format!(
            "{total} triangles is over the testbed's trimesh limit of {MAX_TRIMESH_TRIANGLES}"
        ));
    }
    let mut out = Trimesh {
        vertices: Vec::new(),
        triangles: Vec::with_capacity(total),
        source,
    };
    for inst in &scene.instances {
        let Some(Some(g)) = geoms.get(inst.model) else {
            continue;
        };
        let m = instance_matrix(&inst.transform);
        let base = out.vertices.len() as u32;
        out.vertices
            .extend(g.vertices.iter().map(|p| m.transform_point3(*p)));
        // A mirrored placement flips the winding; a trimesh does not care, but keep it right
        let mirrored = inst.transform.scale.iter().filter(|s| **s < 0.0).count() % 2 == 1;
        out.triangles.extend(g.triangles.iter().map(|t| {
            if mirrored {
                [base + t[0], base + t[2], base + t[1]]
            } else {
                [base + t[0], base + t[1], base + t[2]]
            }
        }));
    }
    Ok(out)
}

/// The keyframe times of a clip (the union over its channels), sorted, in seconds
pub fn keyframe_times(a: &rb::Animation) -> Vec<f32> {
    let mut t: Vec<f32> = a
        .channels
        .iter()
        .flat_map(|c| c.times.iter().copied())
        .collect();
    t.sort_by(|x, y| x.total_cmp(y));
    t.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(flip: bool) -> rb::Model {
        let idx = if flip {
            vec![0, 2, 1, 0, 3, 2]
        } else {
            vec![0, 1, 2, 0, 2, 3]
        };
        rb::Model {
            name: "quad".into(),
            nodes: vec![rb::Node::new("root", None)],
            meshes: vec![rb::Mesh {
                positions: vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [0.0, 1.0, 0.0],
                ],
                normals: vec![[0.0, 0.0, 1.0]; 4],
                indices: idx,
                ..rb::Mesh::default()
            }],
            materials: vec![rb::Material::default()],
            skins: vec![],
        }
    }

    #[test]
    fn counts_flipped_winding() {
        assert_eq!(flipped_triangles(&quad(false).meshes[0]), (0, 2));
        assert_eq!(flipped_triangles(&quad(true).meshes[0]), (2, 2));
    }

    #[test]
    fn bounds_and_transform() {
        let b = model_bounds(&quad(false)).unwrap();
        assert_eq!(b.size(), Vec3::new(1.0, 1.0, 0.0));
        let t = b.transformed(&Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0)));
        assert_eq!(t.min().x, 2.0);
    }

    #[test]
    fn scene_trimesh_places_instances() {
        let scene = rb::Scene {
            models: vec![rb::SceneModel::Inline(quad(false))],
            instances: vec![
                rb::Instance::new(0, rb::Transform::IDENTITY),
                rb::Instance::new(0, rb::Transform::from_translation([5.0, 0.0, 0.0])),
            ],
            ..rb::Scene::default()
        };
        let t = scene_trimesh(&scene, CollisionSource::RenderMeshes).unwrap();
        assert_eq!(t.triangles.len(), 4);
        assert_eq!(t.vertices.len(), 8);
        let (s, b) = scene_stats(&scene);
        assert_eq!(s.instances, 2);
        assert_eq!(s.instanced_triangles, 4);
        assert_eq!(b.unwrap().max()[0], 6.0);
    }

    #[test]
    fn hull_points_are_deduplicated() {
        assert_eq!(hull_points(&quad(false)).len(), 4);
    }
}
