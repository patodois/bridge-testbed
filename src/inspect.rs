//! `bridge-testbed inspect`: the numbers of every item as text, without rendering. It is what the
//! cross-game comparison is made from: sizes, counts, material statistics and, per clip, how far the
//! root of the skeleton travels (root motion).

use anyhow::Result;
use bevy::math::Vec3;
use rewrite_bridge as rb;

use crate::config::{Config, ItemKind};
use crate::loader::{load_item, Payload};
use crate::providers::ProviderSet;

/// How far a clip moves the first translated node (the skeleton's root, as far as the clip says):
/// (node name, last key minus first key)
pub fn root_motion(am: &rb::AnimatedModel, clip: &rb::Animation) -> Option<(String, Vec3)> {
    let ch = clip
        .channels
        .iter()
        .filter(|c| c.target == rb::Target::Translation && c.times.len() >= 2)
        .min_by_key(|c| c.node)?;
    // Cubic splines hold (in-tangent, value, out-tangent) per key
    let stride = if ch.interpolation == rb::Interpolation::CubicSpline {
        9
    } else {
        3
    };
    let offset = if ch.interpolation == rb::Interpolation::CubicSpline {
        3
    } else {
        0
    };
    let at = |k: usize| {
        let i = k * stride + offset;
        Vec3::new(ch.values[i], ch.values[i + 1], ch.values[i + 2])
    };
    let delta = at(ch.times.len() - 1) - at(0);
    Some((am.model.nodes.get(ch.node)?.name.clone(), delta))
}

pub fn run(config: &Config, only: Option<&str>, max_clips: usize) -> Result<()> {
    let providers = ProviderSet::new(&config.providers);
    for item in &config.items {
        if only.is_some_and(|o| o != item.label) {
            continue;
        }
        println!(
            "== {} ({} {} {})",
            item.label,
            item.provider,
            item.kind.as_str(),
            item.id
        );
        let loaded = match load_item(&providers, item) {
            Ok(l) => l,
            Err(e) => {
                println!("   ERROR: {e}");
                continue;
            }
        };
        let s = &loaded.stats;
        if let Some(b) = loaded.bounds {
            let z = b.size();
            println!(
                "   AABB min {:?} max {:?}\n   size {:.3} x {:.3} x {:.3} m (X Y Z); height {:.3}",
                b.min, b.max, z.x, z.y, z.z, z.y
            );
        }
        println!(
            "   {} nodes, {} meshes ({} skinned), {} materials, {} vertices, {} triangles, {} placements, {:.1} MB images",
            s.nodes, s.meshes, s.skinned_meshes, s.materials, s.vertices, s.triangles, s.instances, s.image_megabytes
        );
        if let Some(sky) = &s.sky {
            println!("   sky (the provider's, not drawn by the testbed): {sky}");
        }
        println!(
            "   albedo {:?}, metallic {:?}, roughness {:?}, max emissive {:.2}, flipped {}/{}, double-sided materials {}, blended {}",
            s.mean_albedo, s.mean_metallic, s.mean_roughness, s.max_emissive, s.flipped_triangles,
            s.normal_checked_triangles, s.double_sided_materials, s.blended_materials
        );
        if let Some(c) = &loaded.collision.trimesh {
            let source = match c.source {
                crate::analyze::CollisionSource::ProviderCollision => "the game's own collision",
                crate::analyze::CollisionSource::RenderMeshes => {
                    "render meshes (testbed assumption)"
                }
            };
            println!(
                "   static collider: {source}: {} triangles, {} capsules {}",
                c.triangles.len(),
                c.capsules.len(),
                c.detail
            );
        }
        if let Some(n) = &loaded.collision.note {
            println!("   collision note: {n}");
        }
        for w in &loaded.warnings {
            println!("   ! {w}");
        }
        if let Payload::Scene(scene) = &loaded.payload {
            for m in &scene.models {
                if let rb::SceneModel::Inline(m) = m {
                    if let Some(b) = crate::analyze::model_bounds(m) {
                        let s = b.size();
                        if s.max_element() > crate::analyze::MAX_PLACEMENT_M {
                            let far = m
                                .meshes
                                .iter()
                                .flat_map(|x| &x.positions)
                                .filter(|p| {
                                    p.iter().any(|c| c.abs() > crate::analyze::MAX_PLACEMENT_M)
                                })
                                .count();
                            println!(
                                "   model {:?} is {:.3e} m wide: {far} of its vertices are beyond {} km",
                                m.name,
                                s.max_element(),
                                crate::analyze::MAX_PLACEMENT_M / 1000.0
                            );
                        }
                    }
                }
            }
        }
        if let (ItemKind::Animated, Payload::Model(am)) = (item.kind, &loaded.payload) {
            println!(
                "   {} clips, {} sockets",
                am.animations.len(),
                am.sockets.len()
            );
            for clip in am.animations.iter().take(max_clips) {
                let motion =
                    root_motion(am, clip).map_or("no translation".to_string(), |(n, d)| {
                        format!(
                            "root {n:?} moves ({:.2}, {:.2}, {:.2}) = {:.2} m",
                            d.x,
                            d.y,
                            d.z,
                            d.length()
                        )
                    });
                println!(
                    "     {:<60} {:>6.2} s  {:>3} channels  {}",
                    clip.name,
                    clip.duration_s,
                    clip.channels.len(),
                    motion
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_motion_is_last_key_minus_first() {
        let mut am = rb::AnimatedModel::from(rb::Model {
            name: "m".into(),
            nodes: vec![rb::Node::new("root", None)],
            ..rb::Model::default()
        });
        am.animations.push(rb::Animation {
            name: "walk".into(),
            duration_s: 1.0,
            channels: vec![rb::Channel::new(
                0,
                rb::Target::Translation,
                rb::Interpolation::Linear,
                vec![0.0, 1.0],
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 2.0],
            )],
        });
        let (n, d) = root_motion(&am, &am.animations[0]).unwrap();
        assert_eq!(n, "root");
        assert_eq!(d, Vec3::new(0.0, 0.0, 2.0));
    }
}
