use std::fs;
use std::path::{Path, PathBuf};

use crate::content::frontmatter::Frontmatter;
use crate::content::page::{Page, PageKind};
use crate::i18n::I18n;
use crate::template::TemplateEngine;

#[path = "../tests/support/fixtures.rs"]
mod fixtures;

#[cfg(unix)]
pub(crate) use fixtures::PermissionGuard;
pub(crate) use fixtures::{copy_templates, template_dir, write_test_file};

/// Creates a `TemplateEngine` using embedded test templates.
pub(crate) fn test_engine() -> TemplateEngine {
    TemplateEngine::new(None, Some(&template_dir()), &test_i18n()).unwrap()
}

/// Creates a minimal `I18n` seeded with English strings so build-level tests render
/// deterministic output.
pub(crate) fn test_i18n() -> I18n {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("i18n")).unwrap();
    fs::write(
        dir.path().join("i18n").join("en.toml"),
        r#"all_posts = "All Posts""#,
    )
    .unwrap();
    I18n::load(Path::new("/nonexistent-site"), Some(dir.path()), "en").unwrap()
}

/// Creates a minimal standalone `Page` with the given title and an empty body.
pub(crate) fn test_page(title: &str) -> Page {
    Page {
        frontmatter: Frontmatter {
            title: title.to_owned(),
            ..Frontmatter::default()
        },
        raw_content: String::new(),
        kind: PageKind::Page,
        slug: title.to_lowercase().replace(' ', "-"),
        summary: None,
        source_path: PathBuf::from(format!("content/{title}/index.md")),
        assets: Vec::new(),
    }
}
