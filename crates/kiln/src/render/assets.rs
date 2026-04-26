use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::Serialize;
use strum::AsRefStr;

/// Per-page collection of asset declarations gathered during render.
///
/// Built once per page in the render pipeline and surfaced on
/// [`PostTemplateVars`] so themes can iterate `assets.scripts` and gate
/// `assets.features` instead of relying on per-feature side-channel flags
/// (`math: bool`, `mermaid: bool`, ...) that need a frontmatter / template-var /
/// theme-partial trio for every new feature.
///
/// [`PostTemplateVars`]: crate::template::vars::PostTemplateVars
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PageAssets {
    /// Scripts in registration order. Order matters for dependency chains
    /// (e.g., a library script must be registered before its consumer).
    ///
    /// Reserved for the planned directive→script bridge; no caller writes to
    /// this field today, so themes will currently see an empty list.
    pub scripts: Vec<ScriptTag>,

    /// Features auto-detected during render (math expressions, mermaid fences).
    /// Themes use these to conditionally load CSS / JS for the feature.
    pub features: BTreeSet<Feature>,
}

impl PageAssets {
    /// Registers a script for the current page.
    ///
    /// Re-registering the exact same [`ScriptTag`] (same `url`, `load`, and
    /// `module`) is a no-op. Linear search is fine here — a page registers
    /// at most a handful of scripts.
    ///
    /// # Errors
    ///
    /// Returns an error if a script with the same URL has already been
    /// registered with a different `load` strategy or `module` flag, since
    /// the browser would otherwise see two `<script>` tags fighting for the
    /// same source.
    pub fn register_script(&mut self, tag: ScriptTag) -> Result<()> {
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

    /// Marks a feature as needed by the current page.
    pub fn add_feature(&mut self, feature: Feature) {
        self.features.insert(feature);
    }
}

/// A `<script>` tag declaration.
///
/// Equality compares all fields — re-registering the same URL with different
/// `load` or `module` is a conflict, not a duplicate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptTag {
    pub url: String,
    pub load: LoadStrategy,
    pub module: bool,
}

impl ScriptTag {
    /// Builds a deferred, non-module script tag — the common case.
    #[must_use]
    pub fn deferred(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            load: LoadStrategy::Defer,
            module: false,
        }
    }
}

/// How a `<script>` tag is loaded. `defer` and `async` are mutually exclusive
/// in HTML, so they share an enum rather than two `bool` fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, AsRefStr)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum LoadStrategy {
    #[default]
    Defer,
    Async,
    Sync,
}

/// A page-level feature flag, set during render and read by themes.
///
/// New variants are added when the engine learns to auto-detect a new
/// conditional capability. Site-wide modes (search, fontawesome) are
/// configured separately and do not belong here.
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
    fn register_script_adds_in_order() {
        let mut assets = PageAssets::default();
        assets
            .register_script(ScriptTag::deferred("/a.js"))
            .unwrap();
        assets
            .register_script(ScriptTag::deferred("/b.js"))
            .unwrap();
        assert_eq!(assets.scripts.len(), 2);
        assert_eq!(assets.scripts[0].url, "/a.js");
        assert_eq!(assets.scripts[1].url, "/b.js");
    }

    #[test]
    fn register_script_idempotent_on_identical_tag() {
        let mut assets = PageAssets::default();
        let tag = ScriptTag::deferred("/score.js");
        assets.register_script(tag.clone()).unwrap();
        assets.register_script(tag.clone()).unwrap();
        assets.register_script(tag).unwrap();
        assert_eq!(assets.scripts.len(), 1, "same tag should dedup");
    }

    #[test]
    fn register_script_returns_error_on_conflicting_load_strategy() {
        let mut assets = PageAssets::default();
        assets
            .register_script(ScriptTag::deferred("/x.js"))
            .unwrap();
        let err = assets
            .register_script(ScriptTag {
                url: "/x.js".into(),
                load: LoadStrategy::Async,
                module: false,
            })
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "script \"/x.js\" was already registered as (load=defer, module=false); \
             cannot re-register as (load=async, module=false). \
             Pick one set of attributes per URL.",
        );
        assert_eq!(assets.scripts.len(), 1, "conflicting tag must not be added");
    }

    #[test]
    fn register_script_returns_error_on_conflicting_module_flag() {
        let mut assets = PageAssets::default();
        assets
            .register_script(ScriptTag::deferred("/x.js"))
            .unwrap();
        let err = assets
            .register_script(ScriptTag {
                url: "/x.js".into(),
                load: LoadStrategy::Defer,
                module: true,
            })
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "script \"/x.js\" was already registered as (load=defer, module=false); \
             cannot re-register as (load=defer, module=true). \
             Pick one set of attributes per URL.",
        );
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
        // Themes test membership with `"math" in assets.features`. Verify the
        // strum (AsRef) and serde (Serialize) string forms agree, since
        // MiniJinja serializes via serde while strum drives our internal
        // tooling.
        assert_eq!(Feature::Math.as_ref(), "math");
        assert_eq!(Feature::Mermaid.as_ref(), "mermaid");

        let toml_value = toml::Value::try_from(Feature::Math).unwrap();
        assert_eq!(toml_value.as_str(), Some("math"));
    }
}
