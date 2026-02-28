pub mod build;
pub mod config;
pub mod content;
pub mod directive;
pub mod markdown;
pub mod output;
pub mod render;
pub mod template;

pub use build::build;

#[cfg(test)]
pub(crate) mod test_utils {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::LazyLock;

    use indoc::indoc;
    use tempfile::TempDir;

    use crate::config::Config;
    use crate::template::TemplateEngine;

    static BASE_HTML: &str = indoc! {r#"
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
    "#};

    static POST_HTML: &str = indoc! {r#"
        {% extends "base.html" %}

        {% block title %}<title>{{ title }} - {{ config.title }}</title>{% endblock %}

        {% block head %}
            {%- if description %}
            <meta name="description" content="{{ description }}">
            {%- endif %}
            <link rel="canonical" href="{{ url | safe }}">
            <meta property="og:title" content="{{ title }}">
            <meta property="og:description" content="{{ description }}">
            <meta property="og:url" content="{{ url | safe }}">
            <meta property="og:type" content="article">
            <meta property="og:site_name" content="{{ config.title }}">
            {%- if featured_image %}
            <meta property="og:image" content="{{ config.base_url | safe }}{{ featured_image | safe }}">
            {%- endif %}
            <meta name="twitter:card" content="{% if featured_image %}summary_large_image{% else %}summary{% endif %}">
        {% endblock %}

        {% block body %}
            <article>
              <header>
                <h1>{{ title }}</h1>
                {% if date %}<time datetime="{{ date }}">{{ date }}</time>{% endif %}
              </header>
              {% if toc %}<aside>{{ toc | safe }}</aside>{% endif %}
              <div class="content">{{ content | safe }}</div>
            </article>
        {% endblock %}
    "#};

    /// Persistent temp directory holding test templates (lives for the process).
    static TEST_TEMPLATE_DIR: LazyLock<TempDir> = LazyLock::new(|| {
        let dir = TempDir::new().expect("failed to create test template dir");
        fs::write(dir.path().join("base.html"), BASE_HTML).unwrap();
        fs::write(dir.path().join("post.html"), POST_HTML).unwrap();
        dir
    });

    /// Returns the path to the test template directory.
    pub fn template_dir() -> PathBuf {
        TEST_TEMPLATE_DIR.path().to_owned()
    }

    /// Creates a `TemplateEngine` using embedded test templates.
    pub fn test_engine() -> TemplateEngine {
        TemplateEngine::new(None, Some(TEST_TEMPLATE_DIR.path())).unwrap()
    }

    /// Creates a `Config` with all defaults.
    pub fn test_config() -> Config {
        toml::from_str("").unwrap()
    }

    /// RAII guard that restores filesystem permissions on drop.
    ///
    /// Ensures cleanup happens even if the test panics, preventing
    /// `TempDir::drop` failures from leftover restricted permissions.
    pub struct PermissionGuard {
        path: PathBuf,
        mode: u32,
    }

    impl PermissionGuard {
        pub fn restrict(path: &Path, mode: u32) -> Self {
            let original = fs::metadata(path).unwrap().permissions().mode() & 0o7777;
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
            Self {
                path: path.to_owned(),
                mode: original,
            }
        }
    }

    impl Drop for PermissionGuard {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
        }
    }
}
