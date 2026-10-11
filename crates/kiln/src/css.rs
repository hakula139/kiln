use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;

use anyhow::{Context, Result, bail};
use lightningcss::bundler::{Bundler, FileProvider, ResolveResult, SourceProvider};
use lightningcss::dependencies::{Dependency, DependencyOptions};
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::traits::ToCss;
use lightningcss::values::string::CSSString;
use percent_encoding::percent_decode_str;
use tempfile::TempDir;

use crate::config::{Config, CssProcessor};
use crate::content::page::Page;
use crate::output::write_output;
use crate::static_assets::StaticAssetManifest;
use crate::static_assets::publication::PublishedAssets;
use crate::url::{join_site_url, path_url};

pub(crate) const ENTRY: &str = "assets/css/_src/style.css";

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
    pub(crate) fn discover(root: &Path, config: &Config, pages: &[Page]) -> Self {
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
            let Some(bundle) = page.bundle_dir() else {
                continue;
            };
            let source = bundle.join(ENTRY);
            if source.is_file() {
                styles.insert(
                    page.source_path.clone(),
                    Stylesheet {
                        source,
                        page: Some(page.source_path.clone()),
                        output: page.output_dir.join("assets/css/page.css"),
                    },
                );
            }
        }
        Self {
            shared,
            pages: styles,
        }
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
        manifest: &StaticAssetManifest,
        output_dir: &Path,
    ) -> Result<()> {
        let prefix = config.base_url.prefix();
        let styles: Vec<_> = self.shared.iter().chain(self.pages.values()).collect();
        let compiled: Vec<_> = match config.css.processor.unwrap_or_default() {
            CssProcessor::Plain => styles
                .iter()
                .map(|style| compile_plain(assets, manifest, prefix, style))
                .collect(),
            CssProcessor::Tailwind => run_in_order(
                &styles,
                thread::available_parallelism().unwrap_or(NonZeroUsize::MIN),
                |style| {
                    let run = TailwindRun::spawn(root, config, self.shared.as_ref(), style);
                    (*style, run)
                },
                |(style, run)| run?.finish(assets, manifest, prefix, style),
            ),
        };
        for (style, css) in styles.into_iter().zip(compiled) {
            let css =
                css.with_context(|| format!("failed to compile {}", style.source.display()))?;
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
                Ok(join_site_url(deployment_prefix, hashed))
            })
            .transpose()
    }
}

fn compile_plain(
    assets: &PublishedAssets,
    manifest: &StaticAssetManifest,
    prefix: &str,
    style: &Stylesheet,
) -> Result<String> {
    let provider = CssProvider(FileProvider::new());
    let mut bundler = Bundler::new(&provider, None, ParserOptions::default());
    let stylesheet = bundler
        .bundle(&style.source)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    publish_urls(assets, manifest, prefix, style, &stylesheet)
}

/// A running `kiln-tailwindcss` compilation and the workspace holding its input.
struct TailwindRun {
    _workspace: TempDir,
    input: PathBuf,
    child: Child,
}

impl TailwindRun {
    fn spawn(
        root: &Path,
        config: &Config,
        shared: Option<&Stylesheet>,
        style: &Stylesheet,
    ) -> Result<Self> {
        let workspace = tempfile::tempdir().context("failed to create CSS compiler workspace")?;
        let input = dunce::canonicalize(workspace.path())?.join("input.css");
        let mut source = String::new();
        if style.page.is_some()
            && let Some(shared) = shared
        {
            _ = writeln!(source, "@reference {};", tailwind_path(&shared.source)?);
        }
        _ = writeln!(source, "@import {};", tailwind_path(&style.source)?);
        for dir in std::iter::once(root.join("content"))
            .chain(std::iter::once(root.join("templates")))
            .chain(config.theme_dir(root).map(|dir| dir.join("templates")))
            .filter(|dir| dir.is_dir())
        {
            _ = writeln!(source, "@source {};", tailwind_path(&dir)?);
        }
        fs::write(&input, source)?;
        let binary = if cfg!(windows) {
            "kiln-tailwindcss.cmd"
        } else {
            "kiln-tailwindcss"
        };
        let child = Command::new(binary)
            .arg(&input)
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to run `kiln-tailwindcss`. Install kiln's CSS processor package or use kiln's Nix package")?;
        Ok(Self {
            _workspace: workspace,
            input,
            child,
        })
    }

    fn finish(
        self,
        assets: &PublishedAssets,
        manifest: &StaticAssetManifest,
        prefix: &str,
        style: &Stylesheet,
    ) -> Result<String> {
        let result = self
            .child
            .wait_with_output()
            .context("failed to wait for `kiln-tailwindcss`")?;
        if !result.status.success() {
            bail!(
                "Tailwind CSS exited with {}: {}",
                result.status,
                String::from_utf8_lossy(&result.stderr)
            );
        }
        let compiled =
            String::from_utf8(result.stdout).context("Tailwind CSS output is not UTF-8")?;
        let stylesheet = StyleSheet::parse(
            &compiled,
            ParserOptions {
                filename: self.input.to_string_lossy().into_owned(),
                ..ParserOptions::default()
            },
        )
        .map_err(|error| anyhow::anyhow!("{error}"))?;
        publish_urls(assets, manifest, prefix, style, &stylesheet)
    }
}

