use std::path::Path;

use anyhow::{Context, Result};

use super::{frontmatter, read_optional};

/// Loads an optional archive title, propagating unreadable or malformed metadata.
pub(crate) fn load_index_title(dir: &Path) -> Result<Option<String>> {
    let path = dir.join("_index.md");
    let Some(content) = read_optional(&path)? else {
        return Ok(None);
    };
    let (frontmatter, _) = frontmatter::parse(&content)
        .with_context(|| format!("invalid frontmatter in {}", path.display()))?;
    Ok((!frontmatter.title.is_empty()).then_some(frontmatter.title))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::indoc;

    use super::*;

    // ── load_index_title ──

    #[test]
    fn returns_title_only_when_named() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(load_index_title(directory.path()).unwrap(), None);
        for (content, expected) in [
            (
                indoc! {r#"
                    +++
                    title = ""
                    +++
                "#},
                None,
            ),
            (
                indoc! {r#"
                    +++
                    title = "Notes"
                    +++
                "#},
                Some("Notes"),
            ),
        ] {
            fs::write(directory.path().join("_index.md"), content).unwrap();
            assert_eq!(
                load_index_title(directory.path()).unwrap().as_deref(),
                expected
            );
        }
    }

    #[test]
    fn reports_malformed_or_unreadable_index() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("_index.md");
        fs::write(
            &path,
            indoc! {r"
                +++
                title = [
                +++
            "},
        )
        .unwrap();
        let error = load_index_title(directory.path()).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("invalid frontmatter in {}", path.display())
        );

        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let error = load_index_title(directory.path()).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("failed to read {}", path.display())
        );
    }
}
