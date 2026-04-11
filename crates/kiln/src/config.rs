use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

/// Site-wide configuration loaded from `config.toml`.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_base_url")]
    pub base_url: String,

    #[serde(default = "default_title")]
    pub title: String,

    #[serde(default)]
    pub description: String,

    #[serde(default = "default_language")]
    pub language: String,

    /// Site time zone used to render page dates exposed to templates.
    ///
    /// Uses IANA time zone names such as `Asia/Shanghai`. When unset, kiln
    /// renders dates in UTC.
    #[serde(default)]
    pub timezone: Option<String>,

    #[serde(default = "default_output_dir")]
    pub output_dir: String,

    /// Theme name, resolved to `themes/<name>/` under the site root.
    #[serde(default)]
    pub theme: Option<String>,

    /// Free-form key-value bag for theme and site settings.
    /// Theme defaults from `theme.toml` are merged in at load time.
    #[serde(default)]
    pub params: toml::Table,

    #[serde(default)]
    pub search: Search,

    #[serde(default)]
    pub menu: Menu,

    #[serde(default)]
    pub author: Author,
}

/// Theme metadata loaded from `themes/<name>/theme.toml`.
#[derive(Debug, Deserialize)]
struct ThemeMeta {
    #[serde(default)]
    min_kiln_version: Option<String>,

    #[serde(default)]
    params: toml::Table,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Author {
    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub email: String,

    #[serde(default)]
    pub link: String,
}

/// Full-text search configuration.
///
/// When enabled, kiln runs Pagefind as a post-build step to generate a search
/// index under `{output_dir}/pagefind/`. The `pagefind` binary must be
/// installed separately — see <https://pagefind.app/docs/installation/>.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Search {
    /// Enable Pagefind search indexing after build.
    #[serde(default)]
    pub enabled: bool,

    /// Path or name of the Pagefind binary (defaults to `"pagefind"` on `$PATH`).
    #[serde(default)]
    pub binary: Option<String>,
}

/// Site navigation menus.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Menu {
    #[serde(default)]
    pub main: Vec<MenuItem>,
}

/// A single navigation menu entry.
#[derive(Debug, Deserialize, Serialize)]
pub struct MenuItem {
    pub name: String,
    pub url: String,

    #[serde(default)]
    pub icon: Option<String>,

    /// Sort order (ascending). Items without a weight default to 0.
    #[serde(default)]
    pub weight: i32,

    /// Whether this link points to an external site.
    #[serde(default)]
    pub external: bool,
}

impl Config {
    /// Loads site configuration from `config.toml` in the given root.
    ///
    /// When a theme is configured, also loads its `theme.toml` and merges
    /// default params. Falls back to defaults if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file exists but cannot be read or parsed,
    /// or if a configured theme's `theme.toml` is missing or incompatible.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("config.toml");
        let mut config: Self = if path.exists() {
            let contents = fs::read_to_string(&path).context("failed to read config.toml")?;
            toml::from_str(&contents).context("failed to parse config.toml")?
        } else {
            toml::from_str("").context("failed to construct default config")?
        };

        if let Some(ref theme_name) = config.theme {
            let theme_toml = root.join("themes").join(theme_name).join("theme.toml");
            let theme = ThemeMeta::load(&theme_toml)?;
            theme.check_min_kiln_version(theme_name)?;
            tracing::info!("using theme: {theme_name}");
            merge_params(&mut config.params, &theme.params)?;
        }

        config.menu.main.sort_by_key(|item| item.weight);

        Ok(config)
    }

    /// Returns the resolved theme directory path, if a theme is configured.
    #[must_use]
    pub fn theme_dir(&self, root: &Path) -> Option<PathBuf> {
        self.theme
            .as_ref()
            .map(|name| root.join("themes").join(name))
    }

    /// Resolves the configured site time zone, if present.
    ///
    /// # Errors
    ///
    /// Returns an error if `timezone` is set but is not a valid IANA time zone
    /// name recognized by `jiff`.
    pub fn time_zone(&self) -> Result<Option<TimeZone>> {
        self.timezone
            .as_deref()
            .map(|time_zone_name| {
                TimeZone::get(time_zone_name)
                    .with_context(|| format!("invalid timezone `{time_zone_name}` in config.toml"))
            })
            .transpose()
    }
}

/// Kiln version from `Cargo.toml`, checked at compile time.
const KILN_VERSION: &str = env!("CARGO_PKG_VERSION");

impl ThemeMeta {
    fn load(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read theme.toml at {}", path.display()))?;
        toml::from_str(&contents).context("failed to parse theme.toml")
    }

