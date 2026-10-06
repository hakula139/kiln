mod archive;
mod error;
mod feed;
mod git;
mod home;
mod listing;
mod overview;
mod paginate;
mod sitemap;
mod url;

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use jiff::tz::TimeZone;
use syntect::parsing::SyntaxSet;

use crate::config::Config;
use crate::content::discovery::discover_content;
use crate::content::page::{Page, PageKind};
use crate::i18n::I18n;
use crate::minify::{self, MinifyStats};
use crate::output::{clean_output_dir, copy_file, copy_static, write_output};
use crate::render::RenderOptions;
use crate::render::lqip::ImageResolver;
use crate::render::pipeline::render_page;
use crate::search;
use crate::section::{self, Section, collect_sections};
use crate::static_assets::StaticAssetManifest;
use crate::taxonomy::build_taxonomies;
use crate::template::TemplateEngine;
use crate::template::vars::PostTemplateVars;

use self::git::{GitInfo, updated_timestamp};
use self::listing::{
    build_listing_artifacts, build_listing_buckets, format_page_date, linked_tags, page_section,
    resolve_featured_image,
};
use self::url::{page_url, resolve_relative_url};

/// Shared build state, created once per build invocation.
struct BuildContext {
    config: Config,
    i18n: I18n,
    time_zone: Option<TimeZone>,
    syntax_set: SyntaxSet,
    template_engine: TemplateEngine,
    image_resolver: ImageResolver,
    git_info: Option<GitInfo>,
}

/// Options controlling a single `build()` invocation.
#[derive(Default)]
pub struct BuildOptions<'a> {
    /// Replaces `base_url` from config when set. Used by `kiln serve` so rendered URLs match the
    /// actual server port.
    pub base_url_override: Option<&'a str>,
    /// Writes into this directory instead of `root/<config.output_dir>`. Used by the dev server to
    /// stage a fresh build before swapping it in.
    pub output_dir_override: Option<&'a Path>,
    /// Runs HTML / CSS / JS minification over the output directory before Pagefind indexing.
    pub minify: bool,
}

/// Builds the site from the given project root directory.
///
/// # Errors
///
/// Returns an error if any build stage fails.
#[expect(
    clippy::needless_pass_by_value,
    reason = "callers construct BuildOptions inline with `..Default::default()`, so taking it by value keeps call sites concise"
)]
pub fn build(root: &Path, options: BuildOptions<'_>) -> Result<()> {
    let started = Instant::now();
    let BuildOptions {
        base_url_override,
        output_dir_override,
        minify,
    } = options;

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

    let i18n = I18n::load(root, theme_dir.as_deref(), &config.language)
        .context("failed to load i18n strings")?;

    let content = discover_content(root)?;
    let output_dir = match output_dir_override {
        Some(path) => path.to_owned(),
        None => config.resolved_output_dir(root)?,
    };

    clean_output_dir(&output_dir)?;

    if let Some(ref td) = theme_dir {
        copy_static(&td.join("static"), &output_dir)?;
    }
    copy_static(&root.join("static"), &output_dir)?;

    let minify_stats = if minify {
        eprintln!("Minifying...");
        Some(minify::minify_static_assets(&output_dir).context("minification failed")?)
    } else {
        None
    };

    let static_assets =
        StaticAssetManifest::build(&output_dir).context("failed to fingerprint static assets")?;
    let template_engine = TemplateEngine::new_with_assets(
        Some(&site_templates),
        theme_templates.as_deref(),
        &i18n,
        &static_assets,
    )
    .context("failed to initialize template engine")?;
    let image_resolver = ImageResolver::new(&root.join("static"), config.image.clone());
    let git_info = GitInfo::new(root, config.enable_git_info);
    let ctx = BuildContext {
        config,
        i18n,
        time_zone,
        syntax_set,
        template_engine,
        image_resolver,
        git_info,
    };

    let sections = collect_sections(&content.pages, &content.content_dir);
    let taxonomy_set = build_taxonomies(&content.pages, Some(&content.content_dir))?;

    let artifacts = build_listing_artifacts(
        &content.pages,
        &content.content_dir,
        &ctx.config.base_url,
        ctx.time_zone.as_ref(),
        &sections,
        &ctx.image_resolver,
        &taxonomy_set,
    )?;

    for page in &content.pages {
        build_page(&ctx, page, &content.content_dir, &output_dir, &sections)?;
    }

    let posts_title = section::load_index_title(&content.content_dir.join("posts"))
        .unwrap_or_else(|| ctx.i18n.t("all_posts").into_owned());
    let buckets = build_listing_buckets(&artifacts, &sections, &taxonomy_set, posts_title);

    let mut page_count = content.pages.len();
    page_count += home::build_home_pages(&ctx, &artifacts.listed_posts, &output_dir)?;
    page_count += archive::build_archive_pages(&ctx, &buckets, &output_dir)?;
    page_count += overview::build_overview_pages(&ctx, &buckets, &output_dir)?;

    feed::build_feeds(&ctx, &artifacts.listed_posts, &buckets, &output_dir)?;
    sitemap::build_sitemap_and_robots(&ctx, &artifacts.listed_pages, &output_dir)?;
    page_count += error::build_404(&ctx, &output_dir)?;

    finish_build(
        &ctx,
        &output_dir,
        &static_assets,
        minify_stats,
        page_count,
        content.pages.len(),
        started,
    )
}

