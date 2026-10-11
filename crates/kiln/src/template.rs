mod functions;
pub mod vars;

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use minijinja::path_loader;
use minijinja::value::{Kwargs, Value, merge_maps};
use serde::Serialize;

use self::functions::{
    AssetUrls, tpl_asset_url, tpl_now, tpl_parse_csv, tpl_read_file, tpl_register_script, tpl_t,
};
use self::vars::{ArchivePageVars, HomePageVars, OverviewPageVars, PageMetadata, PostTemplateVars};
use crate::config::Config;
use crate::directive::DirectiveContext;
use crate::i18n::I18n;
use crate::render::PageResources;
use crate::render::assets::{AssetsHandle, PageAssets};
use crate::static_assets::StaticAssetManifest;

#[derive(Debug)]
pub struct TemplateEngine {
    env: minijinja::Environment<'static>,
}

impl TemplateEngine {
    /// Creates a layered template engine: `site_dir` overrides `theme_dir`.
    ///
    /// `site_dir` is silently ignored when missing (optional override). `theme_dir`, when set,
    /// must be a directory. At least one usable directory is required. Every template sees
    /// `config` as a global, `t()` looks up `i18n`, and `asset_url()` resolves through
    /// `static_assets`.
    ///
    /// # Errors
    ///
    /// Returns an error if no usable template directory exists or the theme directory is invalid.
    pub fn new(
        site_dir: Option<&Path>,
        theme_dir: Option<&Path>,
        config: &Config,
        i18n: &I18n,
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
        env.add_global("config", Value::from_serialize(config));
        env.add_function("now", tpl_now);
        env.add_function("read_file", tpl_read_file);
        env.add_function("parse_csv", tpl_parse_csv);

        let asset_urls = Arc::new(AssetUrls {
            manifest: static_assets.clone(),
            prefix: config.base_url.prefix().to_owned(),
        });
        let urls = Arc::clone(&asset_urls);
        env.add_function("asset_url", move |state: &minijinja::State, url: &str| {
            tpl_asset_url(state, &urls, url)
        });
        env.add_function(
            "register_script",
            move |state: &minijinja::State, url: &str, kwargs: Kwargs| {
                tpl_register_script(state, &asset_urls, url, &kwargs)
            },
        );

        let t_i18n = i18n.clone();
        env.add_function("t", move |key: &str, kwargs: Kwargs| {
            tpl_t(&t_i18n, key, &kwargs)
        });

        Ok(Self { env })
    }

    /// Renders a post or standalone page using the `post.html` template.
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_post(&self, vars: &PostTemplateVars<'_>, assets: &PageAssets) -> Result<String> {
        self.render_page_template("post.html", vars, &vars.metadata.url, assets)
    }

    /// Renders the home page using the `home.html` template.
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_home(&self, vars: &HomePageVars<'_>) -> Result<String> {
        self.render_generated("home.html", vars, &vars.metadata.url)
    }

    /// Renders an archive page using the `archive.html` template.
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_archive(&self, vars: &ArchivePageVars<'_>) -> Result<String> {
        self.render_generated("archive.html", vars, &vars.metadata.url)
    }

    /// Renders a bucket overview page (e.g., `/tags/`, `/sections/`).
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_overview(&self, vars: &OverviewPageVars<'_>) -> Result<String> {
        self.render_generated("overview.html", vars, &vars.metadata.url)
    }

    /// Renders the 404 error page using the `404.html` template.
    ///
    /// # Errors
    ///
    /// Returns an error if the template is missing or rendering fails.
    pub fn render_404(&self, vars: &PageMetadata<'_>) -> Result<String> {
        self.render_generated("404.html", vars, &vars.url)
    }

