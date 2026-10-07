use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};

use super::frontmatter;

/// Loads an optional archive title, propagating unreadable or malformed metadata.
pub(crate) fn load_index_title(dir: &Path) -> Result<Option<String>> {
    let path = dir.join("_index.md");
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let (frontmatter, _) = frontmatter::parse(&content)
        .with_context(|| format!("invalid frontmatter in {}", path.display()))?;
    Ok((!frontmatter.title.is_empty()).then_some(frontmatter.title))
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── load_index_title ──

    #[test]
    fn load_index_title_missing_empty_and_named() {
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
    fn load_index_title_malformed_or_unreadable_returns_error() {
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
        assert!(format!("{error:#}").contains("_index.md"));
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(load_index_title(directory.path()).is_err());
    }
}
