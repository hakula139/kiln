use anyhow::{Context, Result};

use super::BuildContext;
use super::bucket::{BucketKind, Listing};
use crate::template::vars::{OverviewPageVars, PageMetadata};

/// Renders the overview of every bucket of `kind`.
pub(super) fn render(
    ctx: &BuildContext,
    kind: BucketKind,
    buckets: &[Listing<'_>],
    url: &str,
) -> Result<String> {
    let title = ctx.i18n.t(&format!("all_{}", kind.plural()));
    let vars = OverviewPageVars {
        metadata: PageMetadata {
            title: &title,
            description: &ctx.config.description,
            url,
        },
        kind: kind.as_ref(),
        buckets: buckets
            .iter()
            .filter(|bucket| bucket.kind == kind)
            .map(|bucket| bucket.summary(&ctx.config.base_url))
            .collect(),
    };
    ctx.template_engine
        .render_overview(&vars)
        .with_context(|| format!("failed to render {} overview", kind.plural()))
}
