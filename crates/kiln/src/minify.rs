//! Output minification for HTML, CSS, and JS files.
//!
//! Parse failures are logged as warnings and leave the original file untouched, so unusual input
//! never fails a `--minify` build. Input nested deeper than about 50,000 levels still overflows a
//! worker stack and aborts the process.

use std::borrow::Cow;
use std::fmt;
use std::fs;
use std::ops::AddAssign;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use lightningcss::stylesheet::{
    MinifyOptions, ParserOptions, PrinterOptions, StyleAttribute, StyleSheet,
};
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use regex::{Captures, Regex};
use swc_common::sync::Lrc;
use swc_common::{FileName, GLOBALS, Globals, Mark, SourceMap};
use swc_ecma_ast::{EsVersion, Program};
use swc_ecma_codegen::text_writer::{JsWriter, omit_trailing_semi};
use swc_ecma_codegen::{Config as JsCodegenConfig, Emitter};
use swc_ecma_minifier::option::{CompressOptions, ExtraOptions, MinifyOptions as JsMinifyOptions};
use swc_ecma_parser::{Syntax, parse_file_as_module, parse_file_as_program};
use swc_ecma_transforms_base::fixer::{fixer, paren_remover};
use swc_ecma_transforms_base::resolver;
use swc_html_ast::{Child, Document, Element, Namespace};
use swc_html_codegen::writer::basic::{BasicHtmlWriter, BasicHtmlWriterConfig};
use swc_html_codegen::{CodeGenerator, CodegenConfig, Emit};
use swc_html_minifier::option::{
    CollapseWhitespaces, JsOptions, JsParserOptions, MinifyCssOption, MinifyJsOption,
    MinifyOptions as HtmlMinifyOptions,
};
use swc_html_minifier::{CssMinificationMode, MinifyCss, minify_document_with_custom_css_minifier};
use swc_html_parser::parse_file_as_document;
use swc_html_parser::parser::ParserConfig;
use walkdir::WalkDir;

/// swc recurses once per level of nested expressions or elements, so worker threads need far more
/// than rayon's default 2 MiB stack.
const WORKER_STACK_SIZE: usize = 64 << 20;

static RAW_TEXT_END_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)</(script|style)([\t\n\x0C\r />])")
        .expect("raw text end tag regex should compile")
});

/// Totals from a minification pass, suitable for printing as a build summary.
#[derive(Debug, Default)]
pub(crate) struct MinifyStats {
    /// Total files inspected (regardless of whether they shrank).
    pub files_processed: u64,
    /// Files whose minified output replaced the original on disk.
    pub files_shrunk: u64,
    /// Sum of original file sizes, in bytes.
    pub bytes_in: u64,
    /// Sum of on-disk sizes after the pass, in bytes.
    pub bytes_out: u64,
}

impl AddAssign for MinifyStats {
    fn add_assign(&mut self, rhs: Self) {
        self.files_processed += rhs.files_processed;
        self.files_shrunk += rhs.files_shrunk;
        self.bytes_in += rhs.bytes_in;
        self.bytes_out += rhs.bytes_out;
    }
}

