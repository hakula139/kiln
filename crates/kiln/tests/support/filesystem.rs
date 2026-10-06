use std::fs;
use std::path::Path;

/// Writes a file at `dir.join(rel_path)`, creating parent directories as needed.
pub fn write_test_file(dir: &Path, rel_path: &str, content: &str) {
    let path = dir.join(rel_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
