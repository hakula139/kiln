use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Writes a file at `dir.join(rel_path)`, creating parent directories as needed.
pub fn write_test_file(dir: &Path, rel_path: &str, content: &str) {
    let path = dir.join(rel_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Returns the checked-in directory of minimal templates for every page kind.
pub fn template_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/templates")
}

/// Copies the test templates into `dest` as real files on disk.
pub fn copy_templates(dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(template_dir()).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), dest.join(entry.file_name())).unwrap();
    }
}

/// Restores a path's original permissions on drop, so `TempDir` cleanup survives a failed test.
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
