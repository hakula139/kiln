use std::fs;

use indoc::indoc;

use kiln::build::BuildOptions;

use super::support::{TestSite, fingerprint};

// ── build: theme ──

#[test]
fn build_fingerprints_merged_static_assets_after_minification() {
    let site = TestSite::empty();
    site.theme("")
        .templates("themes/example/templates")
        .write(
            "themes/example/templates/base.html",
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
        .write(
            "themes/example/static/style.css",
            ".theme { color: blue; }\n",
        )
        .write(
            "themes/example/static/app.js",
            "const value = 1 + 2; console.log(value);\n",
        )
        .write(
            "static/style.css",
            ".site { color: #ff0000; margin: 0px; }\n",
        );

    site.build_with(BuildOptions {
        minify: true,
        ..Default::default()
    })
    .unwrap();

    let html = site.read("index.html");
    for (name, input) in [
        ("style.css", "static/style.css"),
        ("app.js", "themes/example/static/app.js"),
    ] {
        let minified = site.read(name);
        let (stem, extension) = name.split_once('.').unwrap();
        let fingerprinted = format!("{stem}.{}.{extension}", fingerprint(&minified));

        assert_eq!(site.read(&fingerprinted), minified);
        assert!(minified.len() < fs::read(site.path(input)).unwrap().len());
        assert!(
            html.contains(&format!("/{fingerprinted}")),
            "html should reference {fingerprinted}, got:\n{html}"
        );
    }
    assert_eq!(site.read("style.css"), ".site{color:red;margin:0}");
    assert!(site.read("app.js").contains("console.log"));
}