impl fmt::Display for MinifyStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.files_processed == 0 {
            return f.write_str("minified 0 files");
        }
        let saved = self.bytes_in.saturating_sub(self.bytes_out);
        let pct = if self.bytes_in == 0 {
            0.0
        } else {
            u64_to_f64(saved) / u64_to_f64(self.bytes_in) * 100.0
        };
        let noun = if self.files_processed == 1 {
            "file"
        } else {
            "files"
        };
        write!(
            f,
            "minified {} {noun}, {} shrunk ({} → {}, -{pct:.1}%)",
            self.files_processed,
            self.files_shrunk,
            format_bytes(self.bytes_in),
            format_bytes(self.bytes_out),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetKind {
    Html,
    Css,
    Script,
    Module,
}

impl AssetKind {
    /// Whether the kind is JavaScript, either a classic script or an ES module.
    pub(crate) fn is_script(self) -> bool {
        matches!(self, Self::Script | Self::Module)
    }
}

/// Minifies CSS and JS files under `output_dir` in place.
pub(crate) fn minify_static_assets(output_dir: &Path) -> Result<MinifyStats> {
    minify_matching(output_dir, |kind| kind != AssetKind::Html)
}

/// Minifies HTML files under `output_dir` in place.
pub(crate) fn minify_html(output_dir: &Path) -> Result<MinifyStats> {
    minify_matching(output_dir, |kind| kind == AssetKind::Html)
}

fn minify_matching(output_dir: &Path, include: impl Fn(AssetKind) -> bool) -> Result<MinifyStats> {
    let mut files = Vec::new();
    for entry in WalkDir::new(output_dir).follow_links(false) {
        let entry = entry.with_context(|| format!("failed to walk {}", output_dir.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(kind) = classify(entry.path()).filter(|&kind| include(kind)) else {
            continue;
        };
        files.push((entry.into_path(), kind));
    }

    let pool = ThreadPoolBuilder::new()
        .stack_size(WORKER_STACK_SIZE)
        .build()
        .context("failed to start minification threads")?;
    let results: Vec<Result<MinifyStats>> = pool.install(|| {
        files
            .par_iter()
            .map(|(path, kind)| {
                minify_file(path, *kind)
                    .with_context(|| format!("failed to process {}", path.display()))
            })
            .collect()
    });
    results
        .into_iter()
        .try_fold(MinifyStats::default(), |mut total, stats| {
            total += stats?;
            Ok(total)
        })
}

/// Returns the minifier to use for `path`, or `None` if the file should be skipped.
///
/// Matching is case-insensitive so files with uppercase extensions (e.g., `INDEX.HTML` from a
/// migrated Hugo / legacy site) are still handled.
fn classify(path: &Path) -> Option<AssetKind> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name.ends_with(".min.css") || name.ends_with(".min.js") {
        return None;
    }
    asset_kind(path)
}

/// Returns the kind of an HTML, CSS or JavaScript file from its extension, case-insensitively.
pub(crate) fn asset_kind(path: &Path) -> Option<AssetKind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "html" | "htm" => Some(AssetKind::Html),
        "css" => Some(AssetKind::Css),
        "js" => Some(AssetKind::Script),
        "mjs" => Some(AssetKind::Module),
        _ => None,
    }
}

fn minify_file(path: &Path, kind: AssetKind) -> Result<MinifyStats> {
    let input = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let bytes_in = input.len() as u64;

    let output = match kind {
        AssetKind::Html => minify_html_bytes(&input, path),
        AssetKind::Css => minify_css_bytes(&input, path),
        AssetKind::Script => minify_js_bytes(&input, path, false),
        AssetKind::Module => minify_js_bytes(&input, path, true),
    };

    let mut stats = MinifyStats {
        files_processed: 1,
        files_shrunk: 0,
        bytes_in,
        bytes_out: bytes_in,
    };
    if let Some(bytes) = output.filter(|bytes| (bytes.len() as u64) < bytes_in) {
        fs::write(path, &bytes).with_context(|| format!("failed to write {}", path.display()))?;
        stats.files_shrunk = 1;
        stats.bytes_out = bytes.len() as u64;
    }
    Ok(stats)
}

/// Decodes UTF-8 input, or warns and returns `None`.
fn decode_utf8<'a>(input: &'a [u8], path: &Path, kind: &str) -> Option<&'a str> {
    std::str::from_utf8(input)
        .inspect_err(|e| {
            tracing::warn!("skipping {} ({kind}): invalid UTF-8: {e}", path.display());
        })
        .ok()
}

