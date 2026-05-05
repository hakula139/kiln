use std::path::Path;

use anyhow::{Context, Result};
use strum::IntoEnumIterator;

use crate::output::write_output;
use crate::template::vars::{BucketSummary, OverviewPageVars};

use super::BuildContext;
use super::listing::{BucketKind, ListingBucket};

/// Generates overview index pages: `/sections/` and `/tags/`.
///
/// Skipped when `overview.html` is not present in the template set.
pub(crate) fn build_overview_pages(
    ctx: &BuildContext,
    buckets: &[ListingBucket],
    output_dir: &Path,
) -> Result<()> {
    if !ctx.template_engine.has_template("overview.html") {
        return Ok(());
    }

    for kind in BucketKind::iter().filter(|k| k.has_overview()) {
        let summaries: Vec<BucketSummary> = buckets
            .iter()
            .filter(|b| b.kind == kind)
            .map(BucketSummary::from)
            .collect();
        write_overview(ctx, kind, summaries, output_dir)?;
    }

    Ok(())
}

// ── Helpers ──

fn write_overview(
    ctx: &BuildContext,
    kind: BucketKind,
    buckets: Vec<BucketSummary>,
    output_dir: &Path,
) -> Result<()> {
    let vars = OverviewPageVars {
        kind: kind.plural(),
        singular: kind.singular(),
        buckets,
        config: &ctx.config,
    };

    let html = ctx
        .template_engine
        .render_overview(&vars)
        .with_context(|| format!("failed to render {} overview", kind.plural()))?;

    let dest = output_dir.join(kind.plural()).join("index.html");
    write_output(&dest, &html).with_context(|| format!("failed to write {}", dest.display()))
}
