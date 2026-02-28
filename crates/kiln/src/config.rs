use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
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
    pub author: Author,
}

/// Theme metadata loaded from `themes/<name>/theme.toml`.
#[derive(Debug, Deserialize)]
struct ThemeMeta {
    name: String,

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
            theme.check_min_kiln_version()?;
            tracing::info!("using theme: {}", theme.name);
            merge_params(&mut config.params, &theme.params)?;
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

/// Kiln version from `Cargo.toml`, checked at compile time.
const KILN_VERSION: &str = env!("CARGO_PKG_VERSION");

impl ThemeMeta {
    fn load(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read theme.toml at {}", path.display()))?;
        toml::from_str(&contents).context("failed to parse theme.toml")
    }

    fn check_min_kiln_version(&self) -> Result<()> {
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
            bail!(
                "theme `{name}` requires kiln >= {required}, but this is kiln {current}",
                name = self.name
            );
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
    String::from("http://localhost:1313")
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

    use super::*;

    // -- deserialization --

    #[test]
    fn defaults_when_empty() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.base_url, "http://localhost:1313");
        assert_eq!(config.title, "My Site");
        assert!(config.description.is_empty());
        assert_eq!(config.language, "en");
        assert_eq!(config.output_dir, "public");
        assert!(config.theme.is_none());
        assert!(config.params.is_empty());
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

    // -- load --

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
        assert_eq!(config.base_url, "http://localhost:1313");
    }

    #[test]
    fn load_invalid_toml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        fs::write(&config_path, "{{invalid toml").unwrap();

        let result = Config::load(dir.path());
        assert!(result.is_err());
    }

    // -- load (theme) --

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
                fontawesome = false
                max_lines = 40
            "#},
        );

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(
            config.params.get("fontawesome"),
            Some(&toml::Value::Boolean(true)),
            "should override theme defaults"
        );
        assert_eq!(
            config.params.get("max_lines"),
            Some(&toml::Value::Integer(40)),
            "should fill in missing site params from theme"
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
    fn load_theme_missing_name_returns_error() {
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
            indoc! {r"
                [params]
                foo = true
            "},
        );

        let err = Config::load(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("failed to parse theme.toml"),
            "should report parse error, got: {err}"
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

    // -- theme_dir --

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

    // -- merge_params --

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
