use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::render::lqip::ImageConfig;

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

    /// Site time zone (IANA name, e.g., `Asia/Shanghai`). When unset, dates render in UTC.
    #[serde(default)]
    pub timezone: Option<String>,

    #[serde(default)]
    pub enable_git_info: bool,

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
    pub css: Css,

    /// Named menu groups (e.g., `[[menu.main]]`, `[[menu.social]]`). Themes choose which groups
    /// to render and where. kiln has no opinion about group names.
    #[serde(default)]
    pub menu: BTreeMap<String, Vec<MenuItem>>,

    #[serde(default)]
    pub author: Author,

    #[serde(default)]
    pub image: ImageConfig,
}

/// Theme metadata loaded from `themes/<name>/theme.toml`.
#[derive(Debug, Deserialize)]
struct ThemeMeta {
    #[serde(default)]
    min_kiln_version: Option<String>,

    #[serde(default)]
    css: Css,

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
/// When enabled, kiln runs Pagefind as a post-build step to generate a search index under
/// `{output_dir}/pagefind/`. The `pagefind` binary must be installed separately.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Search {
    /// Enable Pagefind search indexing after build.
    #[serde(default)]
    pub enabled: bool,

    /// Path or name of the Pagefind binary (defaults to `"pagefind"` on `$PATH`).
    #[serde(default)]
    pub binary: Option<String>,
}

/// CSS compilation settings inherited from the active theme.
#[derive(Debug, Default, Deserialize, Serialize)]
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

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            title: default_title(),
            description: String::new(),
            language: default_language(),
            timezone: None,
            enable_git_info: false,
            output_dir: default_output_dir(),
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

impl Config {
    /// Loads site configuration from `config.toml` in the given root.
    ///
    /// Missing `config.toml` uses defaults. A configured theme supplies default params and CSS
    /// settings through its required `theme.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file exists but cannot be read or parsed, or if a configured
    /// theme's `theme.toml` is missing or incompatible.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("config.toml");
        let mut config: Self = if path.exists() {
            let contents = fs::read_to_string(&path).context("failed to read config.toml")?;
            toml::from_str(&contents).context("failed to parse config.toml")?
        } else {
            Self::default()
        };

        config
            .image
            .validate()
            .context("invalid image configuration")?;

