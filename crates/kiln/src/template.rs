use std::path::{Component, Path};

use anyhow::{Context, Result, ensure};
use minijinja::path_loader;
use serde::Serialize;

use crate::config::Config;
use crate::pagination::PaginationVars;

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
        env.add_function("read_file", tpl_read_file);
        env.add_function("parse_csv", tpl_parse_csv);

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

    /// Renders a taxonomy index page (e.g., `/tags/` listing all tags).
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_taxonomy(&self, vars: &TaxonomyIndexVars<'_>) -> Result<String> {
        let template = self
            .env
            .get_template("taxonomy.html")
            .context("failed to load taxonomy.html template")?;
        template
            .render(vars)
            .context("failed to render taxonomy template")
    }

    /// Renders a term page (e.g., `/tags/rust/` listing posts with tag "rust").
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_term(&self, vars: &TermPageVars<'_>) -> Result<String> {
        let template = self
            .env
            .get_template("term.html")
            .context("failed to load term.html template")?;
        template
            .render(vars)
            .context("failed to render term template")
    }
}

/// `MiniJinja` template function: reads a file relative to the directive's
/// `source_dir` context variable.
///
/// Usage in templates: `{% set data = read_file("data.csv") %}`
///
/// Rejects `..`, absolute, and rooted path components to prevent reading
/// outside the page's source directory.
fn tpl_read_file(
    state: &minijinja::State,
    filename: &str,
) -> std::result::Result<String, minijinja::Error> {
    let source_dir = state
        .lookup("source_dir")
        .filter(|v| !v.is_none() && !v.is_undefined())
        .ok_or_else(|| {
            minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "read_file requires source_dir in directive context",
            )
        })?;

    let source_dir = source_dir.as_str().ok_or_else(|| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "source_dir must be a string",
        )
    })?;

    let rel = Path::new(filename);
    for component in rel.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(..)
        ) {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                format!("path traversal not allowed: {filename}"),
            ));
        }
    }

    let path = Path::new(source_dir).join(rel);
    std::fs::read_to_string(&path).map_err(|e| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("failed to read {}: {e}", path.display()),
        )
    })
}

