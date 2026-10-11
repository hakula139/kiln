use std::fs;

use indoc::indoc;
use scraper::{Html, Selector};

use kiln::build::BuildOptions;

use super::support::{TestSite, stylesheet_url};

// ── build: theme ──

#[test]
fn build_minifies_asset_scripts_before_content_addressing_template_urls() {
    let site = TestSite::empty();
    site.theme("")
        .templates("themes/example/templates")
        .write(
            "themes/example/templates/base.html",
            indoc! {r#"
                <!DOCTYPE html>
                <html>
                <head>
                  <link rel="stylesheet" href="{{ asset_url('/style.css') }}">
                  <script src="{{ asset_url('/app.js') }}"></script>
                  <script src="{{ asset_url('/assets/js/theme.js') }}"></script>
                  {%- for script in assets.scripts %}
                  <script src="{{ script.url }}" defer></script>
                  {%- endfor %}
                </head>
                <body>{% block body %}{% endblock %}</body>
                </html>
            "#},
        )
        .write(
            "themes/example/templates/directives/widget.html",
            r#"{{ register_script("/assets/js/theme.js") }}<div class="widget"></div>"#,
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
            "themes/example/assets/js/theme.js",
            "const label = 'a' + 'b'; console.log(label);\n",
        )
        .write(
            "static/style.css",
            ".site { color: #ff0000; margin: 0px; }\n",
        )
        .page(
            "posts/widget",
            indoc! {r#"
                +++
                title = "Widget"
                +++
                ::: widget
                :::
            "#},
        );

    site.build_with(&BuildOptions {
        minify: true,
        ..Default::default()
    })
    .unwrap();

    let html = site.read("index.html");
    let script = Selector::parse("script").unwrap();
    let scripts: Vec<_> = Html::parse_document(&html)
        .select(&script)
        .map(|script| script.value().attr("src").unwrap().to_owned())
        .collect();
    assert_eq!(stylesheet_url(&html), "/style.css");
    let theme_js = site.hashed_path("assets/js/theme.js");
    assert_eq!(scripts, ["/app.js".to_owned(), format!("/{theme_js}")]);
    let registered = Selector::parse("script[defer]").unwrap();
    let registered: Vec<_> = Html::parse_document(&site.read("posts/widget/index.html"))
        .select(&registered)
        .map(|script| script.value().attr("src").unwrap().to_owned())
        .collect();
    assert_eq!(registered, [format!("/{theme_js}")]);
    assert_eq!(
        site.read(&theme_js),
        r#"const label="ab";console.log("ab");"#
    );
    assert_eq!(site.read("style.css"), ".site{color:red;margin:0}");
    let js = site.read("app.js");
    assert!(js.contains("console.log"), "{js}");
    assert!(
        js.len()
            < fs::read(site.path("themes/example/static/app.js"))
                .unwrap()
                .len()
    );
    let mut code: Vec<_> = fs::read_dir(site.output(""))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with("app.") || name.starts_with("style."))
        .collect();
    code.sort();
    assert_eq!(code, ["app.js", "style.css"]);
}