        if let Some(ref theme_name) = config.theme {
            validate_theme_name(theme_name)?;
            let theme_toml = root.join("themes").join(theme_name).join("theme.toml");
            let theme = ThemeMeta::load(&theme_toml)?;
            theme.check_min_kiln_version(theme_name)?;
            tracing::info!("using theme: {theme_name}");
            merge_params(&mut config.params, &theme.params)?;
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

    /// Resolves `output_dir`, rejecting overlaps with project inputs and metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be resolved or would overwrite project inputs.
    pub fn resolved_output_dir(&self, root: &Path) -> Result<PathBuf> {
        ensure!(!self.output_dir.is_empty(), "output_dir cannot be empty");
        self.validate_output_dir(root, &root.join(&self.output_dir))
    }

    pub(crate) fn validate_output_dir(&self, root: &Path, output: &Path) -> Result<PathBuf> {
        ensure!(!output.as_os_str().is_empty(), "output_dir cannot be empty");
        let canonical = canonicalize_via_parent(output)?;
        let canonical_root = root
            .canonicalize()
            .with_context(|| format!("failed to canonicalize project root {}", root.display()))?;
        ensure!(
            !canonical_root.starts_with(&canonical),
            "output directory {} would overwrite the project root at {}",
            canonical.display(),
            canonical_root.display()
        );
        ensure!(
            !canonical.exists() || canonical.is_dir(),
            "output directory {} is not a directory",
            canonical.display()
        );

        for name in ["assets", "content", "i18n", "static", "templates"] {
            validate_input_tree(&root.join(name), &canonical)?;
        }
        validate_output_overlap(&root.join("themes"), &canonical)?;
        if let Some(theme) = self.theme_dir(root) {
            for name in ["assets", "i18n", "static", "templates", "theme.toml"] {
                validate_input_tree(&theme.join(name), &canonical)?;
            }
            validate_git_metadata(&theme, &canonical, true)?;
        }
        validate_output_overlap(&root.join("config.toml"), &canonical)?;
        for ancestor in canonical_root.ancestors() {
            validate_git_metadata(ancestor, &canonical, ancestor == canonical_root)?;
        }
        Ok(canonical)
    }

    /// Resolves the configured site time zone, if present.
    ///
    /// # Errors
    ///
    /// Returns an error if `timezone` is set but is not an IANA time zone name `jiff` recognizes.
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

pub(crate) fn validate_theme_name(name: &str) -> Result<()> {
    let mut components = Path::new(name).components();
    if name.is_empty()
        || name.contains(['/', '\\', ':'])
        || !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        bail!("theme must be a single directory name: `{name}`");
    }
    Ok(())
}

/// Merges theme default params into site params. Site values take precedence.
/// Nested tables are merged recursively. Returns an error on type mismatch.
fn merge_params(site: &mut toml::Table, theme_defaults: &toml::Table) -> Result<()> {
    merge_params_at(site, theme_defaults, "")
}

fn merge_params_at(
    site: &mut toml::Table,
    theme_defaults: &toml::Table,
    prefix: &str,
) -> Result<()> {
    for (key, theme_val) in theme_defaults {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if let Some(site_val) = site.get_mut(key) {
            match (site_val, theme_val) {
                (toml::Value::Table(st), toml::Value::Table(tt)) => {
                    merge_params_at(st, tt, &path)?;
                }
                (s, t) if s.type_str() != t.type_str() => {
                    bail!(
                        "param `{path}` has type `{}` in site config but `{}` in theme",
                        s.type_str(),
                        t.type_str(),
                    );
                }
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

fn validate_input_tree(path: &Path, output: &Path) -> Result<()> {
    validate_output_overlap(path, output)?;
    if !path.exists() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(path).follow_links(true) {
        let entry = match entry {
            Err(error)
                if error.loop_ancestor().is_some()
                    || error
                        .io_error()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                continue;
            }
            entry => {
                entry.with_context(|| format!("failed to inspect input {}", path.display()))?
            }
        };
        if entry.path_is_symlink() {
            validate_output_overlap(entry.path(), output)?;
        }
    }
    Ok(())
}

fn validate_git_metadata(root: &Path, output: &Path, reserve_name: bool) -> Result<()> {
    let git_file = root.join(".git");
    if reserve_name || git_file.exists() || git_file.is_symlink() {
        validate_output_overlap(&git_file, output)?;
    }
    if git_file.is_file() {
        let contents = fs::read_to_string(&git_file).context("failed to read .git file")?;
        if let Some(git_dir) = contents.trim().strip_prefix("gitdir: ") {
            let git_dir = root.join(git_dir);
            validate_output_overlap(&git_dir, output)?;
            let common_dir = git_dir.join("commondir");
            if common_dir.is_file() {
                let common = fs::read_to_string(common_dir)
                    .context("failed to read Git common directory")?;
                validate_output_overlap(&git_dir.join(common.trim()), output)?;
            }
        }
    }
    Ok(())
}

fn validate_output_overlap(input: &Path, output: &Path) -> Result<()> {
    let input = canonicalize_via_parent(input)?;
    ensure!(
        !input.starts_with(output) && !output.starts_with(&input),
        "output directory {} overlaps project input {}",
        output.display(),
        input.display()
    );
    Ok(())
}

/// Resolves symlinks before parent components, including paths with nonexistent components.
fn canonicalize_via_parent(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            component => {
                resolved.push(component);
                if resolved.exists() || resolved.is_symlink() {
                    resolved = resolved
                        .canonicalize()
                        .with_context(|| format!("failed to resolve {}", resolved.display()))?;
                }
            }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::serve::{DEFAULT_PORT, localhost_url};

    // ── Config::default ──

    #[test]
    fn default_uses_site_defaults() {
        let config = Config::default();
        assert_eq!(config.base_url, localhost_url(DEFAULT_PORT));
        assert_eq!(config.title, "My Site");
        assert_eq!(config.description, "");
        assert_eq!(config.language, "en");
        assert!(config.timezone.is_none());
        assert!(!config.enable_git_info);
        assert_eq!(config.output_dir, "public");
        assert!(config.theme.is_none());
        assert!(config.params.is_empty());
        assert!(!config.search.enabled);
        assert!(config.search.binary.is_none());
        assert!(config.menu.is_empty());
        assert_eq!(config.author.name, "");
        assert_eq!(config.author.email, "");
        assert_eq!(config.author.link, "");
        assert_eq!(config.image.lqip_size, 16);
        assert_eq!(config.image.lqip_quality, 25);
    }

    #[test]
    fn default_matches_empty_toml() {
        let from_default = Config::default();
        let from_toml: Config = toml::from_str("").unwrap();
        assert_eq!(
            toml::to_string(&from_default).unwrap(),
            toml::to_string(&from_toml).unwrap(),
        );
    }

    // ── deserialization ──

    #[test]
    fn overrides_from_toml() {
        let toml_str = indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"
            description = "Test Description"
            language = "zh-CN"
            timezone = "Asia/Shanghai"
            enable_git_info = true
            output_dir = "dist"
            theme = "example-theme"

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
        assert!(config.enable_git_info);
        assert_eq!(config.output_dir, "dist");
        assert_eq!(config.theme.as_deref(), Some("example-theme"));
        assert_eq!(
            config.params.get("fontawesome"),
            Some(&toml::Value::Boolean(true)),
        );
        assert_eq!(config.author.name, "Alice");
        assert_eq!(config.author.email, "alice@example.com");
        assert_eq!(config.author.link, "https://alice.example.com");
    }

    #[test]
    fn image_from_toml() {
        let config: Config = toml::from_str(indoc! {r"
            [image]
            lqip_size = 24
            lqip_quality = 50
        "})
        .unwrap();
        assert_eq!(config.image.lqip_size, 24);
        assert_eq!(config.image.lqip_quality, 50);
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

        let main = &config.menu["main"];
        assert_eq!(main.len(), 3);
        assert_eq!(main[0].name, "Posts");
        assert_eq!(main[0].url, "/posts/");
        assert_eq!(main[0].icon.as_deref(), Some("fas fa-archive"));
        assert_eq!(main[0].weight, 1);
        assert!(!main[0].external);
        assert_eq!(main[1].name, "GitHub");
        assert_eq!(main[1].url, "https://github.com/user");
        assert_eq!(main[1].weight, 10);
        assert!(main[1].external);
        assert_eq!(main[2].name, "About");
        assert_eq!(main[2].url, "/about/");
        assert_eq!(main[2].weight, 5);
        assert!(main[2].icon.is_none());
    }

    #[test]
    fn menu_item_defaults() {
        let config: Config = toml::from_str(indoc! {r#"
            [[menu.main]]
            name = "Home"
            url = "/"
        "#})
        .unwrap();

        let main = &config.menu["main"];
        assert_eq!(main.len(), 1);
        let item = &main[0];
        assert_eq!(item.name, "Home");
        assert_eq!(item.url, "/");
        assert!(item.icon.is_none());
        assert_eq!(item.weight, 0);
        assert!(!item.external);
    }

    #[test]
    fn deserialize_unknown_css_processor_returns_error() {
        assert!(
            toml::from_str::<Config>(indoc! {r#"
                [css]
                processor = "unknown"
            "#})
            .is_err()
        );
    }

    // ── load ──

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            indoc! {r#"
                base_url = "https://example.com"
                title = "Example Site"
            "#},
        )
        .unwrap();

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.base_url, "https://example.com");
        assert_eq!(config.title, "Example Site");
    }

    #[test]
    fn load_missing_file_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.base_url, localhost_url(DEFAULT_PORT));
        assert!(config.theme.is_none());
        assert!(config.params.is_empty());
    }

    #[test]
    fn load_sorts_menu_groups_independently() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            indoc! {r#"
                [[menu.main]]
                name = "A"
                url = "/a/"
                weight = 2

                [[menu.main]]
                name = "M"
                url = "/m/"
                weight = 3

                [[menu.main]]
                name = "Z"
                url = "/z/"
                weight = 1

                [[menu.social]]
                name = "Y"
                url = "/y/"
                weight = 20

                [[menu.social]]
                name = "Z"
                url = "/z/"
                weight = 10
                external = true
            "#},
        )
        .unwrap();

        let config = Config::load(dir.path()).unwrap();
        let main: Vec<&str> = config.menu["main"]
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        let social: Vec<&str> = config.menu["social"]
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(main, ["Z", "A", "M"]);
        assert_eq!(social, ["Z", "Y"]);
        assert!(config.menu["social"][0].external);
        assert!(!config.menu["main"][0].external);
    }

    #[test]
    fn load_invalid_toml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        fs::write(&config_path, "{{invalid toml").unwrap();

        let result = Config::load(dir.path());
        assert!(result.is_err());
    }

    // ── load (theme) ──

    #[test]
    fn load_theme_merges_scalar_and_nested_params() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            indoc! {r#"
                theme = "test-theme"

                [params]
                fontawesome = true

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

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.params["code_max_lines"], toml::Value::Integer(40));
        assert_eq!(config.params["fontawesome"], toml::Value::Boolean(true));
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
    fn load_css_inherits_theme_and_accepts_explicit_site_override() {
        let root = tempfile::tempdir().unwrap();
        setup_theme(
            root.path(),
            indoc! {r#"
                [css]
                processor = "tailwind"
            "#},
        );
        fs::write(root.path().join("config.toml"), r#"theme = "test-theme""#).unwrap();
        assert_eq!(
            Config::load(root.path()).unwrap().css.processor,
            Some(CssProcessor::Tailwind)
        );

        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                theme = "test-theme"
                [css]
                processor = "plain"
            "#},
        )
        .unwrap();
        assert_eq!(
            Config::load(root.path()).unwrap().css.processor,
            Some(CssProcessor::Plain)
        );
    }

    #[test]
    fn load_theme_accepts_compatible_min_version() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.toml"), r#"theme = "test-theme""#).unwrap();
        setup_theme(
            dir.path(),
            &format!(r#"min_kiln_version = "{KILN_VERSION}""#),
        );

        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.theme.as_deref(), Some("test-theme"));
    }

    #[test]
    fn load_theme_invalid_name_returns_error() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), r#"theme = "../outside""#).unwrap();
        let error = Config::load(root.path()).unwrap_err();
        assert!(
            error.to_string().contains("single directory name"),
            "{error}"
        );
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

    fn setup_theme(root: &Path, theme_toml: &str) {
        let theme_dir = root.join("themes").join("test-theme");
        fs::create_dir_all(&theme_dir).unwrap();
        fs::write(theme_dir.join("theme.toml"), theme_toml).unwrap();
    }

    // ── theme_dir ──

    #[test]
    fn theme_dir_returns_path_when_configured() {
        let config: Config = toml::from_str(r#"theme = "example-theme""#).unwrap();
        let root = Path::new("/project");
        assert_eq!(
            config.theme_dir(root),
            Some(root.join("themes").join("example-theme"))
        );
    }

    #[test]
    fn theme_dir_returns_none_without_theme() {
        let config = Config::default();
        let root = Path::new("/project");
        assert!(config.theme_dir(root).is_none());
    }

    // ── resolved_output_dir ──

    #[test]
    fn resolved_output_dir_default_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_root = dir.path().canonicalize().unwrap();
        let config = Config::default();

        let resolved = config.resolved_output_dir(dir.path()).unwrap();

        assert_eq!(resolved, canonical_root.join("public"));
    }

    #[test]
    fn resolved_output_dir_nested_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_root = dir.path().canonicalize().unwrap();

        let resolved = resolved_output_dir_for(dir.path(), "dist/site").unwrap();

        assert_eq!(resolved, canonical_root.join("dist").join("site"));
    }

    #[test]
    fn resolved_output_dir_canonicalizes_dotdot_segments() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("build")).unwrap();
        let canonical_root = dir.path().canonicalize().unwrap();

        let resolved = resolved_output_dir_for(dir.path(), "build/../out").unwrap();

        assert_eq!(resolved, canonical_root.join("out"));
    }

