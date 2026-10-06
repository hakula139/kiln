use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use lightningcss::bundler::{Bundler, FileProvider, ResolveResult, SourceProvider};
use lightningcss::dependencies::{Dependency, DependencyOptions};
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;
use lightningcss::values::string::CSSString;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use walkdir::WalkDir;

use crate::build::url::{page_url, resolve_relative_url};
use crate::config::{Config, CssProcessor};
use crate::content::page::{Page, is_page_bundle};
use crate::output::write_output;
use crate::static_assets::{StaticAssetManifest, path_to_url};

const ENTRY: &str = "_assets/css/style.css";
const URL_PATH_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'.')
    .remove(b'-')
    .remove(b'_')
    .remove(b'~');

struct Stylesheet {
    source: PathBuf,
    output: PathBuf,
    bundle: Option<PathBuf>,
    bundle_assets: BTreeSet<PathBuf>,
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
    /// Returns an error when a page output or bundle asset source path cannot be resolved.
    pub(crate) fn discover(
        root: &Path,
        config: &Config,
        pages: &[Page],
        content_dir: &Path,
    ) -> Result<Self> {
        let shared_source = std::iter::once(root.to_owned())
            .chain(config.theme_dir(root))
            .map(|dir| dir.join(ENTRY))
            .find(|path| path.is_file());
        let shared = shared_source.map(|source| Stylesheet {
            source,
            output: PathBuf::from("css/style.css"),
            bundle: None,
            bundle_assets: BTreeSet::new(),
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
                        output: output
                            .parent()
                            .context("page output has no parent")?
                            .join("assets/css/style.css"),
                        bundle: Some(bundle.to_owned()),
                        bundle_assets: page
                            .assets
                            .iter()
                            .map(|asset| {
                                asset
                                    .strip_prefix(bundle)
                                    .map(Path::to_owned)
                                    .context("asset is outside its bundle")
                            })
                            .collect::<Result<_>>()?,
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
    pub(crate) fn compile(&self, root: &Path, config: &Config, output_dir: &Path) -> Result<()> {
        for style in self.shared.iter().chain(self.pages.values()) {
            let css = match config.css.processor.unwrap_or_default() {
                CssProcessor::Plain => compile_plain(style, root, config),
                CssProcessor::Tailwind => {
                    compile_tailwind(style, self.shared.as_ref(), root, config)
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
    /// Returns an error for an invalid UTF-8 output path or a missing compiled stylesheet.
    pub(crate) fn page_url(
        &self,
        page: &Page,
        base_url: &str,
        manifest: &StaticAssetManifest,
    ) -> Result<Option<String>> {
        self.pages
            .get(&page.source_path)
            .map(|style| {
                let path =
                    path_to_url(&style.output).context("stylesheet output is not valid UTF-8")?;
                let hashed = manifest.asset_url(&path)?;
                let base = page_url(base_url, Path::new("index.html"));
                Ok(resolve_relative_url(hashed.trim_start_matches('/'), &base))
            })
            .transpose()
    }
}

fn compile_plain(style: &Stylesheet, root: &Path, config: &Config) -> Result<String> {
    let provider = CssProvider(FileProvider::new());
    let mut bundler = Bundler::new(&provider, None, ParserOptions::default());
    let stylesheet = bundler
        .bundle(&style.source)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    publish_urls(&stylesheet, style, root, config)
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

fn compile_tailwind(
    style: &Stylesheet,
    shared: Option<&Stylesheet>,
    root: &Path,
    config: &Config,
) -> Result<String> {
    let temp = tempfile::tempdir().context("failed to create CSS compiler workspace")?;
    let workspace = temp.path().canonicalize()?;
    let input = workspace.join("input.css");
    let output = workspace.join("output.css");
    let mut source = String::new();
    if style.bundle.is_some()
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
        .arg(&output)
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
    let compiled = fs::read_to_string(&output).context("failed to read Tailwind CSS output")?;
    let stylesheet = StyleSheet::parse(
        &compiled,
        ParserOptions {
            filename: output.to_string_lossy().into_owned(),
            ..ParserOptions::default()
        },
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    publish_urls(&stylesheet, style, root, config)
}

fn publish_urls(
    stylesheet: &StyleSheet<'_>,
    style: &Stylesheet,
    root: &Path,
    config: &Config,
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
                let published = published_url(&url.url, &source, style, root, config)?;
                (url.placeholder, published)
            }
        };
        css = css.replace(&css_string(&placeholder)?, &css_string(&url)?);
    }
    Ok(css)
}

fn published_url(
    url: &str,
    source: &Path,
    style: &Stylesheet,
    root: &Path,
    config: &Config,
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
    let asset = published_source(&referenced)
        .with_context(|| format!("cannot resolve CSS asset {url} from {}", source.display()))?;
    if !asset.is_file() {
        bail!(
            "CSS asset {url} from {} does not reference a published file",
            source.display()
        );
    }
    let mut roots = Vec::new();
    if let Some(bundle) = &style.bundle {
        let output = style
            .output
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .context("page stylesheet output has no bundle")?;
        roots.push((bundle.clone(), output.to_owned(), true));
    }
    roots.push((root.join("static"), PathBuf::new(), false));
    if let Some(theme) = config.theme_dir(root) {
        roots.push((theme.join("static"), PathBuf::new(), false));
    }
    for (source_root, output_root, is_bundle) in roots {
        if !source_root.is_dir() {
            continue;
        }
        if let Some(relative) = published_relative(
            &asset,
            &source_root,
            is_bundle.then_some(&style.bundle_assets),
        )? {
            let destination = output_root.join(relative);
            let relative = pathdiff::diff_paths(
                destination,
                style.output.parent().context("CSS output has no parent")?,
            )
            .context("cannot resolve published CSS asset path")?;
            let path = relative
                .to_str()
                .context("published CSS asset path is not valid UTF-8")?
                .replace('\\', "/");
            let encoded = utf8_percent_encode(&path, URL_PATH_ENCODE_SET);
            return Ok(format!("{encoded}{suffix}"));
        }
    }
    bail!(
        "CSS asset {url} from {} is outside published static files and the owning page bundle",
        source.display()
    )
}

fn published_relative(
    asset: &Path,
    source_root: &Path,
    bundle_assets: Option<&BTreeSet<PathBuf>>,
) -> Result<Option<PathBuf>> {
    let relative = [published_source(source_root)?, source_root.canonicalize()?]
        .iter()
        .find_map(|base| asset.strip_prefix(base).ok())
        .map(Path::to_owned);
    if let Some(relative) = &relative
        && bundle_assets.is_none_or(|assets| assets.contains(relative))
    {
        return Ok(Some(relative.clone()));
    }

    // Tailwind resolves imported stylesheets through symlinks before rebasing their asset URLs.
    let canonical_asset = asset.canonicalize()?;
    if let Some(assets) = bundle_assets {
        for relative in assets {
            if source_root.join(relative).canonicalize()? == canonical_asset {
                return Ok(Some(relative.clone()));
            }
        }
    } else {
        for entry in WalkDir::new(source_root)
            .follow_links(true)
            .sort_by_file_name()
        {
            let entry = entry?;
            if entry.path_is_symlink()
                && let Ok(relative) = canonical_asset.strip_prefix(entry.path().canonicalize()?)
            {
                return Ok(Some(entry.path().strip_prefix(source_root)?.join(relative)));
            }
        }
    }
    if relative.is_some() {
        bail!(
            "CSS asset {} is not a published bundle asset",
            asset.display()
        );
    }
    Ok(None)
}

fn published_source(path: &Path) -> Result<PathBuf> {
    // Canonicalization would erase the names under which symlinked assets are published.
    let mut normalized = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        if component == Component::ParentDir {
            normalized.pop();
        } else {
            normalized.push(component);
        }
    }
    Ok(normalized)
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
            output: PathBuf::from("css/style.css"),
            bundle: None,
            bundle_assets: BTreeSet::new(),
        };
        let css = compile_plain(&style, root.path(), &Config::default()).unwrap();
        for url in [
            "https://example.com/base.css",
            "/images/root.svg",
            "data:image/svg+xml,<svg/>",
            "#filter",
        ] {
            assert!(css.contains(url), "{css}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn compile_plain_preserves_static_symlink_paths() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(
            root.path(),
            ENTRY,
            indoc! {r"
                .icon { background: url(../../static/alias.svg); }
                .nested { background: url(../../static/shared/image.svg); }
                .parent { background: url(../../static/shared/../alias.svg); }
            "},
        );
        write_test_file(root.path(), "original.svg", "image");
        fs::create_dir(root.path().join("static")).unwrap();
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
            output: PathBuf::from("css/style.css"),
            bundle: None,
            bundle_assets: BTreeSet::new(),
        };
        let css = compile_plain(&style, root.path(), &Config::default()).unwrap();
        assert!(css.contains("../alias.svg"), "{css}");
        assert!(css.contains("../shared/image.svg"), "{css}");
        assert!(!css.contains("shared/../"), "{css}");
        assert!(!css.contains("original.svg"), "{css}");
    }

    // ── published_url ──

    #[test]
    fn published_url_private_and_unpublished_assets_returns_error() {
        let root = tempfile::tempdir().unwrap();
        write_test_file(root.path(), "content/example/_assets/css/style.css", "");
        write_test_file(root.path(), "content/example/_secret.svg", "private");
        write_test_file(root.path(), "unpublished.svg", "unpublished");
        let style = Stylesheet {
            source: root.path().join("content/example/_assets/css/style.css"),
            output: PathBuf::from("example/assets/css/style.css"),
            bundle: Some(root.path().join("content/example")),
            bundle_assets: BTreeSet::new(),
        };
        let config = Config::default();
        let private = published_url(
            "../../_secret.svg",
            &style.source,
            &style,
            root.path(),
            &config,
        )
        .unwrap_err();
        assert!(
            private.to_string().contains("not a published bundle asset"),
            "{private}"
        );
        let unpublished = published_url(
            "../../../../unpublished.svg",
            &style.source,
            &style,
            root.path(),
            &config,
        )
        .unwrap_err();
        assert!(
            unpublished.to_string().contains("outside published"),
            "{unpublished}"
        );

        write_test_file(root.path(), "content/_assets/css/style.css", "");
        write_test_file(root.path(), "content/_secret.svg", "private");
        let root_style = Stylesheet {
            source: root.path().join("content/_assets/css/style.css"),
            output: PathBuf::from("assets/css/style.css"),
            bundle: Some(root.path().join("content")),
            bundle_assets: BTreeSet::new(),
        };
        let private = published_url(
            "../../_secret.svg",
            &root_style.source,
            &root_style,
            root.path(),
            &config,
        )
        .unwrap_err();
        assert!(
            private.to_string().contains("not a published bundle asset"),
            "{private}"
        );

        write_test_file(root.path(), "content/index.md", "Markdown source");
        for url in ["../../index.md", "../.."] {
            let error = published_url(url, &root_style.source, &root_style, root.path(), &config)
                .unwrap_err();
            assert!(error.to_string().contains("published"), "{error}");
        }
    }
}