/// `MiniJinja` template function: parses CSV text into a list of rows,
/// where each row is a list of field strings.
///
/// Usage in templates: `{% set rows = parse_csv(read_file("data.csv")) %}`
fn tpl_parse_csv(text: &str) -> std::result::Result<minijinja::Value, minijinja::Error> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(text.as_bytes());

    let rows: Vec<minijinja::Value> = reader
        .records()
        .map(|r| {
            let record = r.map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("CSV parse error: {e}"),
                )
            })?;
            Ok(minijinja::Value::from(
                record
                    .iter()
                    .map(|field| minijinja::Value::from(field.to_string()))
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<std::result::Result<_, minijinja::Error>>()?;

    Ok(minijinja::Value::from(rows))
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

/// Lightweight page summary for list / taxonomy templates.
#[derive(Debug, Clone, Serialize)]
pub struct PageSummary {
    pub title: String,
    pub url: String,
    pub date: Option<String>,
    pub description: String,
    pub featured_image: Option<String>,
}

/// Template variables for a taxonomy index page (e.g., `/tags/`).
#[derive(Debug, Serialize)]
pub struct TaxonomyIndexVars<'a> {
    pub kind: &'a str,
    pub singular: &'a str,
    pub terms: Vec<TermSummary>,
    pub config: &'a Config,
}

/// A term entry for the taxonomy index template.
///
/// Templates can use `term.pages | length` to get the page count.
#[derive(Debug, Clone, Serialize)]
pub struct TermSummary {
    pub name: String,
    pub slug: String,
    pub url: String,
    /// All pages with this term, sorted by date descending.
    pub pages: Vec<PageSummary>,
}

/// A group of pages sharing a common key (e.g., year).
#[derive(Debug, Clone, Serialize)]
pub struct PageGroup {
    pub key: String,
    pub pages: Vec<PageSummary>,
}

/// Template variables for a term page (e.g., `/tags/rust/`).
#[derive(Debug, Serialize)]
pub struct TermPageVars<'a> {
    pub kind: &'a str,
    pub singular: &'a str,
    pub term_name: &'a str,
    pub term_slug: &'a str,
    pub page_groups: Vec<PageGroup>,
    pub pagination: PaginationVars,
    pub config: &'a Config,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs as test_fs;

    use indoc::indoc;

    use super::*;

    use crate::serve::{DEFAULT_PORT, localhost_url};
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
        let expected_og_image = format!(
            r#"<meta property="og:image" content="{}/images/hello.webp">"#,
            localhost_url(DEFAULT_PORT),
        );
        assert!(
            html.contains(&expected_og_image),
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

    // -- render_taxonomy --

    #[test]
    fn render_taxonomy_basic() {
        let engine = test_engine();
        let config = test_config();
        let vars = TaxonomyIndexVars {
            kind: "tags",
            singular: "tag",
            terms: vec![
                TermSummary {
                    name: "Rust".into(),
                    slug: "rust".into(),
                    url: "/tags/rust/".into(),
                    pages: vec![PageSummary {
                        title: "Hello Rust".into(),
                        url: "/hello-rust/".into(),
                        date: None,
                        description: String::new(),
                        featured_image: None,
                    }],
                },
                TermSummary {
                    name: "Web".into(),
                    slug: "web".into(),
                    url: "/tags/web/".into(),
                    pages: Vec::new(),
                },
            ],
            config: &config,
        };
        let html = engine.render_taxonomy(&vars).unwrap();
        assert!(
            html.contains("<h1>All tags</h1>"),
            "should have taxonomy heading, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="/tags/rust/">Rust</a> (1)"#),
            "should list terms with counts, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="/hello-rust/">Hello Rust</a>"#),
            "should include term pages, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="/tags/web/">Web</a> (0)"#),
            "should list all terms, html:\n{html}"
        );
    }

    #[test]
    fn render_taxonomy_truncates_pages() {
        let engine = test_engine();
        let config = test_config();
        let pages: Vec<PageSummary> = (1..=7)
            .map(|i| PageSummary {
                title: format!("Post {i}"),
                url: format!("/post-{i}/"),
                date: None,
                description: String::new(),
                featured_image: None,
            })
            .collect();
        let vars = TaxonomyIndexVars {
            kind: "categories",
            singular: "category",
            terms: vec![TermSummary {
                name: "Big".into(),
                slug: "big".into(),
                url: "/categories/big/".into(),
                pages,
            }],
            config: &config,
        };
        let html = engine.render_taxonomy(&vars).unwrap();
        assert!(
            html.contains("Post 5"),
            "should include 5th page, html:\n{html}"
        );
        assert!(
            !html.contains("Post 6"),
            "should truncate after 5 pages, html:\n{html}"
        );
    }

    #[test]
    fn render_taxonomy_missing_template_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let config = test_config();
        let vars = TaxonomyIndexVars {
            kind: "tags",
            singular: "tag",
            terms: Vec::new(),
            config: &config,
        };
        let err = engine.render_taxonomy(&vars).unwrap_err().to_string();
        assert!(
            err.contains("failed to load taxonomy.html template"),
            "should report missing template, got: {err}"
        );
    }

    // -- render_term --

    #[test]
    fn render_term_basic() {
        let engine = test_engine();
        let config = test_config();
        let vars = TermPageVars {
            kind: "tags",
            singular: "tag",
            term_name: "Rust",
            term_slug: "rust",
            page_groups: vec![PageGroup {
                key: "2026".into(),
                pages: vec![PageSummary {
                    title: "Hello Rust".into(),
                    url: "/hello-rust/".into(),
                    date: Some("2026-01-15T00:00:00Z".into()),
                    description: "A post about Rust".into(),
                    featured_image: None,
                }],
            }],
            pagination: PaginationVars::new("/tags/rust", 1, 1),
            config: &config,
        };
        let html = engine.render_term(&vars).unwrap();
        assert!(
            html.contains("<h1>Tag: Rust</h1>"),
            "should have term heading, html:\n{html}"
        );
        assert!(
            html.contains("<h3>2026</h3>"),
            "should have year group heading, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="/hello-rust/">Hello Rust</a>"#),
            "should list page, html:\n{html}"
        );
    }

    #[test]
    fn render_term_with_pagination() {
        let engine = test_engine();
        let config = test_config();
        let vars = TermPageVars {
            kind: "tags",
            singular: "tag",
            term_name: "Rust",
            term_slug: "rust",
            page_groups: vec![PageGroup {
                key: "2025".into(),
                pages: vec![PageSummary {
                    title: "Post".into(),
                    url: "/post/".into(),
                    date: Some("2025-06-01T00:00:00Z".into()),
                    description: String::new(),
                    featured_image: None,
                }],
            }],
            pagination: PaginationVars::new("/tags/rust", 2, 3),
            config: &config,
        };
        let html = engine.render_term(&vars).unwrap();
        assert!(
            html.contains(r#"<a href="/tags/rust/">← Prev</a>"#),
            "should have prev link, html:\n{html}"
        );
        assert!(
            html.contains("Page 2 / 3"),
            "should show page numbers, html:\n{html}"
        );
        assert!(
            html.contains(r#"<span class="active">2</span>"#),
            "should highlight current page, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="/tags/rust/">1</a>"#),
            "should have page 1 link, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="/tags/rust/page/3/">3</a>"#),
            "should have page 3 link, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="/tags/rust/page/3/">Next →</a>"#),
            "should have next link, html:\n{html}"
        );
    }

    #[test]
    fn render_term_missing_template_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let config = test_config();
        let vars = TermPageVars {
            kind: "tags",
            singular: "tag",
            term_name: "Rust",
            term_slug: "rust",
            page_groups: Vec::new(),
            pagination: PaginationVars::new("/tags/rust", 1, 1),
            config: &config,
        };
        let err = engine.render_term(&vars).unwrap_err().to_string();
        assert!(
            err.contains("failed to load term.html template"),
            "should report missing template, got: {err}"
        );
    }

    // -- read_file --

    #[test]
    fn read_file_reads_relative_to_source_dir() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("csv-reader.html"),
            r"{% set data = read_file(positional_args[0]) %}DATA:{{ data }}",
        )
        .unwrap();

        // Create a CSV file in a fake source dir.
        let source = tempfile::tempdir().unwrap();
        test_fs::write(source.path().join("scores.csv"), "A,B\n1,2").unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let ctx = crate::directive::DirectiveContext {
            name: "csv-reader".into(),
            positional_args: vec!["scores.csv".into()],
            named_args: BTreeMap::default(),
            id: None,
            classes: Vec::new(),
            body_html: String::new(),
            body_raw: String::new(),
            source_dir: Some(source.path().to_string_lossy().into_owned()),
        };

        let result = engine.render_directive("csv-reader", ctx);
        let html = result.unwrap().unwrap();
        assert!(
            html.contains("DATA:A,B\n1,2"),
            "should read file content, got: {html}"
        );
    }

    #[test]
    fn read_file_path_traversal_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("reader.html"),
            r"{{ read_file(positional_args[0]) }}",
        )
        .unwrap();

        let source = tempfile::tempdir().unwrap();
        // Place a secret file outside source_dir.
        test_fs::write(source.path().join("secret.txt"), "SECRET").unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let ctx = crate::directive::DirectiveContext {
            name: "reader".into(),
            positional_args: vec!["../secret.txt".into()],
            named_args: BTreeMap::default(),
            id: None,
            classes: Vec::new(),
            body_html: String::new(),
            body_raw: String::new(),
            source_dir: Some(source.path().join("subdir").to_string_lossy().into_owned()),
        };

        let result = engine.render_directive("reader", ctx);
        let err = format!("{:#}", result.unwrap().unwrap_err());
        assert!(
            err.contains("path traversal not allowed"),
            "should reject traversal, got: {err}"
        );
    }

    #[test]
    fn read_file_absolute_path_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("reader.html"),
            r"{{ read_file(positional_args[0]) }}",
        )
        .unwrap();

        let source = tempfile::tempdir().unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let ctx = crate::directive::DirectiveContext {
            name: "reader".into(),
            positional_args: vec!["/etc/passwd".into()],
            named_args: BTreeMap::default(),
            id: None,
            classes: Vec::new(),
            body_html: String::new(),
            body_raw: String::new(),
            source_dir: Some(source.path().to_string_lossy().into_owned()),
        };

        let result = engine.render_directive("reader", ctx);
        let err = format!("{:#}", result.unwrap().unwrap_err());
        assert!(
            err.contains("path traversal not allowed"),
            "should reject absolute path, got: {err}"
        );
    }

    #[test]
    fn read_file_without_source_dir_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("reader.html"),
            r#"{{ read_file("test.csv") }}"#,
        )
        .unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let ctx = crate::directive::DirectiveContext {
            name: "reader".into(),
            positional_args: Vec::new(),
            named_args: BTreeMap::default(),
            id: None,
            classes: Vec::new(),
            body_html: String::new(),
            body_raw: String::new(),
            source_dir: None,
        };

        let result = engine.render_directive("reader", ctx);
        let err = format!("{:#}", result.unwrap().unwrap_err());
        assert!(
            err.contains("read_file requires source_dir"),
            "should report missing source_dir, got: {err}"
        );
    }

    #[test]
    fn read_file_nonexistent_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("reader.html"),
            r#"{{ read_file("missing.csv") }}"#,
        )
        .unwrap();

        let source = tempfile::tempdir().unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let ctx = crate::directive::DirectiveContext {
            name: "reader".into(),
            positional_args: Vec::new(),
            named_args: BTreeMap::default(),
            id: None,
            classes: Vec::new(),
            body_html: String::new(),
            body_raw: String::new(),
            source_dir: Some(source.path().to_string_lossy().into_owned()),
        };

        let result = engine.render_directive("reader", ctx);
        let err = format!("{:#}", result.unwrap().unwrap_err());
        assert!(
            err.contains("failed to read"),
            "should report file read error, got: {err}"
        );
    }

    // -- parse_csv --

    #[test]
    fn parse_csv_basic() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("csv-test.html"),
            r#"{% set rows = parse_csv(read_file(positional_args[0])) %}{% for row in rows %}{{ row | join(",") }};{% endfor %}"#,
        )
        .unwrap();

        let source = tempfile::tempdir().unwrap();
        test_fs::write(source.path().join("data.csv"), "A,B\n1,2\n3,4").unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let ctx = crate::directive::DirectiveContext {
            name: "csv-test".into(),
            positional_args: vec!["data.csv".into()],
            named_args: BTreeMap::default(),
            id: None,
            classes: Vec::new(),
            body_html: String::new(),
            body_raw: String::new(),
            source_dir: Some(source.path().to_string_lossy().into_owned()),
        };

        let html = engine.render_directive("csv-test", ctx).unwrap().unwrap();
        assert_eq!(html, "A,B;1,2;3,4;");
    }

    #[test]
    fn parse_csv_quoted_fields() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("csv-test.html"),
            r#"{% set rows = parse_csv(read_file(positional_args[0])) %}{% for row in rows %}[{{ row | join("|") }}]{% endfor %}"#,
        )
        .unwrap();

        let source = tempfile::tempdir().unwrap();
        test_fs::write(
            source.path().join("data.csv"),
            indoc! {r#"
                name,value
                "field with, comma","has ""quotes"""
            "#},
        )
        .unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();
        let ctx = crate::directive::DirectiveContext {
            name: "csv-test".into(),
            positional_args: vec!["data.csv".into()],
            named_args: BTreeMap::default(),
            id: None,
            classes: Vec::new(),
            body_html: String::new(),
            body_raw: String::new(),
            source_dir: Some(source.path().to_string_lossy().into_owned()),
        };

        let html = engine.render_directive("csv-test", ctx).unwrap().unwrap();
        assert_eq!(
            html,
            "[name|value][field with, comma|has &quot;quotes&quot;]"
        );
    }

    #[test]
    fn parse_csv_empty_input() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("csv-test.html"),
            r#"{% set rows = parse_csv("") %}{{ rows | length }}"#,
        )
        .unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None).unwrap();

        let ctx = crate::directive::DirectiveContext {
            name: "csv-test".into(),
            positional_args: Vec::new(),
            named_args: BTreeMap::default(),
            id: None,
            classes: Vec::new(),
            body_html: String::new(),
            body_raw: String::new(),
            source_dir: None,
        };

        let html = engine.render_directive("csv-test", ctx).unwrap().unwrap();
        assert_eq!(html, "0");
    }
}