    #[test]
    fn resolved_output_dir_absolute_path_outside_root() {
        let root = tempfile::tempdir().unwrap();
        let target_parent = tempfile::tempdir().unwrap();
        let canonical_target_parent = target_parent.path().canonicalize().unwrap();
        let target = canonical_target_parent.join("site");

        let resolved = resolved_output_dir_for(root.path(), &target.to_string_lossy()).unwrap();

        assert_eq!(resolved, target);
    }

    #[test]
    fn resolved_output_dir_relative_path_outside_root() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("project");
        fs::create_dir(&root).unwrap();
        let canonical_outer = outer.path().canonicalize().unwrap();

        let resolved = resolved_output_dir_for(&root, "../sibling").unwrap();

        assert_eq!(resolved, canonical_outer.join("sibling"));
    }

    #[test]
    fn resolved_output_dir_empty_returns_error() {
        let dir = tempfile::tempdir().unwrap();

        let err = resolved_output_dir_for(dir.path(), "")
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("output_dir cannot be empty"),
            "should reject empty string, got: {err}"
        );
    }

    #[test]
    fn resolved_output_dir_root_or_ancestor_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_root = dir.path().canonicalize().unwrap();
        let filesystem_root = canonical_root.ancestors().last().unwrap();

        for output in [
            ".",
            "..",
            canonical_root.to_str().unwrap(),
            filesystem_root.to_str().unwrap(),
        ] {
            let error = resolved_output_dir_for(dir.path(), output)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("would overwrite the project root"),
                "{output}: {error}"
            );
        }
    }

    #[test]
    fn resolved_output_dir_input_or_metadata_overlap_returns_error() {
        let root = tempfile::tempdir().unwrap();
        for output in [
            "content",
            "content/posts/generated",
            "static",
            "assets/css",
            "templates",
            "i18n",
            "themes",
            "themes/example/generated",
            "config.toml",
            ".git",
            ".git/objects",
            "missing/../content",
        ] {
            let error = resolved_output_dir_for(root.path(), output).unwrap_err();
            assert!(
                error.to_string().contains("overlaps project input"),
                "{output}: {error}"
            );
        }
    }

    #[test]
    fn resolved_output_dir_linked_git_metadata_returns_error() {
        let root = tempfile::tempdir().unwrap();
        let repository = tempfile::tempdir().unwrap();
        let git_dir = repository.path().join("worktrees/example");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(
            root.path().join(".git"),
            format!("gitdir: {}", git_dir.display()),
        )
        .unwrap();
        fs::write(git_dir.join("commondir"), "../..").unwrap();

        assert_eq!(
            resolved_output_dir_for(root.path(), "public").unwrap(),
            root.path().canonicalize().unwrap().join("public")
        );

        for output in [git_dir.join("objects"), repository.path().join("objects")] {
            assert!(
                resolved_output_dir_for(root.path(), &output.to_string_lossy())
                    .unwrap_err()
                    .to_string()
                    .contains("overlaps project input")
            );
        }
    }

    #[test]
    fn resolved_output_dir_parent_repository_metadata_returns_error() {
        let repository = tempfile::tempdir().unwrap();
        let root = repository.path().join("site");
        fs::create_dir(&root).unwrap();
        fs::create_dir(repository.path().join(".git")).unwrap();
        assert!(
            resolved_output_dir_for(&root, "../.git/objects")
                .unwrap_err()
                .to_string()
                .contains("overlaps project input")
        );
        assert_eq!(
            resolved_output_dir_for(&root, "../dist").unwrap(),
            repository.path().canonicalize().unwrap().join("dist")
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolved_output_dir_symlink_to_root_or_ancestor_returns_error() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("project");
        fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&root, root.join("root-link")).unwrap();
        std::os::unix::fs::symlink(outer.path(), root.join("ancestor-link")).unwrap();

        for output_dir in ["root-link", "ancestor-link"] {
            let err = resolved_output_dir_for(&root, output_dir)
                .unwrap_err()
                .to_string();

            assert!(
                err.contains("would overwrite the project root"),
                "should reject {output_dir}, got: {err}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolved_output_dir_symlinked_inputs_and_output_returns_error() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("content/posts/example")).unwrap();
        fs::write(external.path().join("image.png"), "original").unwrap();
        std::os::unix::fs::symlink(
            external.path(),
            root.path().join("content/posts/example/images"),
        )
        .unwrap();
        std::os::unix::fs::symlink(root.path().join("content"), root.path().join("alias")).unwrap();

        for output in [
            external.path().to_owned(),
            external.path().join("generated"),
            root.path().join("alias/posts"),
        ] {
            let error =
                resolved_output_dir_for(root.path(), &output.to_string_lossy()).unwrap_err();
            assert!(
                error.to_string().contains("overlaps project input"),
                "{error}"
            );
        }
        assert_eq!(
            fs::read_to_string(external.path().join("image.png")).unwrap(),
            "original"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolved_output_dir_linked_theme_protects_inputs_and_metadata() {
        use std::os::unix::fs::symlink;

        let theme = tempfile::tempdir().unwrap();
        let external_site = tempfile::tempdir().unwrap();
        let linked_assets = tempfile::tempdir().unwrap();
        fs::write(theme.path().join("theme.toml"), "").unwrap();
        symlink(linked_assets.path(), theme.path().join("assets")).unwrap();
        fs::create_dir(theme.path().join(".git")).unwrap();

        for root in [
            theme.path().join("example"),
            external_site.path().join("site"),
        ] {
            fs::create_dir_all(root.join("themes")).unwrap();
            let target = if root.starts_with(theme.path()) {
                Path::new("../..")
            } else {
                theme.path()
            };
            symlink(target, root.join("themes/example")).unwrap();
            fs::write(root.join("config.toml"), r#"theme = "example""#).unwrap();
            let config = Config::load(&root).unwrap();
            assert_eq!(
                config.resolved_output_dir(&root).unwrap(),
                root.canonicalize().unwrap().join("public")
            );
            assert_eq!(
                config
                    .validate_output_dir(&root, &theme.path().join("generated"))
                    .unwrap(),
                theme.path().canonicalize().unwrap().join("generated")
            );

            for output in [
                theme.path().join("theme.toml/generated"),
                theme.path().join("i18n/generated"),
                theme.path().join("static/generated"),
                theme.path().join("templates/generated"),
                linked_assets.path().join("generated"),
                theme.path().join(".git/generated"),
            ] {
                let error = config.validate_output_dir(&root, &output).unwrap_err();
                assert!(error.to_string().contains("overlaps project input"));
            }
            for output in [theme.path(), theme.path().parent().unwrap()] {
                assert!(config.validate_output_dir(&root, output).is_err());
            }
        }
    }

    fn resolved_output_dir_for(root: &Path, output_dir: &str) -> Result<PathBuf> {
        let toml_str = format!("output_dir = {}", toml::Value::String(output_dir.into()));
        let config: Config = toml::from_str(&toml_str).unwrap();
        config.resolved_output_dir(root)
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
    fn merge_params_nested_type_mismatch_returns_error() {
        let mut site: toml::Table = toml::from_str("comments.provider.enabled = true").unwrap();
        let theme: toml::Table = toml::from_str(r#"comments.provider.enabled = "yes""#).unwrap();
        let error = merge_params(&mut site, &theme).unwrap_err();
        assert_eq!(
            error.to_string(),
            "param `comments.provider.enabled` has type `boolean` in site config but `string` in theme"
        );
    }

    #[test]
    fn merge_params_type_mismatch_returns_error() {
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
