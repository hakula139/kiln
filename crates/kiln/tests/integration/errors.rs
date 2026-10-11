use std::fs;

use indoc::{formatdoc, indoc};

use kiln::build::BuildOptions;

#[cfg(unix)]
use super::fixtures::PermissionGuard;
use super::support::TestSite;

// ── build: errors ──

#[test]
fn build_invalid_config_returns_error() {
    let site = TestSite::empty();
    site.write("config.toml", "{{invalid toml");

    let err = format!("{:#}", site.build().unwrap_err());
    assert!(
        err.contains("failed to load config"),
        "should report config failure, got: {err}"
    );
}

#[test]
fn build_missing_templates_returns_error() {
    let site = TestSite::empty();
    site.write("config.toml", "");

    let err = site.build().unwrap_err().to_string();
    assert!(
        err.contains("failed to initialize template engine"),
        "should report template engine failure, got: {err}"
    );
}

#[test]
fn build_failure_preserves_previous_output_returns_error() {
    type BreakSite = fn(&TestSite);
    let cases: &[(&str, Option<&str>, BreakSite)] = &[
        ("failed to render", None, |site| {
            site.write("templates/post.html", "{% invalid %}");
        }),
        ("failed to render", None, |site| {
            site.write("templates/archive.html", "{% invalid %}");
        }),
        ("failed to render", None, |site| {
            site.write("templates/overview.html", "{% invalid %}");
        }),
        ("failed to render", None, |site| {
            site.write(
                "templates/directives/broken.html",
                "{% for k, v in name | items %}{{ k }}{% endfor %}",
            )
            .page(
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
        }),
        ("invalid featured image URL", None, |site| {
            site.page(
                "posts/hello",
                indoc! {r#"
                    +++
                    title = "Hello"
                    featured_image = { src = "http://[" }
                    +++
                    Updated body
                "#},
            );
        }),
        ("reserved asset directory", None, |site| {
            site.write("static/_assets/occupied.png", "occupied");
        }),
        ("base_url", Some("/blog"), |_| {}),
        ("base_url", Some("mailto:example@example.com"), |_| {}),
        ("base_url", Some("https://example.com/?q=1"), |_| {}),
        ("base_url", Some("https://example.com/#page"), |_| {}),
    ];
    for (index, &(expected, base_url_override, break_site)) in cases.iter().enumerate() {
        let site = TestSite::new("");
        site.titled_page("posts/hello", "Hello");
        site.build().unwrap();
        let previous = site.read("posts/hello/index.html");
        break_site(&site);

        let error = site
            .build_with(&BuildOptions {
                base_url_override,
                ..Default::default()
            })
            .unwrap_err();

        assert!(
            format!("{error:#}").contains(expected),
            "case {index}: {error:#}"
        );
        assert_eq!(
            site.read("posts/hello/index.html"),
            previous,
            "case {index}: {expected}, {base_url_override:?}"
        );
    }
}

#[test]
fn build_existing_file_output_returns_error() {
    let site = TestSite::new("");
    site.write("existing", "original");
    let output = site.path("existing");

    for (configured, output_dir_override) in
        [("existing", None), ("public", Some(output.as_path()))]
    {
        site.write("config.toml", &format!(r#"output_dir = "{configured}""#));

        let error = site
            .build_with(&BuildOptions {
                output_dir_override,
                ..Default::default()
            })
            .unwrap_err();

        assert!(error.to_string().contains("is not a directory"), "{error}");
        assert_eq!(fs::read_to_string(&output).unwrap(), "original");
        assert!(!site.output("").exists());
    }
}

#[test]
fn build_output_override_input_overlap_returns_error() {
    let site = TestSite::new("");
    for name in ["content", "assets", "static", ".git"] {
        site.write(&format!("{name}/sentinel.txt"), "original");
    }

    for name in ["content", "assets", "static", ".git"] {
        let output = site.path(name);
        let error = site
            .build_with(&BuildOptions {
                output_dir_override: Some(&output),
                ..Default::default()
            })
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
fn build_search_requires_pagefind_only_when_enabled() {
    let site = TestSite::new("");
    site.titled_page("posts/hello", "Hello");
    for enabled in [false, true] {
        site.write(
            "config.toml",
            &formatdoc! {r#"
                [search]
                enabled = {enabled}
                binary = "nonexistent-pagefind"
            "#},
        );

        let result = site.build();
        if enabled {
            let message = format!("{:#}", result.unwrap_err());
            assert!(message.contains("nonexistent-pagefind"), "{message}");
            assert!(message.contains("Is Pagefind installed?"), "{message}");
        } else {
            result.unwrap();
            assert!(!site.output("pagefind").exists());
            assert!(
                site.read("posts/hello/index.html")
                    .contains("<h1>Hello</h1>")
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn build_staging_permission_denied_preserves_output_returns_error() {
    let site = TestSite::new("");
    site.titled_page("posts/hello", "Hello");
    site.build().unwrap();
    let previous = site.read("posts/hello/index.html");
    let _guard = PermissionGuard::restrict(site.root(), 0o555);

    let err = site.build().unwrap_err().to_string();
    assert!(
        err.contains("failed to create build staging directory"),
        "should report staging failure, got: {err}"
    );
    assert_eq!(site.read("posts/hello/index.html"), previous);
}

#[cfg(unix)]
#[test]
fn build_asset_copy_permission_denied_returns_error() {
    let site = TestSite::new("");
    site.titled_page("posts/hello", "Hello")
        .write("content/posts/hello/image.png", "img-data");
    let _guard = PermissionGuard::restrict(&site.path("content/posts/hello/image.png"), 0o000);

    let err = site.build().unwrap_err().to_string();
    assert!(
        err.contains("failed to copy asset"),
        "should report asset copy failure, got: {err}"
    );
}
