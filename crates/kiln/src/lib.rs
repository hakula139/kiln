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
    use std::path::PathBuf;

    use crate::config::Config;
    use crate::template::TemplateEngine;

    /// Returns the path to the workspace `templates/` directory.
    pub fn template_dir() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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
}
