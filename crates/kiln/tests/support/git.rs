use std::path::Path;
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

/// Runs `git` with `args` in `root`, isolated from any enclosing repository and from user and
/// system configuration.
pub(crate) fn git(root: &Path, args: &[&str]) {
    run_git(git_command(root).args(args));
}

/// Commits staged changes in `root` with `date` as both author and committer time.
pub(crate) fn git_commit(root: &Path, date: &str) {
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
