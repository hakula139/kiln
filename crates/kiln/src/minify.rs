//! Post-build asset minification for HTML, CSS, and JS files.
//!
//! Walks the output directory and rewrites each file in place using
//! Rust-native minifiers. Parse failures are logged as warnings and
//! the original file is left untouched — this mirrors `minify-html`'s
//! internal fallback behavior for inline scripts and keeps `--minify`
//! from aborting builds on unusual input.

use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use minify_html::Cfg;
use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions, CommentOptions};
use oxc_minifier::{CompressOptions, Minifier, MinifierOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;
use walkdir::WalkDir;

/// Totals from a minification pass, suitable for printing as a build summary.
#[derive(Debug, Default)]
pub struct MinifyStats {
    /// Total files inspected (regardless of whether they shrank).
    pub files_processed: u64,
    /// Files whose minified output replaced the original on disk.
    pub files_shrunk: u64,
    /// Sum of original file sizes, in bytes.
    pub bytes_in: u64,
    /// Sum of on-disk sizes after the pass, in bytes.
    pub bytes_out: u64,
}

/// Which minifier to use for a given file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetKind {
    Html,
    Css,
    Js,
}

/// Minifies every HTML, CSS, and JS file under `output_dir` in place.
///
/// Pre-minified files (`*.min.css`, `*.min.js`) are skipped so that vendor
/// bundles (e.g., Pagefind's UI JS) pass through untouched.
///
/// Minifier parse failures are logged at warn level and leave the original
/// file intact. Only filesystem errors (read, write, walk) abort the pass.
///
/// # Errors
///
/// Returns an error if walking the directory or reading / writing a file fails.
pub fn minify_output_dir(output_dir: &Path) -> Result<MinifyStats> {
    let mut stats = MinifyStats::default();

    for entry in WalkDir::new(output_dir).follow_links(false) {
        let entry = entry.with_context(|| format!("failed to walk {}", output_dir.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Some(kind) = classify(path) else {
            continue;
        };

        minify_file(path, kind, &mut stats)
            .with_context(|| format!("failed to process {}", path.display()))?;
    }

    Ok(stats)
}

/// Returns the minifier to use for `path`, or `None` if the file should
/// be skipped.
fn classify(path: &Path) -> Option<AssetKind> {
    let name = path.file_name()?.to_str()?;
    if name.ends_with(".min.css") || name.ends_with(".min.js") {
        return None;
    }
    match path.extension()?.to_str()? {
        "html" | "htm" => Some(AssetKind::Html),
        "css" => Some(AssetKind::Css),
        "js" | "mjs" => Some(AssetKind::Js),
        _ => None,
    }
}

fn minify_file(path: &Path, kind: AssetKind, stats: &mut MinifyStats) -> Result<()> {
    let input = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let bytes_in = input.len() as u64;

    let output = match kind {
        AssetKind::Html => Some(minify_html_bytes(&input)),
        AssetKind::Css => minify_css_bytes(&input, path),
        AssetKind::Js => minify_js_bytes(&input, path),
    };

    stats.files_processed += 1;
    stats.bytes_in += bytes_in;

    // Only replace the file when the minifier actually shrank it. Tiny or
    // already-compact inputs can come back the same size or larger; keeping
    // the original avoids gratuitous rewrites and pointless mtime churn.
    match output {
        Some(bytes) if (bytes.len() as u64) < bytes_in => {
            fs::write(path, &bytes)
                .with_context(|| format!("failed to write {}", path.display()))?;
            stats.files_shrunk += 1;
            stats.bytes_out += bytes.len() as u64;
        }
        _ => {
            stats.bytes_out += bytes_in;
        }
    }
    Ok(())
}

/// Decodes UTF-8 input, or warns and returns `None`. `kind` is the label
/// used in the log message (e.g., `"CSS"`, `"JS"`) so multiple minifier
/// paths can share one warning format without losing context.
fn decode_utf8<'a>(input: &'a [u8], path: &Path, kind: &str) -> Option<&'a str> {
    std::str::from_utf8(input)
        .inspect_err(|e| {
            tracing::warn!("skipping {} ({kind}): invalid UTF-8: {e}", path.display());
        })
        .ok()
}

fn minify_html_bytes(input: &[u8]) -> Vec<u8> {
    let mut cfg = Cfg::new();
    cfg.minify_css = true;
    cfg.minify_js = true;
    cfg.minify_doctype = true;
    minify_html::minify(input, &cfg)
}

