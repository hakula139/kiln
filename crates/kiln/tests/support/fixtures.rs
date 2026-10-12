use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Repository-local variables listed by `git rev-parse --local-env-vars`, which git itself clears
/// when it switches repositories.
const LOCAL_ENV_VARS: [&str; 15] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_DIR",
    "GIT_GRAFT_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_PREFIX",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
    "GIT_WORK_TREE",
];

/// Writes a file at `dir.join(rel_path)`, creating parent directories as needed.
pub fn write_test_file(dir: &Path, rel_path: &str, content: &str) {
    let path = dir.join(rel_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Copies the test templates into `dest` as real files on disk.
pub fn copy_templates(dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(template_dir()).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), dest.join(entry.file_name())).unwrap();
    }
}

/// Returns the checked-in directory of minimal templates for every page kind.
pub fn template_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/templates")
}

/// Runs `git` with `args` in `root`, isolated from any enclosing repository and from user and
/// system configuration.
pub fn git(root: &Path, args: &[&str]) {
    run_git(git_command(root).args(args));
}

/// Commits staged changes in `root` with `date` as both author and committer time.
pub fn git_commit(root: &Path, date: &str) {
    run_git(
        git_command(root)
            .args(["commit", "-qm", "Update content"])
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date),
    );
}

fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    for var in LOCAL_ENV_VARS {
        command.env_remove(var);
    }
    command
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Example")
        .env("GIT_AUTHOR_EMAIL", "example@example.com")
        .env("GIT_COMMITTER_NAME", "Example")
        .env("GIT_COMMITTER_EMAIL", "example@example.com");
    command
}

fn run_git(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// RAII guard that restores filesystem permissions on drop. Prevents `TempDir::drop` failures
/// from leftover restricted permissions when the test panics.
#[cfg(unix)]
pub struct PermissionGuard {
    path: PathBuf,
    mode: u32,
}

#[cfg(unix)]
impl PermissionGuard {
    pub fn restrict(path: &Path, mode: u32) -> Self {
        let original = fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        Self {
            path: path.to_owned(),
            mode: original,
        }
    }
}

#[cfg(unix)]
impl Drop for PermissionGuard {
    fn drop(&mut self) {
        _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
    }
}
