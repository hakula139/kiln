use std::fs;

use indoc::indoc;
use sha2::{Digest, Sha256};

use kiln::build::{BuildOptions, build};

use super::support::{setup_theme, write_page};

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

        assert_eq!(fs::read(output_dir.join(&fingerprinted)).unwrap(), bytes);
        let input = if name == "style.css" {
            site_static.join(name)
        } else {
            theme_dir.join("static").join(name)
        };
        assert!(bytes.len() < fs::read(input).unwrap().len());
        assert!(
            html.contains(&format!(r"/{fingerprinted}")),
            "html should reference {fingerprinted}, got:\n{html}"
        );
    }
    let css = fs::read_to_string(output_dir.join("style.css")).unwrap();
    assert!(css.contains(".site") && !css.contains(".theme"), "{css}");
    assert!(
        css.contains("color:red") || css.contains("color:#f00"),
        "{css}"
    );
    assert!(css.contains("margin:0"), "{css}");
    let js = fs::read_to_string(output_dir.join("app.js")).unwrap();
    assert!(js.contains("console.log"), "{js}");
}
