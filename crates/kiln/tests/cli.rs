use std::fs;
use std::path::Path;

use indoc::{formatdoc, indoc};

#[path = "support/cli.rs"]
mod support;

use support::{kiln, write_executable_file, write_test_file};

// ── build ──

#[test]
fn build_base_url_argument_and_environment_precedence() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(
        root.path(),
        "config.toml",
        r#"base_url = "https://config.example.com""#,
    );
    write_test_file(root.path(), "templates/page.html", "{{ url | safe }}");
    write_test_file(
        root.path(),
        "content/about/index.md",
        indoc! {r#"
            +++
            title = "About"
            +++

            Body.
        "#},
    );

    for (argument, environment, expected) in [
        (None, None, "https://config.example.com/about/"),
        (
            None,
            Some("https://env.example.com"),
            "https://env.example.com/about/",
        ),
        (
            Some("https://arg.example.com"),
            Some("https://env.example.com"),
            "https://arg.example.com/about/",
        ),
        (None, Some(""), "https://config.example.com/about/"),
    ] {
        let mut command = kiln();
        command.arg("build").current_dir(root.path());
        if let Some(value) = argument {
            command.args(["--base-url", value]);
        }
        if let Some(value) = environment {
            command.env("KILN_BASE_URL", value);
        }

        let output = command.output().unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let html = fs::read_to_string(root.path().join("public/about/index.html")).unwrap();
        assert_eq!(
            html, expected,
            "argument: {argument:?}, environment: {environment:?}"
        );
    }
}

#[test]
fn build_search_passes_output_dir_as_single_argument() {
    let root = tempfile::Builder::new()
        .prefix("kiln cli ")
        .tempdir()
        .unwrap();
    write_search_site(
        root.path(),
        indoc! {r#"
            #!/bin/sh
            printf '%s\n' "$@" > "$2/arguments.txt"
        "#},
    );

    let output = kiln()
        .arg("build")
        .current_dir(root.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let arguments = fs::read_to_string(root.path().join("public/arguments.txt")).unwrap();
    let expected = root.path().canonicalize().unwrap().join("public");
    assert_eq!(
        arguments.lines().collect::<Vec<_>>(),
        ["--site", expected.to_str().unwrap()]
    );
}

#[test]
fn build_search_failure_preserves_output_returns_error() {
    let root = tempfile::tempdir().unwrap();
    write_search_site(
        root.path(),
        indoc! {r"
            #!/bin/sh
            echo 'Backend output'
            echo 'Backend diagnostic' >&2
            exit 7
        "},
    );

    let output = kiln()
        .arg("build")
        .current_dir(root.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("search indexing failed"), "{stderr}");
    assert!(
        stderr.contains("Pagefind exited with exit status: 7"),
        "{stderr}"
    );
    assert!(stderr.contains("Backend output"), "{stderr}");
    assert!(stderr.contains("Backend diagnostic"), "{stderr}");
    assert!(
        !stderr.lines().any(|line| line.starts_with("Built ")),
        "{stderr}"
    );
}

// ── convert ──

#[test]
fn convert_relative_source_and_new_destination() {
    let root = tempfile::tempdir().unwrap();
    write_test_file(
        root.path(),
        "source/content/post.md",
        indoc! {r#"
            ---
            title: "Converted Post"
            ---

            Converted body.
        "#},
    );
    write_test_file(root.path(), "source/static/asset.txt", "Static asset");

    let output = kiln()
        .args(["convert", "--source", "source", "--dest", "destination"])
        .current_dir(root.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let converted = fs::read_to_string(root.path().join("destination/content/post.md")).unwrap();
    assert!(converted.starts_with("+++"), "{converted}");
    assert!(
        converted.contains(r#"title = "Converted Post""#),
        "{converted}"
    );
    assert!(converted.contains("Converted body."), "{converted}");
    assert_eq!(
        fs::read_to_string(root.path().join("destination/static/asset.txt")).unwrap(),
        "Static asset"
    );
}

// ── init_theme ──

#[test]
fn init_theme_default_and_explicit_roots() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("site")).unwrap();

    for (argument, expected_root) in [
        (None, root.path().to_path_buf()),
        (Some("site"), root.path().join("site")),
    ] {
        let mut command = kiln();
        command
            .args(["init-theme", "example"])
            .current_dir(root.path());
        if let Some(value) = argument {
            command.args(["--root", value]);
        }

        let output = command.output().unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let theme = expected_root.join("themes/example");
        assert!(theme.join("theme.toml").is_file());
        let post = fs::read_to_string(theme.join("templates/post.html")).unwrap();
        assert!(post.contains("{{ content | safe }}"), "{post}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("Theme `example` created at "), "{stdout}");

        write_test_file(
            &expected_root,
            "config.toml",
            indoc! {r#"
                base_url = "https://example.com"
                title = "Test Site"
                theme = "example"
            "#},
        );
        write_test_file(
            &expected_root,
            "content/posts/post/index.md",
            indoc! {r#"
                +++
                title = "Starter Post"
                +++

                Starter **body**.
            "#},
        );

        let build = kiln()
            .args(["build", "--root"])
            .arg(&expected_root)
            .output()
            .unwrap();

        assert!(
            build.status.success(),
            "{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let html = fs::read_to_string(expected_root.join("public/posts/post/index.html")).unwrap();
        assert!(html.contains("<h1>Starter Post</h1>"), "{html}");
        assert!(html.contains("Starter <strong>body</strong>."), "{html}");
    }
}

fn write_search_site(root: &Path, script: &str) {
    write_test_file(root, "templates/post.html", "{{ content | safe }}");
    let binary = write_executable_file(root, "pagefind", script);
    write_test_file(
        root,
        "config.toml",
        &formatdoc! {r#"
            base_url = "https://example.com"

            [search]
            enabled = true
            binary = "{}"
        "#, binary.display()},
    );
}
