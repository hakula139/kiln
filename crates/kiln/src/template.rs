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
use crate::directive::DirectiveContext;
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
    pub(crate) fn render_directive(
        &self,
        ctx: &DirectiveContext<'_>,
        assets: &AssetsHandle,
        config: &Config,
    ) -> Option<Result<String>> {
        let template_name = format!("directives/{}.html", ctx.name);
        let merged = merge_maps([
            minijinja::context! {
                __assets => Value::from_object(assets.clone()),
                config => Value::from_serialize(config),
            },
            Value::from_serialize(ctx),
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
    use std::fs as test_fs;

    use indoc::indoc;

    use super::*;
    use crate::attrs::Identity;
    use crate::content::frontmatter::FeaturedImage;
    use crate::directive::DirectiveArgs;
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
            .register_script(crate::render::assets::ScriptTag {
                url: "/script.js".into(),
                load: LoadStrategy::Defer,
                module: false,
            })
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
    fn render_directive_renders_template_with_context() {
        let (_dir, engine) = engine_with_directive(
            "test",
            "<div{% if id %} id={{ id }}{% endif %}>{{ name }}: {{ body_html | safe }} {{ config.title }}</div>",
        );
        let args = DirectiveArgs {
            identity: Identity {
                id: Some("anchor".into()),
                classes: Vec::new(),
            },
            ..DirectiveArgs::default()
        };
        let config: Config = toml::from_str(indoc! {r#"
            base_url = "https://example.com"
            title = "Probe"
        "#})
        .unwrap();

        let html = engine
            .render_directive(
                &directive_ctx("test", &args, "<p>hello</p>"),
                &AssetsHandle::default(),
                &config,
            )
            .unwrap()
            .unwrap();
        assert_eq!(html, "<div id=anchor>test: <p>hello</p> Probe</div>");
    }

    #[test]
    fn render_directive_missing_template_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        test_fs::create_dir_all(dir.path().join("directives")).unwrap();
        // A traversal would reach this file through "directives/../secret.html".
        test_fs::write(dir.path().join("secret.html"), "LEAKED").unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        let args = DirectiveArgs::default();
        for name in ["nonexistent", "../secret"] {
            assert!(
                engine
                    .render_directive(
                        &directive_ctx(name, &args, ""),
                        &AssetsHandle::default(),
                        &Config::default()
                    )
                    .is_none(),
                "{name}"
            );
        }
    }

    #[test]
    fn render_directive_template_failure_returns_error() {
        for (source, kind) in [
            ("{% invalid %}", minijinja::ErrorKind::SyntaxError),
            (
                "{% for x in 42 %}{{ x }}{% endfor %}",
                minijinja::ErrorKind::InvalidOperation,
            ),
        ] {
            let (_dir, engine) = engine_with_directive("bad", source);
            let error = engine
                .render_directive(
                    &directive_ctx("bad", &DirectiveArgs::default(), ""),
                    &AssetsHandle::default(),
                    &Config::default(),
                )
                .unwrap()
                .unwrap_err();
            assert_eq!(
                error.downcast_ref::<minijinja::Error>().unwrap().kind(),
                kind
            );
            assert_eq!(
                error.to_string(),
                "failed to render directive template: directives/bad.html"
            );
        }
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

    fn engine_with_directive(name: &str, body: &str) -> (tempfile::TempDir, TemplateEngine) {
        let dir = tempfile::tempdir().unwrap();
        let directives_dir = dir.path().join("directives");
        test_fs::create_dir_all(&directives_dir).unwrap();
        test_fs::write(directives_dir.join(format!("{name}.html")), body).unwrap();
        let engine = TemplateEngine::new(Some(dir.path()), None, &test_i18n()).unwrap();
        (dir, engine)
    }

    fn directive_ctx<'a>(
        name: &'a str,
        args: &'a DirectiveArgs,
        body_html: &'a str,
    ) -> DirectiveContext<'a> {
        DirectiveContext {
            name,
            positional_args: &args.positional,
            named_args: &args.named,
            identity: &args.identity,
            body_html,
            callout: None,
            source_dir: None,
            page_url: "/",
        }
    }
}
