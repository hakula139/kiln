use std::path::Path;

use anyhow::{Context, Result, ensure};
use minijinja::path_loader;
use serde::Serialize;

use crate::config::Config;

#[derive(Debug)]
pub struct TemplateEngine {
    env: minijinja::Environment<'static>,
}

impl TemplateEngine {
    /// Creates a new template engine that loads templates from `template_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if `template_dir` does not exist or is not a directory.
    pub fn new(template_dir: &Path) -> Result<Self> {
        ensure!(
            template_dir.is_dir(),
            "template directory does not exist: {}",
            template_dir.display()
        );
        let mut env = minijinja::Environment::new();
        env.set_loader(path_loader(template_dir));
        Ok(Self { env })
    }

    /// Renders a post page using the `post.html` template.
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_post(&self, vars: &PostTemplateVars<'_>) -> Result<String> {
        let template = self
            .env
            .get_template("post.html")
            .context("failed to load post.html template")?;
        template
            .render(vars)
            .context("failed to render post template")
    }
}

/// Template variables for rendering a post page.
///
/// The `date` field is pre-formatted as a string so the template doesn't need
/// date logic. HTML fields (`content`, `toc`) use `| safe` in the template to
/// avoid double-escaping. All other string fields are auto-escaped by `MiniJinja`.
#[derive(Debug, Serialize)]
pub struct PostTemplateVars<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub url: &'a str,
    pub featured_image: Option<&'a str>,
    pub date: Option<String>,
    pub content: &'a str,
    pub toc: &'a str,
    pub config: &'a Config,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_engine() -> TemplateEngine {
        let template_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("templates");
        TemplateEngine::new(&template_dir).unwrap()
    }

    fn test_config() -> Config {
        toml::from_str("").unwrap()
    }

    // -- new --

    #[test]
    fn new_rejects_nonexistent_directory() {
        let err = TemplateEngine::new(Path::new("/nonexistent/path"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("template directory does not exist"),
            "should reject nonexistent directory, got: {err}"
        );
    }

    // -- render_post --

    #[test]
    fn render_post_basic() {
        let engine = test_engine();
        let config = test_config();
        let vars = PostTemplateVars {
            title: "Hello World",
            description: "A test post",
            url: "https://example.com/posts/hello-world/",
            featured_image: Some("/images/hello.webp"),
            date: Some("2026-02-24T12:34:56Z".into()),
            content: "<p>Body</p>",
            toc: "",
            config: &config,
        };
        let html = engine.render_post(&vars).unwrap();

        // <head>
        assert!(
            html.contains("<title>Hello World - My Site</title>"),
            "should have title tag, html:\n{html}"
        );
        assert!(
            html.contains(r#"<meta name="description" content="A test post">"#),
            "should have meta description, html:\n{html}"
        );
        assert!(
            html.contains(
                r#"<link rel="canonical" href="https://example.com/posts/hello-world/">"#
            ),
            "should have canonical link, html:\n{html}"
        );
        assert!(
            html.contains(r#"<meta property="og:title" content="Hello World">"#),
            "should have og:title, html:\n{html}"
        );
        assert!(
            html.contains(r#"<meta property="og:type" content="article">"#),
            "should have og:type article, html:\n{html}"
        );
        assert!(
            html.contains(
                r#"<meta property="og:image" content="http://localhost:1313/images/hello.webp">"#
            ),
            "should have og:image with absolute URL, html:\n{html}"
        );
        assert!(
            html.contains(r#"<meta name="twitter:card" content="summary_large_image">"#),
            "should use summary_large_image when featured_image present, html:\n{html}"
        );

        // <body>
        assert!(
            html.contains("<h1>Hello World</h1>"),
            "should have title heading, html:\n{html}"
        );
        assert!(
            html.contains("2026-02-24T12:34:56Z"),
            "should have date, html:\n{html}"
        );
        assert!(
            html.contains("<p>Body</p>"),
            "should have content, html:\n{html}"
        );
    }

    #[test]
    fn render_post_html_not_double_escaped() {
        let engine = test_engine();
        let config = test_config();
        let vars = PostTemplateVars {
            title: "Test",
            description: "",
            url: "",
            featured_image: None,
            date: None,
            content: "<strong>bold</strong>",
            toc: r#"<nav class="toc">ToC</nav>"#,
            config: &config,
        };
        let html = engine.render_post(&vars).unwrap();
        assert!(
            html.contains("<strong>bold</strong>"),
            "content should not be double-escaped, html:\n{html}"
        );
        assert!(
            html.contains(r#"<nav class="toc">ToC</nav>"#),
            "toc should not be double-escaped, html:\n{html}"
        );
    }

    #[test]
    fn render_post_title_auto_escaped() {
        let engine = test_engine();
        let config = test_config();
        let vars = PostTemplateVars {
            title: "<script>alert(1)</script>",
            description: "",
            url: "",
            featured_image: None,
            date: None,
            content: "",
            toc: "",
            config: &config,
        };
        let html = engine.render_post(&vars).unwrap();
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "title should be auto-escaped, html:\n{html}"
        );
        assert!(
            html.contains("&lt;script&gt;"),
            "title should contain escaped HTML entities, html:\n{html}"
        );
    }

    #[test]
    fn render_post_missing_template_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new(dir.path()).unwrap();
        let config = test_config();
        let vars = PostTemplateVars {
            title: "Test",
            description: "",
            url: "",
            featured_image: None,
            date: None,
            content: "",
            toc: "",
            config: &config,
        };
        let err = engine.render_post(&vars).unwrap_err().to_string();
        assert!(
            err.contains("failed to load post.html template"),
            "should have context message, got: {err}"
        );
    }
}
