use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use indoc::indoc;

use crate::config::validate_theme_name;

/// Scaffolds a new theme directory under `themes/<name>/`. Fails if the directory already exists.
///
/// # Errors
///
/// Returns an error if the theme directory already exists or if any file operation fails.
pub fn init_theme(root: &Path, name: &str) -> Result<()> {
    validate_theme_name(name)?;
    let theme_dir = root.join("themes").join(name);
    if theme_dir.exists() {
        bail!("theme directory already exists: {}", theme_dir.display());
    }

    let css_dir = theme_dir.join("assets/css/_src");
    let templates_dir = theme_dir.join("templates");
    let i18n_dir = theme_dir.join("i18n");
    fs::create_dir_all(&css_dir).context("failed to create CSS source directory")?;
    fs::create_dir_all(&templates_dir).context("failed to create templates directory")?;
    fs::create_dir_all(theme_dir.join("static")).context("failed to create static directory")?;
    fs::create_dir_all(&i18n_dir).context("failed to create i18n directory")?;

    fs::write(theme_dir.join("theme.toml"), "").context("failed to write theme.toml")?;
    fs::write(css_dir.join("style.css"), "").context("failed to write CSS entry")?;
    fs::write(
        templates_dir.join("base.html"),
        indoc! {r#"
            <!DOCTYPE html>
            <html lang="{{ config.language }}">
              <head>
                <meta charset="utf-8">
                {% block title %}<title>{{ config.title }}</title>{% endblock %}
                <link rel="stylesheet" href="{{ asset_url('/assets/css/site.css') | safe }}">
                {% block head %}{% endblock %}
              </head>
              <body>
                {% block body %}{% endblock %}
              </body>
            </html>
        "#},
    )
    .context("failed to write base.html")?;

    fs::write(
        templates_dir.join("post.html"),
        indoc! {r#"
            {% extends "base.html" %}

            {% block title %}<title>{{ title }} - {{ config.title }}</title>{% endblock %}

            {% block head %}
            {% if page_css %}<link rel="stylesheet" href="{{ page_css | safe }}">{% endif %}
            {% endblock %}

            {% block body %}
            <article>
              <h1>{{ title }}</h1>
              <div class="content">{{ content | safe }}</div>
            </article>
            {% endblock %}
        "#},
    )
    .context("failed to write post.html")?;

    fs::write(i18n_dir.join("en.toml"), DEFAULT_I18N_EN).context("failed to write i18n/en.toml")?;
    fs::write(i18n_dir.join("zh-Hans.toml"), DEFAULT_I18N_ZH_HANS)
        .context("failed to write i18n/zh-Hans.toml")?;

    println!("Theme `{name}` created at {}", theme_dir.display());
    println!(r#"Set `theme = "{name}"` in your config.toml to use it."#);
    Ok(())
}

/// Default English i18n table written to new themes.
const DEFAULT_I18N_EN: &str = indoc! {r#"
    all_posts = "All Posts"
    back_to_top = "Back to Top"
    table_of_contents = "Table of Contents"
"#};

/// Default Simplified Chinese i18n table written to new themes.
const DEFAULT_I18N_ZH_HANS: &str = indoc! {r#"
    all_posts = "全部文章"
    back_to_top = "回到顶部"
    table_of_contents = "目录"
"#};

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

        let site = tempfile::tempdir().unwrap();
        let en_i18n = crate::i18n::I18n::load(site.path(), Some(&theme_dir), "en").unwrap();
        assert_eq!(en_i18n.t("all_posts").as_ref(), "All Posts");
        let zh_i18n = crate::i18n::I18n::load(site.path(), Some(&theme_dir), "zh-Hans").unwrap();
        assert_eq!(zh_i18n.t("all_posts").as_ref(), "全部文章");
    }

    #[test]
    fn init_theme_invalid_name_returns_error() {
        let root = tempfile::tempdir().unwrap();
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "nested/theme",
            "nested\\theme",
            "/absolute",
            "C:theme",
        ] {
            let error = init_theme(root.path(), name).unwrap_err();
            assert!(
                error.to_string().contains("single directory name"),
                "{error}"
            );
        }
        assert!(!root.path().join("themes").exists());
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

    #[cfg(unix)]
    #[test]
    fn init_theme_unwritable_root_returns_error() {
        use crate::test_utils::PermissionGuard;

        let root = tempfile::tempdir().unwrap();
        let _guard = PermissionGuard::restrict(root.path(), 0o555);

        let err = init_theme(root.path(), "my-theme").unwrap_err().to_string();
        assert!(
            err.contains("failed to create CSS source directory"),
            "should report directory creation failure, got: {err}"
        );
    }
}
