use std::fs;
use std::path::{Path, PathBuf};

use indoc::{formatdoc, indoc};
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};

use kiln::build::{BuildOptions, build};

#[path = "support/fixtures.rs"]
mod fixtures;

use fixtures::{PermissionGuard, copy_templates, write_test_file};

// ── build ──

#[test]
fn build_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
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
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output = root
        .path()
        .join("public")
        .join("posts")
        .join("hello")
        .join("index.html");
    assert!(output.exists(), "output file should exist");

    let html = fs::read_to_string(&output).unwrap();

    assert!(
        html.contains("<title>Hello World - Test Site</title>"),
        "should have title, html:\n{html}"
    );
    assert!(
        html.contains(r#"<meta name="description" content="A test post">"#),
        "should have meta description, html:\n{html}"
    );
    assert!(
        html.contains(r#"<link rel="canonical" href="https://example.com/posts/hello/">"#),
        "should have canonical URL, html:\n{html}"
    );

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

#[test]
fn build_with_minify_shrinks_html() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello World"
            date = "2026-02-24T12:34:56Z"
            +++

            Body paragraph with **markup**.
        "#},
    );

    let output = root
        .path()
        .join("public")
        .join("posts")
        .join("hello")
        .join("index.html");

    build(root.path(), BuildOptions::default()).unwrap();
    let plain_size = fs::metadata(&output).unwrap().len();

    build(
        root.path(),
        BuildOptions {
            minify: true,
            ..Default::default()
        },
    )
    .unwrap();
    let minified_size = fs::metadata(&output).unwrap().len();

    assert!(
        minified_size < plain_size,
        "minify should shrink HTML: {plain_size} → {minified_size}",
    );

    let html = fs::read_to_string(&output).unwrap();
    assert!(
        html.contains("Hello World"),
        "visible content should survive minify, html:\n{html}",
    );
    assert!(
        !html.contains("  "),
        "inner whitespace should collapse, html:\n{html}",
    );
}

#[test]
fn build_output_dir_override_preserves_configured_output() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));
    let configured_output = root.path().join("public");
    fs::create_dir(&configured_output).unwrap();
    fs::write(configured_output.join("sentinel.txt"), "Keep me").unwrap();
    let output = tempfile::tempdir().unwrap();
    fs::write(output.path().join("stale.html"), "Stale page").unwrap();
    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );

    build(
        root.path(),
        BuildOptions {
            output_dir_override: Some(output.path()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(configured_output.join("sentinel.txt")).unwrap(),
        "Keep me"
    );
    assert!(!configured_output.join("index.html").exists());
    assert!(!output.path().join("stale.html").exists());
    let html = fs::read_to_string(output.path().join("posts/hello/index.html")).unwrap();
    assert!(html.contains("Hello"));
    assert!(output.path().join("index.html").is_file());
}

#[test]
fn build_failed_render_preserves_previous_output() {
    let root = tempfile::tempdir().unwrap();
    copy_templates(&root.path().join("templates"));
    write_page(
        root.path(),
        "posts/example",
        indoc! {r#"
            +++
            title = "Example"
            +++
            Original body
        "#},
    );
    build(root.path(), BuildOptions::default()).unwrap();
    let output = root.path().join("public/posts/example/index.html");
    let previous = fs::read_to_string(&output).unwrap();
    fs::write(root.path().join("templates/post.html"), "{% invalid %}").unwrap();

    assert!(build(root.path(), BuildOptions::default()).is_err());
    assert_eq!(fs::read_to_string(output).unwrap(), previous);
}

#[test]
fn build_output_override_input_overlap_returns_error() {
    let root = tempfile::tempdir().unwrap();
    copy_templates(&root.path().join("templates"));
    for name in ["content", "assets", "static", ".git"] {
        fs::create_dir(root.path().join(name)).unwrap();
        fs::write(root.path().join(name).join("sentinel.txt"), "original").unwrap();
    }

    for name in ["content", "assets", "static", ".git"] {
        let output = root.path().join(name);
        let error = build(
            root.path(),
            BuildOptions {
                output_dir_override: Some(&output),
                ..BuildOptions::default()
            },
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("overlaps project input"),
            "{error}"
        );
        assert_eq!(
            fs::read_to_string(output.join("sentinel.txt")).unwrap(),
            "original"
        );
    }
}

#[test]
fn build_base_url_override() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );

    build(
        root.path(),
        BuildOptions {
            base_url_override: Some("http://localhost:5456"),
            ..Default::default()
        },
    )
    .unwrap();

    let html = fs::read_to_string(
        root.path()
            .join("public")
            .join("posts")
            .join("hello")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        html.contains("http://localhost:5456/posts/hello/"),
        "canonical URL should use overridden base_url, html:\n{html}"
    );
    assert!(
        !html.contains("https://example.com"),
        "should NOT use config base_url when overridden, html:\n{html}"
    );
}

#[test]
fn build_copies_static_files() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    let static_dir = root.path().join("static");
    fs::create_dir_all(static_dir.join("images")).unwrap();
    fs::write(static_dir.join("favicon.ico"), "icon").unwrap();
    fs::write(static_dir.join("images").join("logo.png"), "logo").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    assert_eq!(
        fs::read_to_string(output_dir.join("favicon.ico")).unwrap(),
        "icon"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("images").join("logo.png")).unwrap(),
        "logo"
    );
}

#[test]
fn build_copies_colocated_assets() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );
    let bundle = root.path().join("content").join("posts").join("hello");
    fs::create_dir_all(bundle.join("assets")).unwrap();
    fs::write(bundle.join("cover.webp"), "cover-data").unwrap();
    fs::write(bundle.join("assets").join("diagram.svg"), "svg-data").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public").join("posts").join("hello");
    assert_eq!(
        fs::read_to_string(output_dir.join("cover.webp")).unwrap(),
        "cover-data"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("assets").join("diagram.svg")).unwrap(),
        "svg-data"
    );
}

