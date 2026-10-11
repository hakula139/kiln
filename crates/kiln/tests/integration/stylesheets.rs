use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;

use indoc::indoc;

use kiln::build::{BuildOptions, build};

use super::support::{TestSite, fingerprint, stylesheet_url};

// ── build: stylesheets ──

#[test]
fn build_keeps_root_page_css_separate_from_shared_css() {
    let site = TestSite::new("");
    site.remove("templates/home.html")
        .titled_page("", "Root")
        .write(
            "content/loose.md",
            indoc! {r#"
                +++
                title = "Loose"
                +++
                Body
            "#},
        )
        .write("assets/css/_src/style.css", ".shared { color: red; }")
        .write(
            "content/assets/css/_src/style.css",
            ".page { color: blue; }",
        );

    site.build().unwrap();

    let shared = site.read("assets/css/site.css");
    let page = site.read("assets/css/page.css");
    assert!(
        shared.contains(".shared") && !shared.contains(".page"),
        "{shared}"
    );
    assert!(
        page.contains(".page") && !page.contains(".shared"),
        "{page}"
    );
    let html = site.read("index.html");
    assert!(
        stylesheet_url(&html).contains("/assets/css/page."),
        "{html}"
    );
    assert!(!site.output("assets/css/_src").exists());
    let loose = site.read("loose/index.html");
    assert!(!loose.contains("stylesheet"), "{loose}");
    assert!(!site.output("loose/assets/css/page.css").exists());
}

#[test]
fn build_compiles_shared_css_with_site_override_and_theme_asset_urls() {
    let site = TestSite::empty();
    site.theme("")
        .templates("templates")
        .write(
            "themes/example/assets/css/_src/style.css",
            r#"@import "./parts/font.css"; .theme { color: red; }"#,
        )
        .write(
            "themes/example/assets/css/_src/parts/font.css",
            "@font-face { font-family: Example; src: url(../../../fonts/example.woff2?v=1#font); }",
        )
        .write("themes/example/assets/fonts/example.woff2", "theme-font");

    site.build().unwrap();

    let css = site.read("assets/css/site.css");
    assert!(
        css.contains(&format!(
            "../../_assets/assets/fonts/example.{}.woff2?v=1#font",
            fingerprint("theme-font")
        )),
        "{css}"
    );
    assert!(css.contains(".theme"), "{css}");

    site.write("assets/css/_src/style.css", ".site { color: blue; }");
    site.build().unwrap();
    let css = site.read("assets/css/site.css");
    assert!(css.contains(".site") && !css.contains(".theme"), "{css}");
    assert!(!site.output("assets/css/_src").exists());
}

#[test]
fn build_compiles_page_styles_with_private_sources_and_final_hashes() {
    let site = TestSite::new("");
    for page in ["example", "example/child", "other"] {
        site.titled_page(page, "Example");
    }
    site.write(
        "content/example/child/assets/css/_src/style.css",
        r#"@import "nested/colors.css";"#,
    )
    .write(
        "content/example/child/assets/css/_src/nested/colors.css",
        ".rating { color: red; & > span { background-image: url(../../../my%20icon.svg?time=12:00#icon:active); } }",
    )
    .write("content/example/child/assets/my icon.svg", "bundle-image")
    .write("content/other/style.css", ".ordinary { color: blue; }")
    .write("static/_custom/settings.txt", "published");

    site.build_with(&BuildOptions {
        base_url_override: Some("https://example.com/subsite"),
        minify: true,
        ..Default::default()
    })
    .unwrap();

    let html = site.read("example/child/index.html");
    let first = stylesheet_url(&html);
    let published = site.output(first.strip_prefix("/subsite/").unwrap());
    let css = fs::read_to_string(&published).unwrap();
    assert!(
        first.starts_with("/subsite/example/child/assets/css/page."),
        "{first}"
    );
    assert!(first.ends_with(&format!("page.{}.css", fingerprint(&css))));
    assert!(
        css.contains("../my%20icon.svg?time=12:00#icon:active"),
        "{css}"
    );
    assert!(css.contains("span"), "{css}");
    assert_eq!(css, site.read("example/child/assets/css/page.css"));
    assert!(!site.output("example/child/assets/css/_src").exists());
    for page in ["example", "other"] {
        let html = site.read(&format!("{page}/index.html"));
        assert!(!html.contains("stylesheet"), "{html}");
    }
    assert_eq!(
        site.read("example/child/assets/my icon.svg"),
        "bundle-image"
    );
    assert_eq!(site.read("_custom/settings.txt"), "published");

    site.write(
        "content/example/child/assets/css/_src/style.css",
        ".rating { color: blue; }",
    );
    site.build_with(&BuildOptions {
        minify: true,
        ..Default::default()
    })
    .unwrap();
    assert!(!published.exists());
    assert!(
        site.read("example/child/assets/css/page.css")
            .contains("#00f")
    );
}

#[test]
fn build_compiles_tailwind_with_shared_context_and_fresh_candidates() {
    let site = TestSite::with_prefix("site with spaces 世界 ");
    site.theme(indoc! {r#"
        [css]
        processor = "tailwind"
    "#})
    .templates("templates")
    .write(
        "themes/example/templates/unused.html",
        r#"<div class="underline"></div>"#,
    )
    .write(
        "themes/example/assets/css/_src/style.css",
        indoc! {r#"
            @import "tailwindcss" source(none);
            @import "./parts/font.css";
            @theme { --color-brand: #123456; }
            @utility theme-image { background-image: url(../../image.svg); }
        "#},
    )
    .write(
        "themes/example/assets/css/_src/parts/font.css",
        "@font-face { font-family: Example; src: url(../../../fonts/example.woff2?version=1#font); }",
    )
    .write("themes/example/assets/fonts/example.woff2", "font")
    .write("themes/example/assets/image.svg", "theme-image")
    .page(
        "example",
        indoc! {r#"
            +++
            title = "Example"
            +++
            ::: {.bg-brand}
            Text.
            :::
        "#},
    )
    .write(
        "content/example/assets/css/_src/style.css",
        indoc! {r#"
            @import "./nested/rating.css";
            .rating { @apply text-brand theme-image; & > span { @apply font-bold; } }
        "#},
    )
    .write(
        "content/example/assets/css/_src/nested/rating.css",
        ".image { background: url(../../../../image.svg?q=1#icon); }",
    )
    .write("content/example/image.svg", "image");

    site.build().unwrap();

    let shared = site.read("assets/css/site.css");
    assert!(
        shared.contains(".bg-brand") && shared.contains(".underline"),
        "{shared}"
    );
    assert!(
        shared.contains(&format!(
            "../../_assets/assets/fonts/example.{}.woff2?version=1#font",
            fingerprint("font")
        )),
        "{shared}"
    );
    let page = site.read("example/assets/css/page.css");
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

    let content = site.path("content/example/index.md");
    let markdown = fs::read_to_string(&content).unwrap();
    fs::write(content, markdown.replace(".bg-brand", ".text-brand")).unwrap();
    build(
        &site.root().canonicalize().unwrap(),
        &BuildOptions::default(),
    )
    .unwrap();
    let rebuilt = site.read("assets/css/site.css");
    assert!(rebuilt.contains(".text-brand"), "{rebuilt}");
    assert!(!rebuilt.contains(".bg-brand"), "{rebuilt}");
}

#[test]
fn build_with_invalid_tailwind_utility_returns_error() {
    let site = TestSite::new(indoc! {r#"
        [css]
        processor = "tailwind"
    "#});
    site.write(
        "assets/css/_src/style.css",
        indoc! {r#"
            @import "tailwindcss" source(none);
            .invalid { @apply kiln-unknown-utility; }
        "#},
    );

    let message = format!("{:#}", site.build().unwrap_err());
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
    let site = TestSite::new(indoc! {r#"
        [css]
        processor = "tailwind"
    "#});
    site.write(
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
    fs::create_dir(site.path("static")).unwrap();
    symlink(external.path(), site.path("static/shared")).unwrap();

    site.build().unwrap();

    let css = site.read("assets/css/site.css");
    assert!(css.contains("../../shared/image.svg"), "{css}");
    assert_eq!(site.read("shared/image.svg"), "external image");
}

#[cfg(unix)]
#[test]
fn build_compiles_tailwind_imports_through_public_bundle_aliases() {
    let site = TestSite::new(indoc! {r#"
        [css]
        processor = "tailwind"
    "#});
    site.titled_page("example", "Example")
        .write(
            "content/example/assets/css/_src/style.css",
            r#"@import "../../../public/vendor.css";"#,
        )
        .write(
            "content/example/_assets/vendor/vendor.css",
            ".image { background: url(image.svg); }",
        )
        .write("content/example/_assets/vendor/image.svg", "image");
    symlink(
        site.path("content/example/_assets/vendor"),
        site.path("content/example/public"),
    )
    .unwrap();

    site.build().unwrap();

    let css = site.read("example/assets/css/page.css");
    assert!(css.contains("../../public/image.svg"), "{css}");
    assert_eq!(site.read("example/public/image.svg"), "image");
    assert!(!site.output("example/_assets").exists());
}