fn minify_css_bytes(input: &[u8], path: &Path) -> Option<Vec<u8>> {
    let source = decode_utf8(input, path, "CSS")?;
    let mut stylesheet = match StyleSheet::parse(source, ParserOptions::default()) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("skipping {} (CSS parse failed): {e}", path.display());
            return None;
        }
    };
    if let Err(e) = stylesheet.minify(MinifyOptions::default()) {
        tracing::warn!("skipping {} (CSS minify failed): {e}", path.display());
        return None;
    }
    match stylesheet.to_css(PrinterOptions {
        minify: true,
        ..PrinterOptions::default()
    }) {
        Ok(result) => Some(result.code.into_bytes()),
        Err(e) => {
            tracing::warn!("skipping {} (CSS print failed): {e}", path.display());
            None
        }
    }
}

fn minify_js_bytes(input: &[u8], path: &Path) -> Option<Vec<u8>> {
    let source = decode_utf8(input, path, "JS")?;

    // Parse as module by default — modules are a near-superset of scripts
    // and modern theme JS routinely uses `import` / `export`.
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| SourceType::mjs());
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if let Some(first) = parsed.errors.first() {
        tracing::warn!("skipping {} (JS parse failed): {first}", path.display());
        return None;
    }
    let mut program = parsed.program;
    let options = MinifierOptions {
        mangle: None,
        compress: Some(CompressOptions::smallest()),
    };
    let min_ret = Minifier::new(options).minify(&allocator, &mut program);
    let result = Codegen::new()
        .with_options(CodegenOptions {
            minify: true,
            comments: CommentOptions::disabled(),
            ..CodegenOptions::default()
        })
        .with_scoping(min_ret.scoping)
        .build(&program);
    Some(result.code.into_bytes())
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
        write!(
            f,
            "minified {} files, {} shrunk ({} → {}, -{pct:.1}%)",
            self.files_processed,
            self.files_shrunk,
            format_bytes(self.bytes_in),
            format_bytes(self.bytes_out),
        )
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

    // ── minify_output_dir ──

    #[test]
    fn minify_output_dir_processes_mixed_tree() {
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
        let js = "const x = 1 + 2;\nconsole.log(x);\n";
        let png = b"\x89PNG\r\n\x1a\n"; // binary — should be ignored
        let already_min = b"a{color:red}"; // should be left alone

        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("page.html"), html).unwrap();
        fs::write(root.join("style.css"), css).unwrap();
        fs::write(root.join("sub").join("app.js"), js).unwrap();
        fs::write(root.join("image.png"), png).unwrap();
        fs::write(root.join("vendor.min.css"), already_min).unwrap();

        let stats = minify_output_dir(root).unwrap();
        assert_eq!(stats.files_processed, 3, "should process html / css / js");
        assert_eq!(
            stats.files_shrunk, 3,
            "all three test inputs should actually shrink",
        );
        assert!(stats.bytes_out < stats.bytes_in);

        // Non-targeted files untouched.
        assert_eq!(fs::read(root.join("image.png")).unwrap(), png);
        assert_eq!(fs::read(root.join("vendor.min.css")).unwrap(), already_min);

        // Each targeted file on disk is smaller than its original.
        assert!(fs::read(root.join("page.html")).unwrap().len() < html.len());
        assert!(fs::read(root.join("style.css")).unwrap().len() < css.len());
        assert!(fs::read(root.join("sub").join("app.js")).unwrap().len() < js.len());
    }

    #[test]
    fn minify_output_dir_tolerates_broken_asset() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let good_css = ".foo { color: #ff0000; }\n";
        let broken_js = b"function () { ]]]";

        fs::write(root.join("style.css"), good_css).unwrap();
        fs::write(root.join("broken.js"), broken_js).unwrap();

        let stats = minify_output_dir(root).unwrap();
        assert_eq!(stats.files_processed, 2);
        assert_eq!(stats.files_shrunk, 1, "only CSS should shrink");

        // Broken JS kept intact.
        assert_eq!(fs::read(root.join("broken.js")).unwrap(), broken_js);
    }

    #[test]
    fn minify_output_dir_empty_directory_returns_zero_stats() {
        let dir = tempfile::tempdir().unwrap();
        let stats = minify_output_dir(dir.path()).unwrap();
        assert_eq!(stats.files_processed, 0);
        assert_eq!(stats.files_shrunk, 0);
        assert_eq!(stats.bytes_in, 0);
        assert_eq!(stats.bytes_out, 0);
    }

    // ── classify ──

    #[test]
    fn classify_recognizes_html_css_js() {
        assert_eq!(classify(Path::new("a/index.html")), Some(AssetKind::Html));
        assert_eq!(classify(Path::new("a/page.htm")), Some(AssetKind::Html));
        assert_eq!(classify(Path::new("a/style.css")), Some(AssetKind::Css));
        assert_eq!(classify(Path::new("a/app.js")), Some(AssetKind::Js));
        assert_eq!(classify(Path::new("a/app.mjs")), Some(AssetKind::Js));
    }

    #[test]
    fn classify_skips_pre_minified() {
        assert_eq!(classify(Path::new("a/vendor.min.css")), None);
        assert_eq!(classify(Path::new("a/vendor.min.js")), None);
    }

    #[test]
    fn classify_skips_unknown_extensions() {
        assert_eq!(classify(Path::new("a/image.png")), None);
        assert_eq!(classify(Path::new("a/data.json")), None);
        assert_eq!(classify(Path::new("a/README")), None);
    }

    // ── minify_html_bytes ──

    #[test]
    fn minify_html_strips_whitespace_and_comments() {
        let input = indoc! {r"
            <!DOCTYPE html>
            <html>
              <head>  <title>Hi</title>  </head>
              <body>
                <!-- a comment -->
                <p>Hello    world</p>
              </body>
            </html>
        "};
        let output = minify_html_bytes(input.as_bytes());
        let text = std::str::from_utf8(&output).unwrap();
        assert!(output.len() < input.len(), "should shrink, got {text}");
        assert!(!text.contains("a comment"), "should drop comments: {text}");
        assert!(
            !text.contains("Hello    world"),
            "should collapse inner whitespace, got: {text}",
        );
        assert!(
            text.contains("<p>Hello world"),
            "should preserve content, got: {text}",
        );
    }

    // ── minify_css_bytes ──

    #[test]
    fn minify_css_shrinks_valid_stylesheet() {
        let input = indoc! {r"
            .foo {
                color: #ff0000;
                margin: 0px 0px 0px 0px;
            }
        "};
        let path = PathBuf::from("style.css");
        let output = minify_css_bytes(input.as_bytes(), &path).expect("should minify");
        let text = std::str::from_utf8(&output).unwrap();
        assert!(output.len() < input.len(), "should shrink, got {text}");
        assert!(
            text.contains(".foo"),
            "should preserve selector, got: {text}"
        );
        assert!(
            text.contains("red") || text.contains("#f00"),
            "should compress color, got: {text}",
        );
    }

    #[test]
    fn minify_css_returns_none_on_invalid_utf8() {
        let path = PathBuf::from("broken.css");
        // `0xff 0xfe 0xfd` is not a valid UTF-8 sequence; hits the early
        // UTF-8 guard before lightningcss ever sees the bytes.
        assert_eq!(minify_css_bytes(&[0xff, 0xfe, 0xfd], &path), None);
    }

    // ── minify_js_bytes ──

    #[test]
    fn minify_js_shrinks_valid_source() {
        let input = indoc! {r"
            const greeting = 'hello';
            function greet(name) {
                console.log(greeting + ', ' + name);
            }
            greet('world');
        "};
        let path = PathBuf::from("app.js");
        let output = minify_js_bytes(input.as_bytes(), &path).expect("should minify");
        let text = std::str::from_utf8(&output).unwrap();
        assert!(output.len() < input.len(), "should shrink, got {text}");
        assert!(
            text.contains("console.log"),
            "should preserve runtime calls, got: {text}",
        );
    }

    #[test]
    fn minify_js_returns_none_on_parse_error() {
        let path = PathBuf::from("broken.js");
        assert_eq!(minify_js_bytes(b"function () { ]]]", &path), None);
    }

    #[test]
    fn minify_js_returns_none_on_invalid_utf8() {
        let path = PathBuf::from("broken.js");
        assert_eq!(minify_js_bytes(&[0xff, 0xfe, 0xfd], &path), None);
    }

    // ── Display for MinifyStats ──

    #[test]
    fn display_for_empty_pass_shows_zero() {
        let stats = MinifyStats::default();
        assert_eq!(format!("{stats}"), "minified 0 files");
    }

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

    // ── format_bytes ──

    #[test]
    fn format_bytes_picks_human_unit() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
    }
}
