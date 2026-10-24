//! `testbed.toml`: which providers to open and which items to put in the gallery.
//!
//! The real file holds local paths to the owner's game installs, so it is untracked;
//! `testbed.example.toml` is the committed template.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// Expands a leading `~` or `~/` in a path to the home folder (`$HOME`, else `%USERPROFILE%`). A path
/// that starts with `~user`, or has no home folder to use, is returned as it is.
pub fn expand_home(path: &Path) -> PathBuf {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    expand_home_with(path, home.as_deref())
}

fn expand_home_with(path: &Path, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    }
}

/// Names a provider can have in `[providers.<name>]` and in an item's `provider`
pub const PROVIDER_NAMES: [&str; 5] = ["wt", "wf", "hytale", "nocturne", "mc"];

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
    /// the launcher's install is the default), the client jar (mc, optional: the newest one a launcher
    /// installed is the default). A leading `~/` is the home folder.
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
        let mut config: Config = toml::from_str(text)?;
        for provider in config.providers.values_mut() {
            if let Some(path) = &provider.path {
                provider.path = Some(expand_home(path));
            }
        }
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
    fn mc_is_a_provider_and_its_path_is_optional() {
        let c = Config::parse(
            "[providers.mc]\n[[items]]\nprovider='mc'\nid='block/stone'\nlabel='Stone'\n",
        )
        .unwrap();
        assert!(c.providers["mc"].path.is_none());
        assert_eq!(c.items[0].provider, "mc");
    }

    #[test]
    fn expands_a_leading_tilde_only() {
        let home = Path::new("/home/me");
        assert_eq!(
            expand_home_with(Path::new("~/Games/smt3hd"), Some(home)),
            Path::new("/home/me/Games/smt3hd")
        );
        assert_eq!(expand_home_with(Path::new("~"), Some(home)), home);
        // not ours to guess: another user's home, a `~` inside a name, an absolute path, no home
        for p in ["~other/x", "/a/~/b", "a~b/c", "/abs/path"] {
            assert_eq!(expand_home_with(Path::new(p), Some(home)), Path::new(p));
        }
        assert_eq!(expand_home_with(Path::new("~/x"), None), Path::new("~/x"));
    }

    #[test]
    fn config_paths_are_expanded() {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        let Some(home) = home else { return };
        let c = Config::parse("[providers.nocturne]\npath = '~/Games/smt3hd'\n").unwrap();
        assert_eq!(
            c.providers["nocturne"].path.as_deref(),
            Some(PathBuf::from(home).join("Games/smt3hd").as_path())
        );
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