/// Starts items in order with at most `limit` running, and finishes every started item in the
/// same order.
fn run_in_order<T, R, O>(
    items: impl IntoIterator<Item = T>,
    limit: NonZeroUsize,
    mut start: impl FnMut(T) -> R,
    mut finish: impl FnMut(R) -> O,
) -> Vec<O> {
    let mut running = VecDeque::with_capacity(limit.get());
    let mut finished = Vec::new();
    for item in items {
        if running.len() == limit.get() {
            finished.extend(running.pop_front().map(&mut finish));
        }
        running.push_back(start(item));
    }
    finished.extend(running.into_iter().map(finish));
    finished
}

fn tailwind_path(path: &Path) -> Result<String> {
    let path = dunce::canonicalize(path)?;
    let path = path.to_string_lossy();
    #[cfg(windows)]
    let path = path.replace('\\', "/");
    css_string(&path)
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
    manifest: &StaticAssetManifest,
    prefix: &str,
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
                let published = published_url(assets, manifest, prefix, style, &source, &url.url)?;
                (url.placeholder, published)
            }
        };
        css = css.replace(&css_string(&placeholder)?, &css_string(&url)?);
    }
    Ok(css)
}

fn published_url(
    assets: &PublishedAssets,
    manifest: &StaticAssetManifest,
    prefix: &str,
    style: &Stylesheet,
    source: &Path,
    url: &str,
) -> Result<String> {
    if crate::static_assets::is_external(url) || url.starts_with('#') {
        return Ok(url.to_owned());
    }
    let split = url.find(['?', '#']).unwrap_or(url.len());
    let (path, suffix) = url.split_at(split);
    let destination_url = if path.starts_with('/') {
        let Some(destination) = manifest.resolve(path, "/", prefix) else {
            return Ok(url.to_owned());
        };
        destination[prefix.trim_end_matches('/').len()..].to_owned()
    } else {
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
        manifest
            .asset_url(&format!("/{}", path_url(destination)))?
            .to_owned()
    };
    let destination = percent_decode_str(destination_url.trim_start_matches('/')).decode_utf8()?;
    let relative = pathdiff::diff_paths(
        destination.as_ref(),
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
    use std::cell::RefCell;

    use indoc::indoc;

    use super::*;
    use crate::content::discovery::discover_content;
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
        let pages = discover_content(root.path()).unwrap().pages;
        let config = Config::default();
        let stylesheets = Stylesheets::discover(root.path(), &config, &pages);
        let output = root.path().join("public");
        let assets = PublishedAssets::publish(root.path(), None, &pages, &output).unwrap();
        stylesheets
            .compile(
                root.path(),
                &config,
                &assets,
                &StaticAssetManifest::default(),
                &output,
            )
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
        let assets =
            PublishedAssets::publish(root.path(), None, &[], &root.path().join("public")).unwrap();
        let manifest = StaticAssetManifest::default();
        let css = compile_plain(&assets, &manifest, "", &style).unwrap();
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
        let assets = PublishedAssets::publish(root.path(), None, &[], &output).unwrap();
        let style = Stylesheet {
            source: root.path().join(ENTRY),
            page: None,
            output: PathBuf::from("assets/css/site.css"),
        };
        let manifest = StaticAssetManifest::build_media(&root.path().join("public")).unwrap();
        let css = compile_plain(&assets, &manifest, "", &style).unwrap();
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
        let assets =
            PublishedAssets::publish(root.path(), None, &[], &root.path().join("public")).unwrap();
        let manifest = StaticAssetManifest::build_media(&root.path().join("public")).unwrap();
        let css = compile_plain(&assets, &manifest, "", &style).unwrap();
        assert!(css.contains("../../alias.svg"), "{css}");
        assert!(css.contains("../../shared/image.svg"), "{css}");
        assert!(!css.contains("shared/../"), "{css}");
        assert!(!css.contains("original.svg"), "{css}");
    }

    // ── run_in_order ──

    #[test]
    fn run_in_order_limits_running_items_and_preserves_order() {
        let events = RefCell::new(Vec::new());
        let finished = run_in_order(
            0..4,
            NonZeroUsize::new(2).unwrap(),
            |item| {
                events.borrow_mut().push(format!("start {item}"));
                item
            },
            |item| {
                events.borrow_mut().push(format!("finish {item}"));
                item * 10
            },
        );

        assert_eq!(finished, [0, 10, 20, 30]);
        assert_eq!(
            events.into_inner(),
            [
                "start 0", "start 1", "finish 0", "start 2", "finish 1", "start 3", "finish 2",
                "finish 3",
            ]
        );
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
        let assets =
            PublishedAssets::publish(root.path(), None, &[], &root.path().join("public")).unwrap();
        let private = published_url(
            &assets,
            &StaticAssetManifest::default(),
            "",
            &style,
            &style.source,
            "../../../_secret.svg",
        )
        .unwrap_err();
        assert!(private.to_string().contains("not a published"), "{private}");
        let unpublished = published_url(
            &assets,
            &StaticAssetManifest::default(),
            "",
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
            &StaticAssetManifest::default(),
            "",
            &root_style,
            &root_style.source,
            "../../../_secret.svg",
        )
        .unwrap_err();
        assert!(private.to_string().contains("not a published"), "{private}");

        write_test_file(root.path(), "content/index.md", "Markdown source");
        for url in ["../../../index.md", "../../.."] {
            let error = published_url(
                &assets,
                &StaticAssetManifest::default(),
                "",
                &root_style,
                &root_style.source,
                url,
            )
            .unwrap_err();
            assert!(error.to_string().contains("published"), "{error}");
        }
    }
}
