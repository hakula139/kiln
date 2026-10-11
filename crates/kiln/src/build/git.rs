use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use icu_normalizer::ComposingNormalizerBorrowed;
use jiff::Timestamp;

pub(super) struct GitInfo {
    root: PathBuf,
    timestamps: HashMap<PathBuf, Timestamp>,
}

impl GitInfo {
    /// Returns `None` when Git history is unavailable or shallow.
    pub(super) fn new(root: &Path, content_dir: &Path) -> Option<Self> {
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

        let content_dir = fs::canonicalize(content_dir).ok()?;
        let output = Command::new("git")
            .args([
                "--literal-pathspecs",
                "log",
                "-z",
                "--no-renames",
                "--format=%x00%cI",
                "--name-only",
                "--",
            ])
            .arg(content_dir.strip_prefix(&git_root).ok()?)
            .current_dir(&git_root)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let timestamps = newest_timestamps(&output.stdout)?;
        Some(Self {
            root: git_root,
            timestamps,
        })
    }

    pub(super) fn last_modified(&self, source: &Path) -> Option<Timestamp> {
        let source = fs::canonicalize(source).ok()?;
        let relative = source.strip_prefix(&self.root).ok()?.to_str()?;
        self.timestamps.get(&normalized_path(relative)).copied()
    }
}

/// Maps each path in `git log -z --format=%x00%cI --name-only` output to its newest commit time.
fn newest_timestamps(log: &[u8]) -> Option<HashMap<PathBuf, Timestamp>> {
    let mut timestamps = HashMap::new();
    let mut committed = None;
    let mut fields = log.split(|&byte| byte == 0);
    while let Some(field) = fields.next() {
        if field.is_empty() {
            let Some(date) = fields.next() else { break };
            committed = Some(std::str::from_utf8(date).ok()?.parse().ok()?);
        } else if let Ok(path) = std::str::from_utf8(field.strip_prefix(b"\n").unwrap_or(field)) {
            timestamps
                .entry(normalized_path(path))
                .or_insert(committed?);
        }
    }
    Some(timestamps)
}

