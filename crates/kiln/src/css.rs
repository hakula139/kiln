use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use lightningcss::bundler::{Bundler, FileProvider, ResolveResult, SourceProvider};
use lightningcss::dependencies::{Dependency, DependencyOptions};
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;
use lightningcss::values::string::CSSString;
use percent_encoding::percent_decode_str;

use crate::config::{Config, CssProcessor};
use crate::content::page::{Page, is_page_bundle};
use crate::output::write_output;
use crate::static_assets::StaticAssetManifest;
use crate::static_assets::publication::PublishedAssets;
use crate::url::{join_site_url, path_url};

const ENTRY: &str = "assets/css/_src/style.css";
struct Stylesheet {
    source: PathBuf,
    page: Option<PathBuf>,
    output: PathBuf,
}

/// Discovered stylesheet ownership and public destinations for one build.
pub(crate) struct Stylesheets {
    shared: Option<Stylesheet>,
    pages: BTreeMap<PathBuf, Stylesheet>,
}

impl Stylesheets {
    /// Resolves site overrides and page-owned entry points before compiling any sources.
    ///
    /// # Errors
    ///
    /// Returns an error when a page source or output path cannot be resolved.
    pub(crate) fn discover(
        root: &Path,
        config: &Config,
        content_dir: &Path,
        pages: &[Page],
    ) -> Result<Self> {
        let shared_source = std::iter::once(root.to_owned())
            .chain(config.theme_dir(root))
            .map(|dir| dir.join(ENTRY))
            .find(|path| path.is_file());
        let shared = shared_source.map(|source| Stylesheet {
            source,
            page: None,
            output: PathBuf::from("assets/css/site.css"),
        });
        let mut styles = BTreeMap::new();
        for page in pages {
            if !is_page_bundle(&page.source_path) {
                continue;
            }
            let bundle = page
                .source_path
                .parent()
                .context("page source has no parent")?;
            let source = bundle.join(ENTRY);
            if source.is_file() {
                let output = page.output_path(content_dir)?;
                styles.insert(
                    page.source_path.clone(),
                    Stylesheet {
                        source,
                        page: Some(page.source_path.clone()),
                        output: output
                            .parent()
                            .context("page output has no parent")?
                            .join("assets/css/page.css"),
                    },
                );
            }
        }
        Ok(Self {
            shared,
            pages: styles,
        })
    }

    /// Compiles private sources into the build output, preserving published asset references.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid sources, unpublished assets, or compiler and output failures.
    pub(crate) fn compile(
        &self,
        root: &Path,
        config: &Config,
        assets: &PublishedAssets,
        output_dir: &Path,
    ) -> Result<()> {
        for style in self.shared.iter().chain(self.pages.values()) {
            let css = match config.css.processor.unwrap_or_default() {
                CssProcessor::Plain => compile_plain(assets, style),
                CssProcessor::Tailwind => {
                    compile_tailwind(root, config, assets, self.shared.as_ref(), style)
                }
            }
            .with_context(|| format!("failed to compile {}", style.source.display()))?;
            write_output(&output_dir.join(&style.output), &css)?;
        }
        Ok(())
    }

    /// Returns the owning page's fingerprinted stylesheet URL, including the site's base path.
    ///
    /// Returns `None` when the page has no owning stylesheet.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing compiled stylesheet.
    pub(crate) fn page_url(
        &self,
        deployment_prefix: &str,
        manifest: &StaticAssetManifest,
        page: &Page,
    ) -> Result<Option<String>> {
        self.pages
            .get(&page.source_path)
            .map(|style| {
                let path = format!("/{}", path_url(&style.output));
                let hashed = manifest.asset_url(&path)?;
                Ok(join_site_url(deployment_prefix, &hashed))
            })
            .transpose()
    }
}

fn compile_plain(assets: &PublishedAssets, style: &Stylesheet) -> Result<String> {
    let provider = CssProvider(FileProvider::new());
    let mut bundler = Bundler::new(&provider, None, ParserOptions::default());
    let stylesheet = bundler
        .bundle(&style.source)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    publish_urls(assets, style, &stylesheet)
}

