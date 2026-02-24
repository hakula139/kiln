use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use walkdir::WalkDir;

/// Removes and recreates the output directory for a clean build.
///
/// Does nothing if the directory does not exist.
///
/// # Errors
///
/// Returns an error if removal or creation fails.
pub fn clean_output_dir(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to clean output directory {}", path.display()))?;
    }
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create output directory {}", path.display()))
}

/// Recursively copies all files from `src` into `dest`, preserving directory structure.
///
/// Skips the copy entirely if `src` does not exist.
///
/// # Errors
///
/// Returns an error if directory creation or file copying fails.
pub fn copy_static(src: &Path, dest: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    for entry in WalkDir::new(src).follow_links(false) {
        let entry = entry.with_context(|| format!("failed to read entry in {}", src.display()))?;
        let relative = entry.path().strip_prefix(src).with_context(|| {
            format!(
                "path {} is not under {}",
                entry.path().display(),
                src.display()
            )
        })?;
        let target = dest.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("failed to create directory {}", target.display()))?;
        } else {
            copy_file(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Copies a single file from `src` to `dest`, creating parent directories as needed.
///
/// # Errors
///
/// Returns an error if directory creation or file copying fails.
pub fn copy_file(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }
    fs::copy(src, dest)
        .with_context(|| format!("failed to copy {} to {}", src.display(), dest.display()))?;
    Ok(())
}

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
    use crate::test_utils::PermissionGuard;

    // -- clean_output_dir --

    #[test]
    fn clean_creates_nonexistent_dir() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("public");

        clean_output_dir(&output).unwrap();

        assert!(output.exists());
    }

    #[test]
    fn clean_removes_existing_contents() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("public");
        fs::create_dir_all(output.join("old")).unwrap();
        fs::write(output.join("old").join("stale.html"), "stale").unwrap();

        clean_output_dir(&output).unwrap();

        assert!(output.exists(), "output dir should be recreated");
        assert!(
            fs::read_dir(&output).unwrap().next().is_none(),
            "output dir should be empty after clean"
        );
    }

    #[test]
    fn clean_permission_denied_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("public");
        fs::create_dir_all(output.join("sub")).unwrap();

        // Lock the parent so remove_dir_all fails on the child.
        let _guard = PermissionGuard::restrict(&output, 0o444);

        let err = clean_output_dir(&output).unwrap_err().to_string();
        assert!(
            err.contains("failed to clean output directory"),
            "should report clean failure, got: {err}"
        );
    }

    // -- copy_static --

    #[test]
    fn copy_static_copies_recursively() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");
        fs::create_dir_all(src.join("images")).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(src.join("favicon.ico"), "icon").unwrap();
        fs::write(src.join("images").join("logo.png"), "logo").unwrap();

        copy_static(&src, &dest).unwrap();

        assert_eq!(
            fs::read_to_string(dest.join("favicon.ico")).unwrap(),
            "icon"
        );
        assert_eq!(
            fs::read_to_string(dest.join("images").join("logo.png")).unwrap(),
            "logo"
        );
    }

    #[test]
    fn copy_static_missing_src_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");

        copy_static(&src, &dest).unwrap();

        assert!(!dest.exists());
    }

    #[test]
    fn copy_static_unreadable_subdir_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");
        let subdir = src.join("broken");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(subdir.join("file.txt"), "content").unwrap();
        fs::create_dir_all(&dest).unwrap();

        let _guard = PermissionGuard::restrict(&subdir, 0o000);

        let err = copy_static(&src, &dest).unwrap_err().to_string();
        assert!(
            err.contains("failed to read entry"),
            "should report entry read failure, got: {err}"
        );
    }

    #[test]
    fn copy_static_unwritable_dest_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("file.txt"), "content").unwrap();
        fs::create_dir_all(&dest).unwrap();

        let _guard = PermissionGuard::restrict(&dest, 0o444);

        let err = copy_static(&src, &dest).unwrap_err().to_string();
        assert!(
            err.contains("failed to copy"),
            "should report copy failure, got: {err}"
        );
    }

    // -- copy_file --

    #[test]
    fn copy_file_creates_parent_and_copies() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.png");
        let dest = dir.path().join("a").join("b").join("dest.png");
        fs::write(&src, "image-data").unwrap();

        copy_file(&src, &dest).unwrap();

        assert_eq!(fs::read_to_string(&dest).unwrap(), "image-data");
    }

    #[test]
    fn copy_file_nonexistent_src_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("missing.png");
        let dest = dir.path().join("dest.png");

        let err = copy_file(&src, &dest).unwrap_err().to_string();
        assert!(
            err.contains("failed to copy"),
            "should report copy failure, got: {err}"
        );
    }

    #[test]
    fn copy_file_unwritable_dest_parent_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.png");
        let readonly = dir.path().join("readonly");
        fs::write(&src, "data").unwrap();
        fs::create_dir(&readonly).unwrap();
        let _guard = PermissionGuard::restrict(&readonly, 0o444);

        let err = copy_file(&src, &readonly.join("sub").join("dest.png"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to create directory"),
            "should report directory creation failure, got: {err}"
        );
    }

    // -- write_output --

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

    #[test]
    fn create_dir_permission_denied_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let readonly = dir.path().join("readonly");
        fs::create_dir(&readonly).unwrap();
        let _guard = PermissionGuard::restrict(&readonly, 0o444);

        let err = write_output(&readonly.join("sub").join("file.html"), "content")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to create directory"),
            "should report directory creation failure, got: {err}"
        );
    }

    #[test]
    fn write_permission_denied_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let readonly = dir.path().join("readonly");
        fs::create_dir(&readonly).unwrap();
        let _guard = PermissionGuard::restrict(&readonly, 0o444);

        let err = write_output(&readonly.join("file.html"), "content")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to write"),
            "should report write failure, got: {err}"
        );
    }
}
