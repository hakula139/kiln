use std::fs;
use std::path::Path;

use indoc::{formatdoc, indoc};

#[path = "support/cli.rs"]
mod support;

#[cfg(unix)]
use support::write_executable_file;
use support::{kiln, write_test_file};

// ── build ──

#[test]
fn build_summary_skips_missing_templates_and_disabled_search_with_minify() {
    let root = tempfile::tempdir().unwrap();
    write_site(root.path());

    let output = kiln()
        .args(["build", "--minify", "--root"])
        .arg(root.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("Built 3 pages (3 content) in "), "{stderr}");
    assert!(!stderr.contains("Search indexing:"), "{stderr}");
    assert!(
        stderr
            .lines()
            .any(|line| line.starts_with("minified 4 files,")),
        "{stderr}"
    );

    assert!(!root.path().join("public/index.html").exists());
    assert!(!root.path().join("public/404.html").exists());
    assert!(root.path().join("public/posts/topic/a/index.html").exists());
}

#[cfg(unix)]
#[test]
fn build_summary_counts_generated_pages_and_includes_search_time() {
    let root = tempfile::tempdir().unwrap();
    write_site(root.path());
    for name in ["home", "archive", "overview", "404"] {
        write_test_file(
            root.path(),
            &format!("templates/{name}.html"),
            "<p>Generated page</p>",
        );
    }

    let binary = write_executable_file(
        root.path(),
        "pagefind",
        indoc! {r"
            #!/bin/sh
            sleep 0.05
            echo 'Finished in 999 seconds'
            echo 'Search warning' >&2
        "},
    );

    let config = root.path().join("config.toml");
    let mut config_text = fs::read_to_string(&config).unwrap();
    config_text.push_str(&formatdoc! {r#"

        [search]
        enabled = true
        binary = "{}"
    "#, binary.display()});
    fs::write(config, config_text).unwrap();

    for log_filter in [None, Some("kiln::search=debug")] {
        let mut command = kiln();
        command.args(["build", "--root"]).arg(root.path());
        if let Some(value) = log_filter {
            command.env("RUST_LOG", value);
        }
        let output = command.output().unwrap();

        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{stderr}");
        let summary = stderr.lines().find(|s| s.starts_with("Built ")).unwrap();
        assert!(
            summary.starts_with("Built 14 pages (3 content) in "),
            "{stderr}"
        );
        let html_count = walkdir::WalkDir::new(root.path().join("public"))
            .into_iter()
            .map(Result::unwrap)
            .filter(|entry| {
                entry.file_type().is_file()
                    && entry.path().extension().is_some_and(|ext| ext == "html")
            })
            .count();
        assert_eq!(html_count, 15);
        assert!(
            !root
                .path()
                .join("public/posts/topic/draft/index.html")
                .exists()
        );

        let total: f64 = summary
            .split(" in ")
            .nth(1)
            .unwrap()
            .trim_end_matches("s.")
            .parse()
            .unwrap();
        let search: f64 = stderr
            .lines()
            .find_map(|s| s.strip_prefix("Search indexing: "))
            .unwrap()
            .trim_end_matches("s.")
            .parse()
            .unwrap();
        assert!(search >= 0.05, "{stderr}");
        assert!(total > search, "{stderr}");

        assert!(stderr.contains("Search warning"), "{stderr}");
        assert!(!stderr.contains("999 seconds"), "{stderr}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(
            stdout.contains("999 seconds"),
            log_filter.is_some(),
            "{stdout}"
        );
    }
}

fn write_site(root: &Path) {
    write_test_file(
        root,
        "config.toml",
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"

            [params]
            paginate = 2
        "#},
    );
    write_test_file(root, "templates/post.html", "{{ content | safe }}");
    write_test_file(root, "static/extra.html", "<p>Copied HTML</p>");
    for (slug, draft) in [("a", false), ("b", false), ("c", false), ("draft", true)] {
        write_test_file(
            root,
            &format!("content/posts/topic/{slug}/index.md"),
            &formatdoc! {r#"
                +++
                title = "Post {slug}"
                tags = ["example"]
                draft = {draft}
                +++

                Body for {slug}.
            "#},
        );
    }
}
