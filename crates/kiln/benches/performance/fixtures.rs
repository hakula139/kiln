use std::fs;
use std::path::Path;

use indoc::{formatdoc, indoc};
use tempfile::TempDir;

pub(super) const PROSE: &str = indoc! {r"
    ## Benchmark heading

    Benchmark paragraph with **emphasis**, a [link](https://example.com), and Unicode 字.

    - First item
    - Second item
"};

/// Owns a temporary site whose files are removed when dropped.
pub(super) struct Site {
    directory: TempDir,
}

impl Site {
    /// Creates `pages` section posts using `body`, with search and Git timestamps disabled.
    pub(super) fn new(pages: usize, body: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("content/posts/notes")).unwrap();
        fs::create_dir(root.join("templates")).unwrap();
        fs::create_dir(root.join("static")).unwrap();
        fs::write(
            root.join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
                title = "Benchmark site"

                [search]
                enabled = false

                [params]
                paginate = 10
            "#},
        )
        .unwrap();
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
                "#, index % 4},
            )
            .unwrap();
        }
        Self { directory }
    }

    /// Returns the site root accepted by the build CLI and configuration loader.
    pub(super) fn root(&self) -> &Path {
        self.directory.path()
    }
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
