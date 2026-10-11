use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::config::{Config, ImageConfig};
use crate::content::frontmatter::Frontmatter;
use crate::content::page::{Page, PageKind};
use crate::i18n::{I18n, Language};
use crate::render::lqip::ImageResolver;
use crate::render::pipeline::{RenderedPage, render_page};
use crate::render::{PageResources, RenderOptions};
use crate::static_assets::StaticAssetManifest;
use crate::template::TemplateEngine;

#[path = "../tests/support/fixtures.rs"]
mod fixtures;

#[cfg(unix)]
pub(crate) use fixtures::PermissionGuard;
pub(crate) use fixtures::{copy_templates, git, git_commit, template_dir, write_test_file};

/// Creates a `TemplateEngine` using embedded test templates.
pub(crate) fn test_engine() -> TemplateEngine {
    test_engine_with(None, Some(&template_dir()), &Config::default())
}

/// Creates a `TemplateEngine` over `site_dir` alone, with default configuration.
pub(crate) fn test_site_engine(site_dir: &Path) -> TemplateEngine {
    test_engine_with(Some(site_dir), None, &Config::default())
}

/// Creates a `TemplateEngine` with test translations and an empty asset manifest.
pub(crate) fn test_engine_with(
    site_dir: Option<&Path>,
    theme_dir: Option<&Path>,
    config: &Config,
) -> TemplateEngine {
    TemplateEngine::new(
        site_dir,
        theme_dir,
        config,
        &test_i18n(),
        &StaticAssetManifest::default(),
    )
    .unwrap()
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
    I18n::load(
        Path::new("/nonexistent-site"),
        Some(dir.path()),
        &Language::default(),
    )
    .unwrap()
}

/// Collects `(key, value)` pairs into owned named directive arguments.
pub(crate) fn named_args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|&(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// Creates a minimal standalone `Page` with the given title and an empty body.
pub(crate) fn test_page(title: &str) -> Page {
    Page {
        frontmatter: Frontmatter {
            title: title.to_owned(),
            ..Frontmatter::default()
        },
        body: String::new(),
        kind: PageKind::Page,
        summary: None,
        source_path: PathBuf::from(format!("content/{title}/index.md")),
        output_dir: PathBuf::from(title),
        assets: Vec::new(),
    }
}

/// Creates `PageResources` for `/` with no published assets and no resolvable images.
pub(crate) fn test_resources() -> PageResources<'static> {
    static IMAGES: LazyLock<ImageResolver> =
        LazyLock::new(|| ImageResolver::new(Path::new(""), ImageConfig::default()));
    static ASSETS: LazyLock<StaticAssetManifest> = LazyLock::new(StaticAssetManifest::default);
    PageResources {
        source_dir: None,
        images: &IMAGES,
        assets: &ASSETS,
        page_url: "/",
        deployment_prefix: "/",
    }
}

/// Renders Markdown through the page pipeline with test templates and resources.
pub(crate) fn render_test_page(input: &str, options: &RenderOptions) -> RenderedPage {
    render_page(input, &test_engine(), options, &test_resources(), false).unwrap()
}
