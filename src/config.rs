//! `testbed.toml`: which providers to open and which items to put in the gallery.
//!
//! The real file holds local paths to the owner's game installs, so it is untracked;
//! `testbed.example.toml` is the committed template.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// Names a provider can have in `[providers.<name>]` and in an item's `provider`
pub const PROVIDER_NAMES: [&str; 4] = ["wt", "wf", "hytale", "nocturne"];

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub gallery: GalleryConfig,
    #[serde(default)]
    pub physics: PhysicsConfig,
    /// What the stack key spawns
    pub stack: Option<StackConfig>,
    #[serde(default)]
    pub items: Vec<ItemConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    /// The game's install: `game` folder (wt), `Cache.Windows` (wf), `Assets.zip` (hytale, optional:
    /// the launcher's install is the default)
    pub path: Option<PathBuf>,
    /// Largest side of a baked texture (wf). Keeps big scenes small; absent means full size.
    #[cfg_attr(not(feature = "wf"), allow(dead_code))]
    pub max_texture: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GalleryConfig {
    /// Items per row; 0 puts all in one row
    pub columns: usize,
    /// Metres between neighbouring AABBs
    pub gap: f32,
    /// Move each item down or up so the bottom of its AABB touches the ground. Off by default: the
    /// bridge says nothing of where the origin lies, and a model floating or sunk shows it.
    pub snap_to_ground: bool,
    /// Spawn the lights a provider's scene carries (and nothing else). Off by default: the testbed
    /// lights everything with the same sun and ambient, which is the point of the comparison.
    pub scene_lights: bool,
}

impl Default for GalleryConfig {
    fn default() -> Self {
        Self {
            columns: 0,
            gap: 1.5,
            snap_to_ground: false,
            scene_lights: false,
        }
    }
}

/// Every value here is a testbed assumption: the bridge carries no physics data yet
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PhysicsConfig {
    /// Kilograms per cubic metre of hull volume (assumption)
    pub density: f32,
    /// Height of the lowest point of a dropped prop above the ground, in metres
    pub drop_height: f32,
    /// m/s², pointing down (assumption)
    pub gravity: f32,
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            density: 1.0,
            drop_height: 3.0,
            gravity: 9.81,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackConfig {
    pub provider: String,
    pub id: String,
    /// Blocks in the pyramid (3 + 2 + 1 = 6 by default)
    #[serde(default = "default_stack_count")]
    pub count: usize,
}

fn default_stack_count() -> usize {
    6
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    /// `Provider::load`
    #[default]
    Model,
    /// `Provider::load_animated`
    Animated,
    /// `Provider::load_scene`
    Scene,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Model => "model",
            ItemKind::Animated => "animated",
            ItemKind::Scene => "scene",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemConfig {
    pub provider: String,
    pub id: String,
    #[serde(default)]
    pub kind: ItemKind,
    pub label: String,
    /// A rigid body for the physics: a convex hull of the model, dropped with the drop key
    #[serde(default)]
    pub dynamic: bool,
    /// A static trimesh of the render meshes. Scenes always get one; models only with this.
    #[serde(default)]
    pub collide: bool,
    /// Scenes only: the id whose scene is the collision geometry, where the provider has real
    /// collision data (wf: the level path with `?collision`). Its meshes become the static collider
    /// instead of the render meshes.
    pub collision_id: Option<String>,
    /// Where to put the item (x, z in metres), instead of the next slot of the layout
    pub at: Option<[f32; 2]>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let config = Self::parse(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(config)
    }

    pub fn parse(text: &str) -> Result<Self> {
        let config: Config = toml::from_str(text)?;
        for name in config.providers.keys() {
            if !PROVIDER_NAMES.contains(&name.as_str()) {
                bail!(
                    "unknown provider {name:?}; the names are {}",
                    PROVIDER_NAMES.join(", ")
                );
            }
        }
        let mut labels = std::collections::HashSet::new();
        for item in &config.items {
            if !PROVIDER_NAMES.contains(&item.provider.as_str()) {
                bail!(
                    "item {:?}: unknown provider {:?}; the names are {}",
                    item.label,
                    item.provider,
                    PROVIDER_NAMES.join(", ")
                );
            }
            if item.label.is_empty()
                || item.label.contains(['/', '\\'])
                || item.label.starts_with('.')
            {
                bail!(
                    "item label {:?} must be a plain file name (it names the shot's PNG)",
                    item.label
                );
            }
            if !labels.insert(item.label.clone()) {
                bail!("two items are labelled {:?}", item.label);
            }
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_example() {
        let c = Config::parse(include_str!("../testbed.example.toml")).unwrap();
        assert!(!c.items.is_empty());
        assert!(c.providers.contains_key("wt"));
    }

    #[test]
    fn rejects_unknown_providers_and_duplicate_labels() {
        assert!(Config::parse("[providers.bogus]\n").is_err());
        let dup = "[[items]]\nprovider='wt'\nid='a'\nlabel='x'\n[[items]]\nprovider='wt'\nid='b'\nlabel='x'\n";
        assert!(Config::parse(dup)
            .unwrap_err()
            .to_string()
            .contains("labelled"));
        let slash = "[[items]]\nprovider='wt'\nid='a'\nlabel='x/y'\n";
        assert!(Config::parse(slash).is_err());
    }
}
