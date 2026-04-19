use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use indoc::indoc;

/// Scaffolds a new theme directory under `themes/<name>/`.
///
/// Creates `theme.toml`, `templates/base.html`, `templates/post.html`,
/// `i18n/en.toml`, `i18n/zh-Hans.toml`, and empty `static/` and `i18n/`
/// directories. Fails if the theme directory already exists to prevent
/// accidental overwrites.
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
    let i18n_dir = theme_dir.join("i18n");
    fs::create_dir_all(&templates_dir).context("failed to create templates directory")?;
    fs::create_dir_all(theme_dir.join("static")).context("failed to create static directory")?;
    fs::create_dir_all(&i18n_dir).context("failed to create i18n directory")?;

    fs::write(theme_dir.join("theme.toml"), "").context("failed to write theme.toml")?;
    fs::write(
        templates_dir.join("base.html"),
        indoc! {r#"
            <!DOCTYPE html>
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
        "#},
    )
    .context("failed to write base.html")?;

    fs::write(
        templates_dir.join("post.html"),
        indoc! {r#"
            {% extends "base.html" %}

            {% block title %}<title>{{ title }} - {{ config.title }}</title>{% endblock %}

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
    println!("Set `theme = \"{name}\"` in your config.toml to use it.");
    Ok(())
}

/// Default English i18n table written to new themes.
///
/// The resolver loads strings from three layers in descending precedence:
/// `<site>/i18n/<lang>.toml` → `<theme>/i18n/<lang>.toml` →
/// `<theme>/i18n/en.toml`. `date_format` is a strftime template used by
/// the `localdate` filter and must appear somewhere in the merge chain.
const DEFAULT_I18N_EN: &str = indoc! {r#"
    # English strings for this theme.
    #
    # The i18n system resolves each key by merging, in order of
    # decreasing precedence:
    #
    #   1. <site>/i18n/<language>.toml  (site override)
    #   2. <theme>/i18n/<language>.toml (active language)
    #   3. <theme>/i18n/en.toml         (this file — ultimate fallback)
    #
    # Keys are flat string values. Templates call `{{ t("key") }}`, or
    # `{{ t("key", name=value) }}` to substitute `{name}` placeholders.

    # `date_format` is a strftime template consumed by the `localdate`
    # filter, e.g. `{{ page.date | localdate }}`.
    date_format = "%Y-%m-%d"

    all_posts = "All Posts"
    back_to_top = "Back to Top"
    table_of_contents = "Table of Contents"
"#};

/// Default Simplified Chinese i18n table written to new themes.
const DEFAULT_I18N_ZH_HANS: &str = indoc! {r#"
    # Simplified Chinese strings for this theme.
    # See i18n/en.toml for a description of the resolution order.

    date_format = "%Y年%m月%d日"

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
        assert!(theme_dir.join("i18n").join("en.toml").exists());
        assert!(theme_dir.join("i18n").join("zh-Hans.toml").exists());

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
    fn init_theme_scaffolds_i18n_files() {
        let root = tempfile::tempdir().unwrap();
        init_theme(root.path(), "my-theme").unwrap();

        let i18n_dir = root.path().join("themes").join("my-theme").join("i18n");
        let en = fs::read_to_string(i18n_dir.join("en.toml")).unwrap();
        assert!(
            en.contains(r#"date_format = "%Y-%m-%d""#),
            "en.toml should declare date_format, got:\n{en}"
        );
        assert!(
            en.contains(r#"all_posts = "All Posts""#),
            "en.toml should include example keys, got:\n{en}"
        );

        let zh = fs::read_to_string(i18n_dir.join("zh-Hans.toml")).unwrap();
        assert!(
            zh.contains("date_format ="),
            "zh-Hans.toml should declare date_format, got:\n{zh}"
        );
        assert!(
            zh.contains(r#"all_posts = "全部文章""#),
            "zh-Hans.toml should include localized example keys, got:\n{zh}"
        );

        // Loader must accept the scaffold as-is in both languages.
        let theme_dir = root.path().join("themes").join("my-theme");
        let site = tempfile::tempdir().unwrap();
        let en_i18n = crate::i18n::I18n::load(site.path(), Some(&theme_dir), "en").unwrap();
        assert_eq!(en_i18n.t("all_posts").as_ref(), "All Posts");
        let zh_i18n = crate::i18n::I18n::load(site.path(), Some(&theme_dir), "zh-Hans").unwrap();
        assert_eq!(zh_i18n.t("all_posts").as_ref(), "全部文章");
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
