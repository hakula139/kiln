use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jiff::Timestamp;

pub(super) struct GitInfo {
    root: PathBuf,
}

impl GitInfo {
    /// Returns `None` when disabled or Git history is unavailable or shallow.
    pub(super) fn new(root: &Path, enabled: bool) -> Option<Self> {
        if !enabled {
            return None;
        }

        let root = fs::canonicalize(root).ok()?;
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel", "--is-shallow-repository"])
            .current_dir(&root)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let text = String::from_utf8(output.stdout).ok()?;
        let mut lines = text.lines();
        let git_root = fs::canonicalize(lines.next()?).ok()?;
        if lines.next()? != "false" {
            return None;
        }

        Some(Self { root: git_root })
    }

    fn last_modified(&self, source: &Path) -> Option<Timestamp> {
        let source = fs::canonicalize(source).ok()?;
        let relative = source.strip_prefix(&self.root).ok()?;
        let output = Command::new("git")
            .args([
                "--literal-pathspecs",
                "log",
                "-1",
                "--follow",
                "--format=%cI",
                "--",
            ])
            .arg(relative)
            .current_dir(&self.root)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        std::str::from_utf8(&output.stdout)
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

pub(super) fn updated_timestamp(
    explicit: Option<Timestamp>,
    source: &Path,
    git_info: Option<&GitInfo>,
) -> Option<Timestamp> {
    explicit.or_else(|| git_info?.last_modified(source))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit(root: &Path, date: &str) {
        let output = Command::new("git")
            .args(["commit", "-qm", "Update article"])
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repo() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.name", "Example"]);
        git(dir.path(), &["config", "user.email", "example@example.com"]);
        dir
    }

    // ── GitInfo::new ──

    #[test]
    fn git_info_new_resolves_nested_site_files() {
        let dir = repo();
        let site = dir.path().join("site");
        fs::create_dir(&site).unwrap();
        let source = site.join("post.md");
        fs::write(&source, "post").unwrap();
        git(dir.path(), &["add", "site/post.md"]);
        commit(dir.path(), "2024-01-01T00:00:00+00:00");

        let info = GitInfo::new(&site, true).unwrap();
        assert_eq!(
            info.last_modified(&source),
            Some("2024-01-01T00:00:00Z".parse().unwrap())
        );
        assert!(GitInfo::new(&site, false).is_none());
    }

    #[test]
    fn git_info_new_omits_missing_and_shallow_history() {
        let dir = repo();
        let source = dir.path().join("post.md");
        fs::write(&source, "post").unwrap();
        git(dir.path(), &["add", "post.md"]);
        commit(dir.path(), "2024-01-01T00:00:00+00:00");

        assert!(GitInfo::new(&dir.path().join("missing"), true).is_none());
        assert!(GitInfo::new(tempfile::tempdir().unwrap().path(), true).is_none());

        let shallow = tempfile::tempdir().unwrap();
        let url = format!("file://{}", dir.path().display());
        let output = Command::new("git")
            .args(["clone", "-q", "--depth=1", &url])
            .arg(shallow.path().join("copy"))
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(GitInfo::new(&shallow.path().join("copy"), true).is_none());
    }

    // ── updated_timestamp ──

    #[test]
    fn updated_timestamp_uses_last_commit_and_prefers_frontmatter() {
        let dir = repo();
        let first = dir.path().join("before.md");
        let current = dir.path().join("after.md");
        fs::write(&first, "first").unwrap();
        git(dir.path(), &["add", "before.md"]);
        commit(dir.path(), "2024-01-01T00:00:00+00:00");

        git(dir.path(), &["mv", "before.md", "after.md"]);
        commit(dir.path(), "2024-02-01T00:00:00+00:00");

        let info = GitInfo::new(dir.path(), true).unwrap();
        assert_eq!(
            updated_timestamp(None, &current, Some(&info)),
            Some("2024-02-01T00:00:00Z".parse().unwrap())
        );
        let explicit = "2024-04-01T00:00:00Z".parse().unwrap();
        assert_eq!(
            updated_timestamp(Some(explicit), &current, Some(&info)),
            Some(explicit)
        );

        let untracked = dir.path().join("draft.md");
        fs::write(&untracked, "draft").unwrap();
        assert_eq!(updated_timestamp(None, &untracked, Some(&info)), None);
    }

    #[test]
    fn updated_timestamp_treats_filename_as_literal_path() {
        let dir = repo();
        let literal = dir.path().join("post[1].md");
        let similar = dir.path().join("post1.md");
        fs::write(&literal, "first").unwrap();
        git(dir.path(), &["add", "post[1].md"]);
        commit(dir.path(), "2024-01-01T00:00:00+00:00");
        fs::write(&similar, "second").unwrap();
        git(dir.path(), &["add", "post1.md"]);
        commit(dir.path(), "2024-02-01T00:00:00+00:00");

        let info = GitInfo::new(dir.path(), true).unwrap();
        assert_eq!(
            updated_timestamp(None, &literal, Some(&info)),
            Some("2024-01-01T00:00:00Z".parse().unwrap())
        );
    }
}