fn minify_html_bytes(input: &[u8], path: &Path) -> Option<Vec<u8>> {
    let source = decode_utf8(input, path, "HTML")?;
    // Inline script minification allocates swc `Mark`s, which require a scoped `GLOBALS`.
    GLOBALS.set(&Globals::new(), || {
        let cm = Lrc::new(SourceMap::default());
        let fm = cm.new_source_file(Lrc::new(FileName::Anon), source.to_owned());
        let parser_config = ParserConfig {
            scripting_enabled: true,
            ..ParserConfig::default()
        };
        let mut document = parse_file_as_document(&fm, parser_config, &mut Vec::new())
            .inspect_err(|e| {
                tracing::warn!(
                    "skipping {} (HTML parse failed): {}",
                    path.display(),
                    e.message()
                );
            })
            .ok()?;

        let options = HtmlMinifyOptions {
            collapse_whitespaces: CollapseWhitespaces::Smart,
            minify_js: MinifyJsOption::Options(Box::new(JsOptions {
                parser: JsParserOptions {
                    target: EsVersion::latest(),
                    ..JsParserOptions::default()
                },
                // swc_html_minifier sets the module flag per inline script.
                minifier: js_minify_options(false),
                codegen: JsCodegenConfig::default(),
            })),
            minify_css: MinifyCssOption::Bool(true),
            ..HtmlMinifyOptions::default()
        };
        minify_document_with_custom_css_minifier(&mut document, &options, &LightningCss);
        escape_raw_text_end_tags(&mut document);

        let mut output = String::new();
        let writer = BasicHtmlWriter::new(&mut output, None, BasicHtmlWriterConfig::default());
        let mut generator = CodeGenerator::new(
            writer,
            CodegenConfig {
                minify: true,
                scripting_enabled: true,
                ..CodegenConfig::default()
            },
        );
        generator.emit(&document).ok()?;
        Some(output.into_bytes())
    })
}

/// Minifies inline `<style>` elements and `style` attributes with lightningcss.
struct LightningCss;

impl MinifyCss for LightningCss {
    type Options = ();

    fn minify_css(
        &self,
        _options: &MinifyCssOption<()>,
        data: String,
        mode: CssMinificationMode,
    ) -> Option<String> {
        match mode {
            CssMinificationMode::Stylesheet => {
                print_minified_stylesheet(StyleSheet::parse(&data, ParserOptions::default()).ok()?)
            }
            CssMinificationMode::ListOfDeclarations => {
                let mut attribute = StyleAttribute::parse(&data, ParserOptions::default()).ok()?;
                attribute.minify(MinifyOptions::default());
                Some(attribute.to_css(minified_printer()).ok()?.code)
            }
            CssMinificationMode::MediaQueryList => None,
        }
    }
}

/// Escapes text that would close its `<script>` or `<style>` element by writing `</` as `<\/`.
///
/// swc and lightningcss unescape such sequences inside string literals (for example
/// `"<\/SCRIPT>"` or `"\3c /style>"`), which would otherwise end the element early. Like the HTML
/// tokenizer, matching ignores case and requires whitespace, `/` or `>` after the tag name. `\/`
/// reads as `/` in both JS and CSS strings.
fn escape_raw_text_end_tags(document: &mut Document) {
    let mut pending = vec![&mut document.children];
    while let Some(children) = pending.pop() {
        for child in children {
            let Child::Element(element) = child else {
                continue;
            };
            let tag_name = &*element.tag_name;
            if has_minified_raw_text(element) {
                for child in &mut element.children {
                    if let Child::Text(text) = child
                        && let Cow::Owned(escaped) =
                            RAW_TEXT_END_TAG_RE.replace_all(&text.data, |caps: &Captures| {
                                if caps[1].eq_ignore_ascii_case(tag_name) {
                                    format!(r"<\/{}{}", &caps[1], &caps[2])
                                } else {
                                    caps[0].to_owned()
                                }
                            })
                    {
                        text.data = escaped.into();
                    }
                }
            }
            pending.push(&mut element.children);
            if let Some(content) = &mut element.content {
                pending.push(&mut content.children);
            }
        }
    }
}

/// Mirrors `swc_html_minifier`'s choice of `<script>` and `<style>` elements whose text it rewrites.
///
/// Text left untouched is exactly what the author wrote, which already parsed as one element.
fn has_minified_raw_text(element: &Element) -> bool {
    if !matches!(element.namespace, Namespace::HTML | Namespace::SVG) {
        return false;
    }
    let type_value = element
        .attributes
        .iter()
        .find(|attribute| attribute.name == "type")
        .and_then(|attribute| attribute.value.as_deref())
        .map(|value| value.trim().to_ascii_lowercase());

    match &*element.tag_name {
        "script" => {
            !element
                .attributes
                .iter()
                .any(|attribute| attribute.name == "src")
                && type_value.as_deref().is_none_or(|value| {
                    matches!(
                        value,
                        "module"
                            | "application/json"
                            | "application/ld+json"
                            | "importmap"
                            | "speculationrules"
                    ) || is_javascript_mime_type(value)
                })
        }
        "style" => type_value.is_none_or(|value| value == "text/css"),
        _ => false,
    }
}

