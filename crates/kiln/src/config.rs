use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::content::{is_single_component, read_optional};
use crate::url::BaseUrl;

/// Default port for `kiln serve` (KILN on a phone keypad: K=5 I=4 L=5 N=6).
pub const DEFAULT_PORT: u16 = 5456;

/// Site root entries that feed a build.
pub(crate) const SITE_INPUTS: &[&str] = &[
    "assets",
    "config.toml",
    "content",
    "i18n",
    "static",
    "templates",
];

/// Theme directory entries that feed a build.
pub(crate) const THEME_INPUTS: &[&str] = &["assets", "i18n", "static", "templates", "theme.toml"];

/// Site-wide configuration loaded from `config.toml`.
#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub base_url: BaseUrl,

    pub title: String,

    pub description: String,

    /// Active BCP 47 language tag, which selects translation files.
    pub language: String,

    /// Time zone for rendered dates, as an IANA name such as `Asia/Shanghai`.
    #[serde(with = "jiff::fmt::serde::tz::required")]
    pub timezone: TimeZone,

    pub enable_git_info: bool,

    pub output_dir: PathBuf,

    /// Theme name, resolved to `themes/<name>/` under the site root.
    pub theme: Option<String>,

    /// Free-form key-value bag for theme and site settings.
    /// Theme defaults from `theme.toml` are merged in at load time.
    pub params: toml::Table,

    pub search: Search,

    pub css: Css,

    /// Named menu groups (e.g., `[[menu.main]]`, `[[menu.social]]`). Themes choose which groups
    /// to render and where. kiln has no opinion about group names.
    pub menu: BTreeMap<String, Vec<MenuItem>>,

    pub author: Author,

    pub image: ImageConfig,
}

/// Theme metadata loaded from `themes/<name>/theme.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ThemeMeta {
    min_kiln_version: Option<semver::Version>,

    css: Css,

    params: toml::Table,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Author {
    pub name: String,
}

/// Full-text search configuration.
///
/// When enabled, kiln runs Pagefind as a post-build step to generate a search index under
/// `{output_dir}/pagefind/`. The `pagefind` binary must be installed separately.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Search {
    /// Enable Pagefind search indexing after build.
    pub enabled: bool,

    /// Path or name of the Pagefind binary (defaults to `"pagefind"` on `$PATH`).
    pub binary: Option<String>,
}

/// CSS compilation settings inherited from the active theme.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Css {
    /// Unset selects plain CSS after theme defaults have been applied.
    pub processor: Option<CssProcessor>,
}

/// Compiler used for site and page-owned stylesheets.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CssProcessor {
    #[default]
    Plain,
    Tailwind,
}

/// A single navigation menu entry.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MenuItem {
    pub name: String,

    pub url: String,

    #[serde(default)]
    pub icon: Option<String>,

    /// Sort order (ascending). Items without a weight default to 0.
    #[serde(default)]
    pub weight: i64,

    #[serde(default)]
    pub external: bool,
}

/// Image-pipeline configuration loaded from the `[image]` section of `config.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImageConfig {
    /// Maximum placeholder width and height, in pixels.
    pub lqip_size: u32,

    /// WebP encoder quality (1–100, lower = smaller / blurrier).
    pub lqip_quality: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: localhost_url(DEFAULT_PORT)
                .parse()
                .expect("the dev server URL is a valid base URL"),
            title: String::from("My Site"),
            description: String::new(),
            language: String::from("en"),
            timezone: TimeZone::UTC,
            enable_git_info: false,
            output_dir: PathBuf::from("public"),
            theme: None,
            params: toml::Table::new(),
            search: Search::default(),
            css: Css::default(),
            menu: BTreeMap::new(),
            author: Author::default(),
            image: ImageConfig::default(),
        }
    }
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            lqip_size: 16,
            lqip_quality: 25,
        }
    }
}

