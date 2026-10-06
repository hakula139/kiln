use std::fs;
use std::path::Path;

use indoc::indoc;
use kiln::{BuildOptions, build};
use sha2::{Digest, Sha256};

// ── build ──

#[test]
fn build_publishes_only_canonical_page_styles_with_private_sources_omitted() {
    let root = tempfile::tempdir().unwrap();
    write_site(root.path());
    write_file(
        root.path(),
        "content/example/assets/css/style.generated.css",
        ".rating { color: red; }",
    );
    write_file(
        root.path(),
        "content/example/assets/css/_src/style.css",
        "@apply hidden;",
    );
    write_file(
        root.path(),
        "content/example/assets/_private/data.json",
        "private",
    );
    write_file(root.path(), "content/example/_notes.txt", "private");
    write_file(root.path(), "content/example/assets/css/image.svg", "image");
    write_file(
        root.path(),
        "content/example/assets/app.js",
        "console.log('page');",
    );
    write_file(
        root.path(),
        "content/other/style.css",
        ".old { color: blue; }",
    );
    write_file(
        root.path(),
        "content/other/assets/css/nested/style.generated.css",
        ".nested {}",
    );

    build_site(root.path(), false);

    let public = root.path().join("public");
    let html = fs::read_to_string(public.join("example/index.html")).unwrap();
    let css_url = stylesheet_url(&html);
    assert!(
        css_url.starts_with("/subsite/example/assets/css/style.generated."),
        "{html}"
    );
    assert_eq!(
        fs::read_to_string(published_path(&public, css_url)).unwrap(),
        ".rating { color: red; }"
    );
    let other = fs::read_to_string(public.join("other/index.html")).unwrap();
    assert!(!other.contains("rel=\"stylesheet\""), "{other}");
    for private in [
        "example/assets/css/_src",
        "example/assets/_private",
        "example/_notes.txt",
    ] {
        assert!(!public.join(private).exists(), "published {private}");
    }
    assert_eq!(
        fs::read_to_string(public.join("example/assets/app.js")).unwrap(),
        "console.log('page');"
    );
    assert_eq!(
        fs::read_to_string(public.join("example/assets/css/image.svg")).unwrap(),
        "image"
    );
    assert_eq!(
        fs::read_to_string(public.join("other/style.css")).unwrap(),
        ".old { color: blue; }"
    );
}

#[test]
fn build_hashes_page_css_after_minification_and_preserves_relative_urls() {
    let root = tempfile::tempdir().unwrap();
    write_site(root.path());
    let source =
        "/* long source comment */ .rating { color: red; background-image: url(./image.svg); }";
    write_file(
        root.path(),
        "content/example/assets/css/style.generated.css",
        source,
    );
    write_file(root.path(), "content/example/assets/css/image.svg", "image");
    write_file(root.path(), "static/css/style.generated.css", source);

    build_site(root.path(), true);

    let public = root.path().join("public");
    let html = fs::read_to_string(public.join("example/index.html")).unwrap();
    let first = stylesheet_url(&html).to_owned();
    let bytes = fs::read(published_path(&public, &first)).unwrap();
    let css = String::from_utf8(bytes.clone()).unwrap();
    let digest = hex::encode(Sha256::digest(&bytes));
    assert!(first.ends_with(&format!("style.generated.{}.css", &digest[..12])));
    assert!(bytes.len() < source.len());
    assert!(!css.contains("long source comment"));
    assert!(css.contains("image.svg"), "{css}");
    assert_eq!(
        bytes,
        fs::read(public.join("example/assets/css/style.generated.css")).unwrap()
    );
    assert_eq!(
        bytes,
        fs::read(public.join(format!("css/style.generated.{}.css", &digest[..12]))).unwrap()
    );
    assert_eq!(
        fs::read_to_string(public.join("example/assets/css/image.svg")).unwrap(),
        "image"
    );

    write_file(
        root.path(),
        "content/example/assets/css/style.generated.css",
        ".rating { color: blue; }",
    );
    build_site(root.path(), true);
    let html = fs::read_to_string(public.join("example/index.html")).unwrap();
    let second = stylesheet_url(&html);
    assert_ne!(first, second);
    assert!(!published_path(&public, &first).exists());
    assert!(
        fs::read_to_string(published_path(&public, second))
            .unwrap()
            .contains("#00f")
    );
}

#[test]
fn build_preserves_prepublished_child_styles_when_copying_parent_bundle_assets() {
    let root = tempfile::tempdir().unwrap();
    write_site(root.path());
    write_file(
        root.path(),
        "content/example/child/index.md",
        indoc! {r#"
        +++
        title = "Child"
        +++
        Child page.
    "#},
    );
    let source = "/* source comment */ .child { color: red; }";
    write_file(
        root.path(),
        "content/example/child/assets/css/style.generated.css",
        source,
    );

    build_site(root.path(), true);

    let public = root.path().join("public");
    let html = fs::read_to_string(public.join("example/child/index.html")).unwrap();
    let fingerprinted = fs::read(published_path(&public, stylesheet_url(&html))).unwrap();
    let original = fs::read(public.join("example/child/assets/css/style.generated.css")).unwrap();
    assert_eq!(original, fingerprinted);
    assert!(original.len() < source.len());
    let parent = fs::read_to_string(public.join("example/index.html")).unwrap();
    assert!(!parent.contains("stylesheet"), "{parent}");
}

fn write_site(root: &Path) {
    write_file(root, "config.toml", "");
    write_file(
        root,
        "templates/post.html",
        indoc! {r#"
        <!doctype html>
        <html><head>{% if page_css %}<link rel="stylesheet" href="{{ page_css | safe }}">{% endif %}</head><body>{{ content | safe }}</body></html>
    "#},
    );
    for page in ["example", "other"] {
        write_file(
            root,
            &format!("content/{page}/index.md"),
            indoc! {r#"
            +++
            title = "Example"
            +++
            Page content.
        "#},
        );
    }
}

fn build_site(root: &Path, minify: bool) {
    build(
        root,
        BuildOptions {
            base_url_override: Some("https://example.com/subsite"),
            minify,
            ..BuildOptions::default()
        },
    )
    .unwrap();
}

fn stylesheet_url(html: &str) -> &str {
    html.split("href=")
        .nth(1)
        .unwrap()
        .trim_start_matches('"')
        .split(['"', ' ', '>'])
        .next()
        .unwrap()
}

fn published_path(root: &Path, url: &str) -> std::path::PathBuf {
    root.join(url.strip_prefix("/subsite/").unwrap())
}

fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}
