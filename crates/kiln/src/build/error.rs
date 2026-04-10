use std::path::Path;

use anyhow::{Context, Result};

use crate::output::write_output;
use crate::template::vars::ErrorPageVars;

use super::BuildContext;

/// Generates the 404 error page if a `404.html` template exists.
pub(crate) fn build_404(ctx: &BuildContext, output_dir: &Path) -> Result<()> {
    let vars = ErrorPageVars {
        title: "404 Not Found",
        config: &ctx.config,
    };
    if let Some(result) = ctx.template_engine.render_404(&vars) {
        let html = result?;
        write_output(&output_dir.join("404.html"), &html).context("failed to write 404.html")?;
    }
    Ok(())
}
