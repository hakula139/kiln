use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;

use indoc::{formatdoc, indoc};
use scraper::{Html, Selector};

use kiln::build::BuildOptions;
use kiln::render::lqip::{ImageConfig, ImageResolver};

use super::support::{TestSite, fingerprint, stylesheet_url};

// ── build: assets ──

#[test]
fn build_publishes_assets_with_owner_precedence_and_private_sources() {
    let site = TestSite::empty();
    site.theme("").templates("templates");
    for (path, value) in [
        ("themes/example/assets/theme.txt", "theme resource"),
        ("themes/example/static/theme.txt", "theme static"),
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
        ("static/favicon.ico", "icon"),
        ("static/images/logo.png", "logo"),
    ] {
        site.write(path, value);
    }
    site.build().unwrap();

    for (path, value) in [
        ("assets/theme.txt", "theme resource"),
        ("theme.txt", "theme static"),
        ("assets/theme-overlay.txt", "theme overlay"),
        ("assets/layers.txt", "site asset"),
        ("assets/overlay.txt", "site overlay"),
        ("_headers", "root headers"),
        ("favicon.ico", "icon"),
        ("images/logo.png", "logo"),
    ] {
        assert_eq!(site.read(path), value);
    }
    assert!(!site.output("assets/_secret.txt").exists());
    assert!(!site.output("assets/nested/_cache").exists());
}

#[test]
fn build_fingerprints_bundle_assets_after_static_collisions() {
    let site = TestSite::new("");
    site.titled_page("example", "Example");
    for (name, bundle, shared) in [
        ("app.js", "console.log('bundle');", "console.log('static');"),
        ("image.avif", "bundle-image", "static-image"),
    ] {
        site.write(&format!("content/example/assets/{name}"), bundle)
            .write(&format!("static/example/assets/{name}"), shared);
    }

    site.build_with(BuildOptions {
        minify: true,
        ..Default::default()
    })
    .unwrap();

    assert_eq!(site.read("example/assets/image.avif"), "bundle-image");
    assert_eq!(
        site.read(&format!(
            "_assets/example/assets/image.{}.avif",
            fingerprint("bundle-image")
        )),
        "bundle-image"
    );
    let js = site.read("example/assets/app.js");
    assert!(js.contains("bundle") && !js.contains("static"), "{js}");
    assert_eq!(
        site.read(&format!("example/assets/app.{}.js", fingerprint(&js))),
        js
    );
}

