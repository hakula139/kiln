use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

/// Writes `content` to the given path, creating parent directories as needed.
///
/// # Errors
///
/// Returns an error if directory creation or file writing fails.
pub fn write_output(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_parent_dirs_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("test.html");

        write_output(&path, "hello").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.html");

        write_output(&path, "first").unwrap();
        write_output(&path, "second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }
}
