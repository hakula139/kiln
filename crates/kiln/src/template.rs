mod functions;
pub mod vars;

use std::path::Path;

use anyhow::{Context, Result, ensure};
use minijinja::path_loader;
use minijinja::value::{Kwargs, Value, merge_maps};
use serde::Serialize;

use self::functions::{
    tpl_asset_url, tpl_now, tpl_parse_csv, tpl_read_file, tpl_register_script, tpl_t,
};
use self::vars::{
    ArchivePageVars, ErrorPageVars, HomePageVars, OverviewPageVars, PostTemplateVars,
};
use crate::config::Config;
use crate::i18n::I18n;
use crate::render::assets::AssetsHandle;
use crate::static_assets::StaticAssetManifest;

#[derive(Debug)]
pub struct TemplateEngine {
    env: minijinja::Environment<'static>,
}

impl TemplateEngine {
    /// Creates a layered template engine: `site_dir` overrides `theme_dir`.
    ///
    /// `site_dir` is silently ignored when missing (optional override). `theme_dir`, when set,
    /// must be a directory. At least one usable directory is required. `i18n` backs `t()`.
    ///
    /// # Errors
    ///
    /// Returns an error if no usable template directory exists or the theme directory is invalid.
    pub fn new(site_dir: Option<&Path>, theme_dir: Option<&Path>, i18n: &I18n) -> Result<Self> {
        Self::new_with_assets(
            site_dir,
            theme_dir,
            i18n,
            "",
            &StaticAssetManifest::default(),
        )
    }

    /// Creates a template engine backed by a static asset manifest.
    ///
    /// # Errors
    ///
    /// Returns an error if no usable template directory exists or the theme directory is invalid.
    pub fn new_with_assets(
        site_dir: Option<&Path>,
        theme_dir: Option<&Path>,
        i18n: &I18n,
        deployment_prefix: &str,
        static_assets: &StaticAssetManifest,
    ) -> Result<Self> {
        if let Some(d) = theme_dir {
            ensure!(
                d.is_dir(),
                "theme template directory does not exist: {}",
                d.display()
            );
        }

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
        env.add_function("now", tpl_now);
        env.add_function("read_file", tpl_read_file);
        env.add_function("parse_csv", tpl_parse_csv);

        let asset_manifest = static_assets.clone();
        let deployment_prefix = deployment_prefix.to_owned();
        env.add_function("asset_url", move |state: &minijinja::State, url: &str| {
            tpl_asset_url(state, &asset_manifest, &deployment_prefix, url)
        });

        env.add_function(
            "register_script",
            |state: &minijinja::State, url: &str, kwargs: Kwargs| {
                tpl_register_script(state, url, &kwargs)
            },
        );

        let t_i18n = i18n.clone();
        env.add_function("t", move |key: &str, kwargs: Kwargs| {
            tpl_t(&t_i18n, key, &kwargs)
        });

        Ok(Self { env })
    }

    /// Renders a post page using the `post.html` template.
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_post(&self, vars: &PostTemplateVars<'_>) -> Result<String> {
        self.render_required_template("post.html", vars, &vars.metadata.url)
    }

    /// Renders a standalone page using the `page.html` template.
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_page(&self, vars: &PostTemplateVars<'_>) -> Result<String> {
        self.render_required_template("page.html", vars, &vars.metadata.url)
    }

    /// Renders the home page using the `home.html` template.
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_home(&self, vars: &HomePageVars<'_>) -> Result<String> {
        self.render_required_template("home.html", vars, &vars.metadata.url)
    }

    /// Renders an archive page using the `archive.html` template.
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_archive(&self, vars: &ArchivePageVars<'_>) -> Result<String> {
        self.render_required_template("archive.html", vars, &vars.metadata.url)
    }

    /// Renders a bucket overview page (e.g., `/tags/`, `/sections/`).
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_overview(&self, vars: &OverviewPageVars<'_>) -> Result<String> {
        self.render_required_template("overview.html", vars, &vars.metadata.url)
    }

    /// Renders the 404 error page using the `404.html` template.
    ///
    /// Returns `None` if the template does not exist. Returns `Some(Err(_))`
    /// if loading or rendering the template fails.
    pub fn render_404(&self, vars: &ErrorPageVars<'_>) -> Option<Result<String>> {
        self.render_optional_template("404.html", vars)
            .map(|result| result.context("failed to render 404 template"))
    }

