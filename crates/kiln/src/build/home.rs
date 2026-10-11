use anyhow::{Context, Result};

use super::BuildContext;
use super::routes::ListingPage;
use crate::template::vars::{HomePageVars, PageMetadata};

pub(super) fn render(ctx: &BuildContext, page: &ListingPage<'_>, url: &str) -> Result<String> {
    let vars = HomePageVars {
        metadata: PageMetadata {
            title: &ctx.config.title,
            description: &ctx.config.description,
            url: url.into(),
        },
        pages: page.pages.iter().map(|page| &page.summary).collect(),
        pagination: page.pagination(ctx.config.base_url.prefix()),
        config: &ctx.config,
    };
    ctx.template_engine
        .render_home(&vars)
        .context("failed to render home page")
}
