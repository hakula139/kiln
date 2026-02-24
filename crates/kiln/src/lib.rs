pub mod build;
pub mod config;
pub mod content;
pub mod directive;
pub mod output;
pub mod render;
pub mod template;

pub use build::build;

#[cfg(test)]
pub(crate) mod test_utils {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use crate::config::Config;
    use crate::template::TemplateEngine;

    /// Returns the path to the workspace `templates/` directory.
    pub fn template_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("templates")
    }

    /// Creates a `TemplateEngine` using the workspace templates.
    pub fn test_engine() -> TemplateEngine {
        TemplateEngine::new(&template_dir()).unwrap()
    }

    /// Creates a `Config` with all defaults.
    pub fn test_config() -> Config {
        toml::from_str("").unwrap()
    }

    /// RAII guard that restores filesystem permissions on drop.
    ///
    /// Ensures cleanup happens even if the test panics, preventing
    /// `TempDir::drop` failures from leftover restricted permissions.
    pub struct PermissionGuard {
        path: PathBuf,
        mode: u32,
    }

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

    impl Drop for PermissionGuard {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.mode));
        }
    }
}