#[test]
fn build_materializes_external_bundle_symlinks() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));
    write_page(
        root.path(),
        "posts/example",
        indoc! {r#"
            +++
            title = "Post A"
            +++
            Body
        "#},
    );
    let bundle = root.path().join("content/posts/example");
    let external = tempfile::tempdir().unwrap();
    fs::create_dir(external.path().join("nested")).unwrap();
    fs::write(external.path().join("nested/image.svg"), "linked image").unwrap();
    fs::write(external.path().join("caption.txt"), "linked caption").unwrap();
    fs::write(external.path().join("notes.md"), "not a bundle asset").unwrap();
    fs::write(
        external.path().join("vendor.css"),
        ".imported { background: url(nested/image.svg); }",
    )
    .unwrap();
    std::os::unix::fs::symlink(external.path(), bundle.join("shared")).unwrap();
    std::os::unix::fs::symlink(
        external.path().join("caption.txt"),
        bundle.join("caption.txt"),
    )
    .unwrap();

    fs::create_dir_all(bundle.join("_cache")).unwrap();
    std::os::unix::fs::symlink(
        external.path().join("missing"),
        bundle.join("_cache/broken"),
    )
    .unwrap();
    std::os::unix::fs::symlink(external.path(), bundle.join("_private")).unwrap();
    write_test_file(root.path(), "static/icon.svg", "static image");
    write_test_file(
        root.path(),
        "content/posts/example/assets/css/_src/style.css",
        indoc! {r#"
            @import "../../../shared/vendor.css";
            .image { background: url(../../../shared/nested/image.svg); }
            .caption { background: url(../../../caption.txt); }
            .static { background: url(../../../../../../static/icon.svg); }
        "#},
    );

    for processor in ["plain", "tailwind"] {
        fs::write(
            root.path().join("config.toml"),
            formatdoc! {r#"
                [css]
                processor = "{processor}"
            "#},
        )
        .unwrap();
        build(root.path(), BuildOptions::default()).unwrap();

        let output = root.path().join("public/posts/example");
        for (path, expected) in [
            ("caption.txt", "linked caption"),
            ("shared/caption.txt", "linked caption"),
            ("shared/nested/image.svg", "linked image"),
        ] {
            let asset = output.join(path);
            assert_eq!(fs::read_to_string(&asset).unwrap(), expected);
            assert!(fs::symlink_metadata(asset).unwrap().file_type().is_file());
        }
        assert!(
            fs::symlink_metadata(output.join("shared"))
                .unwrap()
                .is_dir()
        );
        assert!(!output.join("shared/notes.md").exists());
        assert!(!output.join("_cache").exists());
        assert!(!output.join("_private").exists());
        let css = fs::read_to_string(output.join("assets/css/page.css")).unwrap();
        assert!(css.contains("../../shared/nested/image.svg"), "{css}");
        assert!(css.contains("../../caption.txt"), "{css}");
        assert!(css.contains(".imported"), "{css}");
        assert!(css.contains("../../../../icon.svg"), "{css}");
    }
}

#[test]
fn build_fingerprints_bundle_assets_after_static_collisions() {
    let root = tempfile::tempdir().unwrap();
    copy_templates(&root.path().join("templates"));
    write_page(
        root.path(),
        "example",
        indoc! {r#"
            +++
            title = "Example"
            +++
            Content.
        "#},
    );
    for (name, bundle, shared) in [
        ("app.js", "console.log('bundle');", "console.log('static');"),
        ("image.svg", "bundle-image", "static-image"),
    ] {
        write_test_file(
            root.path(),
            &format!("content/example/assets/{name}"),
            bundle,
        );
        write_test_file(
            root.path(),
            &format!("static/example/assets/{name}"),
            shared,
        );
    }
    build(
        root.path(),
        BuildOptions {
            minify: true,
            ..Default::default()
        },
    )
    .unwrap();

    let public = root.path().join("public");
    assert_eq!(
        fs::read_to_string(public.join("example/assets/image.svg")).unwrap(),
        "bundle-image"
    );
    let js = fs::read_to_string(public.join("example/assets/app.js")).unwrap();
    assert!(js.contains("bundle") && !js.contains("static"), "{js}");
    let hash = hex::encode(Sha256::digest(js.as_bytes()));
    assert_eq!(
        fs::read_to_string(public.join(format!("example/assets/app.{}.js", &hash[..12]))).unwrap(),
        js
    );
}

#[test]
fn build_cleans_stale_output() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    let output_dir = root.path().join("public");
    fs::create_dir_all(output_dir.join("old")).unwrap();
    fs::write(output_dir.join("old").join("stale.html"), "stale").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    assert!(
        !output_dir.join("old").exists(),
        "stale output should be removed"
    );
}

#[test]
fn build_no_content() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    assert!(output_dir.exists(), "output directory should exist");
    assert!(
        output_dir.join("tags").join("index.html").exists(),
        "should generate empty tags index"
    );
}

#[test]
fn build_publishes_assets_with_owner_precedence_and_private_sources() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(root.path(), "config.toml", r#"theme = "example""#);
    write_test_file(root.path(), "themes/example/theme.toml", "");
    fs::create_dir_all(root.path().join("themes/example/templates")).unwrap();
    copy_templates(&root.path().join("templates"));
    for (path, value) in [
        ("themes/example/assets/theme.txt", "theme resource"),
        ("themes/example/assets/overlay.txt", "theme asset"),
        (
            "themes/example/static/assets/theme-overlay.txt",
            "theme overlay",
        ),
        ("themes/example/assets/theme-overlay.txt", "theme asset"),
        ("themes/example/assets/_secret.txt", "private theme"),
        ("themes/example/assets/layers.txt", "theme asset"),
        ("themes/example/static/assets/layers.txt", "theme overlay"),
        ("assets/layers.txt", "site asset"),
        ("assets/overlay.txt", "site asset"),
        ("static/assets/overlay.txt", "site overlay"),
        ("assets/_secret.txt", "private"),
        ("assets/nested/_cache/secret.txt", "private"),
        ("static/_headers", "root headers"),
    ] {
        write_test_file(root.path(), path, value);
    }
    build(root.path(), BuildOptions::default()).unwrap();

    let public = root.path().join("public");
    for (path, value) in [
        ("assets/theme.txt", "theme resource"),
        ("assets/theme-overlay.txt", "theme overlay"),
        ("assets/layers.txt", "site asset"),
        ("assets/overlay.txt", "site overlay"),
        ("_headers", "root headers"),
    ] {
        assert_eq!(fs::read_to_string(public.join(path)).unwrap(), value);
    }
    assert!(!public.join("assets/_secret.txt").exists());
    assert!(!public.join("assets/nested/_cache").exists());
}

#[test]
fn build_reads_image_dimensions_from_published_theme_and_site_assets() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(root.path(), "config.toml", r#"theme = "example""#);
    write_test_file(root.path(), "themes/example/theme.toml", "");
    fs::create_dir_all(root.path().join("themes/example/templates")).unwrap();
    copy_templates(&root.path().join("templates"));
    let theme_images = root.path().join("themes/example/assets/images");
    fs::create_dir_all(&theme_images).unwrap();
    image::RgbaImage::from_pixel(3, 5, image::Rgba([0, 0, 0, 255]))
        .save(theme_images.join("theme.png"))
        .unwrap();
    image::RgbaImage::from_pixel(7, 9, image::Rgba([0, 0, 0, 255]))
        .save(theme_images.join("override.png"))
        .unwrap();
    let site_images = root.path().join("assets/images");
    fs::create_dir_all(&site_images).unwrap();
    image::RgbaImage::from_pixel(11, 13, image::Rgba([0, 0, 0, 255]))
        .save(site_images.join("override.png"))
        .unwrap();
    write_page(
        root.path(),
        "example",
        indoc! {r#"
            +++
            title = "Example"
            +++
            ![Theme](/assets/images/theme.png)

            ![Site](/assets/images/override.png)
        "#},
    );
    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(root.path().join("public/example/index.html")).unwrap();
    let document = Html::parse_document(&html);
    let selector = Selector::parse("img").unwrap();
    let dimensions: Vec<_> = document
        .select(&selector)
        .map(|image| (image.value().attr("width"), image.value().attr("height")))
        .collect();
    assert_eq!(
        dimensions,
        vec![(Some("3"), Some("5")), (Some("11"), Some("13"))]
    );
}

// ── build: stylesheets ──

#[test]
fn build_keeps_root_page_css_separate_from_shared_css() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(root.path(), "config.toml", "");
    copy_templates_except(&root.path().join("templates"), &["home.html"]);
    write_page(
        root.path(),
        "",
        indoc! {r#"
            +++
            title = "Root"
            +++
            Body
        "#},
    );
    write_test_file(
        root.path(),
        "content/loose.md",
        indoc! {r#"
            +++
            title = "Loose"
            +++
            Body
        "#},
    );
    write_test_file(
        root.path(),
        "assets/css/_src/style.css",
        ".shared { color: red; }",
    );
    write_test_file(
        root.path(),
        "content/assets/css/_src/style.css",
        ".page { color: blue; }",
    );
    build(root.path(), BuildOptions::default()).unwrap();

    let public = root.path().join("public");
    let shared = fs::read_to_string(public.join("assets/css/site.css")).unwrap();
    let page = fs::read_to_string(public.join("assets/css/page.css")).unwrap();
    assert!(
        shared.contains(".shared") && !shared.contains(".page"),
        "{shared}"
    );
    assert!(
        page.contains(".page") && !page.contains(".shared"),
        "{page}"
    );
    let html = fs::read_to_string(public.join("index.html")).unwrap();
    assert!(
        stylesheet_url(&html).contains("/assets/css/page."),
        "{html}"
    );
    assert!(!public.join("assets/css/_src").exists());
    let loose = fs::read_to_string(public.join("loose/index.html")).unwrap();
    assert!(!loose.contains("stylesheet"), "{loose}");
    assert!(!public.join("loose/assets/css/page.css").exists());
}

#[test]
fn build_compiles_shared_css_with_site_override_and_theme_asset_urls() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(root.path(), "config.toml", r#"theme = "example""#);
    write_test_file(root.path(), "themes/example/theme.toml", "");
    fs::create_dir_all(root.path().join("themes/example/templates")).unwrap();
    copy_templates(&root.path().join("templates"));
    write_test_file(
        root.path(),
        "themes/example/assets/css/_src/style.css",
        r#"@import "./parts/font.css"; .theme { color: red; }"#,
    );
    write_test_file(
        root.path(),
        "themes/example/assets/css/_src/parts/font.css",
        "@font-face { font-family: Example; src: url(../../../fonts/example.woff2?v=1#font); }",
    );
    write_test_file(
        root.path(),
        "themes/example/assets/fonts/example.woff2",
        "theme-font",
    );
    build(root.path(), BuildOptions::default()).unwrap();
    let output = root.path().join("public/assets/css/site.css");
    let css = fs::read_to_string(&output).unwrap();
    assert!(css.contains("../fonts/example.woff2?v=1#font"), "{css}");
    assert!(css.contains(".theme"), "{css}");

    write_test_file(
        root.path(),
        "assets/css/_src/style.css",
        ".site { color: blue; }",
    );
    build(root.path(), BuildOptions::default()).unwrap();
    let css = fs::read_to_string(&output).unwrap();
    assert!(css.contains(".site") && !css.contains(".theme"), "{css}");
    assert!(!root.path().join("public/assets/css/_src").exists());
}

#[test]
fn build_compiles_page_styles_with_private_sources_and_final_hashes() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));
    for page in ["example", "example/child", "other"] {
        write_page(
            root.path(),
            page,
            indoc! {r#"
                +++
                title = "Example"
                +++
                Page content.
            "#},
        );
    }
    write_test_file(
        root.path(),
        "content/example/child/assets/css/_src/style.css",
        r#"@import "nested/colors.css";"#,
    );
    write_test_file(
        root.path(),
        "content/example/child/assets/css/_src/nested/colors.css",
        ".rating { color: red; & > span { background-image: url(../../../my%20icon.svg?time=12:00#icon:active); } }",
    );
    write_test_file(
        root.path(),
        "content/example/child/assets/my icon.svg",
        "bundle-image",
    );
    write_test_file(
        root.path(),
        "content/other/style.css",
        ".ordinary { color: blue; }",
    );
    write_test_file(root.path(), "static/_custom/settings.txt", "published");
    build(
        root.path(),
        BuildOptions {
            base_url_override: Some("https://example.com/subsite"),
            minify: true,
            ..Default::default()
        },
    )
    .unwrap();

    let public = root.path().join("public");
    let html = fs::read_to_string(public.join("example/child/index.html")).unwrap();
    let first = stylesheet_url(&html);
    let bytes = fs::read(published_path(&public, &first)).unwrap();
    let css = String::from_utf8(bytes.clone()).unwrap();
    let digest = hex::encode(Sha256::digest(&bytes));
    assert!(
        first.starts_with("/subsite/example/child/assets/css/page."),
        "{first}"
    );
    assert!(first.ends_with(&format!("page.{}.css", &digest[..12])));
    assert!(
        css.contains("../my%20icon.svg?time=12:00#icon:active"),
        "{css}"
    );
    assert!(css.contains("span"), "{css}");
    assert_eq!(
        bytes,
        fs::read(public.join("example/child/assets/css/page.css")).unwrap()
    );
    assert!(!public.join("example/child/assets/css/_src").exists());
    for page in ["example", "other"] {
        let html = fs::read_to_string(public.join(page).join("index.html")).unwrap();
        assert!(!html.contains("stylesheet"), "{html}");
    }
    assert_eq!(
        fs::read_to_string(public.join("example/child/assets/my icon.svg")).unwrap(),
        "bundle-image"
    );
    assert_eq!(
        fs::read_to_string(public.join("_custom/settings.txt")).unwrap(),
        "published"
    );

    write_test_file(
        root.path(),
        "content/example/child/assets/css/_src/style.css",
        ".rating { color: blue; }",
    );
    build(
        root.path(),
        BuildOptions {
            minify: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!published_path(&public, &first).exists());
    assert!(
        fs::read_to_string(public.join("example/child/assets/css/page.css"))
            .unwrap()
            .contains("#00f")
    );
}

#[test]
fn build_compiles_tailwind_with_shared_context_and_fresh_candidates() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(root.path(), "config.toml", r#"theme = "example""#);
    write_test_file(
        root.path(),
        "themes/example/theme.toml",
        indoc! {r#"
            [css]
            processor = "tailwind"
        "#},
    );
    copy_templates(&root.path().join("templates"));
    write_test_file(
        root.path(),
        "themes/example/templates/unused.html",
        r#"<div class="underline"></div>"#,
    );
    write_test_file(
        root.path(),
        "themes/example/assets/css/_src/style.css",
        indoc! {r#"
            @import "tailwindcss" source(none);
            @import "./parts/font.css";
            @theme { --color-brand: #123456; }
            @utility theme-image { background-image: url(../../image.svg); }
        "#},
    );
    write_test_file(
        root.path(),
        "themes/example/assets/css/_src/parts/font.css",
        "@font-face { font-family: Example; src: url(../../../fonts/example.woff2?version=1#font); }",
    );
    write_test_file(
        root.path(),
        "themes/example/assets/fonts/example.woff2",
        "font",
    );
    write_page(
        root.path(),
        "example",
        indoc! {r#"
            +++
            title = "Example"
            +++
            ::: {.bg-brand}
            Text.
            :::
        "#},
    );
    write_test_file(
        root.path(),
        "themes/example/assets/image.svg",
        "theme-image",
    );
    write_test_file(
        root.path(),
        "content/example/assets/css/_src/style.css",
        indoc! {r#"
            @import "./nested/rating.css";
            .rating { @apply text-brand theme-image; & > span { @apply font-bold; } }
        "#},
    );
    write_test_file(
        root.path(),
        "content/example/assets/css/_src/nested/rating.css",
        ".image { background: url(../../../../image.svg?q=1#icon); }",
    );
    write_test_file(root.path(), "content/example/image.svg", "image");
    build(root.path(), BuildOptions::default()).unwrap();
    let public = root.path().join("public");
    let shared = fs::read_to_string(public.join("assets/css/site.css")).unwrap();
    assert!(
        shared.contains(".bg-brand") && shared.contains(".underline"),
        "{shared}"
    );
    assert!(
        shared.contains("../fonts/example.woff2?version=1#font"),
        "{shared}"
    );
    let page = fs::read_to_string(public.join("example/assets/css/page.css")).unwrap();
    assert!(
        page.contains("color: var(--color-brand, #123456)") && page.contains("font-weight"),
        "{page}"
    );
    assert!(page.contains("../../image.svg?q=1#icon"), "{page}");
    assert!(page.contains("../../../assets/image.svg"), "{page}");
    assert!(
        !page.contains(".bg-brand") && !page.contains(".underline"),
        "{page}"
    );
    let content = root.path().join("content/example/index.md");
    let markdown = fs::read_to_string(&content).unwrap();
    fs::write(content, markdown.replace(".bg-brand", ".text-brand")).unwrap();
    build(root.path(), BuildOptions::default()).unwrap();
    let rebuilt = fs::read_to_string(public.join("assets/css/site.css")).unwrap();
    assert!(!rebuilt.contains(".bg-brand"), "{rebuilt}");
}

#[test]
fn build_compiles_tailwind_imports_from_static_symlinks() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(
        root.path(),
        "config.toml",
        indoc! {r#"
            [css]
            processor = "tailwind"
        "#},
    );
    copy_templates(&root.path().join("templates"));
    write_test_file(
        root.path(),
        "assets/css/_src/style.css",
        r#"@import "../../../static/shared/vendor.css";"#,
    );
    let external = tempfile::tempdir().unwrap();
    fs::write(
        external.path().join("vendor.css"),
        ".imported { background: url(image.svg); }",
    )
    .unwrap();
    fs::write(external.path().join("image.svg"), "external image").unwrap();
    fs::create_dir(root.path().join("static")).unwrap();
    std::os::unix::fs::symlink(external.path(), root.path().join("static/shared")).unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let public = root.path().join("public");
    let css = fs::read_to_string(public.join("assets/css/site.css")).unwrap();
    assert!(css.contains("../../shared/image.svg"), "{css}");
    assert_eq!(
        fs::read_to_string(public.join("shared/image.svg")).unwrap(),
        "external image"
    );
}

#[test]
fn build_compiles_tailwind_imports_through_public_bundle_aliases() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(
        root.path(),
        "config.toml",
        indoc! {r#"
            [css]
            processor = "tailwind"
        "#},
    );
    copy_templates(&root.path().join("templates"));
    write_page(
        root.path(),
        "example",
        indoc! {r#"
            +++
            title = "Example"
            +++
            Body
        "#},
    );
    write_test_file(
        root.path(),
        "content/example/assets/css/_src/style.css",
        r#"@import "../../../public/vendor.css";"#,
    );
    write_test_file(
        root.path(),
        "content/example/_assets/vendor/vendor.css",
        ".image { background: url(image.svg); }",
    );
    write_test_file(
        root.path(),
        "content/example/_assets/vendor/image.svg",
        "image",
    );
    let bundle = root.path().join("content/example");
    std::os::unix::fs::symlink(bundle.join("_assets/vendor"), bundle.join("public")).unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let output = root.path().join("public/example");
    let css = fs::read_to_string(output.join("assets/css/page.css")).unwrap();
    assert!(css.contains("../../public/image.svg"), "{css}");
    assert_eq!(
        fs::read_to_string(output.join("public/image.svg")).unwrap(),
        "image"
    );
    assert!(!output.join("_assets").exists());
}

#[test]
fn build_with_invalid_tailwind_utility_returns_error() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(
        root.path(),
        "config.toml",
        indoc! {r#"
            [css]
            processor = "tailwind"
        "#},
    );
    copy_templates(&root.path().join("templates"));
    write_test_file(
        root.path(),
        "assets/css/_src/style.css",
        indoc! {r#"
            @import "tailwindcss" source(none);
            .invalid { @apply kiln-unknown-utility; }
        "#},
    );

    let error = build(root.path(), BuildOptions::default()).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("assets/css/_src/style.css"), "{message}");
    assert!(message.contains("Tailwind CSS exited with"), "{message}");
    assert!(
        message.contains("Cannot apply unknown utility class `kiln-unknown-utility`"),
        "{message}"
    );
}

// ── build: theme ──

#[test]
fn build_with_theme() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test"
            theme = "my-theme"
        "#},
    )
    .unwrap();
    setup_theme(root.path(), "my-theme");

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output = root
        .path()
        .join("public")
        .join("posts")
        .join("hello")
        .join("index.html");
    assert!(output.exists(), "output file should exist");
    let html = fs::read_to_string(&output).unwrap();
    assert!(
        html.contains("<h1>Hello</h1>"),
        "should render with theme templates, html:\n{html}"
    );
}

#[test]
fn build_theme_static_files_with_site_override() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), r#"theme = "my-theme""#).unwrap();
    setup_theme(root.path(), "my-theme");

    let theme_static = root.path().join("themes/my-theme/static");
    fs::create_dir_all(&theme_static).unwrap();
    fs::write(theme_static.join("theme.css"), "theme-default").unwrap();
    fs::write(theme_static.join("shared.css"), "from-theme").unwrap();

    let site_static = root.path().join("static");
    fs::create_dir_all(&site_static).unwrap();
    fs::write(site_static.join("shared.css"), "from-site").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    assert_eq!(
        fs::read_to_string(output_dir.join("theme.css")).unwrap(),
        "theme-default",
        "theme-only static file should be copied"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("shared.css")).unwrap(),
        "from-site",
        "site static file should override theme"
    );
}

#[test]
fn build_fingerprints_merged_static_assets_after_minification() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), r#"theme = "my-theme""#).unwrap();
    setup_theme(root.path(), "my-theme");

    let theme_dir = root.path().join("themes/my-theme");
    fs::write(
        theme_dir.join("templates/base.html"),
        indoc! {r#"
            <!DOCTYPE html>
            <html>
            <head>
              <link rel="stylesheet" href="{{ asset_url('/style.css') | safe }}">
              <script src="{{ asset_url('/app.js') | safe }}"></script>
            </head>
            <body>{% block body %}{% endblock %}</body>
            </html>
        "#},
    )
    .unwrap();
    fs::create_dir_all(theme_dir.join("static")).unwrap();
    fs::write(
        theme_dir.join("static/style.css"),
        ".theme { color: blue; }\n",
    )
    .unwrap();
    fs::write(
        theme_dir.join("static/app.js"),
        "const value = 1 + 2; console.log(value);\n",
    )
    .unwrap();

    let site_static = root.path().join("static");
    fs::create_dir_all(&site_static).unwrap();
    fs::write(
        site_static.join("style.css"),
        ".site { color: #ff0000; margin: 0px; }\n",
    )
    .unwrap();

    build(
        root.path(),
        BuildOptions {
            minify: true,
            ..Default::default()
        },
    )
    .unwrap();

    let output_dir = root.path().join("public");
    let html = fs::read_to_string(output_dir.join("index.html")).unwrap();
    for name in ["style.css", "app.js"] {
        let bytes = fs::read(output_dir.join(name)).unwrap();
        let digest = hex::encode(Sha256::digest(&bytes));
        let (stem, extension) = name.split_once('.').unwrap();
        let fingerprinted = format!("{stem}.{}.{extension}", &digest[..12]);

        assert!(output_dir.join(&fingerprinted).is_file());
        assert!(
            html.contains(&format!(r"/{fingerprinted}")),
            "html should reference {fingerprinted}, got:\n{html}"
        );
    }
    assert!(
        fs::read_to_string(output_dir.join("style.css"))
            .unwrap()
            .contains(".site"),
        "the site override should supply the fingerprinted stylesheet"
    );
}

// ── build: page template ──

#[test]
fn build_uses_page_template_for_standalone() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "about-me",
        indoc! {r#"
            +++
            title = "About Me"
            +++
            Hello world.
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output = root
        .path()
        .join("public")
        .join("about-me")
        .join("index.html");
    assert!(output.exists(), "should generate about-me page");
    let html = fs::read_to_string(&output).unwrap();
    assert!(
        html.contains(r#"<article class="page">"#),
        "should use page.html template, html:\n{html}"
    );
}

#[test]
fn build_renders_dates_in_configured_timezone() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            timezone = "Asia/Shanghai"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            date = "2026-03-13T09:36:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(
        root.path()
            .join("public")
            .join("posts")
            .join("note")
            .join("hello")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        html.contains("2026-03-13T17:36:00+08:00"),
        "should render the configured time zone offset, html:\n{html}"
    );
    assert!(
        !html.contains("2026-03-13T09:36:00Z"),
        "should not leave the date in UTC, html:\n{html}"
    );
}

#[test]
fn build_exposes_updated_and_linked_tags_without_git() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            timezone = "Asia/Shanghai"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));
    fs::write(
        root.path().join("templates/post.html"),
        indoc! {r#"
            {% if updated %}<time datetime="{{ updated }}">{{ updated[:10] }}</time>{% endif %}
            {% for tag in tags %}<a href="{{ tag.url | safe }}">{{ tag.name }}</a>{% endfor %}
        "#},
    )
    .unwrap();
    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            updated = "2026-03-13T22:36:00Z"
            tags = ["C++", "<script>"]
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(root.path().join("public/posts/note/hello/index.html")).unwrap();
    assert!(html.contains(r#"<time datetime="2026-03-14T06:36:00+08:00">2026-03-14</time>"#));
    assert!(
        html.contains(r#"<a href="https://example.com/tags/c++/">C++</a>"#),
        "html:\n{html}"
    );
    assert!(html.contains("&lt;script&gt;</a>"));
    assert!(!html.contains("<script>"));
}

#[test]
fn build_heading_numbering_is_per_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"

            [params]
            heading_numbering = true
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));
    let pages = [
        (
            "numbered",
            "heading_numbering = true",
            "",
            "",
            Some(("1", "1.1")),
        ),
        (
            "continued",
            "heading_numbering = true",
            " {numbering-start=2}",
            "",
            Some(("2", "2.1")),
        ),
        (
            "child",
            "heading_numbering = true",
            " {numbering-start=2}",
            " {numbering-start=0}",
            Some(("2", "2.0")),
        ),
        (
            "another",
            "heading_numbering = true",
            "",
            "",
            Some(("1", "1.1")),
        ),
        (
            "disabled",
            "heading_numbering = false",
            " {numbering-start=bad}",
            "",
            None,
        ),
        ("default", "", " {numbering-start=bad}", "", None),
    ];
    for (slug, setting, root_attribute, child_attribute, _) in pages {
        write_page(
            root.path(),
            &format!("posts/{slug}"),
            &formatdoc! {r#"
                +++
                title = "Post"
                {setting}
                +++
                ## Section{root_attribute}
                ### Detail{child_attribute}
            "#},
        );
    }
    build(root.path(), BuildOptions::default()).unwrap();
    for (slug, _, _, _, numbers) in pages {
        let html = fs::read_to_string(root.path().join(format!("public/posts/{slug}/index.html")))
            .unwrap();
        if let Some((root, child)) = numbers {
            for (id, number, title, level) in [
                ("section", root, "Section", 2),
                ("detail", child, "Detail", 3),
            ] {
                assert!(html.contains(&format!(
                    r#"<h{level} id="{id}"><span class="heading-number">{number}</span> {title}</h{level}>"#
                )));
                assert!(html.contains(&format!(
                    r##"href="#{id}"><span class="heading-number">{number}</span> {title}</a>"##
                )));
            }
        } else {
            assert!(html.contains(r#"<h2 id="section">Section</h2>"#));
            assert!(!html.contains("heading-number"));
        }
    }
}

#[test]
fn build_validates_render_options_only_for_content() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            [params]
            emojis = "yes"
        "#},
    )
    .unwrap();
    let templates = root.path().join("templates");
    copy_templates(&templates);
    fs::write(templates.join("home.html"), "<h1>Empty site</h1>").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    assert_eq!(
        fs::read_to_string(root.path().join("public/index.html")).unwrap(),
        "<h1>Empty site</h1>"
    );

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );

    let err = build(root.path(), BuildOptions::default()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "failed to parse render options from [params]"
    );
    assert!(format!("{err:#}").contains("invalid type: string \"yes\", expected a boolean"));
    assert!(!root.path().join("public/posts/hello/index.html").exists());
}

