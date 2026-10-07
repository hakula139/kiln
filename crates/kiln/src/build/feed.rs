use std::path::Path;

use anyhow::{Context, Result};

use super::BuildContext;
use super::listing::{ListingBucket, PreparedPage};
use crate::feed::{self, Channel, DEFAULT_FEED_LIMIT, FeedItem};
use crate::output::write_output;
use crate::url::{join_site_url, path_url};

/// Generates the site-wide RSS feed at `index.xml` plus one per listing bucket
/// (all-posts, per-section, per-tag).
pub(super) fn build_feeds(
    ctx: &BuildContext,
    listed_posts: &[&PreparedPage],
    buckets: &[ListingBucket<'_>],
    output_dir: &Path,
) -> Result<()> {
    let base_url = &ctx.config.base_url;

    let main_channel = Channel {
        title: ctx.config.title.clone(),
        link: join_site_url(base_url, ""),
        feed_url: join_site_url(base_url, "index.xml"),
        description: ctx.config.description.clone(),
        language: ctx.config.language.clone(),
        last_build_date: newest_date(listed_posts),
    };
    let xml = feed::generate_rss(&main_channel, feed_items(listed_posts), DEFAULT_FEED_LIMIT);
    write_output(&output_dir.join("index.xml"), &xml).context("failed to write main RSS feed")?;

    for bucket in buckets {
        write_bucket_feed(ctx, bucket, output_dir)?;
    }

    Ok(())
}

// ── Helpers ──

fn write_bucket_feed(
    ctx: &BuildContext,
    bucket: &ListingBucket<'_>,
    output_dir: &Path,
) -> Result<()> {
    let base_url = &ctx.config.base_url;
    let directory = bucket.base_path();
    let dir_slug = path_url(&directory);
    let channel = Channel {
        title: format!("{} - {}", bucket.name, ctx.config.title),
        link: join_site_url(base_url, &format!("{dir_slug}/")),
        feed_url: join_site_url(base_url, &format!("{dir_slug}/index.xml")),
        description: ctx.config.description.clone(),
        language: ctx.config.language.clone(),
        last_build_date: newest_date(&bucket.pages),
    };
    let xml = feed::generate_rss(&channel, feed_items(&bucket.pages), DEFAULT_FEED_LIMIT);
    let dest = output_dir.join(directory).join("index.xml");
    write_output(&dest, &xml).with_context(|| format!("failed to write RSS feed for {dir_slug}"))
}

/// Returns the RFC 2822 date of the newest page, for `lastBuildDate`.
fn newest_date(pages: &[&PreparedPage]) -> Option<String> {
    pages
        .iter()
        .filter_map(|page| page.updated.or(page.published))
        .max()
        .map(feed::format_rfc2822)
}

fn feed_items<'a>(pages: &'a [&PreparedPage]) -> impl Iterator<Item = FeedItem<'a>> {
    pages.iter().map(|page| FeedItem {
        title: &page.summary.title,
        url: &page.summary.url,
        description: &page.summary.description,
        published: page.published,
    })
}
