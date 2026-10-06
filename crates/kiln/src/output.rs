use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use walkdir::WalkDir;

/// Creates an empty output directory, removing existing contents first.
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

/// Recursively copies included files into `dest`, materializing source symlinks as regular
/// files and directories. `include` filters file and directory names, skipping excluded subtrees.
/// Returns source → destination paths for copied files, or an empty map if `src` does not exist.
///
/// # Errors
///
/// Returns an error if traversal, directory creation, or copying fails, or a source link overlaps
/// the destination directory.
pub fn copy_directory(
    src: &Path,
    dest: &Path,
    include: impl Fn(&OsStr) -> bool,
) -> Result<BTreeMap<PathBuf, PathBuf>> {
    let mut files = BTreeMap::new();
    if !src.exists() && !src.is_symlink() {
        return Ok(files);
    }
    fs::create_dir_all(dest)
        .with_context(|| format!("failed to create directory {}", dest.display()))?;
    let destination = dest
        .canonicalize()
        .with_context(|| format!("failed to resolve destination {}", dest.display()))?;
    for entry in walk_directory(src, &include) {
        let entry = entry.with_context(|| format!("failed to read entry in {}", src.display()))?;
        if entry.path_is_symlink() {
            validate_source_link(entry.path(), &destination)?;
        }
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
            files.insert(entry.into_path(), target);
        }
    }
    Ok(files)
}

/// Walks included names recursively, excluding errors from links with excluded names.
pub(crate) fn walk_directory<'a>(
    src: &Path,
    include: &'a impl Fn(&OsStr) -> bool,
) -> impl Iterator<Item = walkdir::Result<walkdir::DirEntry>> + 'a {
    WalkDir::new(src)
        .follow_links(true)
        .into_iter()
        .filter_entry(move |entry| entry.depth() == 0 || include(entry.file_name()))
        .filter(move |entry| match entry {
            Ok(_) => true,
            Err(error) => {
                error.depth() == 0 || error.path().and_then(Path::file_name).is_none_or(include)
            }
        })
}

