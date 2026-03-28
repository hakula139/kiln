use std::path::Path;

use anyhow::{Context, Result};
use syntect::parsing::SyntaxSet;

use crate::config::Config;
use crate::content::discovery::discover_content;
use crate::content::page::Page;
use crate::output::{clean_output_dir, copy_file, copy_static, write_output};
use crate::pagination::{PaginationVars, Paginator, page_url as pagination_url};
use crate::render::RenderOptions;
use crate::render::pipeline::render_page;
use crate::taxonomy::{TaxonomyKind, Term, build_taxonomies};
use crate::template::{
    PageSummary, PostTemplateVars, TaxonomyIndexVars, TemplateEngine, TermPageVars, TermSummary,
};

/// Shared build state, created once per build invocation.
struct BuildContext {
    config: Config,
    syntax_set: SyntaxSet,
    template_engine: TemplateEngine,
}

/// Builds the site from the given project root directory.
///
/// When `base_url_override` is provided, it replaces the `base_url` from
/// config. This is used by `kiln serve` to match the actual server port.
///
/// # Errors
///
/// Returns an error if configuration loading, content discovery, rendering,
/// or output writing fails.
pub fn build(root: &Path, base_url_override: Option<&str>) -> Result<()> {
    let mut config = Config::load(root).context("failed to load config")?;
    if let Some(base_url) = base_url_override {
        base_url.clone_into(&mut config.base_url);
    }
    let syntax_set = SyntaxSet::load_defaults_newlines();

    let site_templates = root.join("templates");
    let theme_dir = config.theme_dir(root);
    let theme_templates = theme_dir.as_ref().map(|d| d.join("templates"));

    if config.theme.is_none() {
        tracing::warn!("no theme configured; set `theme` in config.toml to use a theme");
    }
    if !site_templates.is_dir() && theme_templates.as_ref().is_none_or(|d| !d.is_dir()) {
        tracing::warn!("no templates found; provide templates/ or configure a theme");
    }

    let template_engine = TemplateEngine::new(Some(&site_templates), theme_templates.as_deref())
        .context("failed to initialize template engine")?;

    let ctx = BuildContext {
        config,
        syntax_set,
        template_engine,
    };

    let content = discover_content(root)?;
    let output_dir = root.join(&ctx.config.output_dir);

    clean_output_dir(&output_dir)?;

    // Theme static files first, then site static files (site overrides theme).
    if let Some(ref td) = theme_dir {
        copy_static(&td.join("static"), &output_dir)?;
    }
    copy_static(&root.join("static"), &output_dir)?;

    // Precompute page summaries for taxonomy listings.
    let page_summaries: Vec<PageSummary> = content
        .pages
        .iter()
        .filter_map(|page| page_summary(page, &content.content_dir, &ctx.config.base_url))
        .collect();

    for page in &content.pages {
        build_page(&ctx, page, &content.content_dir, &output_dir)?;
    }

    build_taxonomy_pages(
        &ctx,
        &page_summaries,
        &content.pages,
        &content.content_dir,
        &output_dir,
    )?;

    println!("Build complete: {} page(s).", content.pages.len());
    Ok(())
}

/// Builds a `PageSummary` for use in taxonomy / listing templates.
///
/// Returns `None` if the output path cannot be computed (shouldn't happen
/// for pages that passed discovery).
fn page_summary(page: &Page, content_dir: &Path, base_url: &str) -> Option<PageSummary> {
    let output_path = page.output_path(content_dir).ok()?;
    let url = page_url(base_url, &output_path);
    Some(PageSummary {
        title: page.frontmatter.title.clone(),
        url,
        date: page.frontmatter.date.map(|d| d.to_string()),
        description: page.frontmatter.description.clone().unwrap_or_default(),
        featured_image: page.frontmatter.featured_image.clone(),
    })
}