    /// Tries to render a directive using `directives/<name>.html`.
    ///
    /// Returns `None` if no template exists. `Some(Err(_))` if loading or rendering fails.
    pub fn render_directive(
        &self,
        name: &str,
        ctx: impl Serialize,
        assets: &AssetsHandle,
        config: &Config,
    ) -> Option<Result<String>> {
        let template_name = format!("directives/{name}.html");
        let merged = merge_maps([
            minijinja::context! {
                __assets => Value::from_object(assets.clone()),
                config => Value::from_serialize(config),
            },
            Value::from_serialize(&ctx),
        ]);
        self.render_optional_template(&template_name, merged)
            .map(|result| {
                result.with_context(|| {
                    format!("failed to render directive template: {template_name}")
                })
            })
    }

    /// Returns `true` if a template with the given name exists.
    ///
    /// A broken template (e.g., syntax error) counts as existing, so the caller's render call will
    /// surface the parse error rather than silently skipping the output.
    pub fn has_template(&self, name: &str) -> bool {
        match self.env.get_template(name) {
            Ok(_) => true,
            Err(e) => e.kind() != minijinja::ErrorKind::TemplateNotFound,
        }
    }

    fn render_required_template(
        &self,
        name: &str,
        vars: impl Serialize,
        page_url: &str,
    ) -> Result<String> {
        self.env
            .get_template(name)
            .with_context(|| format!("failed to load {name} template"))?
            .render(page_context(vars, page_url))
            .with_context(|| format!("failed to render {name} template"))
    }

    fn render_optional_template(
        &self,
        name: &str,
        vars: impl Serialize,
    ) -> Option<Result<String, minijinja::Error>> {
        match self.env.get_template(name) {
            Err(error) if error.kind() == minijinja::ErrorKind::TemplateNotFound => None,
            result => Some(result.and_then(|template| template.render(vars))),
        }
    }
}

