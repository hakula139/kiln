use anyhow::{Context, Result};

use super::BuildContext;
use crate::template::vars::PageMetadata;

/// Renders the 404 page.
pub(super) fn render(ctx: &BuildContext, url: &str) -> Result<String> {
    let title = ctx.i18n.t("error_404_title");
    let vars = PageMetadata {
        title: &title,
        description: &ctx.config.description,
        url,
    };
    ctx.template_engine
        .render_404(&vars)
        .context("failed to render 404 page")
}
