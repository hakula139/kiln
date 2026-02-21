use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Site-wide configuration loaded from `config.toml`.
#[derive(Debug, Deserialize)]
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

    #[serde(default)]
    pub author: Author,
}

#[derive(Debug, Default, Deserialize)]
pub struct Author {
    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub email: String,

    #[serde(default)]
    pub link: String,
}

impl Config {
    /// Load configuration from `config.toml` in the given project root.
    ///
    /// Falls back to defaults if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the config file exists but cannot be read or parsed.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("config.toml");
        if path.exists() {
            let contents = fs::read_to_string(&path).context("failed to read config.toml")?;
            toml::from_str(&contents).context("failed to parse config.toml")
        } else {
            toml::from_str("").context("failed to construct default config")
        }
    }
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
    use super::*;

    #[test]
    fn defaults_when_empty() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.base_url, "http://localhost:1313");
        assert_eq!(config.title, "My Site");
        assert_eq!(config.language, "en");
        assert_eq!(config.output_dir, "public");
        assert!(config.author.name.is_empty());
    }

    #[test]
    fn overrides_from_toml() {
        let toml_str = r#"
            base_url = "https://example.com"
            title = "Test Site"
            description = "Test Description"
            language = "zh-CN"
            output_dir = "dist"

            [author]
            name = "Alice"
            email = "alice@example.com"
            link = "https://alice.example.com"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.base_url, "https://example.com");
        assert_eq!(config.title, "Test Site");
        assert_eq!(config.description, "Test Description");
        assert_eq!(config.language, "zh-CN");
        assert_eq!(config.output_dir, "dist");
        assert_eq!(config.author.name, "Alice");
        assert_eq!(config.author.email, "alice@example.com");
        assert_eq!(config.author.link, "https://alice.example.com");
    }

    #[test]
    fn load_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
                base_url = "https://hakula.xyz"
                title = "HAKULA†CHANNEL"
            "#,
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
        fs::write(&config_path, "Invalid TOML").unwrap();

        let result = Config::load(dir.path());
        assert!(result.is_err());
    }
}
