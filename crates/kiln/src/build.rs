mod archive;
mod bucket;
mod error;
mod feed;
mod git;
mod home;
mod listing;
mod overview;
mod page;
mod routes;
mod sitemap;

use std::fmt::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use self::bucket::Taxonomy;
use self::git::GitInfo;
use self::listing::{prepare_pages, sort_pinned_first};
use self::routes::RoutePlan;
use crate::config::Config;
use crate::content::discovery::discover_content;
use crate::css::Stylesheets;
use crate::i18n::I18n;
use crate::minify::{self, MinifyStats};
use crate::output::OutputTransaction;
use crate::output::guard::resolve_output_dir;
use crate::render::lqip::ImageResolver;
use crate::render::{CopyButton, RenderOptions};
use crate::search;
use crate::static_assets::StaticAssetManifest;
use crate::static_assets::publication::PublishedAssets;
use crate::template::TemplateEngine;

/// Shared build state, created once per build invocation.
struct BuildContext {
    config: Config,
    i18n: I18n,
    render_options: RenderOptions,
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
    /// Publishes into this directory instead of `config.output_dir`. Relative paths resolve
    /// against the project root.
    pub output_dir_override: Option<&'a Path>,
    /// Runs HTML / CSS / JS minification over the output directory before Pagefind indexing.
    pub minify: bool,
}

/// Builds the site from the given project root directory.
///
/// # Errors
///
/// Returns an error if any build stage fails.
pub fn build(root: &Path, options: &BuildOptions<'_>) -> Result<()> {
    let started = Instant::now();
    let &BuildOptions {
        base_url_override,
        output_dir_override,
        minify,
    } = options;

    let mut config = Config::load(root).context("failed to load config")?;
    if let Some(base_url) = base_url_override {
        config.base_url = base_url.parse()?;
    }

    let site_templates = root.join("templates");
    let theme_dir = config.theme_dir(root);
    let theme_templates = theme_dir.as_ref().map(|d| d.join("templates"));

    let i18n = I18n::load(root, theme_dir.as_deref(), &config.language)
        .context("failed to load i18n strings")?;
    let render_options = RenderOptions {
        copy_button: CopyButton::new(&i18n),
        ..RenderOptions::from_params(&config.params)?
    };

    let content = discover_content(root)?;
    let output_dir = resolve_output_dir(root, &config, output_dir_override)?;

    let transaction = OutputTransaction::new(output_dir)?;
    let output_dir = transaction.path().to_owned();

    let assets = PublishedAssets::publish(root, theme_dir.as_deref(), &content.pages, &output_dir)?;
    let stylesheets = Stylesheets::discover(root, &config, &content.pages);
    let mut static_assets = StaticAssetManifest::build_media(&output_dir)?;
    stylesheets.compile(root, &config, &assets, &static_assets, &output_dir)?;

    let minify_stats = if minify {
        eprintln!("Minifying...");
        Some(minify::minify_static_assets(&output_dir).context("minification failed")?)
    } else {
        None
    };

    static_assets
        .fingerprint_code(&output_dir)
        .context("failed to fingerprint static assets")?;
    let template_engine = TemplateEngine::new(
        Some(&site_templates),
        theme_templates.as_deref(),
        &config,
        &i18n,
        &static_assets,
    )
    .context("failed to initialize template engine")?;
    let image_resolver = ImageResolver::new(&output_dir, config.image.clone());
    let git_info = config
        .enable_git_info
        .then(|| GitInfo::new(root, &content.content_dir))
        .flatten();
    let ctx = BuildContext {
        config,
        i18n,
        render_options,
        stylesheets,
        static_assets,
        template_engine,
        image_resolver,
        git_info,
    };

    let taxonomy = Taxonomy::collect(&content.pages, &content.content_dir, || {
        ctx.i18n.t("all_posts").into_owned()
    })?;
    let prepared = prepare_pages(&ctx, &content.pages, &taxonomy)?;
    let listings = taxonomy.resolve(&prepared);
    let mut home = listings.posts.pages.clone();
    sort_pinned_first(&mut home);

    let plan = RoutePlan::new(&ctx, &prepared, &home, &listings, &output_dir)?;
    let page_count = plan.render(&ctx, &output_dir)?;

    finish_build(
        &ctx,
        transaction,
        minify_stats,
        page_count,
        content.pages.len(),
        started,
    )
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
