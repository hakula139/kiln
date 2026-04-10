use std::path::Path;

use anyhow::{Context, Result};
use jiff::Timestamp;

use crate::output::write_output;
use crate::sitemap::{self, SitemapEntry};

use super::BuildContext;
use super::listing::ListedPage;

/// Generates `sitemap.xml` and `robots.txt` in the output directory.
pub(crate) fn build_sitemap_and_robots(
    ctx: &BuildContext,
    listed_pages: &[ListedPage],
    output_dir: &Path,
) -> Result<()> {
    build_sitemap(ctx, listed_pages, output_dir)?;
    build_robots_txt(ctx, output_dir)
}

// ── Sitemap ──

fn build_sitemap(ctx: &BuildContext, listed_pages: &[ListedPage], output_dir: &Path) -> Result<()> {
    let base = ctx.config.base_url.trim_end_matches('/');
    let mut entries = Vec::with_capacity(listed_pages.len() + 1);

    entries.push(SitemapEntry {
        loc: format!("{base}/"),
        lastmod: None,
    });

    for lp in listed_pages {
        entries.push(SitemapEntry {
            loc: lp.summary.url.clone(),
            lastmod: lp.timestamp.map(format_iso_date),
        });
    }

    let xml = sitemap::generate_sitemap(&entries);
    write_output(&output_dir.join("sitemap.xml"), &xml).context("failed to write sitemap.xml")
}

fn format_iso_date(ts: Timestamp) -> String {
    ts.to_string()
}

// ── robots.txt ──

fn build_robots_txt(ctx: &BuildContext, output_dir: &Path) -> Result<()> {
    let txt = sitemap::generate_robots_txt(&ctx.config.base_url);
    write_output(&output_dir.join("robots.txt"), &txt).context("failed to write robots.txt")
}