// ── build: home page ──

#[test]
fn build_generates_home_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let home = root.path().join("public").join("index.html");
    assert!(home.exists(), "should generate home page /index.html");
    let html = fs::read_to_string(&home).unwrap();
    assert!(
        html.contains("Hello"),
        "home page should list posts, html:\n{html}"
    );
    assert!(
        html.contains(r#"<a href="http://localhost:5456/posts/note/hello/">Hello</a>"#),
        "home page should link to the post under /posts/, html:\n{html}"
    );
}

#[test]
fn build_home_orders_by_date_descending() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/aaa-old",
        indoc! {r#"
            +++
            title = "Old Post"
            date = "2025-01-01T00:00:00Z"
            +++
            Body
        "#},
    );
    write_page(
        root.path(),
        "posts/zzz-new",
        indoc! {r#"
            +++
            title = "New Post"
            date = "2026-06-01T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
    let new_pos = html.find("New Post").expect("should list New Post");
    let old_pos = html.find("Old Post").expect("should list Old Post");
    assert!(
        new_pos < old_pos,
        "newer post should appear before older post on home page, html:\n{html}"
    );
}

#[test]
fn build_pins_home_without_reordering_archives() {
    let root = tempfile::tempdir().unwrap();
    write_listing_site(root.path());

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let home = fs::read_to_string(output_dir.join("index.html")).unwrap();
    assert_eq!(
        listing_links(&home),
        [
            r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
            r#"<a href="https://example.com/posts/review/latest/">Post C</a>"#,
            r#"<a href="https://example.com/posts/note/newer/">Post B</a>"#,
        ]
    );
    for (path, expected) in [
        (
            "posts/index.html",
            vec![
                r#"<a href="https://example.com/posts/review/latest/">Post C</a>"#,
                r#"<a href="https://example.com/posts/note/newer/">Post B</a>"#,
                r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
            ],
        ),
        (
            "posts/note/index.html",
            vec![
                r#"<a href="https://example.com/posts/note/newer/">Post B</a>"#,
                r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
            ],
        ),
        (
            "tags/rust/index.html",
            vec![
                r#"<a href="https://example.com/about/">About</a>"#,
                r#"<a href="https://example.com/posts/review/latest/">Post C</a>"#,
                r#"<a href="https://example.com/posts/note/older/">Post A</a>"#,
            ],
        ),
    ] {
        let html = fs::read_to_string(output_dir.join(path)).unwrap();
        assert_eq!(listing_links(&html), expected, "incorrect order in {path}");
    }
}

#[test]
fn build_orphan_posts_on_home_not_in_sections() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/sectioned",
        indoc! {r#"
            +++
            title = "Sectioned Post"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );
    write_page(
        root.path(),
        "posts/orphan",
        indoc! {r#"
            +++
            title = "Orphan Post"
            date = "2026-01-02T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let home_html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
    assert!(
        home_html.contains("Sectioned Post"),
        "sectioned post should also appear on home page, html:\n{home_html}"
    );
    assert!(
        home_html.contains("Orphan Post"),
        "orphan post should appear on home page, html:\n{home_html}"
    );

    let note_html = fs::read_to_string(
        root.path()
            .join("public")
            .join("posts")
            .join("note")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        note_html.contains("Sectioned Post"),
        "sectioned post should appear in section page, html:\n{note_html}"
    );
    assert!(
        !note_html.contains("Orphan Post"),
        "orphan post should NOT appear in section page, html:\n{note_html}"
    );
}

#[test]
fn build_home_pagination() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"

            [params.home]
            paginate = 2
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_paginated_posts(root.path(), "posts/note", None);

    build(root.path(), BuildOptions::default()).unwrap();

    assert_paginated_listing(
        &root.path().join("public"),
        &[
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
        ],
        &[r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#],
        None,
    );
}

#[test]
fn build_standalone_excluded_from_home() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello Post"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );
    write_page(
        root.path(),
        "about-me",
        indoc! {r#"
            +++
            title = "About Me"
            +++
            Bio
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
    assert!(
        html.contains("Hello Post"),
        "home page should list posts, html:\n{html}"
    );
    assert!(
        !html.contains("About Me"),
        "home page should NOT list standalone pages, html:\n{html}"
    );
}

#[test]
fn build_empty_home_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    build(root.path(), BuildOptions::default()).unwrap();

    let home = root.path().join("public").join("index.html");
    assert!(
        home.exists(),
        "should generate home page even with zero posts"
    );
}

#[test]
fn build_skips_home_without_template() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates_except(&root.path().join("templates"), &["home.html"]);

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let home = root.path().join("public").join("index.html");
    assert!(
        !home.exists(),
        "should NOT generate home page without home.html template"
    );
}

// ── build: posts index ──

#[test]
fn build_generates_posts_index() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/post-a",
        indoc! {r#"
            +++
            title = "Post A"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );
    write_page(
        root.path(),
        "posts/essay/post-b",
        indoc! {r#"
            +++
            title = "Post B"
            date = "2026-01-02T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let posts_index = root.path().join("public").join("posts").join("index.html");
    assert!(posts_index.exists(), "should generate /posts/index.html");
    let html = fs::read_to_string(&posts_index).unwrap();
    assert!(
        html.contains("Post A") && html.contains("Post B"),
        "posts index should list all posts across sections, html:\n{html}"
    );
}

#[test]
fn build_posts_index_uses_index_title() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    let posts_dir = root.path().join("content").join("posts");
    fs::create_dir_all(&posts_dir).unwrap();
    fs::write(
        posts_dir.join("_index.md"),
        indoc! {r#"
            +++
            title = "文章"
            +++
        "#},
    )
    .unwrap();

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html =
        fs::read_to_string(root.path().join("public").join("posts").join("index.html")).unwrap();
    assert!(
        html.contains("文章"),
        "should use _index.md title for posts index, html:\n{html}"
    );
}

#[test]
fn build_posts_index_pagination() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"

            [params.section]
            paginate = 2
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_paginated_posts(root.path(), "posts/note", None);

    build(root.path(), BuildOptions::default()).unwrap();

    assert_paginated_listing(
        &root.path().join("public/posts"),
        &[
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
        ],
        &[r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#],
        Some(("/posts/page/2/", "/posts/")),
    );
}

#[test]
fn build_posts_index_generated_even_when_empty() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "about-me",
        indoc! {r#"
            +++
            title = "About Me"
            +++
            Bio
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let posts_index = root.path().join("public").join("posts").join("index.html");
    assert!(
        posts_index.exists(),
        "should generate /posts/index.html even with no posts"
    );
}

// ── build: section pages ──

#[test]
fn build_generates_section_pages() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    for (section, name) in [("note", "post-a"), ("note", "post-b"), ("essay", "hello")] {
        write_page(
            root.path(),
            &format!("posts/{section}/{name}"),
            &format!(
                indoc! {r#"
                    +++
                    title = "{name}"
                    date = "2026-01-01T00:00:00Z"
                    +++
                    Body
                "#},
                name = name,
            ),
        );
    }

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let note_index = output_dir.join("posts").join("note").join("index.html");
    assert!(
        note_index.exists(),
        "should generate /posts/note/index.html"
    );
    let html = fs::read_to_string(&note_index).unwrap();
    assert!(
        html.contains("Note"),
        "should have section title, html:\n{html}"
    );
    assert!(
        html.contains("post-a") && html.contains("post-b"),
        "should list section posts, html:\n{html}"
    );
    assert!(
        html.contains(r#"<a href="http://localhost:5456/posts/note/post-a/">post-a</a>"#),
        "section page should link to posts under /posts/, html:\n{html}"
    );

    let essay_index = output_dir.join("posts").join("essay").join("index.html");
    assert!(
        essay_index.exists(),
        "should generate /posts/essay/index.html"
    );
}

#[test]
fn build_section_uses_index_title() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    let section_dir = root.path().join("content").join("posts").join("note");
    fs::create_dir_all(&section_dir).unwrap();
    fs::write(
        section_dir.join("_index.md"),
        indoc! {r#"
            +++
            title = "笔记"
            +++
        "#},
    )
    .unwrap();

    write_page(
        root.path(),
        "posts/note/my-post",
        indoc! {r#"
            +++
            title = "My Post"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(
        root.path()
            .join("public")
            .join("posts")
            .join("note")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        html.contains("笔记"),
        "should use _index.md title, html:\n{html}"
    );
}

#[test]
fn build_section_pagination() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"

            [params.section]
            paginate = 2
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_paginated_posts(root.path(), "posts/note", None);

    write_page(
        root.path(),
        "posts/review/other",
        indoc! {r#"
            +++
            title = "Other Post"
            date = "2026-01-04T00:00:00Z"
            tags = ["rust"]
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    assert_paginated_listing(
        &root.path().join("public/posts/note"),
        &[
            r#"<a href="https://example.com/posts/note/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/note/post-2/">Post 2</a>"#,
        ],
        &[r#"<a href="https://example.com/posts/note/post-1/">Post 1</a>"#],
        Some(("/posts/note/page/2/", "/posts/note/")),
    );
}

#[test]
fn build_skips_archives_without_template() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates_except(&root.path().join("templates"), &["archive.html"]);

    write_page(
        root.path(),
        "posts/note/my-post",
        indoc! {r#"
            +++
            title = "My Post"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let section_index = root
        .path()
        .join("public")
        .join("posts")
        .join("note")
        .join("index.html");
    assert!(
        !section_index.exists(),
        "should NOT generate archive pages without archive.html template"
    );
}

// ── build: sections index ──

#[test]
fn build_sections_index_generates_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/post-a",
        indoc! {r#"
            +++
            title = "Post A"
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );
    write_page(
        root.path(),
        "posts/essay/post-b",
        indoc! {r#"
            +++
            title = "Post B"
            date = "2026-01-02T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let sections_index = root
        .path()
        .join("public")
        .join("sections")
        .join("index.html");
    assert!(
        sections_index.exists(),
        "should generate /sections/index.html"
    );
    let html = fs::read_to_string(&sections_index).unwrap();
    assert!(
        html.contains("Essay") && html.contains("Note"),
        "should list section names, html:\n{html}"
    );
    assert!(
        html.contains("Post A") && html.contains("Post B"),
        "should list section posts, html:\n{html}"
    );
}

#[test]
fn build_sections_index_skipped_without_overview_template() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates_except(&root.path().join("templates"), &["overview.html"]);

    write_page(
        root.path(),
        "posts/note/post-a",
        indoc! {r#"
            +++
            title = "Post A"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let sections_index = root
        .path()
        .join("public")
        .join("sections")
        .join("index.html");
    assert!(
        !sections_index.exists(),
        "should NOT generate sections index without overview.html"
    );
}

// ── build: taxonomies ──

#[test]
fn build_generates_taxonomy_index_pages() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            tags = ["rust", "web"]
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let tags_index = output_dir.join("tags").join("index.html");
    assert!(tags_index.exists(), "should generate /tags/index.html");
    let html = fs::read_to_string(&tags_index).unwrap();
    assert!(
        html.contains("rust") && html.contains("web"),
        "tags index should list terms, html:\n{html}"
    );
}

#[test]
fn build_generates_tag_archive_pages() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    for (name, tag) in [("post-1", "rust"), ("post-2", "rust"), ("post-3", "web")] {
        write_page(
            root.path(),
            &format!("posts/{name}"),
            &format!(
                indoc! {r#"
                    +++
                    title = "{name}"
                    tags = ["{tag}"]
                    +++
                    Body
                "#},
                name = name,
                tag = tag,
            ),
        );
    }

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let rust_page = output_dir.join("tags").join("rust").join("index.html");
    assert!(rust_page.exists(), "should generate /tags/rust/index.html");
    let html = fs::read_to_string(&rust_page).unwrap();
    assert!(
        html.contains("post-1") && html.contains("post-2"),
        "tag archive should list posts, html:\n{html}"
    );
    assert!(
        !html.contains("post-3"),
        "tag archive should not include unrelated posts, html:\n{html}"
    );
}

#[test]
fn build_generates_paginated_tag_archive_pages() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"

            [params]
            paginate = 2
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_paginated_posts(root.path(), "posts", Some("rust"));

    write_page(
        root.path(),
        "posts/other",
        indoc! {r#"
            +++
            title = "Other Post"
            date = "2026-01-04T00:00:00Z"
            tags = ["web"]
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    assert_paginated_listing(
        &root.path().join("public/tags/rust"),
        &[
            r#"<a href="https://example.com/posts/post-3/">Post 3</a>"#,
            r#"<a href="https://example.com/posts/post-2/">Post 2</a>"#,
        ],
        &[
            r#"<a href="https://example.com/posts/post-1/">Post 1</a>"#,
            r#"<a href="https://example.com/about/">About</a>"#,
        ],
        Some(("/tags/rust/page/2/", "/tags/rust/")),
    );
}

#[test]
fn build_tag_archive_correct_with_standalone_pages() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "about-me",
        indoc! {r#"
            +++
            title = "About Me"
            +++
            Bio
        "#},
    );
    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello Post"
            tags = ["rust"]
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let term_page = root
        .path()
        .join("public")
        .join("tags")
        .join("rust")
        .join("index.html");
    assert!(term_page.exists(), "should generate /tags/rust/index.html");
    let html = fs::read_to_string(&term_page).unwrap();
    assert!(
        html.contains("Hello Post"),
        "tag archive should list the tagged post, html:\n{html}"
    );
    assert!(
        !html.contains("About Me"),
        "tag archive should NOT list standalone pages, html:\n{html}"
    );
}

#[test]
fn build_generates_tags_index_without_tags() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let tags_index = output_dir.join("tags").join("index.html");
    assert!(
        tags_index.exists(),
        "should generate /tags/index.html even with no tags"
    );
}

// ── build: pagination config ──

#[test]
fn build_zero_paginate_falls_back_to_defaults() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r"
            [params]
            paginate = 0

            [params.home]
            paginate = 0

            [params.section]
            paginate = 0
        "},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            tags = ["rust"]
            date = "2026-01-01T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    assert!(
        output_dir.join("index.html").exists(),
        "should build home page"
    );
    assert!(
        output_dir.join("posts").join("index.html").exists(),
        "should build posts index"
    );
    assert!(
        output_dir
            .join("posts")
            .join("note")
            .join("index.html")
            .exists(),
        "should build section page"
    );
    assert!(
        output_dir
            .join("tags")
            .join("rust")
            .join("index.html")
            .exists(),
        "should build tag archive page"
    );
}

// ── build: RSS feeds ──

#[test]
fn build_generates_rss_feeds() {
    let root = tempfile::tempdir().unwrap();
    write_listing_site(root.path());

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    for (path, expected) in [
        (
            "index.xml",
            vec![
                "posts/review/latest",
                "posts/note/newer",
                "posts/note/older",
            ],
        ),
        (
            "posts/index.xml",
            vec![
                "posts/review/latest",
                "posts/note/newer",
                "posts/note/older",
            ],
        ),
        (
            "posts/note/index.xml",
            vec!["posts/note/newer", "posts/note/older"],
        ),
        ("posts/review/index.xml", vec!["posts/review/latest"]),
        (
            "tags/rust/index.xml",
            vec!["about", "posts/review/latest", "posts/note/older"],
        ),
        ("tags/testing/index.xml", vec!["posts/note/newer"]),
    ] {
        let xml = fs::read_to_string(output_dir.join(path)).unwrap();
        let links: Vec<_> = xml
            .split("<item>")
            .skip(1)
            .map(|item| {
                item.split_once("<link>")
                    .unwrap()
                    .1
                    .split_once("</link>")
                    .unwrap()
                    .0
            })
            .collect();
        let expected: Vec<_> = expected
            .iter()
            .map(|path| format!("https://example.com/{path}/"))
            .collect();
        assert_eq!(links, expected, "incorrect membership or order in {path}");
    }

    let main_feed = fs::read_to_string(output_dir.join("index.xml")).unwrap();
    assert!(main_feed.contains("<title>Test Site</title>"));
}

#[test]
fn build_rss_feed_empty_site() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let main_feed = fs::read_to_string(output_dir.join("index.xml")).unwrap();
    assert!(
        !main_feed.contains("<item>"),
        "empty site should have no items, xml:\n{main_feed}"
    );
    assert!(
        !main_feed.contains("<lastBuildDate>"),
        "empty site should have no lastBuildDate, xml:\n{main_feed}"
    );
}

// ── build: sitemap + robots.txt ──

#[test]
fn build_generates_sitemap_and_robots() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            date = "2026-01-15T00:00:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");

    let sitemap = fs::read_to_string(output_dir.join("sitemap.xml")).unwrap();
    assert!(
        sitemap.contains("<loc>https://example.com/</loc>"),
        "sitemap should contain home URL, xml:\n{sitemap}"
    );
    assert!(
        sitemap.contains("<loc>https://example.com/posts/hello/</loc>"),
        "sitemap should contain post URL, xml:\n{sitemap}"
    );
    assert!(
        sitemap.contains("<lastmod>"),
        "sitemap should have lastmod for dated page, xml:\n{sitemap}"
    );

    let robots = fs::read_to_string(output_dir.join("robots.txt")).unwrap();
    assert!(
        robots.contains("Sitemap: https://example.com/sitemap.xml"),
        "robots.txt should reference sitemap, txt:\n{robots}"
    );
}

// ── build: 404 page ──

#[test]
fn build_generates_404_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    let html = fs::read_to_string(output_dir.join("404.html")).unwrap();
    assert!(
        html.contains("404 Not Found"),
        "should contain error message, html:\n{html}"
    );
}

#[test]
fn build_skips_404_without_template() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();

    let templates = root.path().join("templates");
    copy_templates(&templates);
    fs::remove_file(templates.join("404.html")).unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    assert!(
        !output_dir.join("404.html").exists(),
        "should not generate 404.html without template"
    );
}

// ── build: errors ──

#[test]
fn build_invalid_config_returns_error() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "{{invalid toml").unwrap();

    let err = format!(
        "{:#}",
        build(root.path(), BuildOptions::default()).unwrap_err()
    );
    assert!(
        err.contains("failed to load config"),
        "should report config failure, got: {err}"
    );
}

