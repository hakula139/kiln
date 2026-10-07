use std::path::Path;

use anyhow::{Context, Result};

use super::BuildContext;
use super::routes::RoutePlan;
use crate::output::write_output;
use crate::sitemap;

pub(super) fn build_sitemap_and_robots(
    ctx: &BuildContext,
    plan: &RoutePlan,
    output_dir: &Path,
) -> Result<()> {
    build_sitemap(plan, output_dir)?;
    build_robots_txt(ctx, output_dir)
}

// ── Sitemap ──

fn build_sitemap(plan: &RoutePlan, output_dir: &Path) -> Result<()> {
    let entries = plan.sitemap_entries();
    let xml = sitemap::generate_sitemap(entries);
    write_output(&output_dir.join("sitemap.xml"), &xml).context("failed to write sitemap.xml")
}

// ── robots.txt ──

fn build_robots_txt(ctx: &BuildContext, output_dir: &Path) -> Result<()> {
    let txt = sitemap::generate_robots_txt(&ctx.config.base_url);
    write_output(&output_dir.join("robots.txt"), &txt).context("failed to write robots.txt")
}