impl Config {
    /// Loads site configuration from `config.toml` in the given root.
    ///
    /// Missing `config.toml` uses defaults. A configured theme supplies default params and CSS
    /// settings through its required `theme.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file exists but cannot be read, parsed or validated, or if
    /// a configured theme's `theme.toml` is missing or incompatible.
    pub fn load(root: &Path) -> Result<Self> {
        let mut config: Self = match read_optional(&root.join("config.toml"))? {
            Some(contents) => toml::from_str(&contents).context("failed to parse config.toml")?,
            None => Self::default(),
        };

        validate_language(&config.language)?;
        config
            .image
            .validate()
            .context("invalid image configuration")?;

        if let Some(ref theme_name) = config.theme {
            validate_theme_name(theme_name)?;
            let theme = ThemeMeta::load(&root.join("themes").join(theme_name).join("theme.toml"))?;
            theme.check_min_kiln_version(theme_name)?;
            tracing::info!("using theme: {theme_name}");
            merge_params(&mut config.params, theme.params, "")?;
            config.css.processor = config.css.processor.or(theme.css.processor);
        }

        for items in config.menu.values_mut() {
            items.sort_by_key(|item| item.weight);
        }

        Ok(config)
    }

    /// Returns the resolved theme directory path, if a theme is configured.
    #[must_use]
    pub fn theme_dir(&self, root: &Path) -> Option<PathBuf> {
        self.theme
            .as_ref()
            .map(|name| root.join("themes").join(name))
    }
}

/// Constructs the localhost base URL for a given port.
pub(crate) fn localhost_url(port: u16) -> String {
    format!("http://localhost:{port}")
}

/// Validates the tag shape before it is interpolated into translation filenames.
fn validate_language(language: &str) -> Result<()> {
    ensure!(
        language.starts_with(|c: char| c.is_ascii_alphabetic())
            && language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "invalid language tag `{language}`: expected BCP 47 shape (ASCII letters / digits / hyphens)",
    );
    Ok(())
}

impl ImageConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.lqip_size > 0,
            "image.lqip_size must be greater than zero"
        );
        ensure!(
            (1..=100).contains(&self.lqip_quality),
            "image.lqip_quality must be between 1 and 100"
        );
        Ok(())
    }
}

/// Rejects theme names that are not exactly one directory name under `themes/`.
pub(crate) fn validate_theme_name(name: &str) -> Result<()> {
    ensure!(
        is_single_component(name),
        "theme must be a single directory name: `{name}`"
    );
    Ok(())
}

impl ThemeMeta {
    fn load(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read theme.toml at {}", path.display()))?;
        toml::from_str(&contents).with_context(|| format!("failed to parse {}", path.display()))
    }

    fn check_min_kiln_version(&self, theme_name: &str) -> Result<()> {
        let Some(ref required) = self.min_kiln_version else {
            return Ok(());
        };
        let current: semver::Version = env!("CARGO_PKG_VERSION")
            .parse()
            .expect("CARGO_PKG_VERSION is always valid semver");
        ensure!(
            current >= *required,
            "theme `{theme_name}` requires kiln >= {required}, but this is kiln {current}"
        );
        Ok(())
    }
}