#[test]
fn build_invalid_timezone_returns_error() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), r#"timezone = "Mars/Base""#).unwrap();

    let err = build(root.path(), BuildOptions::default()).unwrap_err();
    let chain: Vec<String> = err.chain().map(ToString::to_string).collect();
    assert!(
        chain
            .iter()
            .any(|message| message.contains("invalid timezone `Mars/Base` in config.toml")),
        "should report invalid timezone, got: {chain:?}"
    );
}

#[test]
fn build_missing_templates_returns_error() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();

    let err = build(root.path(), BuildOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("failed to initialize template engine"),
        "should report template engine failure, got: {err}"
    );
}

#[test]
fn build_broken_post_template_returns_error() {
    assert_broken_template_fails("post.html");
}

#[test]
fn build_broken_archive_template_returns_error() {
    assert_broken_template_fails("archive.html");
}

#[test]
fn build_broken_overview_template_returns_error() {
    assert_broken_template_fails("overview.html");
}

#[test]
fn build_broken_directive_template_returns_error() {
    let root = tempfile::tempdir().unwrap();
    setup_site_with_page(root.path());

    let directives = root.path().join("templates").join("directives");
    fs::create_dir_all(&directives).unwrap();
    fs::write(
        directives.join("broken.html"),
        "{% for k, v in name | items %}{{ k }}{% endfor %}",
    )
    .unwrap();

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            ::: broken
            Body
            :::
        "#},
    );

    let err = build(root.path(), BuildOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("failed to render"),
        "should report render failure, got: {err}"
    );
}

