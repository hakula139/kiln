use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use indoc::formatdoc;

const DEFAULT_BINARY: &str = "pagefind";

/// Runs the Pagefind indexer on the given output directory.
///
/// Expects `output_dir` to contain the fully built site HTML. Pagefind writes
/// its search index and client assets to `{output_dir}/pagefind/`.
///
/// # Errors
///
/// Returns an error if the Pagefind binary is not found, exits with a
/// non-zero status, or cannot be spawned.
pub fn run_pagefind(output_dir: &Path, binary: Option<&str>) -> Result<()> {
    let binary = binary.unwrap_or(DEFAULT_BINARY);
    let site_arg = output_dir
        .to_str()
        .context("output directory path is not valid UTF-8")?;

    let output = Command::new(binary)
        .args(["--site", site_arg])
        .output()
        .with_context(|| {
            formatdoc! {"
                failed to run `{binary}` — is Pagefind installed?

                Install with one of:

                  cargo install pagefind
                  npm install -g pagefind
                  npx pagefind --site <dir>

                See https://pagefind.app/docs/installation/ for details."}
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let msg = formatdoc! {"
            Pagefind exited with {status}

            stdout:
            {stdout}
            stderr:
            {stderr}",
            status = output.status,
        };
        bail!(msg);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        eprint!("{stdout}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── run_pagefind ──

    #[test]
    fn missing_binary_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = run_pagefind(dir.path(), Some("nonexistent-pagefind-binary-xyz"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("is Pagefind installed?"),
            "should mention installation, got: {err}"
        );
        assert!(
            err.contains("cargo install pagefind"),
            "should include install instructions, got: {err}"
        );
    }

    #[test]
    fn non_zero_exit_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_pagefind(dir.path(), Some("false"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Pagefind exited with"),
            "should report exit status, got: {err}"
        );
    }
}
