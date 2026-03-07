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
    /// Creates a new template engine with layered template loading.
    ///
    /// Templates are resolved by checking `site_dir` first (user overrides),
    /// then `theme_dir` (theme defaults). At least one directory must be
    /// provided.
    ///
    /// `site_dir` is silently ignored if it doesn't exist (it's an optional
    /// override layer). `theme_dir`, when provided, must exist.
    ///
    /// # Errors
    ///
    /// Returns an error if neither directory is provided, or if `theme_dir`
    /// is provided but does not exist.
    pub fn new(site_dir: Option<&Path>, theme_dir: Option<&Path>) -> Result<Self> {
        if let Some(d) = theme_dir {
            ensure!(
                d.is_dir(),
                "theme template directory does not exist: {}",
                d.display()
            );
        }

        // Site dir is optional — silently ignored if missing.
        let site_dir = site_dir.filter(|d| d.is_dir());

        ensure!(
            site_dir.is_some() || theme_dir.is_some(),
            "no valid template directory found"
        );

        let loaders: Vec<_> = [site_dir, theme_dir]
            .into_iter()
            .flatten()
            .map(path_loader)
            .collect();

        let mut env = minijinja::Environment::new();
        env.set_loader(move |name| {
            for loader in &loaders {
                if let Some(content) = loader(name)? {
                    return Ok(Some(content));
                }
            }
            Ok(None)
        });

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

    /// Tries to render a directive using a theme template at
    /// `directives/<name>.html`.
    ///
    /// Returns `None` if no template exists for the directive name.
    /// Returns `Some(Err(_))` if the template exists but rendering fails.
    pub fn render_directive(&self, name: &str, ctx: impl Serialize) -> Option<Result<String>> {
        let template_name = format!("directives/{name}.html");
        let template = self.env.get_template(&template_name).ok()?;
        Some(
            template
                .render(ctx)
                .with_context(|| format!("failed to render directive template: {template_name}")),
        )
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
    use std::fs as test_fs;

    use super::*;

    use crate::test_utils::{test_config, test_engine};

    // -- new --

    #[test]
    fn new_with_site_dir_only() {
        let dir = tempfile::tempdir().unwrap();
        test_fs::write(dir.path().join("test.html"), "hello").unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let tmpl = engine.env.get_template("test.html").unwrap();
        assert_eq!(tmpl.render(()).unwrap(), "hello");
    }

    #[test]
    fn new_site_overrides_theme() {
        let dir = tempfile::tempdir().unwrap();
        let site_dir = dir.path().join("site");
        let theme_dir = dir.path().join("theme");
        test_fs::create_dir_all(&site_dir).unwrap();
        test_fs::create_dir_all(&theme_dir).unwrap();

        // Same template in both — site should win.
        test_fs::write(site_dir.join("page.html"), "from site").unwrap();
        test_fs::write(theme_dir.join("page.html"), "from theme").unwrap();
        // Template only in theme — should fall through.
        test_fs::write(theme_dir.join("base.html"), "theme base").unwrap();

        let engine = TemplateEngine::new(Some(&site_dir), Some(&theme_dir)).unwrap();
        let page = engine.env.get_template("page.html").unwrap();
        assert_eq!(page.render(()).unwrap(), "from site");
        let base = engine.env.get_template("base.html").unwrap();
        assert_eq!(base.render(()).unwrap(), "theme base");
    }

    #[test]
    fn new_ignores_nonexistent_site_dir() {
        let dir = tempfile::tempdir().unwrap();
        let theme_dir = dir.path().join("theme");
        test_fs::create_dir(&theme_dir).unwrap();
        // site_dir doesn't exist — should not error.
        let result = TemplateEngine::new(Some(Path::new("/nonexistent")), Some(&theme_dir));
        assert!(result.is_ok());
    }

    #[test]
    fn new_rejects_no_dirs() {
        let err = TemplateEngine::new(None, None).unwrap_err().to_string();
        assert!(
            err.contains("no valid template directory found"),
            "should reject when no dirs provided, got: {err}"
        );
    }

    #[test]
    fn new_rejects_nonexistent_theme_dir() {
        let err = TemplateEngine::new(None, Some(Path::new("/nonexistent/path")))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("theme template directory does not exist"),
            "should reject nonexistent theme dir, got: {err}"
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
        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
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

    // -- render_directive --

    #[test]
    fn render_directive_renders_template() {
        #[derive(Serialize)]
        struct Ctx {
            name: String,
            body_html: String,
        }

        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("test.html"),
            "<div>{{ name }}: {{ body_html | safe }}</div>",
        )
        .unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let ctx = Ctx {
            name: "test".into(),
            body_html: "<p>hello</p>".into(),
        };

        let result = engine.render_directive("test", ctx);
        assert!(result.is_some(), "should find template");
        let html = result.unwrap().unwrap();
        assert!(
            html.contains("<div>test: <p>hello</p></div>"),
            "should render with context, html:\n{html}"
        );
    }

    #[test]
    fn render_directive_returns_none_for_missing_template() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        assert!(engine.render_directive("nonexistent", ()).is_none());
    }

    #[test]
    fn render_directive_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        // Place a file outside directives/ that a traversal would reach.
        test_fs::write(dir.path().join("secret.html"), "LEAKED").unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        // `render_directive` builds "directives/../secret.html" — safe_join rejects "..".
        let result = engine.render_directive("../secret", ());
        assert!(result.is_none(), "path traversal should not find template");
    }

    #[test]
    fn render_directive_render_failure_returns_error() {
        #[derive(Serialize)]
        struct Ctx {
            items: i32,
        }

        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("bad.html"),
            "{% for x in items %}{{ x }}{% endfor %}",
        )
        .unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let result = engine.render_directive("bad", Ctx { items: 42 });
        assert!(result.is_some(), "template exists so should return Some");
        let err = result.unwrap().unwrap_err().to_string();
        assert!(
            err.contains("failed to render directive template"),
            "should have context message, got: {err}"
        );
    }
}