#[cfg(unix)]
#[test]
fn build_staging_permission_denied_preserves_output() {
    let root = tempfile::tempdir().unwrap();
    setup_site_with_page(root.path());

    build(root.path(), BuildOptions::default()).unwrap();
    let output_dir = root.path().join("public");
    let previous = fs::read_to_string(output_dir.join("posts/hello/index.html")).unwrap();
    let _guard = PermissionGuard::restrict(root.path(), 0o555);

    let err = build(root.path(), BuildOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("failed to create build staging directory"),
        "should report staging failure, got: {err}"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("posts/hello/index.html")).unwrap(),
        previous
    );
}

#[test]
fn build_asset_copy_permission_denied_returns_error() {
    let root = tempfile::tempdir().unwrap();
    setup_site_with_page(root.path());

    let page_dir = root.path().join("content").join("posts").join("hello");
    let asset = page_dir.join("image.png");
    fs::write(&asset, "img-data").unwrap();
    let _guard = PermissionGuard::restrict(&asset, 0o000);

    let err = build(root.path(), BuildOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("failed to copy asset"),
        "should report asset copy failure, got: {err}"
    );
}

// ── Site fixtures ──

fn setup_site_with_page(root: &Path) {
    fs::write(root.join("config.toml"), "").unwrap();
    copy_templates(&root.join("templates"));
    write_page(
        root,
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );
}

