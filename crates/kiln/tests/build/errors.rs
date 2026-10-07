use std::fs;

use indoc::{formatdoc, indoc};

use kiln::build::{BuildOptions, build};

#[cfg(unix)]
use super::support::PermissionGuard;
use super::support::{
    assert_broken_template_fails, copy_templates, setup_site_with_page, write_page,
};

// ── build: 404 page ──

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

#[cfg(unix)]
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

// ── build: publication ──

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
fn build_existing_file_output_returns_error() {
    let root = tempfile::tempdir().unwrap();
    copy_templates(&root.path().join("templates"));
    let output = root.path().join("existing");
    fs::write(&output, "original").unwrap();

    for (configured, output_dir_override) in
        [("existing", None), ("public", Some(output.as_path()))]
    {
        fs::write(
            root.path().join("config.toml"),
            format!(r#"output_dir = "{configured}""#),
        )
        .unwrap();

        let error = build(
            root.path(),
            BuildOptions {
                output_dir_override,
                ..BuildOptions::default()
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("is not a directory"), "{error}");
        assert_eq!(fs::read_to_string(&output).unwrap(), "original");
        assert!(!root.path().join("public").exists());
    }
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
fn build_invalid_base_url_returns_error_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("public");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("sentinel.txt"), "previous output").unwrap();
    for base_url in [
        "/blog",
        "mailto:example@example.com",
        "https://example.com/?q=1",
        "https://example.com/#page",
    ] {
        let error = build(
            root.path(),
            BuildOptions {
                base_url_override: Some(base_url),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("base_url"));
        assert_eq!(
            fs::read_to_string(output.join("sentinel.txt")).unwrap(),
            "previous output"
        );
    }
}

#[test]
fn build_search_requires_pagefind_only_when_enabled() {
    let root = tempfile::tempdir().unwrap();
    setup_site_with_page(root.path());
    for enabled in [false, true] {
        fs::write(
            root.path().join("config.toml"),
            formatdoc! {r#"
                [search]
                enabled = {enabled}
                binary = "nonexistent-pagefind-for-disabled-search"
            "#},
        )
        .unwrap();

        let result = build(root.path(), BuildOptions::default());
        if enabled {
            let message = format!("{:#}", result.unwrap_err());
            assert!(message.contains("nonexistent-pagefind-for-disabled-search"));
            assert!(message.contains("is Pagefind installed?"));
        } else {
            result.unwrap();
            assert!(!root.path().join("public/pagefind").exists());
            let html =
                fs::read_to_string(root.path().join("public/posts/hello/index.html")).unwrap();
            assert!(html.contains("Hello"));
        }
    }
}