    /// Tries to render a directive using `directives/<name>.html`.
    ///
    /// Returns `None` if no template exists. `Some(Err(_))` if loading or rendering fails.
    pub(crate) fn render_directive(
        &self,
        ctx: &DirectiveContext<'_>,
        resources: &PageResources<'_>,
        assets: &AssetsHandle,
    ) -> Option<Result<String>> {
        let name = format!("directives/{}.html", ctx.name);
        let template = match self.env.get_template(&name) {
            Err(error) if error.kind() == minijinja::ErrorKind::TemplateNotFound => return None,
            template => template,
        };
        let context = merge_maps([
            Value::from_serialize(ctx),
            minijinja::context! {
                __page_url => resources.page_url,
                __source_dir => resources.source_dir.map(Path::to_string_lossy),
                __assets => Value::from_object(assets.clone()),
            },
        ]);
        Some(
            template
                .and_then(|template| template.render(context))
                .with_context(|| format!("failed to render directive template: {name}")),
        )
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

    /// Renders a generated page, whose lack of Markdown content leaves no assets to register.
    fn render_generated(&self, name: &str, vars: impl Serialize, page_url: &str) -> Result<String> {
        self.render_page_template(name, vars, page_url, &PageAssets::default())
    }

    /// Renders a whole page, exposing `assets` and the page URL that `asset_url()` resolves
    /// relative paths against.
    fn render_page_template(
        &self,
        name: &str,
        vars: impl Serialize,
        page_url: &str,
        assets: &PageAssets,
    ) -> Result<String> {
        let context = merge_maps([
            Value::from_serialize(vars),
            minijinja::context! { __page_url => page_url, assets },
        ]);
        self.env
            .get_template(name)
            .with_context(|| format!("failed to load {name} template"))?
            .render(context)
            .with_context(|| format!("failed to render {name} template"))
    }
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
    use crate::render::assets::{Feature, LoadStrategy, ScriptTag};
    use crate::template::vars::{BucketSummary, LinkedTerm, PageGroup, PageSummary};
    use crate::test_utils::{
        test_engine, test_engine_with, test_i18n, test_resources, test_site_engine,
    };

    // ── new ──

    #[test]
    fn new_with_site_dir_only() {
        let dir = tempfile::tempdir().unwrap();
        test_fs::write(dir.path().join("test.html"), "hello").unwrap();
        let engine = test_site_engine(dir.path());
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

        let engine = test_engine_with(Some(&site_dir), Some(&theme_dir), &Config::default());
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
        let engine = test_engine_with(
            Some(Path::new("/nonexistent")),
            Some(&theme_dir),
            &Config::default(),
        );
        assert!(!engine.has_template("post.html"));
    }

    #[test]
    fn new_without_directories_returns_error() {
        let err = new_engine(None, None).unwrap_err().to_string();
        assert!(
            err.contains("no valid template directory found"),
            "should reject when no dirs provided, got: {err}"
        );
    }

    #[test]
    fn new_nonexistent_theme_directory_returns_error() {
        let err = new_engine(None, Some(Path::new("/nonexistent/path")))
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
        let mut vars = post_vars();
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
        let mut assets = PageAssets::default();
        assets.add_feature(Feature::Math);
        assets
            .register_script(ScriptTag {
                url: "/script.js".into(),
                load: LoadStrategy::Defer,
                module: false,
            })
            .unwrap();

        assert_eq!(
            engine.render_post(&vars, &assets).unwrap(),
            "Post A|Description|https:&#x2f;&#x2f;example.com&#x2f;post-a&#x2f;|2026-01-01T12:34:56Z|2026-02-01T12:34:56Z|&#x2f;image.webp|&#x2f;page.css|My Site|All Posts|True|&#x2f;script.js|CC0-1.0|Tag A|Section A"
        );
    }

    #[test]
    fn render_post_escapes_text_and_preserves_explicit_safe_html() {
        let engine = engine_with_template(
            "post.html",
            "{{ title }}|{{ content }}|{{ content | safe }}|{{ toc | safe }}",
        );
        let mut vars = post_vars();
        vars.metadata.title = "<title>";
        vars.content = "<strong>Body</strong>";
        vars.toc = "<nav>Contents</nav>";

        assert_eq!(
            engine.render_post(&vars, &PageAssets::default()).unwrap(),
            "&lt;title&gt;|&lt;strong&gt;Body&lt;&#x2f;strong&gt;|<strong>Body</strong>|<nav>Contents</nav>"
        );
    }

    // ── render_home ──

    #[test]
    fn render_home_exposes_pages_and_pagination() {
        let engine = engine_with_template(
            "home.html",
            "{{ title }}|{{ description }}|{{ url }}|{% for page in pages %}{{ page.title }}:{{ page.url }}{% endfor %}|{{ pagination.current_page }}/{{ pagination.total_pages }}|{{ assets.features | length }}",
        );
        let page = page_summary();
        let vars = HomePageVars {
            metadata: PageMetadata {
                title: "Home",
                description: "Latest pages",
                url: "https://example.com/page/2/".into(),
            },
            pages: vec![&page],
            pagination: PaginationVars::new("/", 2, 3),
        };

        assert_eq!(
            engine.render_home(&vars).unwrap(),
            "Home|Latest pages|https:&#x2f;&#x2f;example.com&#x2f;page&#x2f;2&#x2f;|Post A:&#x2f;post-a&#x2f;|2/3|0"
        );
    }

    // ── render_archive ──

    #[test]
    fn render_archive_exposes_groups_and_pagination() {
        let engine = engine_with_template(
            "archive.html",
            "{{ title }}|{{ description }}|{{ url }}|{{ kind }}|{% for group in page_groups %}{{ group.key }}:{% for page in group.pages %}{{ page.title }}:{{ page.url }}{% endfor %}{% endfor %}|{{ pagination.current_page }}/{{ pagination.total_pages }}|{{ assets.features | length }}",
        );
        let page = page_summary();
        let vars = ArchivePageVars {
            metadata: PageMetadata {
                title: "Rust",
                description: "Description",
                url: "https://example.com/tags/rust/page/2/".into(),
            },
            kind: "tag",
            page_groups: vec![PageGroup {
                key: "2026".into(),
                pages: vec![&page],
            }],
            pagination: PaginationVars::new("/tags/rust/", 2, 3),
        };

        assert_eq!(
            engine.render_archive(&vars).unwrap(),
            "Rust|Description|https:&#x2f;&#x2f;example.com&#x2f;tags&#x2f;rust&#x2f;page&#x2f;2&#x2f;|tag|2026:Post A:&#x2f;post-a&#x2f;|2/3|0"
        );
    }

    // ── render_overview ──

    #[test]
    fn render_overview_exposes_buckets() {
        let engine = engine_with_template(
            "overview.html",
            "{{ title }}|{{ description }}|{{ url }}|{{ kind }}|{% for bucket in buckets %}{{ bucket.name }}:{{ bucket.url }}:{% for page in bucket.pages %}{{ page.title }}:{{ page.url }}{% endfor %}|{% endfor %}{{ assets.features | length }}",
        );
        let page = page_summary();
        let vars = OverviewPageVars {
            metadata: PageMetadata {
                title: "All Tags",
                description: "Description",
                url: "https://example.com/tags/".into(),
            },
            kind: "tag",
            buckets: vec![
                BucketSummary {
                    name: "Rust".into(),
                    url: "/tags/rust/".into(),
                    pages: vec![&page],
                },
                BucketSummary {
                    name: "Empty".into(),
                    url: "/tags/empty/".into(),
                    pages: Vec::new(),
                },
            ],
        };

        assert_eq!(
            engine.render_overview(&vars).unwrap(),
            "All Tags|Description|https:&#x2f;&#x2f;example.com&#x2f;tags&#x2f;|tag|Rust:&#x2f;tags&#x2f;rust&#x2f;:Post A:&#x2f;post-a&#x2f;|Empty:&#x2f;tags&#x2f;empty&#x2f;:|0"
        );
    }

    // ── render_404 ──

    #[test]
    fn render_404_exposes_metadata_and_resolves_relative_assets() {
        let dir = tempfile::tempdir().unwrap();
        let static_dir = dir.path().join("public");
        let templates = dir.path().join("templates");
        test_fs::create_dir_all(&static_dir).unwrap();
        test_fs::write(static_dir.join("bg.png"), "png").unwrap();
        test_fs::create_dir_all(&templates).unwrap();
        test_fs::write(
            templates.join("404.html"),
            "{{ title }}|{{ description }}|{{ url }}|{{ asset_url('bg.png') }}|{{ assets.features | length }}",
        )
        .unwrap();
        let engine = TemplateEngine::new(
            Some(&templates),
            None,
            &Config::default(),
            &test_i18n(),
            &StaticAssetManifest::build(&static_dir).unwrap(),
        )
        .unwrap();

        let vars = PageMetadata {
            title: "Not Found",
            description: "Description",
            url: "https://example.com/404.html".into(),
        };
        assert_eq!(
            engine.render_404(&vars).unwrap(),
            "Not Found|Description|https:&#x2f;&#x2f;example.com&#x2f;404.html|/_assets/bg.8f8cbb7dcf46.png|0"
        );
    }

    // ── render_directive ──

    #[test]
    fn render_directive_renders_template_with_context() {
        let dir = tempfile::tempdir().unwrap();
        let directives = dir.path().join("directives");
        test_fs::create_dir_all(&directives).unwrap();
        test_fs::write(
            directives.join("test.html"),
            "<div{% if id %} id={{ id }}{% endif %}>{{ name }}: {{ body_html | safe }} {{ config.title }} {{ read_file('data.txt') }}</div>",
        )
        .unwrap();
        let source = tempfile::tempdir().unwrap();
        test_fs::write(source.path().join("data.txt"), "data").unwrap();
        let config: Config = toml::from_str(indoc! {r#"
            base_url = "https://example.com"
            title = "Probe"
        "#})
        .unwrap();
        let engine = test_engine_with(Some(dir.path()), None, &config);
        let args = DirectiveArgs {
            identity: Identity {
                id: Some("anchor".into()),
                classes: Vec::new(),
            },
            ..DirectiveArgs::default()
        };

        let html = engine
            .render_directive(
                &directive_ctx("test", &args, "<p>hello</p>"),
                &PageResources {
                    source_dir: Some(source.path()),
                    ..test_resources()
                },
                &AssetsHandle::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(html, "<div id=anchor>test: <p>hello</p> Probe data</div>");
    }

    #[test]
    fn render_directive_missing_template_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        test_fs::create_dir_all(dir.path().join("directives")).unwrap();
        // A traversal would reach this file through "directives/../secret.html".
        test_fs::write(dir.path().join("secret.html"), "LEAKED").unwrap();
        let engine = test_site_engine(dir.path());
        let args = DirectiveArgs::default();
        for name in ["nonexistent", "../secret"] {
            assert!(
                engine
                    .render_directive(
                        &directive_ctx(name, &args, ""),
                        &test_resources(),
                        &AssetsHandle::default(),
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
                    &test_resources(),
                    &AssetsHandle::default(),
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
        assert!(engine.has_template("home.html"));
        assert!(engine.has_template("archive.html"));
        assert!(engine.has_template("overview.html"));
        assert!(engine.has_template("404.html"));
    }

    #[test]
    fn has_template_broken_returns_true() {
        let dir = tempfile::tempdir().unwrap();
        test_fs::write(dir.path().join("broken.html"), "{% invalid %}").unwrap();
        let engine = test_site_engine(dir.path());
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

    // ── render_page_template ──

    #[test]
    fn render_page_template_keeps_internal_values_over_vars() {
        let engine = engine_with_template(
            "post.html",
            "{{ __page_url }}|{{ assets.features | length }}",
        );
        let html = engine
            .render_page_template(
                "post.html",
                minijinja::context! { __page_url => "/vars/", assets => "vars" },
                "/page/",
                &PageAssets::default(),
            )
            .unwrap();
        assert_eq!(html, "&#x2f;page&#x2f;|0");
    }

    #[test]
    fn render_page_template_failure_returns_error() {
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
            let engine = test_site_engine(dir.path());
            let error = engine
                .render_page_template(name, (), "/", &PageAssets::default())
                .unwrap_err();
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

    fn new_engine(site_dir: Option<&Path>, theme_dir: Option<&Path>) -> Result<TemplateEngine> {
        TemplateEngine::new(
            site_dir,
            theme_dir,
            &Config::default(),
            &test_i18n(),
            &StaticAssetManifest::default(),
        )
    }

    fn engine_with_template(name: &'static str, source: &'static str) -> TemplateEngine {
        let mut engine = test_engine();
        engine.env.add_template(name, source).unwrap();
        engine
    }

    fn post_vars() -> PostTemplateVars<'static> {
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
            content: "",
            toc: "",
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
        let engine = test_site_engine(dir.path());
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
        }
    }
}