/// Renders a single page and writes it to the output directory.
fn build_page(
    ctx: &BuildContext,
    page: &Page,
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    let options = RenderOptions::from_params(&ctx.config.params);

    let rendered = render_page(
        &page.raw_content,
        &ctx.syntax_set,
        &ctx.template_engine,
        &options,
        page.source_path.parent(),
    )
    .with_context(|| format!("failed to render {}", page.source_path.display()))?;

    let output_path = page.output_path(content_dir).with_context(|| {
        format!(
            "failed to compute output path for {}",
            page.source_path.display()
        )
    })?;
    let url = page_url(&ctx.config.base_url, &output_path);

    let vars = PostTemplateVars {
        title: &page.frontmatter.title,
        description: page.frontmatter.description.as_deref().unwrap_or(""),
        url: &url,
        featured_image: page.frontmatter.featured_image.as_deref(),
        date: page.frontmatter.date.map(|d| d.to_string()),
        content: &rendered.content_html,
        toc: &rendered.toc_html,
        config: &ctx.config,
    };

    let html = ctx
        .template_engine
        .render_post(&vars)
        .with_context(|| format!("failed to render {}", page.source_path.display()))?;

    let dest = output_dir.join(&output_path);
    write_output(&dest, &html).with_context(|| format!("failed to write {}", dest.display()))?;

    // Copy co-located assets (images, etc.) alongside the rendered page.
    if let Some(bundle_dir) = page.source_path.parent() {
        let asset_output_dir = dest.parent().expect("output file should have a parent");
        for asset in &page.assets {
            let relative = asset.strip_prefix(bundle_dir).with_context(|| {
                format!(
                    "asset {} is not under {}",
                    asset.display(),
                    bundle_dir.display()
                )
            })?;
            let asset_dest = asset_output_dir.join(relative);
            copy_file(asset, &asset_dest)
                .with_context(|| format!("failed to copy asset {}", asset.display()))?;
        }
    }

    Ok(())
}

/// Generates taxonomy index pages and paginated term pages.
fn build_taxonomy_pages(
    ctx: &BuildContext,
    page_summaries: &[PageSummary],
    pages: &[Page],
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    let taxonomy_set = build_taxonomies(pages, Some(content_dir));

    let per_page = ctx
        .config
        .params
        .get("paginate")
        .and_then(toml::Value::as_integer)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(10);

    for taxonomy in &taxonomy_set.taxonomies {
        let kind = taxonomy.kind;
        let base_path = format!("/{}", kind.plural());

        // Build taxonomy index page (e.g., /tags/index.html).
        let term_summaries: Vec<TermSummary> = taxonomy
            .terms
            .iter()
            .map(|term| TermSummary {
                name: term.name.clone(),
                slug: term.slug.clone(),
                url: format!("{base_path}/{}/", term.slug),
                page_count: term.page_count,
            })
            .collect();

        let vars = TaxonomyIndexVars {
            kind: kind.plural(),
            singular: kind.singular(),
            terms: term_summaries,
            config: &ctx.config,
        };

        let html = ctx
            .template_engine
            .render_taxonomy(&vars)
            .with_context(|| format!("failed to render {} index", kind.plural()))?;

        let dest = output_dir.join(kind.plural()).join("index.html");
        write_output(&dest, &html)
            .with_context(|| format!("failed to write {}", dest.display()))?;

        // Build paginated term pages (e.g., /tags/rust/index.html).
        for term in &taxonomy.terms {
            let key = (kind, term.slug.clone());
            let Some(page_indices) = taxonomy_set.term_pages.get(&key) else {
                continue;
            };

            let term_page_summaries: Vec<&PageSummary> = page_indices
                .iter()
                .filter_map(|&idx| page_summaries.get(idx))
                .collect();

            build_term_pages(ctx, kind, term, &term_page_summaries, per_page, output_dir)?;
        }
    }

    Ok(())
}

/// Generates paginated pages for a single taxonomy term.
fn build_term_pages(
    ctx: &BuildContext,
    kind: TaxonomyKind,
    term: &Term,
    page_summaries: &[&PageSummary],
    per_page: usize,
    output_dir: &Path,
) -> Result<()> {
    let term_base = format!("/{}/{}", kind.plural(), term.slug);
    let paginator = Paginator::new(page_summaries, per_page);

    for page_num in 1..=paginator.total_pages() {
        let items = paginator.page_items(page_num);
        let pagination = PaginationVars::new(&term_base, page_num, paginator.total_pages());

        let vars = TermPageVars {
            kind: kind.plural(),
            singular: kind.singular(),
            term_name: &term.name,
            term_slug: &term.slug,
            pages: items.iter().copied().cloned().collect(),
            pagination,
            config: &ctx.config,
        };

        let html = ctx.template_engine.render_term(&vars).with_context(|| {
            format!(
                "failed to render {}/{} page {}",
                kind.plural(),
                term.slug,
                page_num
            )
        })?;

        let rel_path = pagination_url(&term_base, page_num);
        let dest = output_dir
            .join(rel_path.trim_start_matches('/'))
            .join("index.html");
        write_output(&dest, &html)
            .with_context(|| format!("failed to write {}", dest.display()))?;
    }

    Ok(())
}

