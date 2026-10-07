use std::path::Path;

use anyhow::{Context, Result};
use strum::IntoEnumIterator;

use super::BuildContext;
use super::listing::{BucketKind, ListingBucket};
use crate::output::write_output;
use crate::template::vars::{BucketSummary, OverviewPageVars, PageMetadata};
use crate::url::page_url;

/// Generates `/sections/` and `/tags/` overview pages and returns the number written.
///
/// Skipped when `overview.html` is not present in the template set.
pub(super) fn build_overview_pages(
    ctx: &BuildContext,
    buckets: &[ListingBucket<'_>],
    output_dir: &Path,
) -> Result<usize> {
    if !ctx.template_engine.has_template("overview.html") {
        return Ok(0);
    }

    let mut page_count = 0;
    for kind in BucketKind::iter().filter(|k| k.has_overview()) {
        let summaries: Vec<BucketSummary> = buckets
            .iter()
            .filter(|b| b.kind == kind)
            .map(|bucket| bucket.summary(&ctx.config.base_url))
            .collect();
        write_overview(ctx, kind, summaries, output_dir)?;
        page_count += 1;
    }

    Ok(page_count)
}

// ── Helpers ──

fn write_overview(
    ctx: &BuildContext,
    kind: BucketKind,
    buckets: Vec<BucketSummary<'_>>,
    output_dir: &Path,
) -> Result<()> {
    let title = ctx.i18n.t(&format!("all_{}", kind.plural()));
    let vars = OverviewPageVars {
        metadata: PageMetadata {
            title: &title,
            description: &ctx.config.description,
            url: page_url(
                &ctx.config.base_url,
                &Path::new(&kind.plural()).join("index.html"),
            )
            .into(),
        },
        kind: &kind.plural(),
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