fn setup_theme(root: &Path, theme_name: &str) {
    let theme_dir = root.join("themes").join(theme_name);
    let tmpl_dir = theme_dir.join("templates");
    fs::create_dir_all(&tmpl_dir).unwrap();
    copy_templates(&tmpl_dir);
    fs::write(theme_dir.join("theme.toml"), "").unwrap();
}

fn write_paginated_posts(root: &Path, post_dir: &str, tag: Option<&str>) {
    let tags = tag.map_or_else(String::new, |tag| format!(r#"tags = ["{tag}"]"#));
    for i in 1..=3 {
        write_page(
            root,
            &format!("{post_dir}/post-{i}"),
            &formatdoc! {r#"
                +++
                title = "Post {i}"
                date = "2026-01-0{i}T00:00:00Z"
                {tags}
                +++
                Body
            "#},
        );
    }

    write_page(
        root,
        "about",
        indoc! {r#"
            +++
            title = "About"
            tags = ["rust"]
            +++
            Body
        "#},
    );
}

fn write_listing_site(root: &Path) {
    fs::write(
        root.join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"
        "#},
    )
    .unwrap();
    copy_templates(&root.join("templates"));

    for (path, title, tags, date, weight) in [
        (
            "posts/note/older",
            "Post A",
            "rust",
            "2026-01-01",
            "weight = 1",
        ),
        ("posts/note/newer", "Post B", "testing", "2026-01-02", ""),
        ("posts/review/latest", "Post C", "rust", "2026-01-03", ""),
        ("about", "About", "rust", "2026-01-04", ""),
    ] {
        write_page(
            root,
            path,
            &formatdoc! {r#"
                +++
                title = "{title}"
                tags = ["{tags}"]
                date = "{date}T00:00:00Z"
                {weight}
                +++
                Body
            "#},
        );
    }
}

fn write_page(root: &Path, rel_path: &str, content: &str) {
    write_test_file(root, &format!("content/{rel_path}/index.md"), content);
}

fn copy_templates_except(dest: &Path, exclude: &[&str]) {
    copy_templates(dest);
    for name in exclude {
        fs::remove_file(dest.join(name)).unwrap();
    }
}

// ── Listing assertions ──

fn assert_paginated_listing(
    output_dir: &Path,
    page1_links: &[&str],
    page2_links: &[&str],
    navigation_urls: Option<(&str, &str)>,
) {
    let html1 = fs::read_to_string(output_dir.join("index.html")).unwrap();
    let html2 = fs::read_to_string(output_dir.join("page/2/index.html")).unwrap();

    assert_eq!(listing_links(&html1), page1_links);
    assert_eq!(listing_links(&html2), page2_links);
    assert!(html1.contains("Page 1 / 2"));
    assert!(html2.contains("Page 2 / 2"));
    assert!(!output_dir.join("page/3/index.html").exists());
    if let Some((next_url, prev_url)) = navigation_urls {
        assert!(html1.contains(&format!(r#"<a href="{next_url}">Next →</a>"#)));
        assert!(html2.contains(&format!(r#"<a href="{prev_url}">← Prev</a>"#)));
        assert!(!html1.contains("← Prev"));
        assert!(!html2.contains("Next →"));
    }
}

fn listing_links(html: &str) -> Vec<&str> {
    html.split("<li>")
        .skip(1)
        .map(|item| {
            let start = item.find("<a ").unwrap();
            let end = item.find("</a>").unwrap() + "</a>".len();
            &item[start..end]
        })
        .collect()
}

// ── Stylesheet assertions ──

fn stylesheet_url(html: &str) -> String {
    let document = Html::parse_document(html);
    let selector = Selector::parse(r#"link[rel="stylesheet"]"#).unwrap();
    document
        .select(&selector)
        .next()
        .unwrap()
        .value()
        .attr("href")
        .unwrap()
        .to_owned()
}

fn published_path(root: &Path, url: &str) -> PathBuf {
    root.join(url.strip_prefix("/subsite/").unwrap())
}

// ── Error assertions ──

fn assert_broken_template_fails(template_name: &str) {
    let root = tempfile::tempdir().unwrap();
    setup_site_with_page(root.path());

    fs::write(
        root.path().join("templates").join(template_name),
        "{% invalid %}",
    )
    .unwrap();

    let err = build(root.path(), BuildOptions::default())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("failed to render"),
        "should report render failure for {template_name}, got: {err}"
    );
}