fn finish_build(
    ctx: &BuildContext,
    output_dir: &Path,
    static_assets: &StaticAssetManifest,
    mut minify_stats: Option<MinifyStats>,
    page_count: usize,
    content_count: usize,
    started: Instant,
) -> Result<()> {
    if let Some(stats) = &mut minify_stats {
        *stats +=
            minify::minify_output_dir_excluding(output_dir, static_assets.fingerprinted_paths())
                .context("minification failed")?;
    }

    let search_duration = if ctx.config.search.enabled {
        eprintln!("Indexing search...");
        let search_started = Instant::now();
        search::run_pagefind(output_dir, ctx.config.search.binary.as_deref())
            .context("search indexing failed")?;
        Some(search_started.elapsed())
    } else {
        None
    };

    eprintln!(
        "{}",
        format_build_summary(
            page_count,
            content_count,
            started.elapsed(),
            search_duration
        )
    );
    if let Some(stats) = minify_stats {
        eprintln!("{stats}");
    }
    Ok(())
}

fn format_build_summary(
    page_count: usize,
    content_count: usize,
    elapsed: Duration,
    search_duration: Option<Duration>,
) -> String {
    let mut summary = format!(
        "Built {page_count} pages ({content_count} content) in {:.3}s.",
        elapsed.as_secs_f64()
    );
    if let Some(duration) = search_duration {
        _ = write!(
            summary,
            "\nSearch indexing: {:.3}s.",
            duration.as_secs_f64()
        );
    }
    summary
}

// ── Single-page rendering ──

fn build_page(
    ctx: &BuildContext,
    page: &Page,
    content_dir: &Path,
    output_dir: &Path,
    sections: &[Section],
) -> Result<()> {
    let mut options = RenderOptions::from_params(&ctx.config.params)?;
    options.heading_numbering = page.frontmatter.heading_numbering;

    let rendered = render_page(
        &page.raw_content,
        &ctx.syntax_set,
        &ctx.template_engine,
        &ctx.config,
        &options,
        page.source_path.parent(),
        &ctx.image_resolver,
    )
    .with_context(|| format!("failed to render {}", page.source_path.display()))?;

    let output_path = page.output_path(content_dir)?;
    let url = page_url(&ctx.config.base_url, &output_path);

    let featured_image = resolve_featured_image(
        page.frontmatter.featured_image.as_ref(),
        &url,
        &ctx.image_resolver,
        page.source_path.parent(),
    );
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
        updated: updated_timestamp(
            page.frontmatter.updated,
            &page.source_path,
            ctx.git_info.as_ref(),
        )
        .map(|date| format_page_date(date, ctx.time_zone.as_ref())),
        tags: linked_tags(&page.frontmatter.tags, &ctx.config.base_url),
        section: page_section(page, &ctx.config.base_url, sections),
        assets: rendered.assets,
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

/// Returns the URL path of the page bundle's `style.css` asset (e.g., `/posts/my-post/style.css`).
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
    use super::*;

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
    fn find_page_css_without_style_returns_none() {
        let bundle = Path::new("content/posts/my-post");
        let assets = vec![bundle.join("cover.webp")];
        assert!(
            find_page_css(&assets, Some(bundle), "https://example.com/posts/my-post/").is_none()
        );
    }

    #[test]
    fn find_page_css_non_bundle_returns_none() {
        assert!(find_page_css(&[], None, "https://example.com/posts/my-post/").is_none());
    }
}
