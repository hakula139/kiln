use anyhow::{Context, Result};

use super::BuildContext;
use super::routes::ListingPage;
use crate::template::vars::{HomePageVars, PageMetadata};

/// Renders one page of the home listing.
pub(super) fn render(ctx: &BuildContext, page: &ListingPage<'_>, url: &str) -> Result<String> {
    let vars = HomePageVars {
        metadata: PageMetadata {
            title: &ctx.config.title,
            description: &ctx.config.description,
            url,
        },
        pages: page.pages.iter().map(|page| &page.summary).collect(),
        pagination: page.pagination(ctx.config.base_url.prefix()),
    };
    ctx.template_engine
        .render_home(&vars)
        .context("failed to render home page")
}