    fn check_min_kiln_version(&self, theme_name: &str) -> Result<()> {
        let Some(ref required) = self.min_kiln_version else {
            return Ok(());
        };
        let required: semver::Version = required
            .parse()
            .with_context(|| format!("invalid min_kiln_version `{required}` in theme.toml"))?;
        let current: semver::Version = KILN_VERSION
            .parse()
            .expect("CARGO_PKG_VERSION is always valid semver");
        if current < required {
            bail!("theme `{theme_name}` requires kiln >= {required}, but this is kiln {current}");
        }
        Ok(())
    }
}

/// Merges theme default params into site params. Site values take precedence.
/// Nested tables are merged recursively. Returns an error on type mismatch.
fn merge_params(site: &mut toml::Table, theme_defaults: &toml::Table) -> Result<()> {
    for (key, theme_val) in theme_defaults {
        if let Some(site_val) = site.get_mut(key) {
            match (site_val, theme_val) {
                // Both are tables → recursive merge.
                (toml::Value::Table(st), toml::Value::Table(tt)) => {
                    merge_params(st, tt)?;
                }
                // Type mismatch — reject.
                (s, t) if s.type_str() != t.type_str() => {
                    bail!(
                        "param `{key}` has type `{}` in site config but `{}` in theme",
                        s.type_str(),
                        t.type_str(),
                    );
                }
                // Same scalar type — site wins silently.
                _ => {}
            }
        } else {
            site.insert(key.clone(), theme_val.clone());
        }
    }
    Ok(())
}

fn default_base_url() -> String {
    crate::serve::localhost_url(crate::serve::DEFAULT_PORT)
}

fn default_title() -> String {
    String::from("My Site")
}

fn default_language() -> String {
    String::from("en")
}

