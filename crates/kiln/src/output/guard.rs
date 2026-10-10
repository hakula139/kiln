use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};

use super::paths_overlap;
use crate::config::{Config, SITE_INPUTS, THEME_INPUTS};

/// Resolves the build output directory, rejecting overlaps with project inputs and metadata.
///
/// `override_dir` replaces `config.output_dir`. Relative paths resolve against `root`.
///
/// # Errors
///
/// Returns an error if the path is empty, cannot be resolved, or would overwrite project inputs.
pub(crate) fn resolve_output_dir(
    root: &Path,
    config: &Config,
    override_dir: Option<&Path>,
) -> Result<PathBuf> {
    let output = override_dir.unwrap_or(&config.output_dir);
    ensure!(!output.as_os_str().is_empty(), "output_dir cannot be empty");
    let canonical = canonicalize_via_parent(&root.join(output))?;
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("failed to canonicalize project root {}", root.display()))?;
    ensure!(
        !canonical_root.starts_with(&canonical),
        "output directory {} would overwrite the project root at {}",
        canonical.display(),
        canonical_root.display()
    );
    ensure!(
        !canonical.exists() || canonical.is_dir(),
        "output directory {} is not a directory",
        canonical.display()
    );

    for name in SITE_INPUTS {
        validate_input_tree(&root.join(name), &canonical)?;
    }
    validate_output_overlap(&root.join("themes"), &canonical)?;
    if let Some(theme) = config.theme_dir(root) {
        for name in THEME_INPUTS {
            validate_input_tree(&theme.join(name), &canonical)?;
        }
        validate_git_metadata(&theme, &canonical, true)?;
    }
    for ancestor in canonical_root.ancestors() {
        validate_git_metadata(ancestor, &canonical, ancestor == canonical_root)?;
    }
    Ok(canonical)
}

fn validate_input_tree(path: &Path, output: &Path) -> Result<()> {
    validate_output_overlap(path, output)?;
    if !path.exists() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(path).follow_links(true) {
        let entry = match entry {
            Err(error)
                if error.loop_ancestor().is_some()
                    || error
                        .io_error()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                continue;
            }
            entry => {
                entry.with_context(|| format!("failed to inspect input {}", path.display()))?
            }
        };
        if entry.path_is_symlink() {
            validate_output_overlap(entry.path(), output)?;
        }
    }
    Ok(())
}

fn validate_git_metadata(root: &Path, output: &Path, reserve_name: bool) -> Result<()> {
    let git_file = root.join(".git");
    if reserve_name || git_file.exists() || git_file.is_symlink() {
        validate_output_overlap(&git_file, output)?;
    }
    if git_file.is_file() {
        let contents = fs::read_to_string(&git_file).context("failed to read .git file")?;
        if let Some(git_dir) = contents.trim().strip_prefix("gitdir: ") {
            let git_dir = root.join(git_dir);
            validate_output_overlap(&git_dir, output)?;
            let common_dir = git_dir.join("commondir");
            if common_dir.is_file() {
                let common = fs::read_to_string(common_dir)
                    .context("failed to read Git common directory")?;
                validate_output_overlap(&git_dir.join(common.trim()), output)?;
            }
        }
    }
    Ok(())
}

fn validate_output_overlap(input: &Path, output: &Path) -> Result<()> {
    let input = canonicalize_via_parent(input)?;
    ensure!(
        !paths_overlap(&input, output),
        "output directory {} overlaps project input {}",
        output.display(),
        input.display()
    );
    Ok(())
}

