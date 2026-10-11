use anyhow::{Context, Result};

use super::BuildContext;
use super::bucket::Listing;
use super::listing::group_by_year;
use super::routes::ListingPage;
use crate::template::vars::{ArchivePageVars, PageMetadata};

/// Renders one page of the archive of `bucket`.
pub(super) fn render(
    ctx: &BuildContext,
    bucket: &Listing<'_>,
    page: &ListingPage<'_>,
    url: &str,
) -> Result<String> {
    let vars = ArchivePageVars {
        metadata: PageMetadata {
            title: &bucket.name,
            description: &ctx.config.description,
            url: url.into(),
        },
        kind: bucket.kind.as_ref(),
        page_groups: group_by_year(page.pages),
        pagination: page.pagination(ctx.config.base_url.prefix()),
    };
    ctx.template_engine.render_archive(&vars).with_context(|| {
        format!(
            "failed to render archive {}/{}",
            bucket.kind.plural(),
            bucket.slug,
        )
    })
}