fn compile_tailwind(
    root: &Path,
    config: &Config,
    assets: &PublishedAssets,
    shared: Option<&Stylesheet>,
    style: &Stylesheet,
) -> Result<String> {
    let temp = tempfile::tempdir().context("failed to create CSS compiler workspace")?;
    let workspace = temp.path().canonicalize()?;
    let input = workspace.join("input.css");
    let mut source = String::new();
    if style.page.is_some()
        && let Some(shared) = shared
    {
        _ = writeln!(
            source,
            "@reference {};",
            css_string(&shared.source.canonicalize()?.to_string_lossy())?
        );
    }
    _ = writeln!(
        source,
        "@import {};",
        css_string(&style.source.canonicalize()?.to_string_lossy())?
    );
    for dir in std::iter::once(root.join("content"))
        .chain(std::iter::once(root.join("templates")))
        .chain(config.theme_dir(root).map(|dir| dir.join("templates")))
        .filter(|dir| dir.is_dir())
    {
        _ = writeln!(
            source,
            "@source {};",
            css_string(&dir.canonicalize()?.to_string_lossy())?
        );
    }
    fs::write(&input, source)?;
    let binary = if cfg!(windows) {
        "kiln-tailwindcss.cmd"
    } else {
        "kiln-tailwindcss"
    };
    let result = Command::new(binary)
        .arg(&input)
        .current_dir(root)
        .output()
        .context("failed to run `kiln-tailwindcss`. Install kiln's CSS processor package or use kiln's Nix package")?;
    if !result.status.success() {
        bail!(
            "Tailwind CSS exited with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let compiled = String::from_utf8(result.stdout).context("Tailwind CSS output is not UTF-8")?;
    let stylesheet = StyleSheet::parse(
        &compiled,
        ParserOptions {
            filename: input.to_string_lossy().into_owned(),
            ..ParserOptions::default()
        },
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    publish_urls(assets, style, &stylesheet)
}

struct CssProvider(FileProvider);

impl SourceProvider for CssProvider {
    type Error = std::io::Error;

    fn read<'a>(&'a self, file: &Path) -> std::io::Result<&'a str> {
        self.0.read(file)
    }

    fn resolve(&self, specifier: &str, origin: &Path) -> std::io::Result<ResolveResult> {
        if is_external(specifier) {
            Ok(ResolveResult::External(specifier.to_owned()))
        } else {
            self.0.resolve(specifier, origin)
        }
    }
}

fn publish_urls(
    assets: &PublishedAssets,
    style: &Stylesheet,
    stylesheet: &StyleSheet<'_>,
) -> Result<String> {
    let result = stylesheet
        .to_css(PrinterOptions {
            analyze_dependencies: Some(DependencyOptions {
                remove_imports: false,
            }),
            ..PrinterOptions::default()
        })
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let mut css = result.code;
    for dependency in result.dependencies.unwrap_or_default() {
        let (placeholder, url) = match dependency {
            Dependency::Import(import) => (import.placeholder, import.url),
            Dependency::Url(url) => {
                let source = PathBuf::from(&url.loc.file_path);
                let published = published_url(assets, style, &source, &url.url)?;
                (url.placeholder, published)
            }
        };
        css = css.replace(&css_string(&placeholder)?, &css_string(&url)?);
    }
    Ok(css)
}

fn published_url(
    assets: &PublishedAssets,
    style: &Stylesheet,
    source: &Path,
    url: &str,
) -> Result<String> {
    if is_external(url) {
        return Ok(url.to_owned());
    }
    let split = url.find(['?', '#']).unwrap_or(url.len());
    let (path, suffix) = url.split_at(split);
    let decoded = percent_decode_str(path)
        .decode_utf8()
        .context("CSS asset URL is not valid UTF-8")?;
    let referenced = source
        .parent()
        .context("CSS source has no parent")?
        .join(decoded.as_ref());
    let destination = assets
        .resolve(style.page.as_deref(), &referenced)
        .with_context(|| format!("cannot resolve CSS asset {url} from {}", source.display()))?
        .with_context(|| {
            format!(
                "CSS asset {url} from {} is not a published static file or owning-page asset",
                source.display()
            )
        })?;
    let relative = pathdiff::diff_paths(
        destination,
        style.output.parent().context("CSS output has no parent")?,
    )
    .context("cannot resolve published CSS asset path")?;
    Ok(format!("{}{suffix}", path_url(&relative)))
}

fn is_external(url: &str) -> bool {
    url.starts_with('/') || url.starts_with('#') || url::Url::parse(url).is_ok()
}

fn css_string(value: &str) -> Result<String> {
    CSSString(value.into())
        .to_css_string(PrinterOptions::default())
        .map_err(|error| anyhow::anyhow!("{error}"))
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::test_utils::write_test_file;

    // ── Stylesheets::page_url ──

    #[test]
    fn page_url_preserves_base_path_and_encoded_output() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            "content/a % # 世界/index.md",
            indoc! {r#"
                +++
                title = "Example"
                +++
            "#},
        );
        write_test_file(
            root.path(),
            "content/a % # 世界/assets/css/_src/style.css",
            ".page { color: red; }",
        );
        let page = Page::from_file(&root.path().join("content/a % # 世界/index.md")).unwrap();
        let config = Config::default();
        let pages = [page];
        let stylesheets =
            Stylesheets::discover(root.path(), &config, &root.path().join("content"), &pages)
                .unwrap();
        let output = root.path().join("public");
        let assets = PublishedAssets::publish(
            root.path(),
            None,
            &root.path().join("content"),
            &pages,
            &output,
        )
        .unwrap();
        stylesheets
            .compile(root.path(), &config, &assets, &output)
            .unwrap();
        let manifest = StaticAssetManifest::build(&output).unwrap();
        let encoded = "/a%20%25%20%23%20%E4%B8%96%E7%95%8C/assets/css/page.css";
        assert_eq!(
            stylesheets
                .page_url("/blog/", &manifest, &pages[0])
                .unwrap(),
            Some(format!("/blog{}", manifest.asset_url(encoded).unwrap())),
        );
    }

    // ── compile_plain ──

    #[test]
    fn compile_plain_preserves_external_imports_and_urls() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            ENTRY,
            indoc! {r#"
                @import "https://example.com/base.css";
                .root { background: url(/images/root.svg); }
                .data { background: url("data:image/svg+xml,<svg/>"); }
                .fragment { filter: url(#filter); }
            "#},
        );
        let style = Stylesheet {
            source: root.path().join(ENTRY),
            page: None,
            output: PathBuf::from("assets/css/site.css"),
        };
        let assets = PublishedAssets::publish(
            root.path(),
            None,
            &root.path().join("content"),
            &[],
            &root.path().join("public"),
        )
        .unwrap();
        let css = compile_plain(&assets, &style).unwrap();
        for url in [
            "https://example.com/base.css",
            "/images/root.svg",
            "data:image/svg+xml,<svg/>",
            "#filter",
        ] {
            assert!(css.contains(url), "{css}");
        }
    }

    #[test]
    fn compile_plain_encodes_published_paths_and_preserves_url_suffixes() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            ENTRY,
            indoc! {r#"
            .image { background: url("../../../static/a%20%25%20%23%20%E4%B8%96%E7%95%8C.svg?v=1#icon"); }
        "#},
        );
        write_test_file(root.path(), "static/a % # 世界.svg", "image");
        let output = root.path().join("public");
        let assets = PublishedAssets::publish(
            root.path(),
            None,
            &root.path().join("content"),
            &[],
            &output,
        )
        .unwrap();
        let style = Stylesheet {
            source: root.path().join(ENTRY),
            page: None,
            output: PathBuf::from("assets/css/site.css"),
        };
        let css = compile_plain(&assets, &style).unwrap();
        assert!(
            css.contains("../../a%20%25%20%23%20%E4%B8%96%E7%95%8C.svg?v=1#icon"),
            "{css}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn compile_plain_preserves_static_symlink_paths() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            ENTRY,
            indoc! {r"
                .icon { background: url(../../../static/alias.svg); }
                .nested { background: url(../../../static/shared/image.svg); }
                .parent { background: url(../../../static/local/../alias.svg); }
            "},
        );
        write_test_file(root.path(), "original.svg", "image");
        fs::create_dir_all(root.path().join("static/local")).unwrap();
        std::os::unix::fs::symlink(
            root.path().join("original.svg"),
            root.path().join("static/alias.svg"),
        )
        .unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("image.svg"), "linked directory image").unwrap();
        std::os::unix::fs::symlink(external.path(), root.path().join("static/shared")).unwrap();
        let style = Stylesheet {
            source: root.path().join(ENTRY),
            page: None,
            output: PathBuf::from("assets/css/site.css"),
        };
        let assets = PublishedAssets::publish(
            root.path(),
            None,
            &root.path().join("content"),
            &[],
            &root.path().join("public"),
        )
        .unwrap();
        let css = compile_plain(&assets, &style).unwrap();
        assert!(css.contains("../../alias.svg"), "{css}");
        assert!(css.contains("../../shared/image.svg"), "{css}");
        assert!(!css.contains("shared/../"), "{css}");
        assert!(!css.contains("original.svg"), "{css}");
    }

    // ── published_url ──

    #[test]
    fn published_url_private_and_unpublished_assets_returns_error() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(root.path(), "content/example/assets/css/_src/style.css", "");
        write_test_file(root.path(), "content/example/_secret.svg", "private");
        write_test_file(root.path(), "unpublished.svg", "unpublished");
        let style = Stylesheet {
            source: root
                .path()
                .join("content/example/assets/css/_src/style.css"),
            page: Some(root.path().join("content/example/index.md")),
            output: PathBuf::from("example/assets/css/page.css"),
        };
        let assets = PublishedAssets::publish(
            root.path(),
            None,
            &root.path().join("content"),
            &[],
            &root.path().join("public"),
        )
        .unwrap();
        let private =
            published_url(&assets, &style, &style.source, "../../../_secret.svg").unwrap_err();
        assert!(private.to_string().contains("not a published"), "{private}");
        let unpublished = published_url(
            &assets,
            &style,
            &style.source,
            "../../../../../unpublished.svg",
        )
        .unwrap_err();
        assert!(
            unpublished.to_string().contains("not a published"),
            "{unpublished}"
        );

        write_test_file(root.path(), "content/assets/css/_src/style.css", "");
        write_test_file(root.path(), "content/_secret.svg", "private");
        let root_style = Stylesheet {
            source: root.path().join("content/assets/css/_src/style.css"),
            page: Some(root.path().join("content/index.md")),
            output: PathBuf::from("assets/css/page.css"),
        };
        let private = published_url(
            &assets,
            &root_style,
            &root_style.source,
            "../../../_secret.svg",
        )
        .unwrap_err();
        assert!(private.to_string().contains("not a published"), "{private}");

        write_test_file(root.path(), "content/index.md", "Markdown source");
        for url in ["../../../index.md", "../../.."] {
            let error = published_url(&assets, &root_style, &root_style.source, url).unwrap_err();
            assert!(error.to_string().contains("published"), "{error}");
        }
    }
}
