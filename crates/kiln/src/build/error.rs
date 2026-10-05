use std::path::Path;

use anyhow::{Context, Result};

use crate::output::write_output;
use crate::template::vars::ErrorPageVars;

use super::BuildContext;

/// Returns the number of pages written: one if a `404.html` template exists, otherwise zero.
pub(crate) fn build_404(ctx: &BuildContext, output_dir: &Path) -> Result<usize> {
    let vars = ErrorPageVars {
        title: "404 Not Found",
        config: &ctx.config,
    };
    if let Some(result) = ctx.template_engine.render_404(&vars) {
        let html = result?;
        write_output(&output_dir.join("404.html"), &html).context("failed to write 404.html")?;
        return Ok(1);
    }
    Ok(0)
}
