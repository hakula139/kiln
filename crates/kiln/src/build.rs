mod archive;
mod error;
mod feed;
mod home;
mod listing;
mod overview;
mod paginate;
mod sitemap;
mod url;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use jiff::tz::TimeZone;
use syntect::parsing::SyntaxSet;

use crate::config::Config;
use crate::content::discovery::discover_content;
use crate::content::page::{Page, PageKind};
use crate::minify::{self, MinifyStats};
use crate::output::{clean_output_dir, copy_file, copy_static, write_output};
use crate::render::RenderOptions;
use crate::render::pipeline::render_page;
use crate::search;
use crate::section::collect_sections;
use crate::taxonomy::build_taxonomies;
use crate::template::TemplateEngine;
use crate::template::vars::PostTemplateVars;

use self::listing::{
    build_listing_artifacts, format_page_date, page_section, resolve_featured_image,
};
use self::url::{page_url, resolve_relative_url};

/// Shared build state, created once per build invocation.
struct BuildContext {
    config: Config,
    time_zone: Option<TimeZone>,
    syntax_set: SyntaxSet,
    template_engine: TemplateEngine,
}

/// Builds the site from the given project root directory.
///
/// When `base_url_override` is provided, it replaces the `base_url` from
/// config. This is used by `kiln serve` to match the actual server port.
///
/// When `minify` is true, runs HTML / CSS / JS minification over the
/// output directory before Pagefind indexing, if the latter is enabled.
///
/// Search indexing (Pagefind) runs when `[search] enabled = true` in config.
///
/// # Errors
///
/// Returns an error if configuration loading, content discovery, rendering,
/// or output writing fails.
pub fn build(root: &Path, base_url_override: Option<&str>, minify: bool) -> Result<()> {
    build_to(root, base_url_override, None, minify)
}

/// Builds the site, optionally writing to a custom output directory.
///
/// Used by the dev server to build into a staging directory so the live
/// output stays intact until the new build succeeds.
pub(crate) fn build_to(
    root: &Path,
    base_url_override: Option<&str>,
    output_dir_override: Option<&Path>,
    minify: bool,
) -> Result<()> {
    let mut config = Config::load(root).context("failed to load config")?;
    if let Some(base_url) = base_url_override {
        base_url.clone_into(&mut config.base_url);
    }
    let time_zone = config
        .time_zone()
        .context("failed to resolve configured time zone")?;
    let syntax_set = two_face::syntax::extra_newlines();

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
        time_zone,
        syntax_set,
        template_engine,
    };

    let content = discover_content(root)?;
    let output_dir =
        output_dir_override.map_or_else(|| root.join(&ctx.config.output_dir), Path::to_owned);

    clean_output_dir(&output_dir)?;

    if let Some(ref td) = theme_dir {
        copy_static(&td.join("static"), &output_dir)?;
    }
    copy_static(&root.join("static"), &output_dir)?;

    let sections = collect_sections(&content.pages, &content.content_dir);
    let section_titles: HashMap<&str, &str> = sections
        .iter()
        .map(|s| (s.slug.as_str(), s.title.as_str()))
        .collect();

    let artifacts = build_listing_artifacts(
        &content.pages,
        &content.content_dir,
        &ctx.config.base_url,
        ctx.time_zone.as_ref(),
        &section_titles,
    )?;

    for page in &content.pages {
        build_page(
            &ctx,
            page,
            &content.content_dir,
            &output_dir,
            &section_titles,
        )?;
    }

    let taxonomy_set = build_taxonomies(&content.pages, Some(&content.content_dir));

    home::build_home_pages(&ctx, &artifacts.listed_posts, &output_dir)?;
    archive::build_archive_pages(
        &ctx,
        &artifacts,
        &sections,
        &taxonomy_set,
        &content.content_dir,
        &output_dir,
    )?;
    overview::build_overview_pages(&ctx, &artifacts, &sections, &taxonomy_set, &output_dir)?;

    feed::build_feeds(
        &ctx,
        &artifacts,
        &sections,
        &taxonomy_set,
        &content.content_dir,
        &output_dir,
    )?;
    sitemap::build_sitemap_and_robots(&ctx, &artifacts.listed_pages, &output_dir)?;
    error::build_404(&ctx, &output_dir)?;

    let minify_stats = if minify {
        eprintln!("Minifying...");
        Some(minify::minify_output_dir(&output_dir).context("minification failed")?)
    } else {
        None
    };

    if ctx.config.search.enabled {
        eprintln!("Running Pagefind...");
        search::run_pagefind(&output_dir, ctx.config.search.binary.as_deref())
            .context("search indexing failed")?;
    }

    report_build_summary(content.pages.len(), minify_stats.as_ref());
    Ok(())
}