#[test]
fn build_image_urls_and_metadata_share_published_paths() {
    let site = TestSite::new(r#"base_url = "https://example.com/blog""#);
    site.write(
        "templates/page.html",
        indoc! {r#"
            <span style="--lqip-uri:url('{{ featured_image.lqip_uri | safe }}')">
              <img src="{{ featured_image.src | safe }}" width="{{ featured_image.width }}" height="{{ featured_image.height }}">
            </span>
            {{ content | safe }}
        "#},
    )
    .write_png("static/shared.png", 10, 20)
    .write_png("static/blog/shared.png", 30, 40)
    .write_png("content/a/photo.png", 8, 4)
    .write_png("static/a/photo.png", 12, 6);

    for (featured, alias) in [
        ("/shared.png?q=1#view", "/shared.png"),
        ("../a/photo.png?q=1#view", "/a/photo.png"),
    ] {
        site.page(
            "a",
            &formatdoc! {r#"
                +++
                title = "Example"
                slug = "b"
                featured_image = {{ src = {featured:?} }}
                +++
                ![Root](/blog/shared.png?q=1#view)

                Inline ![Relative](../a/photo.png?q=1#view) image.

                ![Relative block](../a/photo.png?q=1#view)

                Inline ![Outside](/shared.png) image.

                ![Outside prefix](/blogger/shared.png)
            "#},
        );

        site.build().unwrap();

        let resolver = ImageResolver::new(&site.output(""), ImageConfig::default());
        let expected: Vec<_> = [alias, "/shared.png", "/a/photo.png", "/a/photo.png"]
            .iter()
            .enumerate()
            .map(|(index, alias)| {
                let path = alias.trim_start_matches('/');
                let origin = if index == 0 {
                    "https://example.com"
                } else {
                    ""
                };
                let meta = resolver.resolve(alias, None).unwrap();
                (
                    format!(
                        "{origin}/blog/_assets/{}.{}.png?q=1#view",
                        path.strip_suffix(".png").unwrap(),
                        fingerprint(fs::read(site.output(path)).unwrap())
                    ),
                    Some(meta.width.to_string()),
                    Some(meta.height.to_string()),
                    Some(format!(
                        "--lqip-uri:url('{}')",
                        meta.lqip_uri.as_ref().unwrap()
                    )),
                )
            })
            .chain([
                ("/shared.png".into(), None, None, None),
                ("/blogger/shared.png".into(), None, None, None),
            ])
            .collect();
        let html = site.read("b/index.html");
        let document = Html::parse_document(&html);
        let actual: Vec<_> = document
            .select(&Selector::parse("img").unwrap())
            .map(|image| {
                let element = image.value();
                let style = image
                    .parent()
                    .and_then(scraper::ElementRef::wrap)
                    .and_then(|parent| parent.value().attr("style"));
                (
                    element.attr("src").unwrap().to_owned(),
                    element.attr("width").map(str::to_owned),
                    element.attr("height").map(str::to_owned),
                    style.map(str::to_owned),
                )
            })
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn build_updates_managed_image_urls_and_retires_previous_fingerprints() {
    let site = TestSite::new(r#"base_url = "https://example.com/blog""#);
    write_image_lifecycle_templates(&site);
    let mut previous: Option<(String, String)> = None;
    for (bytes, featured) in [
        ("original image", "photo%20%25.avif?quality=1#view"),
        ("changed image", "/example/photo%20%25.avif?quality=1#view"),
        ("changed image", "photo%20%25.avif?quality=1#view"),
    ] {
        write_image_lifecycle_page(&site, featured);
        site.write("content/example/photo %.avif", bytes);

        site.build_with(BuildOptions {
            minify: true,
            ..Default::default()
        })
        .unwrap();

        let hash = fingerprint(bytes);
        let path = format!("_assets/example/photo %.{hash}.avif");
        let url = format!("/blog/_assets/example/photo%20%25.{hash}.avif?quality=1#view");
        let html = site.read("example/index.html");
        let document = Html::parse_document(&html);
        let images: Vec<_> = document
            .select(&Selector::parse("img").unwrap())
            .map(|image| image.value().attr("src").unwrap())
            .collect();
        assert_eq!(
            images,
            [
                &format!("https://example.com{url}"),
                url.as_str(),
                "//example.org/logo.svg",
                url.as_str(),
                url.as_str(),
                "missing.avif",
                "https://example.org/image.avif",
                url.as_str(),
            ]
        );
        assert_eq!(site.read(&path), bytes);
        assert_eq!(site.read("example/photo %.avif"), bytes);
        let stylesheet = stylesheet_url(&html);
        let css = site.read(stylesheet.strip_prefix("/blog/").unwrap());
        assert!(css.contains(&format!("../..{}", url.strip_prefix("/blog").unwrap())));
        assert!(stylesheet.ends_with(&format!(".{}.css", fingerprint(&css))));

        if let Some((old_path, old_stylesheet)) = &previous {
            if old_path == &path {
                assert_eq!(old_stylesheet, &stylesheet);
            } else {
                assert!(!site.output(old_path).exists());
                assert_ne!(old_stylesheet, &stylesheet);
            }
        }
        previous = Some((path, stylesheet));
    }
}

fn write_image_lifecycle_templates(site: &TestSite) {
    site.write(
        "templates/page.html",
        indoc! {r#"
            <img id="featured" src="{{ featured_image.src }}">
            <img id="template" src="{{ asset_url('photo%20%25.avif?quality=1#view') }}">
            <img id="external" src="{{ asset_url('//example.org/logo.svg') }}">
            <link rel="stylesheet" href="{{ asset_url('/assets/css/site.css') }}">
            {{ content | safe }}
        "#},
    )
    .write(
        "templates/directives/link.html",
        indoc! {r#"
            {% set url = named_args.url %}
            <a href="{{ url }}"><img src="{{ asset_url(named_args.logo) }}"></a>
        "#},
    )
    .write(
        "assets/css/_src/style.css",
        r#".cover { background: url("/blog/example/photo%20%25.avif?quality=1#view"); }"#,
    );
}

fn write_image_lifecycle_page(site: &TestSite, featured: &str) {
    site.page(
        "example",
        &formatdoc! {r#"
            +++
            title = "Example"
            featured_image = {{ src = {featured:?} }}
            +++
            ![Block](./photo%20%25.avif?quality=1#view)

            Inline ![Inline](../example/photo%20%25.avif?quality=1#view) image.

            ![Missing](missing.avif)

            ![External](https://example.org/image.avif)

            ::: box
            ::: link {{url="https://example.org/target/" logo="photo%20%25.avif?quality=1#view"}}
            :::
            :::
        "#},
    );
}

#[test]
fn build_reads_image_dimensions_from_published_theme_and_site_assets() {
    let site = TestSite::empty();
    site.theme("").templates("templates");
    site.write_png("themes/example/assets/images/theme.png", 3, 5)
        .write_png("themes/example/assets/images/override.png", 7, 9)
        .write_png("assets/images/override.png", 11, 13)
        .page(
            "example",
            indoc! {r#"
                +++
                title = "Example"
                +++
                ![Theme](/assets/images/theme.png)

                ![Site](/assets/images/override.png)
            "#},
        );

    site.build().unwrap();

    let html = site.read("example/index.html");
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

#[cfg(unix)]
#[test]
fn build_materializes_external_bundle_symlinks() {
    let site = TestSite::new("");
    site.titled_page("posts/example", "Post A");
    let bundle = site.path("content/posts/example");
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
    symlink(external.path(), bundle.join("shared")).unwrap();
    symlink(
        external.path().join("caption.txt"),
        bundle.join("caption.txt"),
    )
    .unwrap();
    fs::create_dir_all(bundle.join("_cache")).unwrap();
    symlink(
        external.path().join("missing"),
        bundle.join("_cache/broken"),
    )
    .unwrap();
    symlink(external.path(), bundle.join("_private")).unwrap();
    site.write("static/icon.svg", "static image").write(
        "content/posts/example/assets/css/_src/style.css",
        indoc! {r#"
            @import "../../../shared/vendor.css";
            .image { background: url(../../../shared/nested/image.svg); }
            .caption { background: url(../../../caption.txt); }
            .static { background: url(../../../../../../static/icon.svg); }
        "#},
    );

    for processor in ["plain", "tailwind"] {
        site.write(
            "config.toml",
            &formatdoc! {r#"
                [css]
                processor = "{processor}"
            "#},
        );
        site.build().unwrap();

        let output = site.output("posts/example");
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
