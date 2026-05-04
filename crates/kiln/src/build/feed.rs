use std::path::Path;

use anyhow::{Context, Result};

use crate::feed::{self, Channel, DEFAULT_FEED_LIMIT};
use crate::output::write_output;

use super::BuildContext;
use super::listing::{ListedPage, ListingBucket};

/// Generates the site-wide RSS feed at `index.xml` plus one per listing bucket
/// (all-posts, per-section, per-tag).
pub(crate) fn build_feeds(
    ctx: &BuildContext,
    listed_posts: &[ListedPage],
    buckets: &[ListingBucket],
    output_dir: &Path,
) -> Result<()> {
    let base = ctx.config.base_url.trim_end_matches('/');

    let main_channel = Channel {
        title: ctx.config.title.clone(),
        link: format!("{base}/"),
        feed_url: format!("{base}/index.xml"),
        description: ctx.config.description.clone(),
        language: ctx.config.language.clone(),
        last_build_date: newest_date(listed_posts),
    };
    let items: Vec<_> = listed_posts.iter().map(|lp| lp.summary.clone()).collect();
    let xml = feed::generate_rss(&main_channel, &items, DEFAULT_FEED_LIMIT);
    write_output(&output_dir.join("index.xml"), &xml).context("failed to write main RSS feed")?;

    for bucket in buckets {
        write_bucket_feed(ctx, base, bucket, output_dir)?;
    }

    Ok(())
}

// ── Helpers ──

fn write_bucket_feed(
    ctx: &BuildContext,
    base: &str,
    bucket: &ListingBucket,
    output_dir: &Path,
) -> Result<()> {
    let dir_slug = bucket.base_path().trim_start_matches('/').to_owned();
    let channel = Channel {
        title: format!("{} - {}", bucket.name, ctx.config.title),
        link: format!("{base}/{dir_slug}/"),
        feed_url: format!("{base}/{dir_slug}/index.xml"),
        description: ctx.config.description.clone(),
        language: ctx.config.language.clone(),
        last_build_date: newest_date(&bucket.pages),
    };
    let items: Vec<_> = bucket.pages.iter().map(|lp| lp.summary.clone()).collect();
    let xml = feed::generate_rss(&channel, &items, DEFAULT_FEED_LIMIT);
    let dest = output_dir.join(&dir_slug).join("index.xml");
    write_output(&dest, &xml).with_context(|| format!("failed to write RSS feed for {dir_slug}"))
}

/// Returns the RFC 2822 date of the newest page, for `lastBuildDate`.
fn newest_date(pages: &[ListedPage]) -> Option<String> {
    pages
        .iter()
        .filter_map(|lp| lp.timestamp)
        .max()
        .map(feed::format_rfc2822)
}