/// Prints the end-of-build summary line(s).
fn report_build_summary(page_count: usize, minify_stats: Option<&MinifyStats>) {
    println!("Build complete: {page_count} page(s).");
    if let Some(stats) = minify_stats {
        println!("{stats}");
    }
}

// ── Single-page rendering ──

/// Renders a single page and writes it to the output directory.
fn build_page(
    ctx: &BuildContext,
    page: &Page,
    content_dir: &Path,
    output_dir: &Path,
    section_titles: &HashMap<&str, &str>,
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

    let featured_image = resolve_featured_image(page.frontmatter.featured_image.as_ref(), &url);
    let page_css = find_page_css(&page.assets, page.source_path.parent(), &url);
    let vars = PostTemplateVars {
        title: &page.frontmatter.title,
        description: page
            .frontmatter
            .description
            .as_deref()
            .or(page.summary.as_deref())
            .unwrap_or(""),
        url: &url,
        featured_image,
        page_css,
        date: page
            .frontmatter
            .date
            .map(|date| format_page_date(date, ctx.time_zone.as_ref())),
        section: page_section(page, &ctx.config.base_url, section_titles),
        math: page.frontmatter.math,
        content: &rendered.content_html,
        toc: &rendered.toc_html,
        config: &ctx.config,
    };

    let html = match page.kind {
        PageKind::Page if ctx.template_engine.has_template("page.html") => {
            ctx.template_engine.render_page(&vars)
        }
        _ => ctx.template_engine.render_post(&vars),
    }
    .with_context(|| format!("failed to render {}", page.source_path.display()))?;

    let dest = output_dir.join(&output_path);
    write_output(&dest, &html).with_context(|| format!("failed to write {}", dest.display()))?;

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

/// Finds a `style.css` file in the page bundle's assets and returns its
/// resolved URL path (e.g., `/posts/my-post/style.css`).
fn find_page_css(assets: &[PathBuf], bundle_dir: Option<&Path>, page_url: &str) -> Option<String> {
    let dir = bundle_dir?;
    let css = assets
        .iter()
        .find(|p| p.file_name().and_then(|n| n.to_str()) == Some("style.css"))?;
    let relative = css.strip_prefix(dir).ok()?;
    Some(resolve_relative_url(&relative.to_string_lossy(), page_url))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::indoc;

    use super::*;

    use crate::test_utils::{PermissionGuard, copy_templates, template_dir, write_test_file};

    /// Writes a content page at `content/<rel_path>/index.md`.
    fn write_page(root: &Path, rel_path: &str, content: &str) {
        write_test_file(root, &format!("content/{rel_path}/index.md"), content);
    }

    /// Copies all test templates except those listed in `exclude`.
    fn copy_templates_except(dest: &Path, exclude: &[&str]) {
        let src = template_dir();
        fs::create_dir_all(dest).unwrap();
        for entry in fs::read_dir(&src).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if !name.to_str().is_some_and(|n| exclude.contains(&n)) {
                fs::copy(entry.path(), dest.join(&name)).unwrap();
            }
        }
    }

    // ── build ──

    #[test]
    fn build_no_content() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        assert!(output_dir.exists(), "output directory should exist");
        assert!(
            output_dir.join("tags").join("index.html").exists(),
            "should generate empty tags index"
        );
    }

    #[test]
    fn build_end_to_end() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
                title = "Test Site"
            "#},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
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
        );

        build(root.path(), None, false).unwrap();

        let output = root
            .path()
            .join("public")
            .join("posts")
            .join("hello")
            .join("index.html");
        assert!(output.exists(), "output file should exist");

        let html = fs::read_to_string(&output).unwrap();

        assert!(
            html.contains("<title>Hello World - Test Site</title>"),
            "should have title, html:\n{html}"
        );
        assert!(
            html.contains(r#"<meta name="description" content="A test post">"#),
            "should have meta description, html:\n{html}"
        );
        assert!(
            html.contains(r#"<link rel="canonical" href="https://example.com/posts/hello/">"#),
            "should have canonical URL, html:\n{html}"
        );

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
    fn build_base_url_override() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
            "#},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );

        build(root.path(), Some("http://localhost:5456"), false).unwrap();

        let html = fs::read_to_string(
            root.path()
                .join("public")
                .join("posts")
                .join("hello")
                .join("index.html"),
        )
        .unwrap();
        assert!(
            html.contains("http://localhost:5456/posts/hello/"),
            "canonical URL should use overridden base_url, html:\n{html}"
        );
        assert!(
            !html.contains("https://example.com"),
            "should NOT use config base_url when overridden, html:\n{html}"
        );
    }

    #[test]
    fn build_copies_static_files() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        let static_dir = root.path().join("static");
        fs::create_dir_all(static_dir.join("images")).unwrap();
        fs::write(static_dir.join("favicon.ico"), "icon").unwrap();
        fs::write(static_dir.join("images").join("logo.png"), "logo").unwrap();

        build(root.path(), None, false).unwrap();

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

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );
        let bundle = root.path().join("content").join("posts").join("hello");
        fs::create_dir_all(bundle.join("assets")).unwrap();
        fs::write(bundle.join("cover.webp"), "cover-data").unwrap();
        fs::write(bundle.join("assets").join("diagram.svg"), "svg-data").unwrap();

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public").join("posts").join("hello");
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

        let output_dir = root.path().join("public");
        fs::create_dir_all(output_dir.join("old")).unwrap();
        fs::write(output_dir.join("old").join("stale.html"), "stale").unwrap();

        build(root.path(), None, false).unwrap();

        assert!(
            !output_dir.join("old").exists(),
            "stale output should be removed"
        );
    }

    // ── build: theme ──

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

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let output = root
            .path()
            .join("public")
            .join("posts")
            .join("hello")
            .join("index.html");
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

        let theme_static = root.path().join("themes/my-theme/static");
        fs::create_dir_all(&theme_static).unwrap();
        fs::write(theme_static.join("theme.css"), "theme-default").unwrap();
        fs::write(theme_static.join("shared.css"), "from-theme").unwrap();

        let site_static = root.path().join("static");
        fs::create_dir_all(&site_static).unwrap();
        fs::write(site_static.join("shared.css"), "from-site").unwrap();

        build(root.path(), None, false).unwrap();

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

    // ── build: page template ──

    #[test]
    fn build_uses_page_template_for_standalone() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "about-me",
            indoc! {r#"
                +++
                title = "About Me"
                +++
                Hello world.
            "#},
        );

        build(root.path(), None, false).unwrap();

        let output = root
            .path()
            .join("public")
            .join("about-me")
            .join("index.html");
        assert!(output.exists(), "should generate about-me page");
        let html = fs::read_to_string(&output).unwrap();
        assert!(
            html.contains(r#"<article class="page">"#),
            "should use page.html template, html:\n{html}"
        );
    }

    #[test]
    fn build_renders_dates_in_configured_timezone() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                timezone = "Asia/Shanghai"
            "#},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello"
                date = "2026-03-13T09:36:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let html = fs::read_to_string(
            root.path()
                .join("public")
                .join("posts")
                .join("note")
                .join("hello")
                .join("index.html"),
        )
        .unwrap();
        assert!(
            html.contains("2026-03-13T17:36:00+08:00"),
            "should render the configured time zone offset, html:\n{html}"
        );
        assert!(
            !html.contains("2026-03-13T09:36:00Z"),
            "should not leave the date in UTC, html:\n{html}"
        );
    }

    // ── build: page CSS ──

    #[test]
    fn build_injects_page_css_link() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );
        let bundle = root.path().join("content").join("posts").join("hello");
        fs::write(bundle.join("style.css"), ".custom { color: red; }").unwrap();

        build(root.path(), None, false).unwrap();

        let html = fs::read_to_string(
            root.path()
                .join("public")
                .join("posts")
                .join("hello")
                .join("index.html"),
        )
        .unwrap();
        assert!(
            html.contains(r#"<link rel="stylesheet" href="/posts/hello/style.css">"#),
            "should inject per-page CSS link, html:\n{html}"
        );
    }

    #[test]
    fn build_omits_page_css_without_style() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let html = fs::read_to_string(
            root.path()
                .join("public")
                .join("posts")
                .join("hello")
                .join("index.html"),
        )
        .unwrap();
        assert!(
            !html.contains("style.css"),
            "should NOT inject page CSS link when no style.css, html:\n{html}"
        );
    }

    // ── build: home page ──

    #[test]
    fn build_generates_home_page() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let home = root.path().join("public").join("index.html");
        assert!(home.exists(), "should generate home page /index.html");
        let html = fs::read_to_string(&home).unwrap();
        assert!(
            html.contains("Hello"),
            "home page should list posts, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="http://localhost:5456/posts/note/hello/">Hello</a>"#),
            "home page should link to the post under /posts/, html:\n{html}"
        );
    }

    #[test]
    fn build_home_orders_by_date_descending() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/aaa-old",
            indoc! {r#"
                +++
                title = "Old Post"
                date = "2025-01-01T00:00:00Z"
                +++
                Body
            "#},
        );
        write_page(
            root.path(),
            "posts/zzz-new",
            indoc! {r#"
                +++
                title = "New Post"
                date = "2026-06-01T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
        let new_pos = html.find("New Post").expect("should list New Post");
        let old_pos = html.find("Old Post").expect("should list Old Post");
        assert!(
            new_pos < old_pos,
            "newer post should appear before older post on home page, html:\n{html}"
        );
    }

    #[test]
    fn build_empty_home_page() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        build(root.path(), None, false).unwrap();

        let home = root.path().join("public").join("index.html");
        assert!(
            home.exists(),
            "should generate home page even with zero posts"
        );
    }

    #[test]
    fn build_orphan_posts_on_home_not_in_sections() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/sectioned",
            indoc! {r#"
                +++
                title = "Sectioned Post"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );
        write_page(
            root.path(),
            "posts/orphan",
            indoc! {r#"
                +++
                title = "Orphan Post"
                date = "2026-01-02T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let home_html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
        assert!(
            home_html.contains("Sectioned Post"),
            "sectioned post should also appear on home page, html:\n{home_html}"
        );
        assert!(
            home_html.contains("Orphan Post"),
            "orphan post should appear on home page, html:\n{home_html}"
        );

        let note_html = fs::read_to_string(
            root.path()
                .join("public")
                .join("posts")
                .join("note")
                .join("index.html"),
        )
        .unwrap();
        assert!(
            note_html.contains("Sectioned Post"),
            "sectioned post should appear in section page, html:\n{note_html}"
        );
        assert!(
            !note_html.contains("Orphan Post"),
            "orphan post should NOT appear in section page, html:\n{note_html}"
        );
    }

    #[test]
    fn build_skips_home_without_template() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates_except(&root.path().join("templates"), &["home.html"]);

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let home = root.path().join("public").join("index.html");
        assert!(
            !home.exists(),
            "should NOT generate home page without home.html template"
        );
    }

    #[test]
    fn build_home_pagination() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r"
                [params.home]
                paginate = 2
            "},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        for i in 1..=3 {
            write_page(
                root.path(),
                &format!("posts/note/post-{i}"),
                &format!(
                    indoc! {r#"
                        +++
                        title = "Post {i}"
                        date = "2026-01-0{i}T00:00:00Z"
                        +++
                        Body
                    "#},
                    i = i,
                ),
            );
        }

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        let page1 = output_dir.join("index.html");
        assert!(page1.exists(), "should generate home page 1");
        let html1 = fs::read_to_string(&page1).unwrap();
        assert!(
            html1.contains("Page 1 / 2"),
            "should show pagination, html:\n{html1}"
        );

        let page2 = output_dir.join("page").join("2").join("index.html");
        assert!(page2.exists(), "should generate home page 2");
    }

    #[test]
    fn build_standalone_excluded_from_home() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello Post"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );
        write_page(
            root.path(),
            "about-me",
            indoc! {r#"
                +++
                title = "About Me"
                +++
                Bio
            "#},
        );

        build(root.path(), None, false).unwrap();

        let html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
        assert!(
            html.contains("Hello Post"),
            "home page should list posts, html:\n{html}"
        );
        assert!(
            !html.contains("About Me"),
            "home page should NOT list standalone pages, html:\n{html}"
        );
    }

    // ── build: posts index ──

    #[test]
    fn build_generates_posts_index() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/post-a",
            indoc! {r#"
                +++
                title = "Post A"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );
        write_page(
            root.path(),
            "posts/essay/post-b",
            indoc! {r#"
                +++
                title = "Post B"
                date = "2026-01-02T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let posts_index = root.path().join("public").join("posts").join("index.html");
        assert!(posts_index.exists(), "should generate /posts/index.html");
        let html = fs::read_to_string(&posts_index).unwrap();
        assert!(
            html.contains("Post A") && html.contains("Post B"),
            "posts index should list all posts across sections, html:\n{html}"
        );
    }

    #[test]
    fn build_posts_index_uses_index_title() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        let posts_dir = root.path().join("content").join("posts");
        fs::create_dir_all(&posts_dir).unwrap();
        fs::write(
            posts_dir.join("_index.md"),
            indoc! {r#"
                +++
                title = "文章"
                +++
            "#},
        )
        .unwrap();

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let html = fs::read_to_string(root.path().join("public").join("posts").join("index.html"))
            .unwrap();
        assert!(
            html.contains("文章"),
            "should use _index.md title for posts index, html:\n{html}"
        );
    }

    #[test]
    fn build_posts_index_generated_even_when_empty() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "about-me",
            indoc! {r#"
                +++
                title = "About Me"
                +++
                Bio
            "#},
        );

        build(root.path(), None, false).unwrap();

        let posts_index = root.path().join("public").join("posts").join("index.html");
        assert!(
            posts_index.exists(),
            "should generate /posts/index.html even with no posts"
        );
    }

    #[test]
    fn build_posts_index_pagination() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r"
                [params.section]
                paginate = 2
            "},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        for i in 1..=3 {
            write_page(
                root.path(),
                &format!("posts/note/post-{i}"),
                &format!(
                    indoc! {r#"
                        +++
                        title = "Post {i}"
                        date = "2026-01-0{i}T00:00:00Z"
                        +++
                        Body
                    "#},
                    i = i,
                ),
            );
        }

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        let page1 = output_dir.join("posts").join("index.html");
        assert!(page1.exists(), "should generate /posts/ page 1");
        let html1 = fs::read_to_string(&page1).unwrap();
        assert!(
            html1.contains("Page 1 / 2"),
            "should show pagination on /posts/, html:\n{html1}"
        );

        let page2 = output_dir
            .join("posts")
            .join("page")
            .join("2")
            .join("index.html");
        assert!(page2.exists(), "should generate /posts/ page 2");
    }

    // ── build: section pages ──

    #[test]
    fn build_generates_section_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        for (section, name) in [("note", "post-a"), ("note", "post-b"), ("essay", "hello")] {
            write_page(
                root.path(),
                &format!("posts/{section}/{name}"),
                &format!(
                    indoc! {r#"
                        +++
                        title = "{name}"
                        date = "2026-01-01T00:00:00Z"
                        +++
                        Body
                    "#},
                    name = name,
                ),
            );
        }

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        let note_index = output_dir.join("posts").join("note").join("index.html");
        assert!(
            note_index.exists(),
            "should generate /posts/note/index.html"
        );
        let html = fs::read_to_string(&note_index).unwrap();
        assert!(
            html.contains("Note"),
            "should have section title, html:\n{html}"
        );
        assert!(
            html.contains("post-a") && html.contains("post-b"),
            "should list section posts, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="http://localhost:5456/posts/note/post-a/">post-a</a>"#),
            "section page should link to posts under /posts/, html:\n{html}"
        );

        let essay_index = output_dir.join("posts").join("essay").join("index.html");
        assert!(
            essay_index.exists(),
            "should generate /posts/essay/index.html"
        );
    }

    #[test]
    fn build_skips_archives_without_template() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates_except(&root.path().join("templates"), &["archive.html"]);

        write_page(
            root.path(),
            "posts/note/my-post",
            indoc! {r#"
                +++
                title = "My Post"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let section_index = root
            .path()
            .join("public")
            .join("posts")
            .join("note")
            .join("index.html");
        assert!(
            !section_index.exists(),
            "should NOT generate archive pages without archive.html template"
        );
    }

    #[test]
    fn build_section_uses_index_title() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        let section_dir = root.path().join("content").join("posts").join("note");
        fs::create_dir_all(&section_dir).unwrap();
        fs::write(
            section_dir.join("_index.md"),
            indoc! {r#"
                +++
                title = "笔记"
                +++
            "#},
        )
        .unwrap();

        write_page(
            root.path(),
            "posts/note/my-post",
            indoc! {r#"
                +++
                title = "My Post"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let html = fs::read_to_string(
            root.path()
                .join("public")
                .join("posts")
                .join("note")
                .join("index.html"),
        )
        .unwrap();
        assert!(
            html.contains("笔记"),
            "should use _index.md title, html:\n{html}"
        );
    }

    #[test]
    fn build_section_pagination() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r"
                [params.section]
                paginate = 2
            "},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        for i in 1..=3 {
            write_page(
                root.path(),
                &format!("posts/note/post-{i}"),
                &format!(
                    indoc! {r#"
                        +++
                        title = "Post {i}"
                        date = "2026-01-0{i}T00:00:00Z"
                        +++
                        Body
                    "#},
                    i = i,
                ),
            );
        }

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        let page1 = output_dir.join("posts").join("note").join("index.html");
        assert!(page1.exists(), "should generate section page 1");
        let html1 = fs::read_to_string(&page1).unwrap();
        assert!(
            html1.contains("Page 1 / 2"),
            "should show pagination, html:\n{html1}"
        );

        let page2 = output_dir
            .join("posts")
            .join("note")
            .join("page")
            .join("2")
            .join("index.html");
        assert!(page2.exists(), "should generate section page 2");
    }

    // ── build: sections index ──

    #[test]
    fn build_sections_index_generates_page() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/post-a",
            indoc! {r#"
                +++
                title = "Post A"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );
        write_page(
            root.path(),
            "posts/essay/post-b",
            indoc! {r#"
                +++
                title = "Post B"
                date = "2026-01-02T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let sections_index = root
            .path()
            .join("public")
            .join("sections")
            .join("index.html");
        assert!(
            sections_index.exists(),
            "should generate /sections/index.html"
        );
        let html = fs::read_to_string(&sections_index).unwrap();
        assert!(
            html.contains("Essay") && html.contains("Note"),
            "should list section names, html:\n{html}"
        );
        assert!(
            html.contains("Post A") && html.contains("Post B"),
            "should list section posts, html:\n{html}"
        );
    }

    #[test]
    fn build_sections_index_skipped_without_overview_template() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates_except(&root.path().join("templates"), &["overview.html"]);

        write_page(
            root.path(),
            "posts/note/post-a",
            indoc! {r#"
                +++
                title = "Post A"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let sections_index = root
            .path()
            .join("public")
            .join("sections")
            .join("index.html");
        assert!(
            !sections_index.exists(),
            "should NOT generate sections index without overview.html"
        );
    }

    // ── build: taxonomies ──

    #[test]
    fn build_generates_taxonomy_index_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                tags = ["rust", "web"]
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        let tags_index = output_dir.join("tags").join("index.html");
        assert!(tags_index.exists(), "should generate /tags/index.html");
        let html = fs::read_to_string(&tags_index).unwrap();
        assert!(
            html.contains("rust") && html.contains("web"),
            "tags index should list terms, html:\n{html}"
        );
    }

    #[test]
    fn build_generates_tag_archive_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        for (name, tag) in [("post-1", "rust"), ("post-2", "rust"), ("post-3", "web")] {
            write_page(
                root.path(),
                &format!("posts/{name}"),
                &format!(
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
            );
        }

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        let rust_page = output_dir.join("tags").join("rust").join("index.html");
        assert!(rust_page.exists(), "should generate /tags/rust/index.html");
        let html = fs::read_to_string(&rust_page).unwrap();
        assert!(
            html.contains("post-1") && html.contains("post-2"),
            "tag archive should list posts, html:\n{html}"
        );
        assert!(
            !html.contains("post-3"),
            "tag archive should not include unrelated posts, html:\n{html}"
        );
    }

    #[test]
    fn build_generates_paginated_tag_archive_pages() {
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

        for i in 1..=3 {
            write_page(
                root.path(),
                &format!("posts/post-{i}"),
                &format!(
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
            );
        }

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");

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
    fn build_no_tag_archive_pages_without_tags() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        let tags_index = output_dir.join("tags").join("index.html");
        assert!(
            tags_index.exists(),
            "should generate /tags/index.html even with no tags"
        );
    }

    #[test]
    fn build_tag_archive_correct_with_standalone_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "about-me",
            indoc! {r#"
                +++
                title = "About Me"
                +++
                Bio
            "#},
        );
        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello Post"
                tags = ["rust"]
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let term_page = root
            .path()
            .join("public")
            .join("tags")
            .join("rust")
            .join("index.html");
        assert!(term_page.exists(), "should generate /tags/rust/index.html");
        let html = fs::read_to_string(&term_page).unwrap();
        assert!(
            html.contains("Hello Post"),
            "tag archive should list the tagged post, html:\n{html}"
        );
        assert!(
            !html.contains("About Me"),
            "tag archive should NOT list standalone pages, html:\n{html}"
        );
    }

    // ── build: pagination config ──

    #[test]
    fn build_zero_paginate_falls_back_to_defaults() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r"
                [params]
                paginate = 0

                [params.home]
                paginate = 0

                [params.section]
                paginate = 0
            "},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello"
                tags = ["rust"]
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        assert!(
            output_dir.join("index.html").exists(),
            "should build home page"
        );
        assert!(
            output_dir.join("posts").join("index.html").exists(),
            "should build posts index"
        );
        assert!(
            output_dir
                .join("posts")
                .join("note")
                .join("index.html")
                .exists(),
            "should build section page"
        );
        assert!(
            output_dir
                .join("tags")
                .join("rust")
                .join("index.html")
                .exists(),
            "should build tag archive page"
        );
    }

    // ── build: RSS feeds ──

    #[test]
    fn build_generates_rss_feeds() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
                title = "Test Site"
            "#},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello"
                tags = ["rust"]
                date = "2026-01-15T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        let main_feed = fs::read_to_string(output_dir.join("index.xml")).unwrap();
        assert!(
            main_feed.contains("<title>Test Site</title>"),
            "main feed should have site title, xml:\n{main_feed}"
        );
        assert!(
            main_feed.contains("<title>Hello</title>"),
            "main feed should include post, xml:\n{main_feed}"
        );

        let posts_feed = fs::read_to_string(output_dir.join("posts").join("index.xml")).unwrap();
        assert!(
            posts_feed.contains("<title>Hello</title>"),
            "all-posts feed should include post, xml:\n{posts_feed}"
        );

        let section_feed =
            fs::read_to_string(output_dir.join("posts").join("note").join("index.xml")).unwrap();
        assert!(
            section_feed.contains("<title>Hello</title>"),
            "section feed should include section post, xml:\n{section_feed}"
        );

        let tag_feed =
            fs::read_to_string(output_dir.join("tags").join("rust").join("index.xml")).unwrap();
        assert!(
            tag_feed.contains("<title>Hello</title>"),
            "tag feed should include tagged post, xml:\n{tag_feed}"
        );
    }

    #[test]
    fn build_rss_feed_empty_site() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        let main_feed = fs::read_to_string(output_dir.join("index.xml")).unwrap();
        assert!(
            !main_feed.contains("<item>"),
            "empty site should have no items, xml:\n{main_feed}"
        );
        assert!(
            !main_feed.contains("<lastBuildDate>"),
            "empty site should have no lastBuildDate, xml:\n{main_feed}"
        );
    }

    // ── build: sitemap + robots.txt ──

    #[test]
    fn build_generates_sitemap_and_robots() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
            "#},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                date = "2026-01-15T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");

        let sitemap = fs::read_to_string(output_dir.join("sitemap.xml")).unwrap();
        assert!(
            sitemap.contains("<loc>https://example.com/</loc>"),
            "sitemap should contain home URL, xml:\n{sitemap}"
        );
        assert!(
            sitemap.contains("<loc>https://example.com/posts/hello/</loc>"),
            "sitemap should contain post URL, xml:\n{sitemap}"
        );
        assert!(
            sitemap.contains("<lastmod>"),
            "sitemap should have lastmod for dated page, xml:\n{sitemap}"
        );

        let robots = fs::read_to_string(output_dir.join("robots.txt")).unwrap();
        assert!(
            robots.contains("Sitemap: https://example.com/sitemap.xml"),
            "robots.txt should reference sitemap, txt:\n{robots}"
        );
    }

    // ── build: 404 page ──

    #[test]
    fn build_generates_404_page() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        let html = fs::read_to_string(output_dir.join("404.html")).unwrap();
        assert!(
            html.contains("404 Not Found"),
            "should contain error message, html:\n{html}"
        );
    }

    #[test]
    fn build_skips_404_without_template() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();

        let templates = root.path().join("templates");
        copy_templates(&templates);
        fs::remove_file(templates.join("404.html")).unwrap();

        build(root.path(), None, false).unwrap();

        let output_dir = root.path().join("public");
        assert!(
            !output_dir.join("404.html").exists(),
            "should not generate 404.html without template"
        );
    }

    // ── build: errors ──

    fn setup_site_with_page(root: &Path) {
        fs::write(root.join("config.toml"), "").unwrap();
        copy_templates(&root.join("templates"));
        write_page(
            root,
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );
    }

    #[test]
    fn build_invalid_config_returns_error() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "{{invalid toml").unwrap();

        let err = format!("{:#}", build(root.path(), None, false).unwrap_err());
        assert!(
            err.contains("failed to load config"),
            "should report config failure, got: {err}"
        );
    }

    #[test]
    fn build_invalid_timezone_returns_error() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), r#"timezone = "Mars/Base""#).unwrap();

        let err = build(root.path(), None, false).unwrap_err();
        let chain: Vec<String> = err.chain().map(ToString::to_string).collect();
        assert!(
            chain
                .iter()
                .any(|message| message.contains("invalid timezone `Mars/Base` in config.toml")),
            "should report invalid timezone, got: {chain:?}"
        );
    }

    #[test]
    fn build_missing_templates_returns_error() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();

        let err = build(root.path(), None, false).unwrap_err().to_string();
        assert!(
            err.contains("failed to initialize template engine"),
            "should report template engine failure, got: {err}"
        );
    }

    #[test]
    fn build_broken_post_template_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        fs::write(
            root.path().join("templates").join("post.html"),
            "{% invalid %}",
        )
        .unwrap();

        let err = build(root.path(), None, false).unwrap_err().to_string();
        assert!(
            err.contains("failed to render"),
            "should report render failure, got: {err}"
        );
    }

    #[test]
    fn build_write_permission_denied_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        build(root.path(), None, false).unwrap();
        let output_dir = root.path().join("public");
        let _guard = PermissionGuard::restrict(&output_dir, 0o555);

        let err = build(root.path(), None, false).unwrap_err().to_string();
        assert!(
            err.contains("failed to write") || err.contains("failed to clean"),
            "should report write or clean failure, got: {err}"
        );
    }

    #[test]
    fn build_asset_copy_permission_denied_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        let page_dir = root.path().join("content").join("posts").join("hello");
        fs::write(page_dir.join("image.png"), "img-data").unwrap();

        build(root.path(), None, false).unwrap();

        let page_output = root.path().join("public").join("posts").join("hello");
        let _guard = PermissionGuard::restrict(&page_output, 0o555);

        let err = build(root.path(), None, false).unwrap_err().to_string();
        assert!(
            err.contains("failed to copy asset") || err.contains("failed to clean"),
            "should report asset copy or clean failure, got: {err}"
        );
    }

    #[test]
    fn build_broken_directive_template_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        let directives = root.path().join("templates").join("directives");
        fs::create_dir_all(&directives).unwrap();
        fs::write(
            directives.join("broken.html"),
            "{% for k, v in name | items %}{{ k }}{% endfor %}",
        )
        .unwrap();

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                ::: broken
                Body
                :::
            "#},
        );

        let err = build(root.path(), None, false).unwrap_err().to_string();
        assert!(
            err.contains("failed to render"),
            "should report render failure, got: {err}"
        );
    }

    // ── find_page_css ──

    #[test]
    fn find_page_css_detects_root_style() {
        let bundle = Path::new("content/posts/my-post");
        let assets = vec![bundle.join("cover.webp"), bundle.join("style.css")];
        let result = find_page_css(&assets, Some(bundle), "https://example.com/posts/my-post/");
        assert_eq!(result.as_deref(), Some("/posts/my-post/style.css"));
    }

    #[test]
    fn find_page_css_detects_nested_style() {
        let bundle = Path::new("content/posts/my-post");
        let assets = vec![
            bundle.join("assets/cover.webp"),
            bundle.join("assets/style.css"),
        ];
        let result = find_page_css(&assets, Some(bundle), "https://example.com/posts/my-post/");
        assert_eq!(result.as_deref(), Some("/posts/my-post/assets/style.css"));
    }

    #[test]
    fn find_page_css_returns_none_without_style() {
        let bundle = Path::new("content/posts/my-post");
        let assets = vec![bundle.join("cover.webp")];
        assert!(
            find_page_css(&assets, Some(bundle), "https://example.com/posts/my-post/").is_none()
        );
    }

    #[test]
    fn find_page_css_returns_none_for_non_bundle() {
        assert!(find_page_css(&[], None, "https://example.com/posts/my-post/").is_none());
    }
}