/// Composes a path to NFC, which Git reports on macOS even for decomposed on-disk names.
fn normalized_path(path: &str) -> PathBuf {
    PathBuf::from(
        ComposingNormalizerBorrowed::new_nfc()
            .normalize(path)
            .as_ref(),
    )
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::test_utils::{git, git_commit};

    // ── GitInfo::new ──

    #[test]
    fn resolves_nested_site_files() {
        let dir = repo();
        let content = dir.path().join("my site/content");
        fs::create_dir_all(&content).unwrap();
        let source = content.join("文章 one.md");
        let decomposed = content.join("cafe\u{301}.md");
        fs::write(&source, "post").unwrap();
        fs::write(&decomposed, "post").unwrap();
        git(dir.path(), &["add", "."]);
        git_commit(dir.path(), "2024-01-01T00:00:00+08:00");

        let site = dir.path().join("my site");
        let info = GitInfo::new(&site, &content).unwrap();
        let expected = Some("2023-12-31T16:00:00Z".parse().unwrap());
        assert_eq!(info.last_modified(&source), expected);
        assert_eq!(info.last_modified(&decomposed), expected);
    }

    #[test]
    fn omits_missing_empty_and_shallow_history() {
        let dir = repo();
        let content = dir.path().join("content");
        fs::create_dir(&content).unwrap();
        fs::write(content.join("post.md"), "post").unwrap();
        git(dir.path(), &["add", "."]);
        git_commit(dir.path(), "2024-01-01T00:00:00+00:00");

        let missing = dir.path().join("missing");
        assert!(GitInfo::new(&missing, &missing.join("content")).is_none());
        let plain = tempfile::tempdir().unwrap();
        assert!(GitInfo::new(plain.path(), &plain.path().join("content")).is_none());
        let empty = repo();
        fs::create_dir(empty.path().join("content")).unwrap();
        assert!(GitInfo::new(empty.path(), &empty.path().join("content")).is_none());

        let shallow = tempfile::tempdir().unwrap();
        let url = format!("file://{}", dir.path().display());
        git(shallow.path(), &["clone", "-q", "--depth=1", &url, "copy"]);
        let copy = shallow.path().join("copy");
        assert!(GitInfo::new(&copy, &copy.join("content")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn reads_site_with_leading_colon() {
        let dir = repo();
        let content = dir.path().join(":my site/content");
        fs::create_dir_all(&content).unwrap();
        let source = content.join("post.md");
        fs::write(&source, "post").unwrap();
        git(dir.path(), &["add", "."]);
        git_commit(dir.path(), "2024-01-01T00:00:00+00:00");

        let info = GitInfo::new(&dir.path().join(":my site"), &content).unwrap();
        assert_eq!(
            info.last_modified(&source),
            Some("2024-01-01T00:00:00Z".parse().unwrap())
        );
    }

    // ── GitInfo::last_modified ──

    #[test]
    fn uses_newest_commit_per_file() {
        let dir = repo();
        let content = dir.path().join("content");
        fs::create_dir(&content).unwrap();
        for name in ["edited.md", "moved.md", "kept.md", "merged.md"] {
            fs::write(content.join(name), name).unwrap();
        }
        fs::write(dir.path().join("outside.md"), "outside").unwrap();
        git(dir.path(), &["add", "."]);
        git_commit(dir.path(), "2024-01-01T00:00:00+00:00");

        git(
            dir.path(),
            &["mv", "content/moved.md", "content/renamed.md"],
        );
        git_commit(dir.path(), "2024-02-01T00:00:00+00:00");

        git(dir.path(), &["switch", "-qc", "side"]);
        fs::write(content.join("merged.md"), "side").unwrap();
        git(dir.path(), &["add", "."]);
        git_commit(dir.path(), "2024-03-01T00:00:00+00:00");
        git(dir.path(), &["switch", "-q", "-"]);
        fs::write(content.join("edited.md"), "edited").unwrap();
        fs::write(dir.path().join("outside.md"), "edited").unwrap();
        git(dir.path(), &["add", "."]);
        git_commit(dir.path(), "2024-04-01T00:00:00+00:00");
        git(
            dir.path(),
            &["merge", "-q", "--no-ff", "--no-commit", "side"],
        );
        git_commit(dir.path(), "2024-05-01T00:00:00+00:00");

        git(dir.path(), &["switch", "-qc", "discarded"]);
        fs::write(content.join("kept.md"), "discarded").unwrap();
        git(dir.path(), &["add", "."]);
        git_commit(dir.path(), "2024-06-01T00:00:00+00:00");
        git(dir.path(), &["switch", "-q", "-"]);
        git(
            dir.path(),
            &["merge", "-q", "-s", "ours", "--no-commit", "discarded"],
        );
        git_commit(dir.path(), "2024-07-01T00:00:00+00:00");

        let info = GitInfo::new(dir.path(), &content).unwrap();
        let last_modified = |name: &str| info.last_modified(&content.join(name));
        assert_eq!(
            last_modified("edited.md"),
            Some("2024-04-01T00:00:00Z".parse().unwrap())
        );
        assert_eq!(
            last_modified("renamed.md"),
            Some("2024-02-01T00:00:00Z".parse().unwrap())
        );
        assert_eq!(
            last_modified("kept.md"),
            Some("2024-01-01T00:00:00Z".parse().unwrap())
        );
        assert_eq!(
            last_modified("merged.md"),
            Some("2024-03-01T00:00:00Z".parse().unwrap())
        );
        assert_eq!(info.last_modified(&dir.path().join("outside.md")), None);

        fs::write(content.join("draft.md"), "draft").unwrap();
        assert_eq!(last_modified("draft.md"), None);
    }

    // ── newest_timestamps ──

    #[test]
    fn keeps_first_commit_and_skips_non_utf8_paths() {
        let log = b"\x002024-02-01T00:00:00Z\x00\nbad\xff.md\x00post.md\x00\x002024-01-01T00:00:00Z\x00\npost.md\x00old.md\x00";

        assert_eq!(
            newest_timestamps(log),
            Some(HashMap::from([
                (
                    PathBuf::from("post.md"),
                    "2024-02-01T00:00:00Z".parse().unwrap()
                ),
                (
                    PathBuf::from("old.md"),
                    "2024-01-01T00:00:00Z".parse().unwrap()
                ),
            ]))
        );
    }

    fn repo() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        dir
    }
}
