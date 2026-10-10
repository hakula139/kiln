use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Result, bail};
use minijinja::value::Object;
use serde::Serialize;
use strum::{AsRefStr, EnumString};

/// Per-page collection of asset declarations gathered during render.
///
/// Surfaced on [`PostTemplateVars`] so themes can iterate `assets.scripts` and `assets.features`
/// without per-feature frontmatter flags.
///
/// [`PostTemplateVars`]: crate::template::vars::PostTemplateVars
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PageAssets {
    /// Scripts in registration order. Order matters for dependency chains (e.g., a library script
    /// must be registered before its consumer).
    scripts: Vec<ScriptTag>,

    /// Features auto-detected during render (math expressions, mermaid fences).
    /// Themes use these to conditionally load CSS / JS for the feature.
    pub features: BTreeSet<Feature>,
}

impl PageAssets {
    /// Registers a script for the current page.
    ///
    /// Re-registering the exact same [`ScriptTag`] (same `url`, `load`, and `module`) is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error for synchronous modules or conflicting attributes for an existing URL.
    pub fn register_script(&mut self, tag: ScriptTag) -> Result<()> {
        if tag.module && tag.load == LoadStrategy::Sync {
            bail!("module script {:?} cannot use synchronous loading", tag.url);
        }

        if let Some(existing) = self.scripts.iter().find(|s| s.url == tag.url) {
            if existing == &tag {
                return Ok(());
            }
            bail!(
                "script \"{url}\" was already registered as (load={old_load}, module={old_mod}); \
                 cannot re-register as (load={new_load}, module={new_mod}). \
                 Pick one set of attributes per URL.",
                url = tag.url,
                old_load = existing.load.as_ref(),
                old_mod = existing.module,
                new_load = tag.load.as_ref(),
                new_mod = tag.module,
            );
        }
        self.scripts.push(tag);
        Ok(())
    }

    /// Scripts in registration order.
    #[must_use]
    pub fn scripts(&self) -> &[ScriptTag] {
        &self.scripts
    }

    /// Marks a feature as needed by the current page.
    pub fn add_feature(&mut self, feature: Feature) {
        self.features.insert(feature);
    }
}

/// Shared asset registry for directive templates.
///
/// The mutex satisfies `MiniJinja`'s `Object: Send + Sync` requirement.
#[derive(Debug, Default, Clone)]
pub struct AssetsHandle {
    inner: Arc<Mutex<PageAssets>>,
}

impl AssetsHandle {
    /// Locks the underlying assets for direct mutation by the render pipeline.
    ///
    /// # Panics
    ///
    /// Panics if the underlying mutex is poisoned.
    pub(crate) fn lock(&self) -> MutexGuard<'_, PageAssets> {
        self.inner.lock().expect("PageAssets mutex poisoned")
    }

    /// Returns an owned snapshot of the current assets.
    #[must_use]
    pub(crate) fn snapshot(&self) -> PageAssets {
        self.lock().clone()
    }
}

impl Object for AssetsHandle {}

/// A script declaration validated by [`PageAssets::register_script`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptTag {
    pub url: String,
    pub load: LoadStrategy,
    pub module: bool,
}

impl ScriptTag {
    /// Builds a deferred, non-module script tag (the common case).
    #[must_use]
    pub fn deferred(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            load: LoadStrategy::Defer,
            module: false,
        }
    }
}

/// Script execution strategy. Modules support deferred or asynchronous execution only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, AsRefStr, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum LoadStrategy {
    #[default]
    Defer,
    Async,
    Sync,
}

/// A page-level feature flag, set during render and read by themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, AsRefStr)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Feature {
    Math,
    Mermaid,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PageAssets::register_script ──

    #[test]
    fn register_script_preserves_order_and_deduplicates_identical_tags() {
        let mut assets = PageAssets::default();
        let first = ScriptTag::deferred("/b.js");
        let second = ScriptTag::deferred("/a.js");
        for tag in [&first, &second, &first] {
            assets.register_script(tag.clone()).unwrap();
        }
        assert_eq!(assets.scripts(), &[first, second]);
    }

    #[test]
    fn register_script_supported_strategies() {
        for (load, module) in [
            (LoadStrategy::Defer, false),
            (LoadStrategy::Async, false),
            (LoadStrategy::Sync, false),
            (LoadStrategy::Defer, true),
            (LoadStrategy::Async, true),
        ] {
            let mut assets = PageAssets::default();
            let tag = ScriptTag {
                url: "/module.js".into(),
                load,
                module,
            };
            assets.register_script(tag.clone()).unwrap();
            assert_eq!(assets.scripts(), &[tag]);
        }
    }

    #[test]
    fn register_script_synchronous_module_returns_error() {
        let mut assets = PageAssets::default();
        let error = assets
            .register_script(ScriptTag {
                url: "/module.js".into(),
                load: LoadStrategy::Sync,
                module: true,
            })
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            r#"module script "/module.js" cannot use synchronous loading"#
        );
        assert_eq!(assets.scripts(), []);
    }

    #[test]
    fn register_script_conflicting_attributes_returns_error() {
        for (load, module, attributes) in [
            (LoadStrategy::Async, false, "load=async, module=false"),
            (LoadStrategy::Defer, true, "load=defer, module=true"),
        ] {
            let mut assets = PageAssets::default();
            let original = ScriptTag::deferred("/x.js");
            assets.register_script(original.clone()).unwrap();
            let error = assets
                .register_script(ScriptTag {
                    url: "/x.js".into(),
                    load,
                    module,
                })
                .unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "script \"/x.js\" was already registered as (load=defer, module=false); \
                     cannot re-register as ({attributes}). \
                     Pick one set of attributes per URL."
                ),
            );
            assert_eq!(assets.scripts(), &[original]);
        }
    }

    // ── PageAssets::add_feature ──

    #[test]
    fn add_feature_dedupes() {
        let mut assets = PageAssets::default();
        assets.add_feature(Feature::Math);
        assets.add_feature(Feature::Math);
        assets.add_feature(Feature::Mermaid);
        assert_eq!(assets.features.len(), 2);
        assert!(assets.features.contains(&Feature::Math));
        assert!(assets.features.contains(&Feature::Mermaid));
    }

    // ── Feature string form ──

    #[test]
    fn feature_string_form_is_lowercase_for_templates() {
        // Themes test membership with `"math" in assets.features`. Verify the strum (AsRef) and
        // serde (Serialize) string forms agree, since MiniJinja serializes via serde while strum
        // drives our internal tooling.
        assert_eq!(Feature::Math.as_ref(), "math");
        assert_eq!(Feature::Mermaid.as_ref(), "mermaid");

        let toml_value = toml::Value::try_from(Feature::Math).unwrap();
        assert_eq!(toml_value.as_str(), Some("math"));
    }
}