/// Merges theme default params into site params. Site values take precedence.
/// Nested tables are merged recursively. Returns an error on type mismatch.
fn merge_params(site: &mut toml::Table, theme_defaults: toml::Table, prefix: &str) -> Result<()> {
    for (key, theme_value) in theme_defaults {
        let Some(site_value) = site.get_mut(&key) else {
            site.insert(key, theme_value);
            continue;
        };
        match (site_value, theme_value) {
            (toml::Value::Table(site_table), toml::Value::Table(theme_table)) => {
                merge_params(site_table, theme_table, &format!("{prefix}{key}."))?;
            }
            (site_value, theme_value) => ensure!(
                site_value.type_str() == theme_value.type_str(),
                "param `{prefix}{key}` has type `{}` in site config but `{}` in theme",
                site_value.type_str(),
                theme_value.type_str(),
            ),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::test_utils::write_test_file;

    // ── Config::load ──

    #[test]
    fn load_missing_file_uses_defaults() {
        let root = tempfile::tempdir().unwrap();
        let config = Config::load(root.path()).unwrap();
        assert_eq!(
            config.base_url,
            localhost_url(DEFAULT_PORT).parse().unwrap()
        );
        assert_eq!(config.title, "My Site");
        assert_eq!(config.language, "en");
        assert_eq!(config.timezone, TimeZone::UTC);
        assert_eq!(config.output_dir, Path::new("public"));
        assert_eq!(config.theme, None);
        assert_eq!(
            (config.image.lqip_size, config.image.lqip_quality),
            (16, 25)
        );
    }

    #[test]
    fn load_reads_tables_and_sorts_menu_groups() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "config.toml",
            indoc! {r#"
                base_url = "https://example.com/blog"
                title = "Example Site"
                description = "Example description"
                language = "zh-Hans"
                timezone = "Asia/Shanghai"
                enable_git_info = true
                output_dir = "dist"

                [params]
                fontawesome = true

                [search]
                enabled = true
                binary = "/usr/local/bin/pagefind"

                [css]
                processor = "tailwind"

                [author]
                name = "Alice"

                [image]
                lqip_size = 24

                [[menu.main]]
                name = "B"
                url = "/b/"
                weight = 2

                [[menu.main]]
                name = "A"
                url = "https://example.org/"
                icon = "fas fa-link"
                weight = 1
                external = true

                [[menu.social]]
                name = "C"
                url = "/c/"
            "#},
        );

        let config = Config::load(root.path()).unwrap();

        assert_eq!(config.base_url.prefix(), "/blog/");
        assert_eq!(
            (
                config.title.as_str(),
                config.description.as_str(),
                config.language.as_str()
            ),
            ("Example Site", "Example description", "zh-Hans")
        );
        assert_eq!(config.timezone.iana_name(), Some("Asia/Shanghai"));
        assert!(config.enable_git_info);
        assert_eq!(config.output_dir, Path::new("dist"));
        assert_eq!(config.params["fontawesome"], toml::Value::Boolean(true));
        assert!(config.search.enabled);
        assert_eq!(
            config.search.binary.as_deref(),
            Some("/usr/local/bin/pagefind")
        );
        assert_eq!(config.css.processor, Some(CssProcessor::Tailwind));
        assert_eq!(config.author.name, "Alice");
        assert_eq!(
            (config.image.lqip_size, config.image.lqip_quality),
            (24, 25)
        );
        let menu: Vec<_> = config
            .menu
            .iter()
            .flat_map(|(group, items)| {
                items.iter().map(move |item| {
                    (
                        group.as_str(),
                        item.name.as_str(),
                        item.url.as_str(),
                        item.icon.as_deref(),
                        item.weight,
                        item.external,
                    )
                })
            })
            .collect();
        assert_eq!(
            menu,
            [
                (
                    "main",
                    "A",
                    "https://example.org/",
                    Some("fas fa-link"),
                    1,
                    true
                ),
                ("main", "B", "/b/", None, 2, false),
                ("social", "C", "/c/", None, 0, false),
            ]
        );
    }

    #[test]
    fn load_invalid_config_returns_error() {
        for (contents, expected) in [
            ("{{invalid toml", "failed to parse config.toml"),
            ("enable_gitinfo = true", "unknown field `enable_gitinfo`"),
            ("[analytics]\nenabled = true", "unknown field `analytics`"),
            ("[search]\nenable = true", "unknown field `enable`"),
            (
                "[author]\nemail = \"a@example.com\"",
                "unknown field `email`",
            ),
            (
                "[[menu.main]]\nname = \"A\"\nurl = \"/\"\nbadge = \"x\"",
                "unknown field `badge`",
            ),
            (
                "[css]\nprocessor = \"unknown\"",
                "unknown variant `unknown`",
            ),
            ("[image]\nlqip = false", "unknown field `lqip`"),
            ("[image]\nlqip_quality = 0", "image.lqip_quality"),
            ("[image]\nlqip_size = 0", "image.lqip_size"),
            ("timezone = \"Mars/Base\"", "Mars/Base"),
            ("base_url = \"example.com\"", "invalid base_url"),
            (
                "base_url = \"https://example.com/?preview=1\"",
                "must not contain a query",
            ),
            ("language = \"\"", "invalid language tag"),
            ("language = \"../etc/passwd\"", "invalid language tag"),
            ("language = \"1en\"", "invalid language tag"),
            ("language = \"en_US\"", "invalid language tag"),
        ] {
            let root = tempfile::tempdir().unwrap();
            write_test_file(root.path(), "config.toml", contents);
            let error = format!("{:#}", Config::load(root.path()).unwrap_err());
            assert!(error.contains(expected), "{contents}: {error}");
        }
    }

    #[test]
    fn load_theme_merges_scalar_and_nested_params() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "config.toml",
            indoc! {r#"
                theme = "test-theme"

                [params]
                fontawesome = true

                [params.social]
                github = "user"

                [params.social.links]
                github = "https://github.com/user"
            "#},
        );
        write_test_file(
            root.path(),
            "themes/test-theme/theme.toml",
            indoc! {r#"
                name = "test-theme"
                min_kiln_version = "0.0.1"

                [params]
                code_max_lines = 40
                fontawesome = false

                [params.social]
                github = "default"
                twitter = "default"

                [params.social.links]
                github = "https://github.com/default"
                twitter = "https://twitter.com/default"
            "#},
        );

        let config = Config::load(root.path()).unwrap();
        assert_eq!(
            toml::Value::Table(config.params),
            toml::from_str::<toml::Value>(indoc! {r#"
                code_max_lines = 40
                fontawesome = true

                [social]
                github = "user"
                twitter = "default"

                [social.links]
                github = "https://github.com/user"
                twitter = "https://twitter.com/default"
            "#})
            .unwrap()
        );
    }

    #[test]
    fn load_css_inherits_theme_and_accepts_explicit_site_override() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "themes/test-theme/theme.toml",
            indoc! {r#"
                [css]
                processor = "tailwind"
            "#},
        );
        for (site, expected) in [
            ("", CssProcessor::Tailwind),
            ("[css]\nprocessor = \"plain\"", CssProcessor::Plain),
        ] {
            write_test_file(
                root.path(),
                "config.toml",
                &format!("theme = \"test-theme\"\n{site}"),
            );
            assert_eq!(
                Config::load(root.path()).unwrap().css.processor,
                Some(expected)
            );
        }
    }

    #[test]
    fn load_invalid_theme_returns_error() {
        for (theme, theme_toml, expected) in [
            ("../outside", None, "single directory name"),
            ("C:theme", None, "single directory name"),
            ("missing", None, "failed to read theme.toml"),
            (
                "test-theme",
                Some(r#"min_kiln_version = "999.0.0""#),
                "requires kiln >= 999.0.0",
            ),
            (
                "test-theme",
                Some(r#"min_kiln_version = "not-a-version""#),
                "min_kiln_version",
            ),
            (
                "test-theme",
                Some("[css]\nengine = \"plain\""),
                "unknown field `engine`",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            write_test_file(root.path(), "config.toml", &format!("theme = {theme:?}"));
            if let Some(theme_toml) = theme_toml {
                write_test_file(root.path(), "themes/test-theme/theme.toml", theme_toml);
            }
            let error = format!("{:#}", Config::load(root.path()).unwrap_err());
            assert!(error.contains(expected), "{theme}: {error}");
        }
    }

    #[test]
    fn load_param_type_mismatch_returns_error() {
        for (site, theme, expected) in [
            (
                r#"key = "site""#,
                "[params.key]\nfoo = \"bar\"",
                "param `key` has type `string` in site config but `table` in theme",
            ),
            (
                "comments.provider.enabled = true",
                "[params]\ncomments.provider.enabled = \"yes\"",
                "param `comments.provider.enabled` has type `boolean` in site config but `string` in theme",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            write_test_file(
                root.path(),
                "config.toml",
                &format!("theme = \"test-theme\"\n[params]\n{site}"),
            );
            write_test_file(root.path(), "themes/test-theme/theme.toml", theme);
            assert_eq!(Config::load(root.path()).unwrap_err().to_string(), expected);
        }
    }

    // ── Config::theme_dir ──

    #[test]
    fn theme_dir_resolves_configured_theme() {
        let root = Path::new("/project");
        let mut config = Config::default();
        assert_eq!(config.theme_dir(root), None);
        config.theme = Some("example-theme".into());
        assert_eq!(
            config.theme_dir(root),
            Some(root.join("themes/example-theme"))
        );
    }
}
