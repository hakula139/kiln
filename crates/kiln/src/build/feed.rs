use std::path::Path;

use anyhow::{Context, Result};

use crate::feed::{self, Channel, DEFAULT_FEED_LIMIT};
use crate::output::write_output;
use crate::section::{self, Section};
use crate::taxonomy::{TaxonomyKind, TaxonomySet, Term};

use super::BuildContext;
use super::listing::{ListedPage, ListingArtifacts, resolve_term_pages};

/// Generates RSS feeds: main site feed, per-section feeds, and per-term feeds.
pub(crate) fn build_feeds(
    ctx: &BuildContext,
    artifacts: &ListingArtifacts,
    sections: &[Section],
    taxonomy_set: &TaxonomySet,
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    let base = ctx.config.base_url.trim_end_matches('/');
    let last_build_date = newest_date(&artifacts.listed_posts);

    let main_channel = Channel {
        title: ctx.config.title.clone(),
        link: format!("{base}/"),
        feed_url: format!("{base}/index.xml"),
        description: ctx.config.description.clone(),
        language: ctx.config.language.clone(),
        last_build_date,
    };
    let items: Vec<_> = artifacts
        .listed_posts
        .iter()
        .map(|lp| lp.summary.clone())
        .collect();
    let xml = feed::generate_rss(&main_channel, &items, DEFAULT_FEED_LIMIT);
    write_output(&output_dir.join("index.xml"), &xml).context("failed to write main RSS feed")?;

    let posts_title = section::load_index_title(&content_dir.join("posts"))
        .unwrap_or_else(|| "All Posts".to_owned());
    write_section_feed(
        ctx,
        base,
        &posts_title,
        "posts",
        &artifacts.listed_posts,
        output_dir,
    )?;

    for section in sections {
        let posts = artifacts
            .section_posts
            .get(section.slug.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        let dir_slug = format!("posts/{}", section.slug);
        write_section_feed(ctx, base, &section.title, &dir_slug, posts, output_dir)?;
    }

    for taxonomy in &taxonomy_set.taxonomies {
        let kind = taxonomy.kind;
        for term in &taxonomy.terms {
            write_term_feed(
                ctx,
                base,
                kind,
                term,
                &artifacts.listed_pages,
                taxonomy_set,
                output_dir,
            )?;
        }
    }

    Ok(())
}

// ── Helpers ──

fn write_section_feed(
    ctx: &BuildContext,
    base: &str,
    title: &str,
    dir_slug: &str,
    listed_posts: &[ListedPage],
    output_dir: &Path,
) -> Result<()> {
    let channel = Channel {
        title: format!("{title} - {}", ctx.config.title),
        link: format!("{base}/{dir_slug}/"),
        feed_url: format!("{base}/{dir_slug}/index.xml"),
        description: ctx.config.description.clone(),
        language: ctx.config.language.clone(),
        last_build_date: newest_date(listed_posts),
    };
    let items: Vec<_> = listed_posts.iter().map(|lp| lp.summary.clone()).collect();
    let xml = feed::generate_rss(&channel, &items, DEFAULT_FEED_LIMIT);
    let dest = output_dir.join(dir_slug).join("index.xml");
    write_output(&dest, &xml).with_context(|| format!("failed to write RSS feed for {dir_slug}"))
}

fn write_term_feed(
    ctx: &BuildContext,
    base: &str,
    kind: TaxonomyKind,
    term: &Term,
    listed_pages: &[ListedPage],
    taxonomy_set: &TaxonomySet,
    output_dir: &Path,
) -> Result<()> {
    let pages = resolve_term_pages(taxonomy_set, kind, &term.slug, listed_pages);
    let dir_slug = format!("{}/{}", kind.plural(), term.slug);
    let channel = Channel {
        title: format!("{} - {}", term.name, ctx.config.title),
        link: format!("{base}/{dir_slug}/"),
        feed_url: format!("{base}/{dir_slug}/index.xml"),
        description: ctx.config.description.clone(),
        language: ctx.config.language.clone(),
        last_build_date: newest_date(&pages),
    };
    let items: Vec<_> = pages.iter().map(|lp| lp.summary.clone()).collect();
    let xml = feed::generate_rss(&channel, &items, DEFAULT_FEED_LIMIT);
    let dest = output_dir
        .join(kind.plural())
        .join(&term.slug)
        .join("index.xml");
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
