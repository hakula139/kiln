use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

const BASE_HTML: &str = r#"<!DOCTYPE html>
<html lang="{{ config.language }}">
  <head>
    <meta charset="utf-8">
    {% block title %}<title>{{ config.title }}</title>{% endblock %}
    {% block head %}{% endblock %}
  </head>
  <body>
    {% block body %}{% endblock %}
  </body>
</html>
"#;

const POST_HTML: &str = r#"{% extends "base.html" %}

{% block title %}<title>{{ title }} - {{ config.title }}</title>{% endblock %}

{% block body %}
<article>
  <h1>{{ title }}</h1>
  <div class="content">{{ content | safe }}</div>
</article>
{% endblock %}
"#;

/// Scaffolds a new theme directory under `themes/<name>/`.
///
/// Creates `theme.toml`, `templates/base.html`, `templates/post.html`,
/// and an empty `static/` directory. Fails if the theme directory already
/// exists to prevent accidental overwrites.
///
/// # Errors
///
/// Returns an error if the theme directory already exists or if any file
/// operation fails.
pub fn init_theme(root: &Path, name: &str) -> Result<()> {
    let theme_dir = root.join("themes").join(name);
    if theme_dir.exists() {
        bail!("theme directory already exists: {}", theme_dir.display());
    }

    let templates_dir = theme_dir.join("templates");
    fs::create_dir_all(&templates_dir).context("failed to create templates directory")?;
    fs::create_dir_all(theme_dir.join("static")).context("failed to create static directory")?;

    fs::write(theme_dir.join("theme.toml"), "").context("failed to write theme.toml")?;
    fs::write(templates_dir.join("base.html"), BASE_HTML).context("failed to write base.html")?;
    fs::write(templates_dir.join("post.html"), POST_HTML).context("failed to write post.html")?;

    println!("Theme `{name}` created at {}", theme_dir.display());
    println!("Set `theme = \"{name}\"` in your config.toml to use it.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── init_theme ──

    #[test]
    fn init_theme_creates_structure() {
        let root = tempfile::tempdir().unwrap();
        init_theme(root.path(), "my-theme").unwrap();

        let theme_dir = root.path().join("themes").join("my-theme");
        assert!(theme_dir.join("theme.toml").exists());
        assert!(theme_dir.join("templates").join("base.html").exists());
        assert!(theme_dir.join("templates").join("post.html").exists());
        assert!(theme_dir.join("static").is_dir());

        // Templates should be valid (non-empty).
        let base = fs::read_to_string(theme_dir.join("templates").join("base.html")).unwrap();
        assert!(
            base.contains("{% block body %}"),
            "base.html should have body block"
        );
        let post = fs::read_to_string(theme_dir.join("templates").join("post.html")).unwrap();
        assert!(
            post.contains(r#"{% extends "base.html" %}"#),
            "post.html should extend base.html"
        );
    }

    #[test]
    fn init_theme_unwritable_root_returns_error() {
        use crate::test_utils::PermissionGuard;

        let root = tempfile::tempdir().unwrap();
        let _guard = PermissionGuard::restrict(root.path(), 0o555);

        let err = init_theme(root.path(), "my-theme").unwrap_err().to_string();
        assert!(
            err.contains("failed to create templates directory"),
            "should report directory creation failure, got: {err}"
        );
    }

    #[test]
    fn init_theme_existing_dir_returns_error() {
        let root = tempfile::tempdir().unwrap();
        init_theme(root.path(), "my-theme").unwrap();

        let err = init_theme(root.path(), "my-theme").unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "should report existing directory, got: {err}"
        );
    }
}
