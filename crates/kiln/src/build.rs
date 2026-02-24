use std::path::Path;

use anyhow::{Context, Result};
use syntect::parsing::SyntaxSet;

use crate::config::Config;
use crate::content::discovery::discover_content;
use crate::content::page::Page;
use crate::output::write_output;
use crate::render::pipeline::render_page;
use crate::template::{PostTemplateVars, TemplateEngine};

/// Shared build state, created once per build invocation.
struct BuildContext {
    config: Config,
    syntax_set: SyntaxSet,
    template_engine: TemplateEngine,
}

/// Builds the site from the given project root directory.
///
/// # Errors
///
/// Returns an error if configuration loading, content discovery, rendering,
/// or output writing fails.
pub fn build(root: &Path) -> Result<()> {
    let config = Config::load(root).context("failed to load config")?;
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let template_engine = TemplateEngine::new(&root.join("templates"))
        .context("failed to initialize template engine")?;

    let ctx = BuildContext {
        config,
        syntax_set,
        template_engine,
    };

    let content = discover_content(root)?;
    let output_dir = root.join(&ctx.config.output_dir);

    for page in &content.pages {
        build_page(&ctx, page, &content.content_dir, &output_dir)?;
    }

    println!("Build complete: {} page(s).", content.pages.len());
    Ok(())
}

/// Renders a single page and writes it to the output directory.
fn build_page(
    ctx: &BuildContext,
    page: &Page,
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    let rendered = render_page(&page.raw_content, &ctx.syntax_set);
    let output_path = page.output_path(content_dir).with_context(|| {
        format!(
            "failed to compute output path for {}",
            page.source_path.display()
        )
    })?;
    let url = page_url(&ctx.config.base_url, &output_path);

    let vars = PostTemplateVars {
        title: &page.frontmatter.title,
        description: page.frontmatter.description.as_deref().unwrap_or(""),
        url: &url,
        featured_image: page.frontmatter.featured_image.as_deref(),
        date: page.frontmatter.date.map(|d| d.to_string()),
        content: &rendered.content_html,
        toc: &rendered.toc_html,
        config: &ctx.config,
    };

    let html = ctx
        .template_engine
        .render_post(&vars)
        .with_context(|| format!("failed to render {}", page.source_path.display()))?;

    let dest = output_dir.join(&output_path);
    write_output(&dest, &html).with_context(|| format!("failed to write {}", dest.display()))
}

/// Computes the canonical URL for a page from its output path.
///
/// For `index.html` pages (page bundles), returns the directory path with a
/// trailing slash. For other files, returns the file path as-is.
fn page_url(base_url: &str, output_path: &Path) -> String {
    let base = base_url.trim_end_matches('/');
    let rel = output_path.to_string_lossy();

    // index.html → directory URL with trailing slash
    if let Some(dir) = rel.strip_suffix("index.html") {
        format!("{base}/{dir}")
    } else {
        format!("{base}/{rel}")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::indoc;

    use super::*;

    fn copy_templates(dest: &Path) {
        fs::create_dir_all(dest).unwrap();
        for entry in fs::read_dir(crate::test_utils::template_dir()).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                fs::copy(entry.path(), dest.join(entry.file_name())).unwrap();
            }
        }
    }

    // -- page_url --

    #[test]
    fn page_url_index_html() {
        assert_eq!(
            page_url("https://example.com", Path::new("foo/bar/index.html")),
            "https://example.com/foo/bar/"
        );
    }

    #[test]
    fn page_url_root_index() {
        assert_eq!(
            page_url("https://example.com", Path::new("index.html")),
            "https://example.com/"
        );
    }

    #[test]
    fn page_url_non_index() {
        assert_eq!(
            page_url("https://example.com", Path::new("standalone.html")),
            "https://example.com/standalone.html"
        );
    }

    // -- build --

    #[test]
    fn build_no_content() {
        let root = tempfile::tempdir().unwrap();

        // Config + templates, but no content directory
        fs::write(root.path().join("config.toml"), "").unwrap();
        let template_dest = root.path().join("templates");
        copy_templates(&template_dest);

        build(root.path()).unwrap();

        // Output directory should not be created when there are no pages
        assert!(
            !root.path().join("public").exists(),
            "output directory should not exist for empty site"
        );
    }

    #[test]
    fn build_end_to_end() {
        let root = tempfile::tempdir().unwrap();

        // Config
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
                title = "Test Site"
            "#},
        )
        .unwrap();

        // Templates — copy from real templates directory
        let template_dest = root.path().join("templates");
        copy_templates(&template_dest);

        // Content
        let content_dir = root.path().join("content").join("posts").join("hello");
        fs::create_dir_all(&content_dir).unwrap();
        fs::write(
            content_dir.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello World"
                description = "A test post"
                date = "2026-02-24T12:34:56Z"
                +++

                ## First

                This is a test **post**.

                ## Second

                More content.
            "#},
        )
        .unwrap();

        // Build
        build(root.path()).unwrap();

        // Verify output
        let output = root.path().join("public").join("hello").join("index.html");
        assert!(output.exists(), "output file should exist");

        let html = fs::read_to_string(&output).unwrap();

        // <head>
        assert!(
            html.contains("<title>Hello World - Test Site</title>"),
            "should have title, html:\n{html}"
        );
        assert!(
            html.contains(r#"<meta name="description" content="A test post">"#),
            "should have meta description, html:\n{html}"
        );
        assert!(
            html.contains(r#"<link rel="canonical" href="https://example.com/hello/">"#),
            "should have canonical URL, html:\n{html}"
        );

        // <body>
        assert!(
            html.contains("<h1>Hello World</h1>"),
            "should have title heading, html:\n{html}"
        );
        assert!(
            html.contains("2026-02-24T12:34:56Z"),
            "should have date, html:\n{html}"
        );
        assert!(
            html.contains(r##"<a href="#first">First</a>"##),
            "should have ToC with links to headings, html:\n{html}"
        );
        assert!(
            html.contains("<p>This is a test <strong>post</strong>.</p>"),
            "should have rendered content, html:\n{html}"
        );
    }
}
