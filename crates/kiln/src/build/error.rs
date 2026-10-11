use anyhow::Result;

use super::BuildContext;
use crate::template::vars::ErrorPageVars;

/// Renders the 404 page, or returns `None` when the template set has no `404.html`.
pub(super) fn render(ctx: &BuildContext) -> Result<Option<String>> {
    let vars = ErrorPageVars {
        title: "404 Not Found",
        config: &ctx.config,
    };
    ctx.template_engine.render_404(&vars).transpose()
}