fn default_output_dir() -> String {
    String::from("public")
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use crate::serve::{DEFAULT_PORT, localhost_url};

    use super::*;

    // ── deserialization ──

    #[test]
    fn defaults_when_empty() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.base_url, localhost_url(DEFAULT_PORT));
        assert_eq!(config.title, "My Site");
        assert!(config.description.is_empty());
        assert_eq!(config.language, "en");
        assert!(config.timezone.is_none());
        assert_eq!(config.output_dir, "public");
        assert!(config.theme.is_none());
        assert!(config.params.is_empty());
        assert!(!config.search.enabled);
        assert!(config.search.binary.is_none());
        assert!(config.menu.main.is_empty());
        assert!(config.author.name.is_empty());
        assert!(config.author.email.is_empty());
        assert!(config.author.link.is_empty());
    }

    #[test]
    fn overrides_from_toml() {
        let toml_str = indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"
            description = "Test Description"
            language = "zh-CN"
            timezone = "Asia/Shanghai"
            output_dir = "dist"
            theme = "IgnIt"

            [params]
            fontawesome = true

            [author]
            name = "Alice"
            email = "alice@example.com"
            link = "https://alice.example.com"
        "#};
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.base_url, "https://example.com");
        assert_eq!(config.title, "Test Site");
        assert_eq!(config.description, "Test Description");
        assert_eq!(config.language, "zh-CN");
        assert_eq!(config.timezone.as_deref(), Some("Asia/Shanghai"));
        assert_eq!(config.output_dir, "dist");
        assert_eq!(config.theme.as_deref(), Some("IgnIt"));
        assert_eq!(
            config.params.get("fontawesome"),
            Some(&toml::Value::Boolean(true)),
        );
        assert_eq!(config.author.name, "Alice");
        assert_eq!(config.author.email, "alice@example.com");
        assert_eq!(config.author.link, "https://alice.example.com");
    }

    #[test]
    fn search_from_toml() {
        let config: Config = toml::from_str(indoc! {r#"
            [search]
            enabled = true
            binary = "/usr/local/bin/pagefind"
        "#})
        .unwrap();
        assert!(config.search.enabled);
        assert_eq!(
            config.search.binary.as_deref(),
            Some("/usr/local/bin/pagefind"),
        );
    }

    /// Verifies TOML field parsing for menu items.
    ///
    /// Items appear in TOML source order here because this test uses
    /// `toml::from_str` directly, bypassing `Config::load()` which sorts
    /// by weight. See `menu_sorts_by_weight_on_load` for the sorting test.
    #[test]
    fn menu_from_toml_parses_fields() {
        let config: Config = toml::from_str(indoc! {r#"
            [[menu.main]]
            name = "Posts"
            url = "/posts/"
            icon = "fas fa-archive"
            weight = 1

            [[menu.main]]
            name = "GitHub"
            url = "https://github.com/user"
            weight = 10
            external = true

            [[menu.main]]
            name = "About"
            url = "/about/"
            weight = 5
        "#})
        .unwrap();

        // Items in TOML source order (not sorted by weight).
        assert_eq!(config.menu.main.len(), 3);
        assert_eq!(config.menu.main[0].name, "Posts");
        assert_eq!(config.menu.main[0].url, "/posts/");
        assert_eq!(config.menu.main[0].icon.as_deref(), Some("fas fa-archive"));
        assert_eq!(config.menu.main[0].weight, 1);
        assert!(!config.menu.main[0].external);
        assert_eq!(config.menu.main[1].name, "GitHub");
        assert_eq!(config.menu.main[1].url, "https://github.com/user");
        assert_eq!(config.menu.main[1].weight, 10);
        assert!(config.menu.main[1].external);
        assert_eq!(config.menu.main[2].name, "About");
        assert_eq!(config.menu.main[2].url, "/about/");
        assert_eq!(config.menu.main[2].weight, 5);
        assert!(config.menu.main[2].icon.is_none());
    }

    #[test]
    fn menu_item_defaults() {
        let config: Config = toml::from_str(indoc! {r#"
            [[menu.main]]
            name = "Home"
            url = "/"
        "#})
        .unwrap();

        assert_eq!(config.menu.main.len(), 1);
        let item = &config.menu.main[0];
        assert_eq!(item.name, "Home");
        assert_eq!(item.url, "/");
        assert!(item.icon.is_none());
        assert_eq!(item.weight, 0);
        assert!(!item.external);
    }

    // ── load ──

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            indoc! {r#"
                base_url = "https://hakula.xyz"
                title = "HAKULA†CHANNEL"
            "#},
        )
        .unwrap();

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.base_url, "https://hakula.xyz");
        assert_eq!(config.title, "HAKULA†CHANNEL");
    }

    #[test]
    fn load_missing_file_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.base_url, localhost_url(DEFAULT_PORT));
    }

    #[test]
    fn load_invalid_toml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        fs::write(&config_path, "{{invalid toml").unwrap();

        let result = Config::load(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn menu_sorts_by_weight_on_load() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            indoc! {r#"
                [[menu.main]]
                name = "Last"
                url = "/last/"
                weight = 10

                [[menu.main]]
                name = "First"
                url = "/first/"
                weight = 1

                [[menu.main]]
                name = "Middle"
                url = "/middle/"
                weight = 5
            "#},
        )
        .unwrap();

        let config = Config::load(dir.path()).unwrap();
        let names: Vec<&str> = config.menu.main.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["First", "Middle", "Last"]);
    }

    // ── load (theme) ──

    fn setup_theme(root: &Path, theme_toml: &str) {
        let theme_dir = root.join("themes").join("test-theme");
        fs::create_dir_all(&theme_dir).unwrap();
        fs::write(theme_dir.join("theme.toml"), theme_toml).unwrap();
    }

    #[test]
    fn load_no_theme_skips_theme() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.toml"), "").unwrap();

        let config = Config::load(dir.path()).unwrap();
        assert!(config.theme.is_none());
        assert!(config.params.is_empty());
    }

    #[test]
    fn load_theme_merges_params() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            indoc! {r#"
                theme = "test-theme"

                [params]
                fontawesome = true
            "#},
        )
        .unwrap();
        setup_theme(
            dir.path(),
            indoc! {r#"
                name = "test-theme"

                [params]
                code_max_lines = 40
                fontawesome = false
            "#},
        );

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(
            config.params.get("code_max_lines"),
            Some(&toml::Value::Integer(40)),
            "should fill in missing site params from theme"
        );
        assert_eq!(
            config.params.get("fontawesome"),
            Some(&toml::Value::Boolean(true)),
            "should override theme defaults"
        );
    }

    #[test]
    fn load_theme_merges_nested_params() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            indoc! {r#"
                theme = "test-theme"

                [params.social]
                github = "user"

                [params.social.links]
                github = "https://github.com/user"
            "#},
        )
        .unwrap();
        setup_theme(
            dir.path(),
            indoc! {r#"
                name = "test-theme"

                [params.social]
                github = "default"
                twitter = "default"

                [params.social.links]
                github = "https://github.com/default"
                twitter = "https://twitter.com/default"
            "#},
        );

        let config = Config::load(dir.path()).unwrap();
        let social = config.params["social"].as_table().unwrap();
        assert_eq!(
            social.get("github"),
            Some(&toml::Value::String("user".into())),
            "should override nested theme default"
        );
        assert_eq!(
            social.get("twitter"),
            Some(&toml::Value::String("default".into())),
            "should fill in missing nested param from theme"
        );

        let links = social["links"].as_table().unwrap();
        assert_eq!(
            links.get("github"),
            Some(&toml::Value::String("https://github.com/user".into())),
            "should override deeply nested theme default"
        );
        assert_eq!(
            links.get("twitter"),
            Some(&toml::Value::String("https://twitter.com/default".into())),
            "should fill in missing deeply nested param from theme"
        );
    }

    #[test]
    fn load_theme_no_min_version_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            indoc! {r#"
                theme = "test-theme"
            "#},
        )
        .unwrap();
        setup_theme(
            dir.path(),
            indoc! {r#"
                name = "test-theme"
            "#},
        );

        assert!(Config::load(dir.path()).is_ok());
    }

    #[test]
    fn load_theme_compatible_version_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            indoc! {r#"
                theme = "test-theme"
            "#},
        )
        .unwrap();
        setup_theme(
            dir.path(),
            &format!(
                indoc! {r#"
                    name = "test-theme"
                    min_kiln_version = "{version}"
                "#},
                version = KILN_VERSION,
            ),
        );

        assert!(Config::load(dir.path()).is_ok());
    }

    #[test]
    fn load_theme_missing_theme_toml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            indoc! {r#"
                theme = "nonexistent"
            "#},
        )
        .unwrap();

        let err = Config::load(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("failed to read theme.toml"),
            "should report missing theme.toml, got: {err}"
        );
    }

    #[test]
    fn load_theme_incompatible_version_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            indoc! {r#"
                theme = "test-theme"
            "#},
        )
        .unwrap();
        setup_theme(
            dir.path(),
            indoc! {r#"
                name = "test-theme"
                min_kiln_version = "999.0.0"
            "#},
        );

        let err = Config::load(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("requires kiln >= 999.0.0"),
            "should report version mismatch, got: {err}"
        );
    }

    #[test]
    fn load_theme_invalid_version_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            indoc! {r#"
                theme = "test-theme"
            "#},
        )
        .unwrap();
        setup_theme(
            dir.path(),
            indoc! {r#"
                name = "test-theme"
                min_kiln_version = "not-a-version"
            "#},
        );

        let err = Config::load(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("invalid min_kiln_version `not-a-version`"),
            "should report invalid version, got: {err}"
        );
    }

    // ── theme_dir ──

    #[test]
    fn theme_dir_returns_path_when_configured() {
        let config: Config = toml::from_str(r#"theme = "IgnIt""#).unwrap();
        let root = Path::new("/project");
        assert_eq!(
            config.theme_dir(root),
            Some(root.join("themes").join("IgnIt"))
        );
    }

    #[test]
    fn theme_dir_returns_none_without_theme() {
        let config: Config = toml::from_str("").unwrap();
        let root = Path::new("/project");
        assert!(config.theme_dir(root).is_none());
    }

    // ── time_zone ──

    #[test]
    fn time_zone_resolves_configured_iana_name() {
        let config: Config = toml::from_str(r#"timezone = "Asia/Shanghai""#).unwrap();
        let time_zone = config.time_zone().unwrap().unwrap();
        assert_eq!(time_zone.iana_name(), Some("Asia/Shanghai"));
    }

    #[test]
    fn time_zone_invalid_returns_error() {
        let config: Config = toml::from_str(r#"timezone = "Mars/Base""#).unwrap();
        let err = config.time_zone().unwrap_err().to_string();
        assert!(
            err.contains("invalid timezone `Mars/Base` in config.toml"),
            "should report invalid timezone, got: {err}"
        );
    }

    // ── merge_params ──

    #[test]
    fn merge_params_empty_site() {
        let mut site = toml::Table::new();
        let theme: toml::Table = toml::from_str(r#"key = "theme""#).unwrap();
        merge_params(&mut site, &theme).unwrap();
        assert_eq!(site.get("key"), Some(&toml::Value::String("theme".into())));
    }

    #[test]
    fn merge_params_empty_theme() {
        let mut site: toml::Table = toml::from_str(r#"key = "site""#).unwrap();
        let theme = toml::Table::new();
        merge_params(&mut site, &theme).unwrap();
        assert_eq!(site.get("key"), Some(&toml::Value::String("site".into())));
    }

    #[test]
    fn merge_params_site_wins_for_scalars() {
        let mut site: toml::Table = toml::from_str(r#"key = "site""#).unwrap();
        let theme: toml::Table = toml::from_str(r#"key = "theme""#).unwrap();
        merge_params(&mut site, &theme).unwrap();
        assert_eq!(site.get("key"), Some(&toml::Value::String("site".into())));
    }

    #[test]
    fn merge_params_rejects_type_mismatch() {
        let mut site: toml::Table = toml::from_str(r#"key = "site""#).unwrap();
        let theme: toml::Table = toml::from_str(indoc! {r#"
            [key]
            foo = "bar"
        "#})
        .unwrap();

        let err = merge_params(&mut site, &theme).unwrap_err().to_string();
        assert!(
            err.contains("param `key` has type `string` in site config but `table` in theme"),
            "should report type mismatch, got: {err}"
        );
    }
}
