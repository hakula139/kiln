use std::fs;
use std::path::Path;
use std::process::Command;

use indoc::{formatdoc, indoc};

use super::support::{TestSite, stylesheet_urls};

// ── build: CLI ──

#[test]
fn build_base_url_argument_and_environment_precedence() {
    let site = TestSite::empty();
    site.write("config.toml", r#"base_url = "https://config.example.com""#)
        .write("templates/post.html", "{{ url }}")
        .titled_page("about", "About");

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
        command.arg("build").current_dir(site.root());
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
        assert_eq!(
            site.read("about/index.html"),
            expected,
            "argument: {argument:?}, environment: {environment:?}"
        );
    }
}

#[test]
fn build_summary_skips_missing_templates_and_disabled_search_with_minify() {
    let site = summary_site();

    let output = kiln()
        .args(["build", "--minify", "--root"])
        .arg(site.root())
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
    assert!(!site.output("index.html").exists());
    assert!(!site.output("404.html").exists());
    assert!(site.output("posts/topic/a/index.html").exists());
}

#[cfg(unix)]
#[test]
fn build_summary_counts_generated_pages_and_includes_search_time() {
    let site = summary_site();
    for name in ["home", "archive", "overview", "404"] {
        site.write(&format!("templates/{name}.html"), "<p>Generated page</p>");
    }
    enable_search(
        &site,
        indoc! {r"
            #!/bin/sh
            sleep 0.05
            echo 'Finished in 999 seconds'
            echo 'Search warning' >&2
        "},
    );

    for log_filter in [None, Some("kiln::search=debug")] {
        let mut command = kiln();
        command.args(["build", "--root"]).arg(site.root());
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
        let html_count = walkdir::WalkDir::new(site.output(""))
            .into_iter()
            .map(Result::unwrap)
            .filter(|entry| {
                entry.file_type().is_file()
                    && entry.path().extension().is_some_and(|ext| ext == "html")
            })
            .count();
        assert_eq!(html_count, 15);
        assert!(!site.output("posts/topic/draft/index.html").exists());

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
        assert_eq!(
            stderr.contains("999 seconds"),
            log_filter.is_some(),
            "{stderr}"
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
    }
}

#[cfg(unix)]
#[test]
fn build_search_passes_output_dir_as_single_argument() {
    let site = search_site(indoc! {r#"
        #!/bin/sh
        test -f "$2/posts/hello/index.html" || exit 8
        printf '%s\n' "$@" > "$2/arguments.txt"
    "#});

    let output = kiln()
        .arg("build")
        .current_dir(site.root())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let arguments = site.read("arguments.txt");
    let arguments: Vec<_> = arguments.lines().collect();
    let [flag, staged] = arguments.as_slice() else {
        panic!("unexpected Pagefind arguments: {arguments:?}");
    };
    assert_eq!(*flag, "--site");
    let staged = Path::new(staged);
    assert_eq!(
        staged.parent().unwrap().parent().unwrap(),
        site.root().canonicalize().unwrap()
    );
    assert!(!staged.exists());
    assert!(site.output("posts/hello/index.html").is_file());
}

#[cfg(unix)]
#[test]
fn build_search_failure_preserves_output_returns_error() {
    let site = search_site(indoc! {r"
        #!/bin/sh
        echo 'Backend output'
        echo 'Backend diagnostic' >&2
        exit 7
    "});
    site.write("public/sentinel.html", "Previous output");

    let output = kiln()
        .arg("build")
        .current_dir(site.root())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(site.read("sentinel.html"), "Previous output");
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

#[cfg(unix)]
#[test]
fn build_tailwind_failure_waits_for_all_compilers_returns_error() {
    let site = TestSite::new(indoc! {r#"
        [css]
        processor = "tailwind"
    "#});
    site.write("assets/css/_src/style.css", ".shared {}")
        .titled_page("first", "First")
        .write("content/first/assets/css/_src/style.css", ".first {}")
        .titled_page("second", "Second")
        .write("content/second/assets/css/_src/style.css", ".second {}");
    let finished = site.path("finished.txt");
    site.write_executable(
        "bin/kiln-tailwindcss",
        &formatdoc! {r#"
            #!/bin/sh
            if grep -q content/second/ "$1"; then
              sleep 2
              echo second > '{}'
            elif grep -q content/first/ "$1"; then
              echo 'First failure' >&2
              exit 4
            else
              sleep 1
              echo 'Shared failure' >&2
              exit 3
            fi
        "#, finished.display()},
    );
    let mut path = vec![site.path("bin")];
    path.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));

    let output = kiln()
        .arg("build")
        .current_dir(site.root())
        .env("PATH", std::env::join_paths(path).unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    let shared = site
        .root()
        .canonicalize()
        .unwrap()
        .join("assets/css/_src/style.css");
    assert!(
        stderr.contains(&format!("failed to compile {}", shared.display())),
        "{stderr}"
    );
    assert!(
        stderr.contains("Tailwind CSS exited with exit status: 3: Shared failure"),
        "{stderr}"
    );
    assert!(!stderr.contains("First failure"), "{stderr}");
    assert_eq!(fs::read_to_string(finished).unwrap(), "second\n");
}

/// Creates a site with three tagged posts and a draft, paginated by two, whose only template
/// renders posts.
fn summary_site() -> TestSite {
    let site = TestSite::empty();
    site.write(
        "config.toml",
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"

            [params]
            paginate = 2
        "#},
    )
    .write("templates/post.html", "{{ content | safe }}")
    .write("static/extra.html", "<p>Copied HTML</p>");
    for (slug, draft) in [("a", false), ("b", false), ("c", false), ("draft", true)] {
        site.page(
            &format!("posts/topic/{slug}"),
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
    site
}

/// Creates a site under a root path containing spaces, with one post and search indexing by
/// a fake Pagefind running `script`.
#[cfg(unix)]
fn search_site(script: &str) -> TestSite {
    let site = TestSite::with_prefix("kiln cli ");
    site.write("config.toml", r#"base_url = "https://example.com""#)
        .write("templates/post.html", "{{ content | safe }}")
        .page(
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Searchable body.
            "#},
        );
    enable_search(&site, script);
    site
}

/// Appends a `[search]` table to `config.toml` that runs `script` as Pagefind.
#[cfg(unix)]
fn enable_search(site: &TestSite, script: &str) {
    let binary = site.write_executable("pagefind", script);
    let config = fs::read_to_string(site.path("config.toml")).unwrap();
    site.write(
        "config.toml",
        &formatdoc! {r#"
            {config}

            [search]
            enabled = true
            binary = "{}"
        "#, binary.display()},
    );
}

// ── convert: CLI ──

#[test]
fn convert_relative_paths_and_warns_with_file_path() {
    let site = TestSite::empty();
    site.write(
        "source/content/post.md",
        indoc! {r#"
            ---
            title: "Converted Post"
            aliases: [/old/]
            ---

            Converted body.

            {{< image src="a.webp" alt="Alt" caption="Caption" >}}
        "#},
    )
    .write(
        "source/content/other/_index.md",
        indoc! {"
            ---
            title: Other
            ---
        "},
    )
    .write("source/static/asset.txt", "Static asset");

    let output = kiln()
        .args(["convert", "--source", "source", "--dest", "destination"])
        .current_dir(site.root())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(site.path("destination/content/post.md")).unwrap(),
        indoc! {r#"
            +++
            title = "Converted Post"
            +++

            Converted body.

            ![Alt](a.webp)
        "#}
    );
    assert_eq!(
        fs::read_to_string(site.path("destination/static/asset.txt")).unwrap(),
        "Static asset"
    );
    let converted = fs::canonicalize(site.path("destination/content/post.md")).unwrap();
    let span = format!("convert{{path={}}}", converted.display());
    let logs = String::from_utf8_lossy(&output.stderr);
    for warning in [
        "frontmatter field requires manual migration",
        "image caption differs from alt",
    ] {
        assert!(
            logs.lines()
                .any(|line| line.contains(&span) && line.contains(warning)),
            "{warning}: {logs}"
        );
    }
    let index = fs::canonicalize(site.path("source/content/other/_index.md")).unwrap();
    let index_field = format!("path={}", index.display());
    assert!(
        logs.lines().any(|line| line.contains(&index_field)
            && line.contains("section index requires manual migration")),
        "{logs}"
    );
}

// ── init_theme ──

#[test]
fn init_theme_default_and_explicit_roots() {
    let site = TestSite::empty();
    site.write("site/.keep", "");

    for (argument, theme) in [
        (None, "themes/example"),
        (Some("site"), "site/themes/example"),
    ] {
        let mut command = kiln();
        command
            .args(["init-theme", "example"])
            .current_dir(site.root());
        if let Some(value) = argument {
            command.args(["--root", value]);
        }

        let output = command.output().unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(site.path(&format!("{theme}/theme.toml")).is_file());
        let stdout = String::from_utf8(output.stdout).unwrap();
        let created = stdout
            .lines()
            .find_map(|line| line.strip_prefix("Theme `example` created at "))
            .unwrap_or_else(|| panic!("missing creation message:\n{stdout}"));
        assert_eq!(
            Path::new(created).canonicalize().unwrap(),
            site.path(theme).canonicalize().unwrap()
        );
    }
}

#[test]
fn init_theme_scaffold_builds_styled_post() {
    let site = TestSite::empty();
    let init = kiln()
        .args(["init-theme", "example", "--root"])
        .arg(site.root())
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let entry = "themes/example/assets/css/_src/style.css";
    assert!(site.path(entry).is_file(), "scaffold should create {entry}");
    site.write(entry, ".site { color: red; }")
        .write(
            "config.toml",
            indoc! {r#"
                base_url = "https://example.com"
                title = "Test Site"
                theme = "example"
            "#},
        )
        .page(
            "posts/post",
            indoc! {r#"
                +++
                title = "Starter Post"
                +++

                Starter **body**.
            "#},
        )
        .write(
            "content/posts/post/assets/css/_src/style.css",
            ".page { color: blue; }",
        );

    let build = kiln()
        .args(["build", "--root"])
        .arg(site.root())
        .output()
        .unwrap();

    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let html = site.read("posts/post/index.html");
    assert!(
        html.contains("<title>Starter Post - Test Site</title>"),
        "{html}"
    );
    assert!(html.contains("<h1>Starter Post</h1>"), "{html}");
    assert!(html.contains("Starter <strong>body</strong>."), "{html}");
    let stylesheets: Vec<_> = stylesheet_urls(&html)
        .iter()
        .map(|url| site.read(url.trim_start_matches('/')))
        .collect();
    let [shared, page] = stylesheets.as_slice() else {
        panic!("expected shared and page stylesheets:\n{html}");
    };
    assert!(shared.contains(".site"), "{shared}");
    assert!(page.contains(".page"), "{page}");
}

// ── CLI command ──

/// Returns a `kiln` command isolated from the caller's base URL and log filter.
fn kiln() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kiln"));
    command.env_remove("KILN_BASE_URL").env_remove("RUST_LOG");
    command
}
