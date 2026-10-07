mod archive;
mod error;
mod feed;
mod git;
mod home;
mod listing;
mod overview;
mod paginate;
mod routes;
mod sitemap;

use std::fmt::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use jiff::tz::TimeZone;
use syntect::parsing::SyntaxSet;

use self::git::GitInfo;
use self::listing::{
    PreparedPage, build_listing_artifacts, build_listing_buckets, format_page_date,
};
use self::routes::RoutePlan;
use crate::config::Config;
use crate::content::discovery::{ContentSet, discover_content};
use crate::content::index::load_index_title;
use crate::content::page::{Page, PageKind};
use crate::css::Stylesheets;
use crate::i18n::I18n;
use crate::minify::{self, MinifyStats};
use crate::output::{OutputTransaction, write_output};
use crate::render::RenderOptions;
use crate::render::lqip::ImageResolver;
use crate::render::pipeline::render_page;
use crate::search;
use crate::section::collect_sections;
use crate::static_assets::StaticAssetManifest;
use crate::static_assets::publication::PublishedAssets;
use crate::taxonomy::build_taxonomies;
use crate::template::TemplateEngine;
use crate::template::vars::{PageMetadata, PostTemplateVars};

/// Shared build state, created once per build invocation.
struct BuildContext {
    config: Config,
    deployment_prefix: String,
    i18n: I18n,
    time_zone: Option<TimeZone>,
    syntax_set: SyntaxSet,
    stylesheets: Stylesheets,
    static_assets: StaticAssetManifest,
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
    /// Publishes into this directory instead of `root/<config.output_dir>`.
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

    let (config, deployment_prefix) = load_build_config(root, base_url_override)?;
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
        Some(path) => config.validate_output_dir(root, path)?,
        None => config.resolved_output_dir(root)?,
    };

    let transaction = OutputTransaction::new(output_dir)?;
    let output_dir = transaction.path().to_owned();

    let assets = PublishedAssets::publish(
        root,
        theme_dir.as_deref(),
        &content.content_dir,
        &content.pages,
        &output_dir,
    )?;
    let stylesheets = Stylesheets::discover(root, &config, &content.content_dir, &content.pages)?;
    stylesheets.compile(root, &config, &assets, &output_dir)?;

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
        &deployment_prefix,
        &static_assets,
    )
    .context("failed to initialize template engine")?;
    let image_resolver = ImageResolver::new(&output_dir, config.image.clone());
    let git_info = GitInfo::new(root, config.enable_git_info);
    let ctx = BuildContext {
        config,
        deployment_prefix,
        i18n,
        time_zone,
        syntax_set,
        stylesheets,
        static_assets,
        template_engine,
        image_resolver,
        git_info,
    };

    let sections = collect_sections(&content.pages, &content.content_dir)?;
    let taxonomy_set = build_taxonomies(&content.pages, Some(&content.content_dir))?;

    let artifacts = build_listing_artifacts(
        &ctx,
        &content.pages,
        &content.content_dir,
        &sections,
        &taxonomy_set,
    )?;

    let posts_title = load_index_title(&content.content_dir.join("posts"))?
        .unwrap_or_else(|| ctx.i18n.t("all_posts").into_owned());
    let buckets = build_listing_buckets(&artifacts, &sections, &taxonomy_set, posts_title);

    let plan = RoutePlan::new(&ctx, &content.pages, &artifacts, &buckets, &output_dir)?;
    build_content_pages(&ctx, &content, &output_dir, &artifacts.pages)?;
    let posts = artifacts.posts();
    let mut page_count = content.pages.len();
    page_count += home::build_home_pages(&ctx, &posts, &output_dir)?;
    page_count += archive::build_archive_pages(&ctx, &buckets, &output_dir)?;
    page_count += overview::build_overview_pages(&ctx, &buckets, &output_dir)?;

    feed::build_feeds(&ctx, &posts, &buckets, &output_dir)?;
    sitemap::build_sitemap_and_robots(&ctx, &plan, &output_dir)?;
    page_count += error::build_404(&ctx, &output_dir)?;

    finish_build(
        &ctx,
        transaction,
        minify_stats,
        page_count,
        content.pages.len(),
        started,
    )
}

fn load_build_config(root: &Path, base_url_override: Option<&str>) -> Result<(Config, String)> {
    let mut config = Config::load(root).context("failed to load config")?;
    if let Some(base_url) = base_url_override {
        base_url.clone_into(&mut config.base_url);
    }
    let base_url = url::Url::parse(&config.base_url).context("invalid base_url")?;
    ensure!(
        !base_url.cannot_be_a_base() && base_url.has_host(),
        "base_url must be an absolute URL with a host"
    );
    ensure!(
        base_url.query().is_none() && base_url.fragment().is_none(),
        "base_url must not contain a query or fragment"
    );
    let deployment_prefix = base_url.path().to_owned();
    base_url
        .as_str()
        .trim_end_matches('/')
        .clone_into(&mut config.base_url);

    Ok((config, deployment_prefix))
}

fn finish_build(
    ctx: &BuildContext,
    transaction: OutputTransaction,
    mut minify_stats: Option<MinifyStats>,
    page_count: usize,
    content_count: usize,
    started: Instant,
) -> Result<()> {
    let output_dir = transaction.path();
    if let Some(stats) = &mut minify_stats {
        *stats += minify::minify_output_dir_excluding(
            output_dir,
            ctx.static_assets.fingerprinted_paths(),
        )
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

    transaction.commit()?;

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

// ── Content rendering ──

fn build_content_pages(
    ctx: &BuildContext,
    content: &ContentSet,
    output_dir: &Path,
    prepared: &[PreparedPage],
) -> Result<()> {
    if content.pages.is_empty() {
        return Ok(());
    }

    let options = RenderOptions::from_params(&ctx.config.params)?;

    for (page, prepared) in content.pages.iter().zip(prepared) {
        build_page(ctx, &options, page, output_dir, prepared)?;
    }

    Ok(())
}

fn build_page(
    ctx: &BuildContext,
    options: &RenderOptions,
    page: &Page,
    output_dir: &Path,
    prepared: &PreparedPage,
) -> Result<()> {
    let mut options = options.clone();
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

    let page_css = ctx
        .stylesheets
        .page_url(&ctx.deployment_prefix, &ctx.static_assets, page)?;
    let vars = PostTemplateVars {
        metadata: PageMetadata {
            title: &prepared.summary.title,
            description: &prepared.summary.description,
            url: prepared.summary.url.as_str().into(),
        },
        featured_image: prepared.summary.featured_image.as_ref(),
        license: page.frontmatter.license.as_deref(),
        page_css,
        date: prepared.summary.date.as_deref(),
        updated: prepared
            .updated
            .map(|date| format_page_date(date, ctx.time_zone.as_ref())),
        tags: &prepared.summary.tags,
        section: prepared.summary.section.as_ref(),
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

    let dest = output_dir.join(&prepared.output_path);
    write_output(&dest, &html).with_context(|| format!("failed to write {}", dest.display()))?;

    Ok(())
}
