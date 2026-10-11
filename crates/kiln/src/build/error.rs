use std::path::Path;

use anyhow::Result;

use super::BuildContext;
use crate::output::write_output;
use crate::template::vars::ErrorPageVars;

/// Returns the number of pages written: one if a `404.html` template exists, otherwise zero.
pub(super) fn build_404(ctx: &BuildContext, output_dir: &Path) -> Result<usize> {
    let vars = ErrorPageVars {
        title: "404 Not Found",
        config: &ctx.config,
    };
    if let Some(result) = ctx.template_engine.render_404(&vars) {
        let html = result?;
        write_output(&output_dir.join("404.html"), &html)?;
        return Ok(1);
    }
    Ok(0)
}
