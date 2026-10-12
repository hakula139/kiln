use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use indoc::formatdoc;

const DEFAULT_BINARY: &str = "pagefind";

/// Indexes built site HTML in `output_dir`, writing search assets to `output_dir/pagefind/`.
pub(crate) fn run_pagefind(output_dir: &Path, binary: Option<&str>) -> Result<()> {
    let command = binary.unwrap_or(DEFAULT_BINARY);
    let run = |program: &str| Command::new(program).arg("--site").arg(output_dir).output();

    let output = run(command);
    #[cfg(windows)]
    let output = output.or_else(|error| {
        if binary.is_none() && error.kind() == std::io::ErrorKind::NotFound {
            run("pagefind.cmd")
        } else {
            Err(error)
        }
    });
    let output = output.with_context(|| {
        formatdoc! {"
            failed to run `{command}`. Is Pagefind installed?

            Install with one of:

              npm install -g pagefind
              cargo install pagefind --features extended

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
        tracing::debug!("{stdout}");
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    // ── run_pagefind ──

    #[test]
    fn indexes_site() {
        let dir = tempfile::Builder::new()
            .prefix("pagefind site & (test) ")
            .tempdir()
            .unwrap();
        fs::write(
            dir.path().join("index.html"),
            r#"<html lang="en"><body><main data-pagefind-body>Searchable example</main></body></html>"#,
        )
        .unwrap();

        run_pagefind(dir.path(), None).unwrap();

        assert!(dir.path().join("pagefind/pagefind.js").is_file());
        let entry: serde_yaml::Value = serde_yaml::from_slice(
            &fs::read(dir.path().join("pagefind/pagefind-entry.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(entry["languages"]["en"]["page_count"].as_u64(), Some(1));
    }

    #[test]
    fn reports_missing_binary() {
        let dir = tempfile::tempdir().unwrap();
        let err = run_pagefind(dir.path(), Some("nonexistent-pagefind-binary-xyz"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to run `nonexistent-pagefind-binary-xyz`"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn succeeds_with_and_without_output() {
        let dir = tempfile::tempdir().unwrap();
        for binary in ["echo", "true"] {
            run_pagefind(dir.path(), Some(binary)).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn reports_non_zero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let err = run_pagefind(dir.path(), Some("false"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Pagefind exited with"), "{err}");
    }
}