/// Resolves symlinks before parent components, including paths with nonexistent components.
fn canonicalize_via_parent(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            component => {
                resolved.push(component);
                if resolved.exists() || resolved.is_symlink() {
                    resolved = resolved
                        .canonicalize()
                        .with_context(|| format!("failed to resolve {}", resolved.display()))?;
                }
            }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_output_dir ──

    #[test]
    fn resolve_output_dir_default_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_root = dir.path().canonicalize().unwrap();
        let resolved = resolve_output_dir(dir.path(), &Config::default(), None).unwrap();

        assert_eq!(resolved, canonical_root.join("public"));
    }

    #[test]
    fn resolve_output_dir_nested_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_root = dir.path().canonicalize().unwrap();

        let resolved = output_dir_for(dir.path(), "dist/site").unwrap();

        assert_eq!(resolved, canonical_root.join("dist").join("site"));
    }

    #[test]
    fn resolve_output_dir_canonicalizes_dotdot_segments() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("build")).unwrap();
        let canonical_root = dir.path().canonicalize().unwrap();

        let resolved = output_dir_for(dir.path(), "build/../out").unwrap();

        assert_eq!(resolved, canonical_root.join("out"));
    }

    #[test]
    fn resolve_output_dir_absolute_path_outside_root() {
        let root = tempfile::tempdir().unwrap();
        let target_parent = tempfile::tempdir().unwrap();
        let canonical_target_parent = target_parent.path().canonicalize().unwrap();
        let target = canonical_target_parent.join("site");

        let resolved = output_dir_for(root.path(), &target).unwrap();

        assert_eq!(resolved, target);
    }

    #[test]
    fn resolve_output_dir_relative_path_outside_root() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("project");
        fs::create_dir(&root).unwrap();
        let canonical_outer = outer.path().canonicalize().unwrap();

        let resolved = output_dir_for(&root, "../sibling").unwrap();

        assert_eq!(resolved, canonical_outer.join("sibling"));
    }

    #[test]
    fn resolve_output_dir_empty_returns_error() {
        let dir = tempfile::tempdir().unwrap();

        let err = output_dir_for(dir.path(), "").unwrap_err().to_string();

        assert!(
            err.contains("output_dir cannot be empty"),
            "should reject empty string, got: {err}"
        );
    }

    #[test]
    fn resolve_output_dir_root_or_ancestor_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let canonical_root = dir.path().canonicalize().unwrap();
        let filesystem_root = canonical_root.ancestors().last().unwrap();

        for output in [
            ".",
            "..",
            canonical_root.to_str().unwrap(),
            filesystem_root.to_str().unwrap(),
        ] {
            let error = output_dir_for(dir.path(), output).unwrap_err().to_string();
            assert!(
                error.contains("would overwrite the project root"),
                "{output}: {error}"
            );
        }
    }

    #[test]
    fn resolve_output_dir_input_or_metadata_overlap_returns_error() {
        let root = tempfile::tempdir().unwrap();
        for output in [
            "content",
            "content/posts/generated",
            "static",
            "assets/css",
            "templates",
            "i18n",
            "themes",
            "themes/example/generated",
            "config.toml",
            ".git",
            ".git/objects",
            "missing/../content",
        ] {
            let error = output_dir_for(root.path(), output).unwrap_err();
            assert!(
                error.to_string().contains("overlaps project input"),
                "{output}: {error}"
            );
        }
    }

    #[test]
    fn resolve_output_dir_linked_git_metadata_returns_error() {
        let root = tempfile::tempdir().unwrap();
        let repository = tempfile::tempdir().unwrap();
        let git_dir = repository.path().join("worktrees/example");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(
            root.path().join(".git"),
            format!("gitdir: {}", git_dir.display()),
        )
        .unwrap();
        fs::write(git_dir.join("commondir"), "../..").unwrap();

        assert_eq!(
            output_dir_for(root.path(), "public").unwrap(),
            root.path().canonicalize().unwrap().join("public")
        );

        for output in [git_dir.join("objects"), repository.path().join("objects")] {
            assert!(
                output_dir_for(root.path(), &output)
                    .unwrap_err()
                    .to_string()
                    .contains("overlaps project input")
            );
        }
    }

    #[test]
    fn resolve_output_dir_parent_repository_metadata_returns_error() {
        let repository = tempfile::tempdir().unwrap();
        let root = repository.path().join("site");
        fs::create_dir(&root).unwrap();
        fs::create_dir(repository.path().join(".git")).unwrap();
        assert!(
            output_dir_for(&root, "../.git/objects")
                .unwrap_err()
                .to_string()
                .contains("overlaps project input")
        );
        assert_eq!(
            output_dir_for(&root, "../dist").unwrap(),
            repository.path().canonicalize().unwrap().join("dist")
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_output_dir_symlink_to_root_or_ancestor_returns_error() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().join("project");
        fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&root, root.join("root-link")).unwrap();
        std::os::unix::fs::symlink(outer.path(), root.join("ancestor-link")).unwrap();

        for output_dir in ["root-link", "ancestor-link"] {
            let err = output_dir_for(&root, output_dir).unwrap_err().to_string();

            assert!(
                err.contains("would overwrite the project root"),
                "should reject {output_dir}, got: {err}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolve_output_dir_symlinked_inputs_and_output_returns_error() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("content/posts/example")).unwrap();
        fs::write(external.path().join("image.png"), "original").unwrap();
        std::os::unix::fs::symlink(
            external.path(),
            root.path().join("content/posts/example/images"),
        )
        .unwrap();
        std::os::unix::fs::symlink(root.path().join("content"), root.path().join("alias")).unwrap();

        for output in [
            external.path().to_owned(),
            external.path().join("generated"),
            root.path().join("alias/posts"),
        ] {
            let error = output_dir_for(root.path(), &output).unwrap_err();
            assert!(
                error.to_string().contains("overlaps project input"),
                "{error}"
            );
        }
        assert_eq!(
            fs::read_to_string(external.path().join("image.png")).unwrap(),
            "original"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_output_dir_linked_theme_protects_inputs_and_metadata() {
        use std::os::unix::fs::symlink;

        let theme = tempfile::tempdir().unwrap();
        let external_site = tempfile::tempdir().unwrap();
        let linked_assets = tempfile::tempdir().unwrap();
        fs::write(theme.path().join("theme.toml"), "").unwrap();
        symlink(linked_assets.path(), theme.path().join("assets")).unwrap();
        fs::create_dir(theme.path().join(".git")).unwrap();

        for root in [
            theme.path().join("example"),
            external_site.path().join("site"),
        ] {
            fs::create_dir_all(root.join("themes")).unwrap();
            let target = if root.starts_with(theme.path()) {
                Path::new("../..")
            } else {
                theme.path()
            };
            symlink(target, root.join("themes/example")).unwrap();
            fs::write(root.join("config.toml"), r#"theme = "example""#).unwrap();
            let config = Config::load(&root).unwrap();
            assert_eq!(
                resolve_output_dir(&root, &config, None).unwrap(),
                root.canonicalize().unwrap().join("public")
            );
            assert_eq!(
                resolve_output_dir(&root, &config, Some(&theme.path().join("generated"))).unwrap(),
                theme.path().canonicalize().unwrap().join("generated")
            );

            for output in [
                theme.path().join("theme.toml/generated"),
                theme.path().join("i18n/generated"),
                theme.path().join("static/generated"),
                theme.path().join("templates/generated"),
                linked_assets.path().join("generated"),
                theme.path().join(".git/generated"),
            ] {
                let error = resolve_output_dir(&root, &config, Some(&output)).unwrap_err();
                assert!(error.to_string().contains("overlaps project input"));
            }
            for output in [theme.path(), theme.path().parent().unwrap()] {
                assert!(resolve_output_dir(&root, &config, Some(output)).is_err());
            }
        }
    }

    fn output_dir_for(root: &Path, output_dir: impl AsRef<Path>) -> Result<PathBuf> {
        let config = Config {
            output_dir: output_dir.as_ref().to_owned(),
            ..Config::default()
        };
        resolve_output_dir(root, &config, None)
    }
}