/// Rejects source links whose resolved targets overlap the canonical destination.
fn validate_source_link(path: &Path, destination: &Path) -> Result<()> {
    let source = path
        .canonicalize()
        .with_context(|| format!("failed to resolve source link {}", path.display()))?;
    ensure!(
        !source.starts_with(destination) && !destination.starts_with(&source),
        "source link {} overlaps destination {}",
        path.display(),
        destination.display()
    );
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
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::test_utils::PermissionGuard;

    // ── clean_output_dir ──

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

    // ── copy_directory ──

    #[test]
    fn copy_directory_copies_recursively() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");
        fs::create_dir_all(src.join("images")).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(src.join("favicon.ico"), "icon").unwrap();
        fs::write(src.join("images").join("logo.png"), "logo").unwrap();

        copy_directory(&src, &dest, |_| true).unwrap();

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
    fn copy_directory_materializes_external_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");
        let external = tempfile::tempdir().unwrap();
        fs::create_dir_all(&src).unwrap();
        fs::create_dir(external.path().join("nested")).unwrap();
        fs::write(external.path().join("nested/data.txt"), "linked directory").unwrap();
        fs::write(external.path().join("data.txt"), "linked file").unwrap();
        fs::write(external.path().join("_private.txt"), "private").unwrap();
        fs::write(external.path().join("_headers"), "nested headers").unwrap();
        symlink(external.path(), src.join("shared")).unwrap();
        symlink(external.path(), src.join("_private")).unwrap();
        symlink(external.path().join("data.txt"), src.join("data.txt")).unwrap();
        symlink(external.path().join("_headers"), src.join("_headers")).unwrap();

        copy_directory(&src, &dest, |_| true).unwrap();

        for (path, expected) in [
            ("data.txt", "linked file"),
            ("shared/data.txt", "linked file"),
            ("shared/nested/data.txt", "linked directory"),
        ] {
            let output = dest.join(path);
            assert_eq!(fs::read_to_string(&output).unwrap(), expected);
            assert!(fs::symlink_metadata(output).unwrap().file_type().is_file());
        }
        assert!(fs::symlink_metadata(dest.join("shared")).unwrap().is_dir());
        assert_eq!(
            fs::read_to_string(dest.join("_headers")).unwrap(),
            "nested headers"
        );
        for path in ["_private/_private.txt", "shared/_private.txt"] {
            assert_eq!(fs::read_to_string(dest.join(path)).unwrap(), "private");
        }
        assert_eq!(
            fs::read_to_string(dest.join("shared/_headers")).unwrap(),
            "nested headers"
        );
    }

    #[test]
    fn copy_directory_symlink_root() {
        let dir = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("data.txt"), "linked root").unwrap();
        let src = dir.path().join("static");
        symlink(external.path(), &src).unwrap();
        let dest = dir.path().join("public");

        copy_directory(&src, &dest, |_| true).unwrap();

        assert_eq!(
            fs::read_to_string(dest.join("data.txt")).unwrap(),
            "linked root"
        );
        assert!(fs::symlink_metadata(&dest).unwrap().is_dir());
    }

    #[test]
    fn copy_directory_preserves_underscore_names_at_every_depth() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");
        let files = [
            "_headers",
            "_redirects",
            "_custom/data.txt",
            "nested/_headers",
        ];
        for file in files {
            let path = src.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file).unwrap();
        }

        copy_directory(&src, &dest, |_| true).unwrap();

        for file in files {
            assert_eq!(fs::read_to_string(dest.join(file)).unwrap(), file);
        }
    }

    // macOS APFS rejects non-UTF-8 filenames, while Linux ext4 and btrfs accept them.
    #[cfg(target_os = "linux")]
    #[test]
    fn copy_directory_copies_files_with_non_utf8_names() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");
        fs::create_dir_all(&src).unwrap();
        let name = OsStr::from_bytes(&[0xff, 0xfe]);
        fs::write(src.join(name), "binary").unwrap();

        copy_directory(&src, &dest, |_| true).unwrap();

        assert_eq!(fs::read(dest.join(name)).unwrap(), b"binary");
    }

    #[test]
    fn copy_directory_excludes_invalid_links_and_preserves_public_errors() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("assets");
        let dest = dir.path().join("public");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("visible.txt"), "published").unwrap();
        symlink(src.join("missing"), src.join("_broken")).unwrap();
        symlink(&src, src.join("_cycle")).unwrap();

        copy_directory(&src, &dest, |name| !crate::content::is_private(name)).unwrap();

        assert_eq!(
            fs::read_to_string(dest.join("visible.txt")).unwrap(),
            "published"
        );
        assert!(!dest.join("_broken").exists());
        assert!(!dest.join("_cycle").exists());

        symlink(src.join("missing"), src.join("broken")).unwrap();
        let error =
            copy_directory(&src, &dest, |name| !crate::content::is_private(name)).unwrap_err();
        assert_eq!(
            error.downcast_ref::<walkdir::Error>().unwrap().path(),
            Some(src.join("broken").as_path())
        );
    }

    #[test]
    fn copy_directory_missing_src_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");

        copy_directory(&src, &dest, |_| true).unwrap();

        assert!(!dest.exists());
    }

    #[test]
    fn copy_directory_unreadable_subdir_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");
        let subdir = src.join("broken");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(subdir.join("file.txt"), "content").unwrap();
        fs::create_dir_all(&dest).unwrap();

        let _guard = PermissionGuard::restrict(&subdir, 0o000);

        let err = copy_directory(&src, &dest, |_| true)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to read entry"),
            "should report entry read failure, got: {err}"
        );
    }

    #[test]
    fn copy_directory_broken_symlinks_returns_error() {
        for root_link in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("static");
            let link = if root_link {
                src.clone()
            } else {
                fs::create_dir(&src).unwrap();
                src.join("broken")
            };
            symlink(dir.path().join("missing"), &link).unwrap();

            let err = copy_directory(&src, &dir.path().join("public"), |_| true).unwrap_err();

            assert!(err.to_string().contains("failed to read entry"));
            assert!(format!("{err:#}").contains(link.to_str().unwrap()));
        }
    }

    #[test]
    fn copy_directory_symlink_cycle_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        fs::create_dir(&src).unwrap();
        symlink(&src, src.join("cycle")).unwrap();

        let err = copy_directory(&src, &dir.path().join("public"), |_| true).unwrap_err();

        assert!(format!("{err:#}").contains("loop"));
    }

    #[test]
    fn copy_directory_symlink_overlaps_destination_returns_error() {
        for target_is_ancestor in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("static");
            let dest = dir.path().join("public");
            fs::create_dir(&src).unwrap();
            fs::create_dir(&dest).unwrap();
            fs::write(dest.join("sentinel.txt"), "unchanged").unwrap();
            let target = if target_is_ancestor {
                dir.path()
            } else {
                &dest
            };
            symlink(target, src.join("linked")).unwrap();

            let err = copy_directory(&src, &dest, |_| true).unwrap_err();

            assert!(err.to_string().contains("overlaps destination"));
            assert_eq!(
                fs::read_to_string(dest.join("sentinel.txt")).unwrap(),
                "unchanged"
            );
            assert!(!dest.join("linked").exists());
        }
    }

    #[test]
    fn copy_directory_unwritable_dest_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("static");
        let dest = dir.path().join("public");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("file.txt"), "content").unwrap();
        fs::create_dir_all(&dest).unwrap();

        let _guard = PermissionGuard::restrict(&dest, 0o444);

        let err = copy_directory(&src, &dest, |_| true)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to copy"),
            "should report copy failure, got: {err}"
        );
    }

    // ── validate_source_link ──

    #[test]
    fn validate_source_link_broken_target_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("broken");
        symlink(dir.path().join("missing"), &link).unwrap();

        let destination = dir.path().canonicalize().unwrap().join("public");

        let err = validate_source_link(&link, &destination).unwrap_err();

        assert_eq!(
            err.to_string(),
            format!("failed to resolve source link {}", link.display())
        );
        assert_eq!(
            err.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::NotFound
        );
    }

    // ── copy_file ──

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

    // ── write_output ──

    #[test]
    fn write_output_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("test.html");

        write_output(&path, "hello").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn write_output_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.html");

        write_output(&path, "first").unwrap();
        write_output(&path, "second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn write_output_create_dir_permission_denied_returns_error() {
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
    fn write_output_permission_denied_returns_error() {
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
