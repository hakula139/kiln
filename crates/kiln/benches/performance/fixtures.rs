use std::fmt::Write;
use std::fs;
use std::path::Path;
use std::process::Command;

use indoc::{formatdoc, indoc};
use tempfile::TempDir;

pub(super) const PROSE: &str = indoc! {r"
    ## Benchmark heading

    Benchmark paragraph with **emphasis**, a [link](https://example.com), and Unicode 字.

    - First item
    - Second item
"};

/// Posts per listing page.
pub(super) const PAGINATE: usize = 10;

/// Number of `Tag {n}` tags that partition the posts round-robin.
pub(super) const INDEX_TAGS: usize = 4;

/// Commit time recorded by [`Site::commit_to_git`].
pub(super) const COMMITTED: &str = "2025-06-01T00:00:00Z";

/// Owns a temporary site whose files are removed when dropped.
pub(super) struct Site {
    directory: TempDir,
}

impl Site {
    /// Writes a site of `pages` section posts using `body`, plus `params` as extra `[params]`
    /// settings, with search disabled.
    pub(super) fn new(pages: usize, body: &str, params: usize) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("content/posts/notes")).unwrap();
        fs::create_dir(root.join("templates")).unwrap();
        fs::create_dir(root.join("static")).unwrap();
        let mut config = formatdoc! {r#"
            base_url = "https://example.com"
            title = "Benchmark site"

            [search]
            enabled = false

            [params]
            paginate = {PAGINATE}
        "#};
        for (name, value) in extra_params(params) {
            _ = writeln!(config, "{name} = {value}");
        }
        fs::write(root.join("config.toml"), config).unwrap();
        write_templates(root);
        fs::write(root.join("static/style.css"), "body { color: #123456; }\n").unwrap();
        fs::write(root.join("static/script.js"), "const value = 42;\n").unwrap();

        for index in 0..pages {
            let date = if index == 0 {
                "2024-12-31"
            } else {
                "2024-01-01"
            };
            fs::write(
                root.join(format!("content/posts/notes/post-{index}.md")),
                formatdoc! {r#"
                    +++
                    title = "Post {index}"
                    date = {date}T00:00:00Z
                    tags = ["Common", "Tag {}"]
                    +++
                    {body}
                "#, index % INDEX_TAGS},
            )
            .unwrap();
        }
        Self { directory }
    }

    /// Enables Git commit timestamps and commits every site file at [`COMMITTED`].
    pub(super) fn commit_to_git(&self) {
        let root = self.root();
        let config = fs::read_to_string(root.join("config.toml")).unwrap();
        fs::write(
            root.join("config.toml"),
            format!("enable_git_info = true\n{config}"),
        )
        .unwrap();
        for args in [
            &["init", "-q"][..],
            &["add", "."],
            &["commit", "-qm", "Add benchmark site"],
        ] {
            let status = Command::new("git")
                .args(args)
                .current_dir(root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "Example")
                .env("GIT_AUTHOR_EMAIL", "example@example.com")
                .env("GIT_COMMITTER_NAME", "Example")
                .env("GIT_COMMITTER_EMAIL", "example@example.com")
                .env("GIT_AUTHOR_DATE", COMMITTED)
                .env("GIT_COMMITTER_DATE", COMMITTED)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed: {status}");
        }
    }

    /// Returns the site root accepted by the build CLI and configuration loader.
    pub(super) fn root(&self) -> &Path {
        self.directory.path()
    }
}

/// Generates string settings outside the supported rendering options.
pub(super) fn extra_params(count: usize) -> impl Iterator<Item = (String, toml::Value)> {
    (0..count).map(|index| {
        (
            format!("setting_{index}"),
            format!("Example value {index}").into(),
        )
    })
}

fn write_templates(root: &Path) {
    for (name, template) in [
        (
            "post",
            indoc! {r"
                <!DOCTYPE html><html><head><title>{{ title }}</title></head><body>
                <h1>{{ title }}</h1>{{ content | safe }}{{ toc | safe }}
                </body></html>
            "},
        ),
        (
            "home",
            indoc! {r#"
                <!DOCTYPE html><html><body>
                {% for page in pages %}<a href="{{ page.url }}">{{ page.title }}</a>{% endfor %}
                </body></html>
            "#},
        ),
        (
            "archive",
            indoc! {r#"
                <!DOCTYPE html><html><body><h1>{{ name }}</h1>
                {% for group in page_groups %}{% for page in group.pages %}
                <a href="{{ page.url }}">{{ page.title }}</a>
                {% endfor %}{% endfor %}
                </body></html>
            "#},
        ),
        (
            "overview",
            indoc! {r#"
                <!DOCTYPE html><html><body><h1>{{ kind }}</h1>
                {% for bucket in buckets %}<a href="{{ bucket.url }}">{{ bucket.name }}</a>{% endfor %}
                </body></html>
            "#},
        ),
        (
            "404",
            "<!DOCTYPE html><html><body>Page missing</body></html>",
        ),
    ] {
        fs::write(root.join(format!("templates/{name}.html")), template).unwrap();
    }
}