fn is_javascript_mime_type(value: &str) -> bool {
    matches!(
        value.split(';').next().unwrap_or(value),
        "application/javascript"
            | "application/ecmascript"
            | "application/x-ecmascript"
            | "application/x-javascript"
            | "text/ecmascript"
            | "text/javascript"
            | "text/javascript1.0"
            | "text/javascript1.1"
            | "text/javascript1.2"
            | "text/javascript1.3"
            | "text/javascript1.4"
            | "text/javascript1.5"
            | "text/jscript"
            | "text/livescript"
            | "text/x-ecmascript"
            | "text/x-javascript"
    )
}

fn minify_css_bytes(input: &[u8], path: &Path) -> Option<Vec<u8>> {
    let source = decode_utf8(input, path, "CSS")?;
    let stylesheet = StyleSheet::parse(source, ParserOptions::default())
        .inspect_err(|e| {
            tracing::warn!("skipping {} (CSS parse failed): {e}", path.display());
        })
        .ok()?;
    print_minified_stylesheet(stylesheet).map(String::into_bytes)
}

fn print_minified_stylesheet(mut stylesheet: StyleSheet) -> Option<String> {
    // lightningcss's `minify` and `to_css` don't fail in practice with default options, so
    // silently keep the original if they ever do.
    stylesheet.minify(MinifyOptions::default()).ok()?;
    Some(stylesheet.to_css(minified_printer()).ok()?.code)
}

fn minified_printer<'a>() -> PrinterOptions<'a> {
    PrinterOptions {
        minify: true,
        ..PrinterOptions::default()
    }
}

fn minify_js_bytes(input: &[u8], path: &Path, is_module: bool) -> Option<Vec<u8>> {
    let source = decode_utf8(input, path, "JS")?;

    GLOBALS.set(&Globals::new(), || {
        let cm = Lrc::new(SourceMap::default());
        let program = parse_js(&cm, source, is_module)
            .map_err(|message| {
                tracing::warn!("skipping {} (JS parse failed): {message}", path.display());
            })
            .ok()?;
        let is_module = matches!(program, Program::Module(_));
        let target = EsVersion::latest();

        let unresolved_mark = Mark::new();
        let top_level_mark = Mark::new();
        let program = swc_ecma_minifier::optimize(
            program
                .apply(resolver(unresolved_mark, top_level_mark, false))
                .apply(paren_remover(None)),
            cm.clone(),
            None,
            None,
            &js_minify_options(is_module),
            &ExtraOptions {
                unresolved_mark,
                top_level_mark,
                mangle_name_cache: None,
            },
        )
        .apply(fixer(None));

        let mut output = Vec::new();
        Emitter {
            cfg: JsCodegenConfig::default()
                .with_minify(true)
                .with_target(target),
            cm: cm.clone(),
            comments: None,
            wr: omit_trailing_semi(JsWriter::new(cm, "\n", &mut output, None)),
        }
        .emit_program(&program)
        .ok()?;
        Some(output)
    })
}

/// Parses `is_module` sources as ES modules and detects module syntax in the rest, returning the
/// first parse error's message on failure.
pub(crate) fn parse_js(
    cm: &Lrc<SourceMap>,
    source: &str,
    is_module: bool,
) -> Result<Program, Cow<'static, str>> {
    let fm = cm.new_source_file(Lrc::new(FileName::Anon), source.to_owned());
    let target = EsVersion::latest();
    let mut errors = Vec::new();
    let parsed = if is_module {
        parse_file_as_module(&fm, Syntax::default(), target, None, &mut errors).map(Program::Module)
    } else {
        parse_file_as_program(&fm, Syntax::default(), target, None, &mut errors)
    };
    match (parsed, errors.into_iter().next()) {
        (Ok(program), None) => Ok(program),
        (Err(error), _) | (Ok(_), Some(error)) => Err(error.kind().msg()),
    }
}

