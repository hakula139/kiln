use std::fs;

use indoc::indoc;
use sha2::{Digest, Sha256};

use kiln::build::{BuildOptions, build};

use super::support::{
    copy_templates, copy_templates_except, published_path, stylesheet_url, write_page,
    write_test_file,
};

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

#[cfg(unix)]
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

#[cfg(unix)]
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