fn page_context(vars: impl Serialize, page_url: &str) -> Value {
    merge_maps([
        minijinja::context! { __page_url => page_url },
        Value::from_serialize(vars),
    ])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs as test_fs;

    use indoc::indoc;

    use super::*;
    use crate::content::frontmatter::FeaturedImage;
    use crate::pagination::PaginationVars;
    use crate::render::assets::{LoadStrategy, PageAssets};
    use crate::template::vars::{
        ArchivePageVars, BucketSummary, ErrorPageVars, HomePageVars, LinkedTerm, OverviewPageVars,
        PageGroup, PageMetadata, PageSummary, PostTemplateVars,
    };
    use crate::test_utils::{test_engine, test_i18n};

    // ── new ──

    #[test]
    fn new_with_site_dir_only() {
        let dir = tempfile::tempdir().unwrap();
        test_fs::write(dir.path().join("test.html"), "hello").unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
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

        // Same template in both, so site should win.
        test_fs::write(site_dir.join("page.html"), "from site").unwrap();
        test_fs::write(theme_dir.join("page.html"), "from theme").unwrap();
        // Template only in theme, so it should fall through.
        test_fs::write(theme_dir.join("base.html"), "theme base").unwrap();

        let engine = TemplateEngine::new(Some(&site_dir), Some(&theme_dir), &test_i18n()).unwrap();
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
        let result = TemplateEngine::new(
            Some(Path::new("/nonexistent")),
            Some(&theme_dir),
            &test_i18n(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn new_without_directories_returns_error() {
        let err = TemplateEngine::new(None, None, &test_i18n())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("no valid template directory found"),
            "should reject when no dirs provided, got: {err}"
        );
    }

    #[test]
    fn new_nonexistent_theme_directory_returns_error() {
        let err = TemplateEngine::new(None, Some(Path::new("/nonexistent/path")), &test_i18n())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("theme template directory does not exist"),
            "should reject nonexistent theme dir, got: {err}"
        );
    }

    // ── render_post ──

    #[test]
    fn render_post_exposes_context() {
        let engine = engine_with_template(
            "post.html",
            "{{ title }}|{{ description }}|{{ url }}|{{ date }}|{{ updated }}|{{ featured_image.src }}|{{ page_css }}|{{ config.title }}|{{ t('all_posts') }}|{{ 'math' in assets.features }}|{{ assets.scripts[0].url }}|{{ license }}|{{ tags[0].name }}|{{ section.name }}",
        );
        let config = Config::default();
        let mut vars = post_vars(&config);
        let featured_image = FeaturedImage {
            src: "/image.webp".into(),
            ..Default::default()
        };
        let tags = [LinkedTerm {
            name: "Tag A".into(),
            url: "/tags/a/".into(),
        }];
        let section = LinkedTerm {
            name: "Section A".into(),
            url: "/posts/a/".into(),
        };
        vars.featured_image = Some(&featured_image);
        vars.tags = &tags;
        vars.section = Some(&section);
        vars.license = Some("CC0-1.0");
        vars.page_css = Some("/page.css".into());
        vars.date = Some("2026-01-01T12:34:56Z");
        vars.updated = Some("2026-02-01T12:34:56Z".into());
        vars.assets
            .add_feature(crate::render::assets::Feature::Math);
        vars.assets
            .register_script(crate::render::assets::ScriptTag::deferred("/script.js"))
            .unwrap();

        assert_eq!(
            engine.render_post(&vars).unwrap(),
            "Post A|Description|https:&#x2f;&#x2f;example.com&#x2f;post-a&#x2f;|2026-01-01T12:34:56Z|2026-02-01T12:34:56Z|&#x2f;image.webp|&#x2f;page.css|My Site|All Posts|True|&#x2f;script.js|CC0-1.0|Tag A|Section A"
        );
    }

    #[test]
    fn render_post_escapes_text_and_preserves_explicit_safe_html() {
        let engine = engine_with_template(
            "post.html",
            "{{ title }}|{{ content }}|{{ content | safe }}|{{ toc | safe }}",
        );
        let config = Config::default();
        let mut vars = post_vars(&config);
        vars.metadata.title = "<title>";
        vars.content = "<strong>Body</strong>";
        vars.toc = "<nav>Contents</nav>";

        assert_eq!(
            engine.render_post(&vars).unwrap(),
            "&lt;title&gt;|&lt;strong&gt;Body&lt;&#x2f;strong&gt;|<strong>Body</strong>|<nav>Contents</nav>"
        );
    }

    // ── render_page ──

    #[test]
    fn render_page_exposes_post_context() {
        let engine = engine_with_template("page.html", "page:{{ title }}|{{ content | safe }}");
        let config = Config::default();
        let mut vars = post_vars(&config);
        vars.content = "<p>Body</p>";

        assert_eq!(
            engine.render_page(&vars).unwrap(),
            "page:Post A|<p>Body</p>"
        );
    }

    // ── render_home ──

    #[test]
    fn render_home_exposes_pages_and_pagination() {
        let engine = engine_with_template(
            "home.html",
            "{{ title }}|{{ description }}|{{ url }}|{% for page in pages %}{{ page.title }}:{{ page.url }}{% endfor %}|{{ pagination.current_page }}/{{ pagination.total_pages }}|{{ assets is defined }}",
        );
        let config = Config::default();
        let page = page_summary();
        let vars = HomePageVars {
            metadata: PageMetadata {
                title: "Home",
                description: "Latest pages",
                url: "https://example.com/page/2/".into(),
            },
            pages: vec![&page],
            pagination: PaginationVars::new("", 2, 3),
            config: &config,
        };

        assert_eq!(
            engine.render_home(&vars).unwrap(),
            "Home|Latest pages|https:&#x2f;&#x2f;example.com&#x2f;page&#x2f;2&#x2f;|Post A:&#x2f;post-a&#x2f;|2/3|False"
        );
    }

    // ── render_archive ──

    #[test]
    fn render_archive_exposes_groups_and_pagination() {
        let engine = engine_with_template(
            "archive.html",
            "{{ title }}|{{ description }}|{{ url }}|{{ kind }}|{{ singular }}|{{ name }}|{{ slug }}|{% for group in page_groups %}{{ group.key }}:{% for page in group.pages %}{{ page.title }}:{{ page.url }}{% endfor %}{% endfor %}|{{ pagination.current_page }}/{{ pagination.total_pages }}|{{ assets is defined }}",
        );
        let config = Config::default();
        let page = page_summary();
        let vars = ArchivePageVars {
            metadata: PageMetadata {
                title: "Tagged Rust",
                description: "Description",
                url: "https://example.com/tags/rust/page/2/".into(),
            },
            kind: "tags",
            singular: "tag",
            name: "Rust",
            slug: "rust",
            page_groups: vec![PageGroup {
                key: "2026".into(),
                pages: vec![&page],
            }],
            pagination: PaginationVars::new("/tags/rust", 2, 3),
            config: &config,
        };

        assert_eq!(
            engine.render_archive(&vars).unwrap(),
            "Tagged Rust|Description|https:&#x2f;&#x2f;example.com&#x2f;tags&#x2f;rust&#x2f;page&#x2f;2&#x2f;|tags|tag|Rust|rust|2026:Post A:&#x2f;post-a&#x2f;|2/3|False"
        );
    }

    // ── render_overview ──

    #[test]
    fn render_overview_exposes_buckets() {
        let engine = engine_with_template(
            "overview.html",
            "{{ title }}|{{ description }}|{{ url }}|{{ kind }}|{{ singular }}|{% for bucket in buckets %}{{ bucket.name }}:{{ bucket.slug }}:{{ bucket.url }}:{% for page in bucket.pages %}{{ page.title }}:{{ page.url }}{% endfor %}|{% endfor %}{{ assets is defined }}",
        );
        let config = Config::default();
        let page = page_summary();
        let vars = OverviewPageVars {
            metadata: PageMetadata {
                title: "All Tags",
                description: "Description",
                url: "https://example.com/tags/".into(),
            },
            kind: "tags",
            singular: "tag",
            buckets: vec![
                BucketSummary {
                    name: "Rust".into(),
                    slug: "rust".into(),
                    url: "/tags/rust/".into(),
                    pages: vec![&page],
                },
                BucketSummary {
                    name: "Empty".into(),
                    slug: "empty".into(),
                    url: "/tags/empty/".into(),
                    pages: Vec::new(),
                },
            ],
            config: &config,
        };

        assert_eq!(
            engine.render_overview(&vars).unwrap(),
            "All Tags|Description|https:&#x2f;&#x2f;example.com&#x2f;tags&#x2f;|tags|tag|Rust:rust:&#x2f;tags&#x2f;rust&#x2f;:Post A:&#x2f;post-a&#x2f;|Empty:empty:&#x2f;tags&#x2f;empty&#x2f;:|False"
        );
    }

    // ── render_404 ──

    #[test]
    fn render_404_basic() {
        let engine = test_engine();
        let config = Config::default();
        let vars = ErrorPageVars {
            title: "404 Not Found",
            config: &config,
        };
        let result = engine.render_404(&vars);
        assert!(result.is_some(), "should find 404 template");
        let html = result.unwrap().unwrap();
        assert!(
            html.contains("<title>404 Not Found - My Site</title>"),
            "should have title, html:\n{html}"
        );
        assert!(
            html.contains("<h1>404 Not Found</h1>"),
            "should have heading, html:\n{html}"
        );
    }

    #[test]
    fn render_404_missing_template_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        let config = Config::default();
        let vars = ErrorPageVars {
            title: "404 Not Found",
            config: &config,
        };
        assert!(
            engine.render_404(&vars).is_none(),
            "should return None when 404.html is missing"
        );
    }

    #[test]
    fn render_404_template_failure_returns_error() {
        for (source, kind) in [
            ("{% invalid %}", minijinja::ErrorKind::SyntaxError),
            (
                "{% for x in 42 %}{{ x }}{% endfor %}",
                minijinja::ErrorKind::InvalidOperation,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            test_fs::write(dir.path().join("404.html"), source).unwrap();
            let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
            let config = Config::default();
            let vars = ErrorPageVars {
                title: "404 Not Found",
                config: &config,
            };

            let error = engine.render_404(&vars).unwrap().unwrap_err();
            assert_eq!(
                error.downcast_ref::<minijinja::Error>().unwrap().kind(),
                kind
            );
        }
    }

    // ── render_directive ──

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

        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        let ctx = Ctx {
            name: "test".into(),
            body_html: "<p>hello</p>".into(),
        };

        let result =
            engine.render_directive("test", ctx, &AssetsHandle::default(), &Config::default());
        assert!(result.is_some(), "should find template");
        let html = result.unwrap().unwrap();
        assert!(
            html.contains("<div>test: <p>hello</p></div>"),
            "should render with context, html:\n{html}"
        );
    }

    #[test]
    fn render_directive_exposes_config_to_template() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(
            directives_dir.join("probe.html"),
            "title={{ config.title }} lang={{ config.language }}",
        )
        .unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        let config: Config = toml::from_str(indoc! {r#"
            base_url = "https://example.com"
            title = "Probe"
            language = "fr"
        "#})
        .unwrap();

        let html = engine
            .render_directive("probe", (), &AssetsHandle::default(), &config)
            .unwrap()
            .unwrap();
        assert_eq!(html, "title=Probe lang=fr");
    }

    #[test]
    fn render_directive_missing_template_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        assert!(
            engine
                .render_directive(
                    "nonexistent",
                    (),
                    &AssetsHandle::default(),
                    &Config::default()
                )
                .is_none()
        );
    }

    #[test]
    fn render_directive_path_traversal_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        // Place a file outside directives/ that a traversal would reach.
        test_fs::write(dir.path().join("secret.html"), "LEAKED").unwrap();

        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        // `render_directive` builds "directives/../secret.html", which safe_join rejects.
        let result = engine.render_directive(
            "../secret",
            (),
            &AssetsHandle::default(),
            &Config::default(),
        );
        assert!(result.is_none(), "path traversal should not find template");
    }

    #[test]
    fn render_directive_malformed_template_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(directives_dir.join("bad.html"), "{% invalid %}").unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();

        let error = engine
            .render_directive("bad", (), &AssetsHandle::default(), &Config::default())
            .unwrap()
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<minijinja::Error>().unwrap().kind(),
            minijinja::ErrorKind::SyntaxError,
        );
        assert!(error.to_string().contains("directives/bad.html"));
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

        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        let result = engine.render_directive(
            "bad",
            Ctx { items: 42 },
            &AssetsHandle::default(),
            &Config::default(),
        );
        assert!(result.is_some(), "template exists so should return Some");
        let err = result.unwrap().unwrap_err().to_string();
        assert!(
            err.contains("failed to render directive template"),
            "should have context message, got: {err}"
        );
    }

    // ── has_template ──

    #[test]
    fn has_template_existing() {
        let engine = test_engine();
        assert!(engine.has_template("post.html"));
        assert!(engine.has_template("page.html"));
        assert!(engine.has_template("home.html"));
        assert!(engine.has_template("archive.html"));
        assert!(engine.has_template("overview.html"));
        assert!(engine.has_template("404.html"));
    }

    #[test]
    fn has_template_broken_returns_true() {
        let dir = tempfile::tempdir().unwrap();
        test_fs::write(dir.path().join("broken.html"), "{% invalid %}").unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        assert!(
            engine.has_template("broken.html"),
            "broken templates exist. Rendering should surface the parse error",
        );
    }

    #[test]
    fn has_template_missing_returns_false() {
        let engine = test_engine();
        assert!(!engine.has_template("nonexistent.html"));
    }

    // ── render_required_template ──

    #[test]
    fn render_required_template_failure_returns_error() {
        let name = "post.html";
        for (source, kind, phase) in [
            (None, minijinja::ErrorKind::TemplateNotFound, "load"),
            (
                Some("{% invalid %}"),
                minijinja::ErrorKind::SyntaxError,
                "load",
            ),
            (
                Some("{{ missing_function() }}"),
                minijinja::ErrorKind::UnknownFunction,
                "render",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            if let Some(source) = source {
                test_fs::write(dir.path().join(name), source).unwrap();
            }
            let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
            let error = engine.render_required_template(name, (), "/").unwrap_err();
            assert_eq!(
                error.to_string(),
                format!("failed to {phase} {name} template")
            );
            assert_eq!(
                error.downcast_ref::<minijinja::Error>().unwrap().kind(),
                kind
            );
        }
    }

    // ── tpl_now ──

    #[test]
    fn tpl_now_returns_current_timestamp() {
        let engine = test_engine();
        let before = jiff::Timestamp::now();
        let result = engine.env.render_str("{{ now() }}", ()).unwrap();
        let after = jiff::Timestamp::now();

        let timestamp = result.parse::<jiff::Zoned>().unwrap().timestamp();
        assert!(timestamp >= before && timestamp <= after, "{result}");
    }

    // ── tpl_read_file ──

    #[test]
    fn tpl_read_file_reads_relative_to_source_dir() {
        let source = tempfile::tempdir().unwrap();
        let contents = indoc! {"
            A,B
            1,2
        "};
        test_fs::write(source.path().join("scores.csv"), contents).unwrap();

        let html = render_read_file("scores.csv", Some(source.path())).unwrap();
        assert_eq!(html, contents);
    }

    #[test]
    fn tpl_read_file_path_traversal_returns_error() {
        let source = tempfile::tempdir().unwrap();
        let source_dir = source.path().join("subdir");
        test_fs::create_dir(&source_dir).unwrap();
        test_fs::write(source.path().join("outside.txt"), "outside").unwrap();

        let err = format!(
            "{:#}",
            render_read_file("../outside.txt", Some(&source_dir)).unwrap_err()
        );
        assert!(
            err.contains("path traversal not allowed"),
            "should reject traversal, got: {err}"
        );
    }

    #[test]
    fn tpl_read_file_absolute_path_returns_error() {
        let source = tempfile::tempdir().unwrap();
        let file = source.path().join("example.txt");
        test_fs::write(&file, "body").unwrap();

        let err = format!(
            "{:#}",
            render_read_file(file.to_str().unwrap(), Some(source.path())).unwrap_err()
        );
        assert!(
            err.contains("path traversal not allowed"),
            "should reject absolute path, got: {err}"
        );
    }

    #[test]
    fn tpl_read_file_without_source_dir_returns_error() {
        let err = format!("{:#}", render_read_file("test.csv", None).unwrap_err());
        assert!(
            err.contains("read_file requires source_dir"),
            "should report missing source_dir, got: {err}"
        );
    }

    #[test]
    fn tpl_read_file_nonexistent_file_returns_error() {
        let source = tempfile::tempdir().unwrap();

        let err = format!(
            "{:#}",
            render_read_file("missing.csv", Some(source.path())).unwrap_err()
        );
        assert!(
            err.contains("failed to read"),
            "should report file read error, got: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn tpl_read_file_follows_external_symlink() {
        let source = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        test_fs::write(external.path().join("data.txt"), "external <data>").unwrap();
        std::os::unix::fs::symlink(
            external.path().join("data.txt"),
            source.path().join("data.txt"),
        )
        .unwrap();

        let html = render_read_file("data.txt", Some(source.path())).unwrap();
        assert_eq!(html, "external &lt;data&gt;");
    }

    // ── tpl_parse_csv ──

    #[test]
    fn tpl_parse_csv_reads_and_escapes_rows_from_source_file() {
        let (_templates, engine) = engine_with_directive(
            "csv-test",
            r#"{% set rows = parse_csv(read_file(positional_args[0])) %}{% for row in rows %}[{{ row | join("|") }}]{% endfor %}"#,
        );
        let source = tempfile::tempdir().unwrap();
        test_fs::write(
            source.path().join("data.csv"),
            indoc! {r#"
                name,value
                "field with, comma","has ""quotes"""
                one,two
            "#},
        )
        .unwrap();
        let mut ctx = empty_ctx("csv-test");
        ctx.positional_args = vec!["data.csv".into()];
        ctx.source_dir = Some(source.path().to_string_lossy().into_owned());

        let html = engine
            .render_directive(
                "csv-test",
                ctx,
                &AssetsHandle::default(),
                &Config::default(),
            )
            .unwrap()
            .unwrap();

        assert_eq!(
            html,
            "[name|value][field with, comma|has &quot;quotes&quot;][one|two]"
        );
    }

    // ── tpl_t ──

    #[test]
    fn tpl_t_returns_string_for_known_key() {
        let engine = test_engine();
        let result = engine
            .env
            .render_str(r#"{{ t("all_posts") }}"#, ())
            .unwrap();
        assert_eq!(result, "All Posts");
    }

    #[test]
    fn tpl_t_interpolates_keyword_arguments() {
        let dir = tempfile::tempdir().unwrap();
        test_fs::create_dir_all(dir.path().join("i18n")).unwrap();
        test_fs::write(
            dir.path().join("i18n").join("en.toml"),
            r#"greeting = "Hi {name}!""#,
        )
        .unwrap();
        let i18n =
            crate::i18n::I18n::load(Path::new("/nonexistent"), Some(dir.path()), "en").unwrap();

        let templates = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new(Some(templates.path()), None, &i18n).unwrap();
        for (template, expected) in [
            (r#"{{ t("greeting", name="Alex") }}"#, "Hi Alex!"),
            (r#"{{ t("greeting", name=none) }}"#, "Hi !"),
        ] {
            let result = engine.env.render_str(template, ()).unwrap();
            assert_eq!(result, expected, "template: {template}");
        }
    }

    #[test]
    fn tpl_t_returns_key_literal_for_missing_key() {
        let engine = test_engine();
        let result = engine
            .env
            .render_str(r#"{{ t("not_defined_anywhere") }}"#, ())
            .unwrap();
        assert_eq!(result, "not_defined_anywhere");
    }

    // ── tpl_asset_url ──

    #[test]
    fn tpl_asset_url_renders_prefixed_and_encoded_manifest_urls() {
        let static_dir = tempfile::tempdir().unwrap();
        for path in ["shared.css", "shared.js", "page script.js"] {
            test_fs::write(static_dir.path().join(path), "abc").unwrap();
        }
        test_fs::create_dir(static_dir.path().join("blog")).unwrap();
        test_fs::write(static_dir.path().join("blog/shared.css"), "different").unwrap();
        let manifest = StaticAssetManifest::build(static_dir.path()).unwrap();
        let templates = tempfile::tempdir().unwrap();
        test_fs::write(
            templates.path().join("assets.html"),
            indoc! {r#"
                <link rel="stylesheet" href="{{ asset_url('/shared.css') | safe }}">
                <script src="{{ asset_url('/shared.js') | safe }}"></script>
                <script src="{{ asset_url('/page%20script.js') | safe }}"></script>
                <link rel="stylesheet" href="{{ asset_url('/blog/shared.css') | safe }}">
            "#},
        )
        .unwrap();
        let engine = TemplateEngine::new_with_assets(
            Some(templates.path()),
            None,
            &test_i18n(),
            "/blog",
            &manifest,
        )
        .unwrap();

        assert_eq!(
            engine
                .env
                .get_template("assets.html")
                .unwrap()
                .render(())
                .unwrap(),
            indoc! {r#"
                <link rel="stylesheet" href="/blog/shared.ba7816bf8f01.css">
                <script src="/blog/shared.ba7816bf8f01.js"></script>
                <script src="/blog/page%20script.ba7816bf8f01.js"></script>
                <link rel="stylesheet" href="/blog/blog/shared.9d6f965ac832.css">
            "#}
            .trim_end(),
        );
    }

    #[test]
    fn tpl_asset_url_missing_file_or_page_context_returns_error() {
        let engine = test_engine();
        for (template, expected) in [
            (
                r"{{ asset_url('/missing.png') }}",
                "static asset not found: /missing.png",
            ),
            (
                r"{{ asset_url('image.png') }}",
                "requires a page URL for relative paths",
            ),
        ] {
            let error = engine.env.render_str(template, ()).unwrap_err();
            assert_eq!(error.kind(), minijinja::ErrorKind::InvalidOperation);
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    // ── tpl_register_script ──

    #[test]
    fn tpl_register_script_records_and_deduplicates_default_deferred_tag() {
        let (_dir, engine) = engine_with_directive(
            "widget",
            r#"{{ register_script("/js/widget.js") }}<widget>"#,
        );
        let assets = AssetsHandle::default();
        let html = engine
            .render_directive("widget", empty_ctx("widget"), &assets, &Config::default())
            .unwrap()
            .unwrap();

        assert_eq!(html, "<widget>", "register_script must return empty string");
        assert_eq!(
            engine
                .render_directive("widget", empty_ctx("widget"), &assets, &Config::default())
                .unwrap()
                .unwrap(),
            "<widget>"
        );
        let snapshot = assets.snapshot();
        assert_eq!(snapshot.scripts().len(), 1);
        assert_eq!(snapshot.scripts()[0].url, "/js/widget.js");
        assert_eq!(snapshot.scripts()[0].load, LoadStrategy::Defer);
        assert!(!snapshot.scripts()[0].module);
    }

    #[test]
    fn tpl_register_script_honors_load_async_kwarg() {
        let (_dir, engine) = engine_with_directive(
            "widget",
            r#"{{ register_script("/js/widget.js", load="async") }}"#,
        );
        let assets = AssetsHandle::default();
        engine
            .render_directive("widget", empty_ctx("widget"), &assets, &Config::default())
            .unwrap()
            .unwrap();

        let snapshot = assets.snapshot();
        assert_eq!(snapshot.scripts()[0].load, LoadStrategy::Async);
        assert!(!snapshot.scripts()[0].module);
    }

    #[test]
    fn tpl_register_script_synchronous_module_returns_error() {
        let (_dir, engine) = engine_with_directive(
            "widget",
            r#"{{ register_script("/js/widget.js", load="sync", module=true) }}"#,
        );
        let assets = AssetsHandle::default();
        let error = engine
            .render_directive("widget", empty_ctx("widget"), &assets, &Config::default())
            .unwrap()
            .unwrap_err();

        let error = error.downcast_ref::<minijinja::Error>().unwrap();
        assert_eq!(error.kind(), minijinja::ErrorKind::InvalidOperation);
        assert!(error.to_string().contains("cannot use synchronous loading"));
        assert_eq!(assets.snapshot().scripts(), []);
    }

    #[test]
    fn tpl_register_script_conflicting_attributes_returns_error() {
        let (_dir, engine) = engine_with_directive(
            "widget",
            indoc! {r#"
                {{- register_script("/js/widget.js") -}}
                {{- register_script("/js/widget.js", module=true) -}}
            "#},
        );
        let assets = AssetsHandle::default();
        let err = format!(
            "{:#}",
            engine
                .render_directive("widget", empty_ctx("widget"), &assets, &Config::default())
                .unwrap()
                .unwrap_err(),
        );
        assert!(
            err.contains("Pick one set of attributes per URL"),
            "should surface the assets-layer conflict error, got: {err}"
        );
    }

    #[test]
    fn tpl_register_script_outside_directive_context_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        let err = engine
            .env
            .render_str(r#"{{ register_script("/x.js") }}"#, ())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("only callable from directive templates"),
            "non-directive renders should reject register_script, got: {err}"
        );
    }

    #[test]
    fn tpl_register_script_wrong_assets_type_returns_error() {
        // Unreachable through `render_directive`. This pins the contract for any
        // future path that populates `__assets`.
        let dir = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        let err = engine
            .env
            .render_str(
                r#"{{ register_script("/x.js") }}"#,
                minijinja::context! { __assets => "not a handle" },
            )
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("__assets is not a recognized asset handle"),
            "wrong-type __assets should surface a typed error, got: {err}"
        );
    }

    #[test]
    fn tpl_register_script_invalid_keyword_argument_returns_error() {
        for (template, message) in [
            (r#"{{ register_script("/x.js", bogus=true) }}"#, "bogus"),
            (
                r#"{{ register_script("/x.js", load="eager") }}"#,
                r#"load must be one of "defer", "async", "sync"; got "eager""#,
            ),
            (
                r#"{{ register_script("/x.js", module="yes") }}"#,
                "cannot convert",
            ),
        ] {
            let (_dir, engine) = engine_with_directive("widget", template);
            let error = engine
                .render_directive(
                    "widget",
                    empty_ctx("widget"),
                    &AssetsHandle::default(),
                    &Config::default(),
                )
                .unwrap()
                .unwrap_err();
            assert!(
                format!("{error:#}").contains(message),
                "{template}: {error:#}"
            );
        }
    }

    fn engine_with_template(name: &'static str, source: &'static str) -> TemplateEngine {
        let mut engine = test_engine();
        engine.env.add_template(name, source).unwrap();
        engine
    }

    fn post_vars(config: &Config) -> PostTemplateVars<'_> {
        PostTemplateVars {
            metadata: PageMetadata {
                title: "Post A",
                description: "Description",
                url: "https://example.com/post-a/".into(),
            },
            featured_image: None,
            license: None,
            page_css: None,
            date: None,
            updated: None,
            tags: &[],
            section: None,
            assets: PageAssets::default(),
            content: "",
            toc: "",
            config,
        }
    }

    fn page_summary() -> PageSummary {
        PageSummary {
            title: "Post A".into(),
            url: "/post-a/".into(),
            date: None,
            pinned: false,
            description: String::new(),
            featured_image: None,
            tags: Vec::new(),
            section: None,
        }
    }

    fn render_read_file(filename: &str, source_dir: Option<&Path>) -> Result<String> {
        let (_templates, engine) =
            engine_with_directive("reader", r"{{ read_file(positional_args[0]) }}");
        let mut ctx = empty_ctx("reader");
        ctx.positional_args = vec![filename.into()];
        ctx.source_dir = source_dir.map(|path| path.to_string_lossy().into_owned());

        engine
            .render_directive("reader", ctx, &AssetsHandle::default(), &Config::default())
            .unwrap()
    }

    fn engine_with_directive(name: &str, body: &str) -> (tempfile::TempDir, TemplateEngine) {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(directives_dir.join(format!("{name}.html")), body).unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        (dir, engine)
    }

    fn empty_ctx(name: &str) -> crate::directive::DirectiveContext {
        crate::directive::DirectiveContext {
            name: name.into(),
            positional_args: Vec::new(),
            named_args: BTreeMap::default(),
            id: None,
            classes: Vec::new(),
            body_html: String::new(),
            body_raw: String::new(),
            source_dir: None,
            page_url: "/".into(),
        }
    }
}
