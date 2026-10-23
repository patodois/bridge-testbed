//! War Thunder's own collision: the shapes of placed objects (`WarThunder::load_object_collision`,
//! `load_level_collision`) as a static trimesh plus capsules, in the item's space. The ground has no
//! collision entry of its own: the level scene's `terrain` model is the landmesh's collision mesh (and the
//! heightmap's grid where that is finer), so it is used as it is.

use bevy::math::Vec3;
use rewrite_bridge as rb;
use wt_unpack::level_collision::{LevelCollision, ObjectCollision};

use crate::analyze::{self, CollisionSource, Trimesh, MAX_TRIMESH_TRIANGLES};

fn capsules_of(o: &ObjectCollision) -> Vec<([Vec3; 2], f32)> {
    o.capsules
        .iter()
        .map(|c| ([Vec3::from(c.a), Vec3::from(c.b)], c.radius))
        .collect()
}

/// One object's collision (a model item)
pub fn object_trimesh(o: &ObjectCollision) -> Result<Trimesh, String> {
    let mut t = match &o.model {
        Some(m) => {
            let g = analyze::rest_geometry(m);
            Trimesh {
                vertices: g.vertices,
                triangles: g.triangles,
                capsules: Vec::new(),
                source: CollisionSource::ProviderCollision,
                detail: String::new(),
            }
        }
        None => Trimesh {
            vertices: Vec::new(),
            triangles: Vec::new(),
            capsules: Vec::new(),
            source: CollisionSource::ProviderCollision,
            detail: String::new(),
        },
    };
    t.capsules = capsules_of(o);
    if t.triangles.is_empty() && t.capsules.is_empty() {
        return Err(format!("{} has no collision shapes", o.resource));
    }
    t.detail = format!(
        "{} ({} triangles, {} capsules)",
        o.resource,
        t.triangles.len(),
        t.capsules.len()
    );
    Ok(t)
}

/// A level's collision: the terrain model, and the collision of every placed object at its instances
pub fn level_trimesh(scene: &rb::Scene, lc: &LevelCollision) -> Result<Trimesh, String> {
    struct Shapes {
        geometry: Option<analyze::Geometry>,
        capsules: Vec<([Vec3; 2], f32)>,
    }
    let shapes: Vec<Option<Shapes>> = scene
        .models
        .iter()
        .map(|sm| {
            let rb::SceneModel::Inline(m) = sm else {
                return None;
            };
            if m.name == "terrain" {
                return Some(Shapes {
                    geometry: Some(analyze::rest_geometry(m)),
                    capsules: Vec::new(),
                });
            }
            let o = lc.object(&m.name)?;
            Some(Shapes {
                geometry: o.model.as_ref().map(analyze::rest_geometry),
                capsules: capsules_of(o),
            })
        })
        .collect();
    let total: usize = scene
        .instances
        .iter()
        .filter(|i| analyze::placement_ok(&i.transform))
        .filter_map(|i| shapes.get(i.model)?.as_ref()?.geometry.as_ref())
        .map(|g| g.triangles.len())
        .sum();
    if total > MAX_TRIMESH_TRIANGLES {
        return Err(format!(
            "{total} triangles is over the testbed's trimesh limit of {MAX_TRIMESH_TRIANGLES}"
        ));
    }
    let mut out = Trimesh {
        vertices: Vec::new(),
        triangles: Vec::with_capacity(total),
        capsules: Vec::new(),
        source: CollisionSource::ProviderCollision,
        detail: String::new(),
    };
    for inst in scene
        .instances
        .iter()
        .filter(|i| analyze::placement_ok(&i.transform))
    {
        let Some(Some(s)) = shapes.get(inst.model) else {
            continue;
        };
        let m = analyze::instance_matrix(&inst.transform);
        if let Some(g) = &s.geometry {
            let base = out.vertices.len() as u32;
            out.vertices
                .extend(g.vertices.iter().map(|p| m.transform_point3(*p)));
            let mirrored = inst.transform.scale.iter().filter(|s| **s < 0.0).count() % 2 == 1;
            out.triangles.extend(g.triangles.iter().map(|t| {
                if mirrored {
                    [base + t[0], base + t[2], base + t[1]]
                } else {
                    [base + t[0], base + t[1], base + t[2]]
                }
            }));
        }
        let scale = inst
            .transform
            .scale
            .iter()
            .fold(0.0f32, |a, s| a.max(s.abs()));
        out.capsules.extend(
            s.capsules
                .iter()
                .map(|([a, b], r)| ([m.transform_point3(*a), m.transform_point3(*b)], r * scale)),
        );
    }
    if out.triangles.is_empty() && out.capsules.is_empty() {
        return Err("the level has no collision shapes".into());
    }
    out.detail = format!(
        "terrain model + {} placed objects with collision ({} without, {} failed), {} triangles, {} capsules",
        lc.objects.len(),
        lc.without_collision.len(),
        lc.failed.len(),
        out.triangles.len(),
        out.capsules.len()
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wt_unpack::level_collision::CapsuleShape;

    fn quad(name: &str) -> rb::Model {
        rb::Model {
            name: name.into(),
            nodes: vec![rb::Node::new("root", None)],
            meshes: vec![rb::Mesh {
                positions: vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [1.0, 0.0, 1.0],
                    [0.0, 0.0, 1.0],
                ],
                indices: vec![0, 2, 1, 0, 3, 2],
                ..rb::Mesh::default()
            }],
            materials: vec![rb::Material::default()],
            skins: vec![],
        }
    }

    #[test]
    fn a_level_collides_with_its_terrain_and_the_objects_collision_not_their_render_meshes() {
        let scene = rb::Scene {
            models: vec![
                rb::SceneModel::Inline(quad("terrain")),
                rb::SceneModel::Inline(quad("tree_render")),
                rb::SceneModel::Inline(quad("rock")),
            ],
            instances: vec![
                rb::Instance::new(0, rb::Transform::IDENTITY),
                rb::Instance::new(1, rb::Transform::from_translation([5.0, 0.0, 0.0])),
                rb::Instance::new(2, rb::Transform::from_translation([9.0, 0.0, 0.0])),
                // a stray placement is left out
                rb::Instance::new(2, rb::Transform::from_translation([1e11, 0.0, 0.0])),
            ],
            ..rb::Scene::default()
        };
        let lc = LevelCollision {
            level: "l".into(),
            objects: vec![
                ObjectCollision {
                    name: "tree_render".into(),
                    resource: "tree_render_collision".into(),
                    model: None,
                    capsules: vec![CapsuleShape {
                        name: "trunk".into(),
                        a: [0.0, 0.0, 0.0],
                        b: [0.0, 4.0, 0.0],
                        radius: 0.3,
                        materials: vec![],
                        behavior: 0,
                    }],
                    bbox: [[0.0; 3]; 2],
                },
                ObjectCollision {
                    name: "rock".into(),
                    resource: "rock_collision".into(),
                    model: Some(quad("rock_collision")),
                    capsules: vec![],
                    bbox: [[0.0; 3]; 2],
                },
            ],
            without_collision: vec!["debris".into()],
            failed: vec![],
        };
        let t = level_trimesh(&scene, &lc).unwrap();
        // terrain (2 triangles) + the rock's collision (2); the tree has only a capsule
        assert_eq!(t.triangles.len(), 4);
        assert_eq!(t.capsules.len(), 1);
        assert_eq!(t.capsules[0].0[1], Vec3::new(5.0, 4.0, 0.0));
        assert_eq!(t.source, CollisionSource::ProviderCollision);
        assert!(t.detail.contains("1 without"));
    }
}