/// Computes the canonical URL for a page from its output path.
///
/// For `index.html` pages (page bundles), returns the directory path with a
/// trailing slash. For other files, returns the file path as-is.
pub(crate) fn page_url(base_url: &str, output_path: &Path) -> String {
    let base = base_url.trim_end_matches('/');
    let rel = output_path.to_string_lossy();

    // index.html → directory URL with trailing slash
    if let Some(dir) = rel.strip_suffix("index.html") {
        format!("{base}/{dir}")
    } else {
        format!("{base}/{rel}")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::indoc;

    use super::*;

    use crate::test_utils::{PermissionGuard, copy_templates};

    // -- page_url --

    #[test]
    fn page_url_index_html() {
        assert_eq!(
            page_url("https://example.com", Path::new("foo/bar/index.html")),
            "https://example.com/foo/bar/"
        );
    }

    #[test]
    fn page_url_root_index() {
        assert_eq!(
            page_url("https://example.com", Path::new("index.html")),
            "https://example.com/"
        );
    }

    #[test]
    fn page_url_non_index() {
        assert_eq!(
            page_url("https://example.com", Path::new("standalone.html")),
            "https://example.com/standalone.html"
        );
    }

    // -- build --

    #[test]
    fn build_no_content() {
        let root = tempfile::tempdir().unwrap();

        // Config + templates, but no content directory.
        fs::write(root.path().join("config.toml"), "").unwrap();
        let template_dest = root.path().join("templates");
        copy_templates(&template_dest);

        build(root.path(), None).unwrap();

        // Output directory exists and contains taxonomy index pages (but no post pages).
        let output_dir = root.path().join("public");
        assert!(output_dir.exists(), "output directory should exist");
        assert!(
            output_dir.join("tags").join("index.html").exists(),
            "should generate empty tags index"
        );
        assert!(
            output_dir.join("categories").join("index.html").exists(),
            "should generate empty categories index"
        );
    }

    #[test]
    fn build_end_to_end() {
        let root = tempfile::tempdir().unwrap();

        // Config
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
                title = "Test Site"
            "#},
        )
        .unwrap();

        // Templates — copy from real templates directory
        let template_dest = root.path().join("templates");
        copy_templates(&template_dest);

        // Content
        let content_dir = root.path().join("content").join("posts").join("hello");
        fs::create_dir_all(&content_dir).unwrap();
        fs::write(
            content_dir.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello World"
                description = "A test post"
                date = "2026-02-24T12:34:56Z"
                +++

                ## First

                This is a test **post**.

                ## Second

                More content.
            "#},
        )
        .unwrap();

        // Build
        build(root.path(), None).unwrap();

        // Verify output
        let output = root.path().join("public").join("hello").join("index.html");
        assert!(output.exists(), "output file should exist");

        let html = fs::read_to_string(&output).unwrap();

        // <head>
        assert!(
            html.contains("<title>Hello World - Test Site</title>"),
            "should have title, html:\n{html}"
        );
        assert!(
            html.contains(r#"<meta name="description" content="A test post">"#),
            "should have meta description, html:\n{html}"
        );
        assert!(
            html.contains(r#"<link rel="canonical" href="https://example.com/hello/">"#),
            "should have canonical URL, html:\n{html}"
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
            html.contains(r##"<a href="#first">First</a>"##),
            "should have ToC with links to headings, html:\n{html}"
        );
        assert!(
            html.contains("<p>This is a test <strong>post</strong>.</p>"),
            "should have rendered content, html:\n{html}"
        );
    }

    #[test]
    fn build_copies_static_files() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        // Create static files
        let static_dir = root.path().join("static");
        fs::create_dir_all(static_dir.join("images")).unwrap();
        fs::write(static_dir.join("favicon.ico"), "icon").unwrap();
        fs::write(static_dir.join("images").join("logo.png"), "logo").unwrap();

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        assert_eq!(
            fs::read_to_string(output_dir.join("favicon.ico")).unwrap(),
            "icon"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("images").join("logo.png")).unwrap(),
            "logo"
        );
    }

    #[test]
    fn build_copies_colocated_assets() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        // Page bundle with co-located assets
        let bundle = root.path().join("content").join("posts").join("hello");
        let assets_dir = bundle.join("assets");
        fs::create_dir_all(&assets_dir).unwrap();
        fs::write(
            bundle.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        )
        .unwrap();
        fs::write(bundle.join("cover.webp"), "cover-data").unwrap();
        fs::write(assets_dir.join("diagram.svg"), "svg-data").unwrap();

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public").join("hello");
        assert_eq!(
            fs::read_to_string(output_dir.join("cover.webp")).unwrap(),
            "cover-data"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("assets").join("diagram.svg")).unwrap(),
            "svg-data"
        );
    }

    #[test]
    fn build_cleans_stale_output() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        // Pre-existing stale output
        let output_dir = root.path().join("public");
        fs::create_dir_all(output_dir.join("old")).unwrap();
        fs::write(output_dir.join("old").join("stale.html"), "stale").unwrap();

        build(root.path(), None).unwrap();

        assert!(
            !output_dir.join("old").exists(),
            "stale output should be removed"
        );
    }

    // -- build with theme --

    /// Sets up a minimal theme for build tests.
    fn setup_theme(root: &Path, theme_name: &str) {
        let theme_dir = root.join("themes").join(theme_name);
        let tmpl_dir = theme_dir.join("templates");
        fs::create_dir_all(&tmpl_dir).unwrap();
        copy_templates(&tmpl_dir);
        fs::write(theme_dir.join("theme.toml"), "").unwrap();
    }

    #[test]
    fn build_with_theme() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
                title = "Test"
                theme = "my-theme"
            "#},
        )
        .unwrap();
        setup_theme(root.path(), "my-theme");

        let content_dir = root.path().join("content").join("posts").join("hello");
        fs::create_dir_all(&content_dir).unwrap();
        fs::write(
            content_dir.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        )
        .unwrap();

        build(root.path(), None).unwrap();

        let output = root.path().join("public").join("hello").join("index.html");
        assert!(output.exists(), "output file should exist");
        let html = fs::read_to_string(&output).unwrap();
        assert!(
            html.contains("<h1>Hello</h1>"),
            "should render with theme templates, html:\n{html}"
        );
    }

    #[test]
    fn build_theme_static_files_with_site_override() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), r#"theme = "my-theme""#).unwrap();
        setup_theme(root.path(), "my-theme");

        // Theme static file.
        let theme_static = root.path().join("themes/my-theme/static");
        fs::create_dir_all(&theme_static).unwrap();
        fs::write(theme_static.join("theme.css"), "theme-default").unwrap();
        fs::write(theme_static.join("shared.css"), "from-theme").unwrap();

        // Site static file overrides shared.css.
        let site_static = root.path().join("static");
        fs::create_dir_all(&site_static).unwrap();
        fs::write(site_static.join("shared.css"), "from-site").unwrap();

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        assert_eq!(
            fs::read_to_string(output_dir.join("theme.css")).unwrap(),
            "theme-default",
            "theme-only static file should be copied"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("shared.css")).unwrap(),
            "from-site",
            "site static file should override theme"
        );
    }

    // -- build with taxonomies --

    #[test]
    fn build_generates_taxonomy_index_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        let page_dir = root.path().join("content").join("posts").join("hello");
        fs::create_dir_all(&page_dir).unwrap();
        fs::write(
            page_dir.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                tags = ["rust", "web"]
                categories = ["tutorial"]
                +++
                Body
            "#},
        )
        .unwrap();

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        let tags_index = output_dir.join("tags").join("index.html");
        assert!(tags_index.exists(), "should generate /tags/index.html");
        let html = fs::read_to_string(&tags_index).unwrap();
        assert!(
            html.contains("rust") && html.contains("web"),
            "tags index should list terms, html:\n{html}"
        );

        let cats_index = output_dir.join("categories").join("index.html");
        assert!(
            cats_index.exists(),
            "should generate /categories/index.html"
        );
        let html = fs::read_to_string(&cats_index).unwrap();
        assert!(
            html.contains("tutorial"),
            "categories index should list terms, html:\n{html}"
        );
    }

    #[test]
    fn build_generates_term_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        for (name, tag) in &[("post-1", "rust"), ("post-2", "rust"), ("post-3", "web")] {
            let page_dir = root.path().join("content").join("posts").join(name);
            fs::create_dir_all(&page_dir).unwrap();
            fs::write(
                page_dir.join("index.md"),
                format!(
                    indoc! {r#"
                        +++
                        title = "{name}"
                        tags = ["{tag}"]
                        +++
                        Body
                    "#},
                    name = name,
                    tag = tag,
                ),
            )
            .unwrap();
        }

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        let rust_page = output_dir.join("tags").join("rust").join("index.html");
        assert!(rust_page.exists(), "should generate /tags/rust/index.html");
        let html = fs::read_to_string(&rust_page).unwrap();
        assert!(
            html.contains("post-1") && html.contains("post-2"),
            "term page should list posts, html:\n{html}"
        );
        assert!(
            !html.contains("post-3"),
            "term page should not include unrelated posts, html:\n{html}"
        );
    }

    #[test]
    fn build_generates_paginated_term_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r"
                [params]
                paginate = 2
            "},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        // Create 3 pages with the same tag → 2 pages of pagination.
        for i in 1..=3 {
            let page_dir = root
                .path()
                .join("content")
                .join("posts")
                .join(format!("post-{i}"));
            fs::create_dir_all(&page_dir).unwrap();
            fs::write(
                page_dir.join("index.md"),
                format!(
                    indoc! {r#"
                        +++
                        title = "Post {i}"
                        tags = ["rust"]
                        date = "2026-01-0{i}T00:00:00Z"
                        +++
                        Body
                    "#},
                    i = i,
                ),
            )
            .unwrap();
        }

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");

        // Page 1.
        let page1 = output_dir.join("tags").join("rust").join("index.html");
        assert!(page1.exists(), "should generate page 1");
        let html1 = fs::read_to_string(&page1).unwrap();
        assert!(
            html1.contains("Page 1 / 2"),
            "should show pagination, html:\n{html1}"
        );
        assert!(
            html1.contains("Next"),
            "page 1 should have next link, html:\n{html1}"
        );

        // Page 2.
        let page2 = output_dir
            .join("tags")
            .join("rust")
            .join("page")
            .join("2")
            .join("index.html");
        assert!(page2.exists(), "should generate page 2");
        let html2 = fs::read_to_string(&page2).unwrap();
        assert!(
            html2.contains("Page 2 / 2"),
            "should show page 2, html:\n{html2}"
        );
        assert!(
            html2.contains("Prev"),
            "page 2 should have prev link, html:\n{html2}"
        );
    }

    #[test]
    fn build_no_taxonomy_pages_without_tags() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        let page_dir = root.path().join("content").join("posts").join("hello");
        fs::create_dir_all(&page_dir).unwrap();
        fs::write(
            page_dir.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        )
        .unwrap();

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        // Taxonomy index pages should still be generated (even if empty).
        let tags_index = output_dir.join("tags").join("index.html");
        assert!(
            tags_index.exists(),
            "should generate /tags/index.html even with no tags"
        );
    }

    // -- build errors --

    /// Creates a minimal site with one page for error-path tests.
    fn setup_site_with_page(root: &Path) {
        fs::write(root.join("config.toml"), "").unwrap();
        copy_templates(&root.join("templates"));
        let page_dir = root.join("content").join("posts").join("hello");
        fs::create_dir_all(&page_dir).unwrap();
        fs::write(
            page_dir.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        )
        .unwrap();
    }

    #[test]
    fn build_invalid_config_returns_error() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "{{invalid toml").unwrap();

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to load config"),
            "should report config failure, got: {err}"
        );
    }

    #[test]
    fn build_missing_templates_returns_error() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to initialize template engine"),
            "should report template engine failure, got: {err}"
        );
    }

    #[test]
    fn build_broken_post_template_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        // Overwrite post.html with invalid Jinja syntax.
        fs::write(
            root.path().join("templates").join("post.html"),
            "{% invalid %}",
        )
        .unwrap();

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to render"),
            "should report render failure, got: {err}"
        );
    }

    #[test]
    fn build_write_permission_denied_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        // Build once to create output structure, then restrict the output dir.
        build(root.path(), None).unwrap();
        let output_dir = root.path().join("public");
        let _guard = PermissionGuard::restrict(&output_dir, 0o555);

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to write") || err.contains("failed to clean"),
            "should report write or clean failure, got: {err}"
        );
    }

    #[test]
    fn build_asset_copy_permission_denied_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        // Add a co-located asset.
        let page_dir = root.path().join("content").join("posts").join("hello");
        fs::write(page_dir.join("image.png"), "img-data").unwrap();

        // Build once to create output structure.
        build(root.path(), None).unwrap();

        // Make the page output dir read-only so asset copy fails on rebuild,
        // but the parent output dir stays writable for clean_output_dir.
        let page_output = root.path().join("public").join("hello");
        let _guard = PermissionGuard::restrict(&page_output, 0o555);

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to copy asset") || err.contains("failed to clean"),
            "should report asset copy or clean failure, got: {err}"
        );
    }

    #[test]
    fn build_broken_directive_template_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        // Directive template that compiles but fails at render time:
        // `items()` filter requires a map, not a string.
        let directives = root.path().join("templates").join("directives");
        fs::create_dir_all(&directives).unwrap();
        fs::write(
            directives.join("broken.html"),
            "{% for k, v in name | items %}{{ k }}{% endfor %}",
        )
        .unwrap();

        let page_dir = root.path().join("content").join("posts").join("hello");
        fs::write(
            page_dir.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                ::: broken
                Body
                :::
            "#},
        )
        .unwrap();

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to render"),
            "should report render failure, got: {err}"
        );
    }
}