fn js_minify_options(module: bool) -> JsMinifyOptions {
    JsMinifyOptions {
        compress: Some(CompressOptions {
            ecma: EsVersion::latest(),
            module,
            ..CompressOptions::default()
        }),
        ..JsMinifyOptions::default()
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let b = u64_to_f64(bytes);
    if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "byte sizes in a build pass stay far below 2^52, where f64 starts losing integer precision"
)]
fn u64_to_f64(value: u64) -> f64 {
    value as f64
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use indoc::indoc;

    use super::*;

    // ── Display for MinifyStats ──

    #[test]
    fn display_reports_counts_and_savings() {
        let stats = MinifyStats {
            files_processed: 4,
            files_shrunk: 3,
            bytes_in: 2048,
            bytes_out: 512,
        };
        assert_eq!(
            format!("{stats}"),
            "minified 4 files, 3 shrunk (2.0 KB → 512 B, -75.0%)",
        );
    }

    #[test]
    fn display_for_empty_pass_shows_zero() {
        let stats = MinifyStats::default();
        assert_eq!(format!("{stats}"), "minified 0 files");
    }

    #[test]
    fn display_reports_zero_percent_when_no_input_bytes() {
        // Empty files get counted in `files_processed` but contribute zero bytes, so the percent
        // calculation must short-circuit to avoid dividing by zero.
        let stats = MinifyStats {
            files_processed: 1,
            files_shrunk: 0,
            bytes_in: 0,
            bytes_out: 0,
        };
        assert_eq!(
            format!("{stats}"),
            "minified 1 file, 0 shrunk (0 B → 0 B, -0.0%)"
        );
    }

    // ── minify_static_assets ──

    #[test]
    fn skips_html() {
        let dir = tempfile::tempdir().unwrap();
        let html = "<html>  <body>  text  </body>  </html>";
        let css = ".foo { color: #ff0000; margin: 0px; }\n";
        fs::write(dir.path().join("index.html"), html).unwrap();
        fs::write(dir.path().join("style.css"), css).unwrap();

        let stats = minify_static_assets(dir.path()).unwrap();

        assert_eq!(stats.files_processed, 1);
        assert_eq!(
            fs::read_to_string(dir.path().join("index.html")).unwrap(),
            html
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("style.css")).unwrap(),
            ".foo{color:red;margin:0}"
        );
    }

    // ── minify_html ──

    #[test]
    fn skips_css_and_js() {
        let dir = tempfile::tempdir().unwrap();
        let html = "<html>  <body>  text  </body>  </html>";
        let css = ".foo { color: #ff0000; margin: 0px; }\n";
        fs::write(dir.path().join("index.html"), html).unwrap();
        fs::write(dir.path().join("style.css"), css).unwrap();

        let stats = minify_html(dir.path()).unwrap();

        assert_eq!(stats.files_processed, 1);
        assert_eq!(
            fs::read_to_string(dir.path().join("index.html")).unwrap(),
            "text"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("style.css")).unwrap(),
            css
        );
    }

    // ── minify_matching ──

    #[test]
    fn processes_mixed_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let html = indoc! {r"
            <!DOCTYPE html>
            <html>
              <body>
                <p>Hello     world</p>
              </body>
            </html>
        "};
        let css = ".foo { color: #ff0000; margin: 0px; }\n";
        let js = indoc! {"
            const x = 1 + 2;
            console.log(x);
        "};
        let module = indoc! {"
            { function blockScoped() { return 2; } }
            console.log(typeof blockScoped);
        "};
        let png = b"\x89PNG\r\n\x1a\n"; // binary, should be ignored
        let already_min = b"a{color:red}"; // should be left alone

        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("page.html"), html).unwrap();
        fs::write(root.join("style.css"), css).unwrap();
        fs::write(root.join("sub").join("app.js"), js).unwrap();
        fs::write(root.join("sub").join("module.mjs"), module).unwrap();
        fs::write(root.join("image.png"), png).unwrap();
        fs::write(root.join("vendor.min.css"), already_min).unwrap();

        let stats = minify_matching(root, |_| true).unwrap();
        assert_eq!(
            stats.files_processed, 4,
            "should process html / css / js / mjs"
        );
        assert_eq!(
            stats.files_shrunk, 4,
            "all four test inputs should actually shrink",
        );
        assert_eq!(
            stats.bytes_in,
            (html.len() + css.len() + js.len() + module.len()) as u64
        );
        let bytes_out = ["page.html", "style.css", "sub/app.js", "sub/module.mjs"]
            .into_iter()
            .map(|path| fs::metadata(root.join(path)).unwrap().len())
            .sum::<u64>();
        assert_eq!(stats.bytes_out, bytes_out);

        assert_eq!(fs::read(root.join("image.png")).unwrap(), png);
        assert_eq!(fs::read(root.join("vendor.min.css")).unwrap(), already_min);

        assert_eq!(
            fs::read_to_string(root.join("page.html")).unwrap(),
            "<!doctype html><p>Hello world"
        );
        assert_eq!(
            fs::read_to_string(root.join("style.css")).unwrap(),
            ".foo{color:red;margin:0}"
        );
        assert_eq!(
            fs::read_to_string(root.join("sub/app.js")).unwrap(),
            "const x=3;console.log(3);"
        );
        assert_eq!(
            fs::read_to_string(root.join("sub/module.mjs")).unwrap(),
            "console.log(typeof blockScoped);"
        );
    }

    #[test]
    fn handles_deeply_nested_input() {
        let dir = tempfile::tempdir().unwrap();
        let chain = format!(
            "var a = 1;\nconsole.log({});\n",
            vec!["a"; 3_000].join(" + ")
        );
        let spans = format!(
            "<p>{}x{}</p>",
            "<span>".repeat(2_000),
            "</span>".repeat(2_000)
        );
        fs::write(dir.path().join("chain.js"), &chain).unwrap();
        fs::write(dir.path().join("spans.html"), &spans).unwrap();

        let stats = minify_matching(dir.path(), |_| true).unwrap();

        assert_eq!(stats.files_shrunk, 2);
        assert_eq!(
            fs::read_to_string(dir.path().join("chain.js")).unwrap(),
            format!("var a=1;console.log({});", vec!["a"; 3_000].join("+"))
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("spans.html")).unwrap(),
            format!("<p>{}x{}", "<span>".repeat(2_000), "</span>".repeat(2_000))
        );
    }

    #[test]
    fn preserves_smaller_original() {
        let dir = tempfile::tempdir().unwrap();
        let original = "console.log(1)";
        fs::write(dir.path().join("app.js"), original).unwrap();

        let stats = minify_matching(dir.path(), |_| true).unwrap();

        assert_eq!(stats.files_processed, 1);
        assert_eq!(stats.files_shrunk, 0);
        assert_eq!(stats.bytes_in, original.len() as u64);
        assert_eq!(stats.bytes_out, original.len() as u64);
        assert_eq!(
            fs::read_to_string(dir.path().join("app.js")).unwrap(),
            original
        );
    }

    #[test]
    fn tolerates_broken_asset() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let good_css = ".foo { color: #ff0000; }\n";
        let broken_js = b"function () { ]]]";

        fs::write(root.join("style.css"), good_css).unwrap();
        fs::write(root.join("broken.js"), broken_js).unwrap();

        let stats = minify_matching(root, |_| true).unwrap();
        assert_eq!(stats.files_processed, 2);
        assert_eq!(stats.files_shrunk, 1, "only CSS should shrink");
        assert_eq!(stats.bytes_in, (good_css.len() + broken_js.len()) as u64);
        assert_eq!(
            stats.bytes_out,
            fs::metadata(root.join("style.css")).unwrap().len()
                + fs::metadata(root.join("broken.js")).unwrap().len()
        );
        assert_eq!(fs::read(root.join("broken.js")).unwrap(), broken_js);
    }

    #[test]
    fn empty_directory_yields_zero_stats() {
        let dir = tempfile::tempdir().unwrap();
        let stats = minify_matching(dir.path(), |_| true).unwrap();
        assert_eq!(stats.files_processed, 0);
        assert_eq!(stats.files_shrunk, 0);
        assert_eq!(stats.bytes_in, 0);
        assert_eq!(stats.bytes_out, 0);
    }

    // ── classify ──

    #[test]
    fn maps_extensions_case_insensitively() {
        for (path, expected) in [
            ("a/index.html", Some(AssetKind::Html)),
            ("a/page.htm", Some(AssetKind::Html)),
            ("a/INDEX.HTML", Some(AssetKind::Html)),
            ("a/style.css", Some(AssetKind::Css)),
            ("a/Style.CSS", Some(AssetKind::Css)),
            ("a/app.js", Some(AssetKind::Script)),
            ("a/App.JS", Some(AssetKind::Script)),
            ("a/app.mjs", Some(AssetKind::Module)),
            ("a/App.MJS", Some(AssetKind::Module)),
            ("a/vendor.min.css", None),
            ("a/vendor.min.js", None),
            ("a/vendor.MIN.JS", None),
            ("a/image.png", None),
            ("a/data.json", None),
            ("a/README", None),
        ] {
            assert_eq!(classify(Path::new(path)), expected, "{path}");
        }
    }

    // ── minify_html_bytes ──

    #[test]
    fn strips_whitespace_and_comments() {
        let input = indoc! {r"
            <!DOCTYPE html>
            <html>
              <head>  <title>Hi</title>  </head>
              <body>
                <!-- a comment -->
                <!--[if IE]><p>Legacy</p><![endif]-->
                <p>Hello    world</p>
              </body>
            </html>
        "};
        let output = minify_html_bytes(input.as_bytes(), Path::new("page.html")).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "<!doctype html><title>Hi</title><body><!--[if IE]><p>Legacy</p><![endif]--><p>Hello world"
        );
    }

    #[test]
    fn keeps_significant_whitespace() {
        let input = indoc! {r#"
            <div>
              <a href="/a">One</a>
              <a href="/b">Two</a>
              <em>three</em>
            </div>
            <pre>  indented
                code  </pre>
            <textarea>  keep   spaces </textarea>
        "#};
        let output = minify_html_bytes(input.as_bytes(), Path::new("page.html")).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "<div><a href=/a>One</a> <a href=/b>Two</a> <em>three</em></div><pre>\n  indented\n    code  </pre><textarea>  keep   spaces </textarea>"
        );
    }

    #[test]
    fn minifies_inline_scripts_by_type() {
        let input = indoc! {r#"
            <script type="application/ld+json">
              { "@type": "Article", "headline": "A </b> B" }
            </script>
            <script>
              // dropped comment
              var greeting = "hello <\/script>";
              window.total = 1 + 2;
            </script>
            <script type="text/template"><p>  kept   as-is  </p></script>
        "#};
        let output = minify_html_bytes(input.as_bytes(), Path::new("page.html")).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                r#"<script type=application/ld+json>{"@type":"Article","headline":"A \u003C/b> B"}</script>"#,
                r#"<script>var greeting="hello <\/script>";window.total=3</script>"#,
                "<script type=text/template><p>  kept   as-is  </p></script>",
            )
        );
    }

    #[test]
    fn minifies_inline_css_with_lightningcss() {
        let input = indoc! {r#"
            <style>
              .a { color: #ff0000; margin: 0px 0px; }
            </style>
            <link rel="stylesheet" href="/print.css" media="print  and  (min-width: 640px)">
            <p style="color: #ff0000;  font-weight: bold">Text</p>
        "#};
        let output = minify_html_bytes(input.as_bytes(), Path::new("page.html")).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                "<style>.a{color:red;margin:0}</style>",
                r#"<link rel=stylesheet href=/print.css media="print  and  (min-width: 640px)">"#,
                "<p style=color:red;font-weight:700>Text",
            )
        );
    }

    #[test]
    fn escapes_only_closing_raw_text_end_tags() {
        let input = indoc! {r#"
            <style>
              .a::before { content: "\3c /style>"; }
              .b::after { content: "\3c /STYLE>"; }
              .c::after { content: "</script>"; }
            </style>
            <template>
              <script>
                var upper = "<\/SCRIPT>";
                var spaced = "<\/script >";
                var longer = String.raw`</scripts>`;
              </script>
            </template>
            <script type="text/template"><style>.a{}</style><p>x</p></script>
            <script type="text/plain">a</scripts>b</script>
            <script type="text/javascript; charset=utf-8">var legacy = "<\/script>";</script>
            <style type="TEXT/CSS">.d::after { content: "\3c /style>"; }</style>
            <script type="text/x"><!--<script></script>--></script>
        "#};
        let output = minify_html_bytes(input.as_bytes(), Path::new("page.html")).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                r#"<style>.a:before{content:"<\/style>"}.b:after{content:"<\/STYLE>"}"#,
                r#".c:after{content:"</script>"}</style>"#,
                r#"<template> <script>var upper="<\/SCRIPT>",spaced="<\/script >","#,
                "longer=String.raw`</scripts>`</script></template>",
                "<script type=text/template><style>.a{}</style><p>x</p></script>",
                "<script type=text/plain>a</scripts>b</script>",
                r#"<script>var legacy="<\/script>"</script>"#,
                r#"<style>.d:after{content:"<\/style>"}</style>"#,
                "<script type=text/x><!--<script></script>--></script>",
            )
        );
    }

    #[test]
    fn skips_invalid_utf8_html() {
        assert_eq!(
            minify_html_bytes(&[0xff, 0xfe, 0xfd], Path::new("broken.html")),
            None
        );
    }

    // ── has_minified_raw_text ──

    #[test]
    fn mirrors_swc_raw_text_selection() {
        let cases = [
            ("<!doctype html><script></script>", true),
            (r#"<script type=" Module "></script>"#, true),
            (
                r#"<script type="text/javascript; charset=utf-8"></script>"#,
                true,
            ),
            (r#"<script type="application/ld+json"></script>"#, true),
            (r#"<script type="text/template"></script>"#, false),
            (r#"<script src="/app.js"></script>"#, false),
            ("<svg><style></style></svg>", true),
            (r#"<style type="TEXT/CSS"></style>"#, true),
            (r#"<style type="text/less"></style>"#, false),
            ("<math><style></style></math>", false),
        ];
        for (html, expected) in cases {
            let file =
                SourceMap::default().new_source_file(Lrc::new(FileName::Anon), html.to_owned());
            let document =
                parse_file_as_document(&file, ParserConfig::default(), &mut Vec::new()).unwrap();
            let element = find_raw_text_element(&document.children).unwrap();
            assert_eq!(has_minified_raw_text(element), expected, "{html}");
        }
    }

    fn find_raw_text_element(children: &[Child]) -> Option<&Element> {
        children.iter().find_map(|child| match child {
            Child::Element(element) if matches!(&*element.tag_name, "script" | "style") => {
                Some(element)
            }
            Child::Element(element) => find_raw_text_element(&element.children),
            _ => None,
        })
    }

    // ── minify_css_bytes ──

    #[test]
    fn skips_invalid_utf8_css() {
        let path = PathBuf::from("broken.css");
        assert_eq!(minify_css_bytes(&[0xff, 0xfe, 0xfd], &path), None);
    }

    #[test]
    fn skips_unparsable_css() {
        let path = PathBuf::from("broken.css");
        // `@@@` is lexically invalid, and lightningcss fails with an unexpected-end-of-input error.
        assert_eq!(minify_css_bytes(b"@@@", &path), None);
    }

    // ── minify_js_bytes ──

    #[test]
    fn preserves_classic_globals() {
        let input = indoc! {r"
            var sharedValue = 1 + 2;
            function readSharedValue() {
                return sharedValue;
            }
        "};
        let output = minify_js_bytes(input.as_bytes(), Path::new("app.js"), false).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "var sharedValue=3;function readSharedValue(){return sharedValue}"
        );
    }

    #[test]
    fn detects_module_syntax_in_scripts() {
        let output =
            minify_js_bytes(b"export const value = 1 + 2;", Path::new("app.js"), false).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "export const value=3;");
    }

    #[test]
    fn parses_modules_in_strict_mode() {
        let source = b"with (scope) { run(); }";
        let output = minify_js_bytes(source, Path::new("app.js"), false).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "with(scope)run();");
        assert_eq!(minify_js_bytes(source, Path::new("app.mjs"), true), None);
    }

    #[test]
    fn keeps_module_functions_block_scoped() {
        let input = indoc! {"
            export const marker = 1;
            { function blockScoped() { return 2; } }
            console.log(typeof blockScoped);
        "};
        let output = minify_js_bytes(input.as_bytes(), Path::new("app.js"), false).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "export const marker=1;console.log(typeof blockScoped);"
        );
    }

    #[test]
    fn skips_invalid_utf8_js() {
        let path = PathBuf::from("broken.js");
        assert_eq!(minify_js_bytes(&[0xff, 0xfe, 0xfd], &path, false), None);
    }

    #[test]
    fn skips_unparsable_js() {
        let path = PathBuf::from("broken.js");
        assert_eq!(minify_js_bytes(b"function () { ]]]", &path, false), None);
    }

    // ── format_bytes ──

    #[test]
    fn picks_human_unit() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
    }
}
