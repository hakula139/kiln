use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "filesystem.rs"]
mod filesystem;

pub use filesystem::write_test_file;

pub fn kiln() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kiln"));
    command.env_remove("KILN_BASE_URL").env_remove("RUST_LOG");
    command
}

pub fn write_executable_file(root: &Path, path: &str, content: &str) -> PathBuf {
    write_test_file(root, path, content);
    let binary = root.join(path);
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    binary
}
