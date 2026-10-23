//! Opening the game readers. Each is opened on its first use, and a failure is kept as text so the
//! gallery can print it at the slots of that provider's items instead of crashing.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use rewrite_bridge::Provider;

use crate::config::ProviderConfig;

pub type DynProvider = Arc<dyn Provider + Send + Sync>;

/// An opened provider. War Thunder's collision calls are not part of `Provider`, so its typed handle is
/// kept as well (the same object).
pub struct Opened {
    pub provider: DynProvider,
    #[cfg(feature = "wt")]
    pub wt: Option<Arc<wt_unpack::bridge::WarThunder>>,
}

impl Opened {
    fn plain(provider: DynProvider) -> Self {
        Self {
            provider,
            #[cfg(feature = "wt")]
            wt: None,
        }
    }
}

pub struct ProviderSet {
    config: BTreeMap<String, ProviderConfig>,
    opened: BTreeMap<String, OnceLock<Result<Opened, String>>>,
}

impl ProviderSet {
    pub fn new(config: &BTreeMap<String, ProviderConfig>) -> Self {
        Self {
            config: config.clone(),
            opened: config
                .keys()
                .map(|k| (k.clone(), OnceLock::new()))
                .collect(),
        }
    }

    /// A set holding ready-made providers, for tests
    #[cfg(test)]
    pub fn with_providers(providers: Vec<(&str, DynProvider)>) -> Self {
        let mut set = Self::new(&BTreeMap::new());
        for (name, p) in providers {
            let cell = OnceLock::new();
            let _ = cell.set(Ok(Opened::plain(p)));
            set.opened.insert(name.to_string(), cell);
            set.config
                .insert(name.to_string(), ProviderConfig::default());
        }
        set
    }

    /// The provider `name`, opening it the first time. `Err` says why it cannot be used.
    pub fn get(&self, name: &str) -> Result<&DynProvider, String> {
        let (Some(cell), Some(config)) = (self.opened.get(name), self.config.get(name)) else {
            return Err(format!(
                "provider {name:?} has no [providers.{name}] section in the config"
            ));
        };
        // A panic inside a reader is a failure of that provider, not of the app
        let result = cell.get_or_init(|| {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| open(name, config))) {
                Ok(r) => r.map_err(|e| format!("{e:#}")),
                Err(_) => Err("the reader panicked while opening".to_string()),
            }
        });
        result
            .as_ref()
            .map(|o| &o.provider)
            .map_err(|e| format!("{name} failed to open: {e}"))
    }

    /// The War Thunder reader behind `name`, for its collision calls (`None` for any other provider)
    #[cfg(feature = "wt")]
    pub fn wt(&self, name: &str) -> Option<Arc<wt_unpack::bridge::WarThunder>> {
        self.opened.get(name)?.get()?.as_ref().ok()?.wt.clone()
    }

    /// Every provider opened so far, for scenes that name another game's assets
    pub fn opened(&self) -> Vec<&dyn Provider> {
        self.opened
            .values()
            .filter_map(|c| c.get())
            .filter_map(|r| r.as_ref().ok())
            .map(|o| o.provider.as_ref() as &dyn Provider)
            .collect()
    }
}

fn open(name: &str, config: &ProviderConfig) -> anyhow::Result<Opened> {
    let _ = config;
    match name {
        #[cfg(feature = "wt")]
        "wt" => {
            let path = config
                .path
                .clone()
                .ok_or_else(|| anyhow::anyhow!("[providers.wt] needs `path` (the game folder)"))?;
            anyhow::ensure!(path.is_dir(), "{} is not a folder", path.display());
            let wt = Arc::new(wt_unpack::bridge::WarThunder::new(path));
            Ok(Opened {
                provider: wt.clone(),
                wt: Some(wt),
            })
        }
        #[cfg(feature = "wf")]
        "wf" => {
            let path = config.path.clone().ok_or_else(|| {
                anyhow::anyhow!("[providers.wf] needs `path` (the Cache.Windows folder)")
            })?;
            anyhow::ensure!(path.is_dir(), "{} is not a folder", path.display());
            Ok(Opened::plain(Arc::new(
                wf_unpack::bridge::Warframe::open(&path)?.with_max_texture(config.max_texture),
            )))
        }
        #[cfg(feature = "hytale")]
        "hytale" => Ok(Opened::plain(Arc::new(match &config.path {
            Some(p) => hytale_unpack::bridge::Hytale::open(p)?,
            None => hytale_unpack::bridge::Hytale::open_default()?,
        }))),
        #[cfg(feature = "nocturne")]
        "nocturne" => {
            let path = config.path.clone().ok_or_else(|| {
                anyhow::anyhow!(
                    "[providers.nocturne] needs `path` (the game folder that holds smt3hd_Data)"
                )
            })?;
            anyhow::ensure!(path.is_dir(), "{} is not a folder", path.display());
            Ok(Opened::plain(Arc::new(
                nocturne_unpack::bridge::Nocturne::open(&path)?,
            )))
        }
        other => anyhow::bail!("this build has no `{other}` provider (cargo feature `{other}`)"),
    }
}
