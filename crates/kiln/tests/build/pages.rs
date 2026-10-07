use std::fs;

use indoc::{formatdoc, indoc};

use kiln::build::{BuildOptions, build};

use super::support::{copy_templates, write_page};

// ── build: page template ──

#[test]
fn build_uses_page_template_for_standalone() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("config.toml"), "").unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "about-me",
        indoc! {r#"
            +++
            title = "About Me"
            +++
            Hello world.
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let output = root
        .path()
        .join("public")
        .join("about-me")
        .join("index.html");
    assert!(output.exists(), "should generate about-me page");
    let html = fs::read_to_string(&output).unwrap();
    assert!(
        html.contains(r#"<article class="page">"#),
        "should use page.html template, html:\n{html}"
    );
}

#[test]
fn build_renders_dates_in_configured_timezone() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            timezone = "Asia/Shanghai"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));

    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            date = "2026-03-13T09:36:00Z"
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(
        root.path()
            .join("public")
            .join("posts")
            .join("note")
            .join("hello")
            .join("index.html"),
    )
    .unwrap();
    assert!(
        html.contains("2026-03-13T17:36:00+08:00"),
        "should render the configured time zone offset, html:\n{html}"
    );
    assert!(
        !html.contains("2026-03-13T09:36:00Z"),
        "should not leave the date in UTC, html:\n{html}"
    );
}

#[test]
fn build_exposes_updated_and_linked_tags_without_git() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            timezone = "Asia/Shanghai"
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));
    fs::write(
        root.path().join("templates/post.html"),
        indoc! {r#"
            {% if updated %}<time datetime="{{ updated }}">{{ updated[:10] }}</time>{% endif %}
            {% for tag in tags %}<a href="{{ tag.url | safe }}">{{ tag.name }}</a>{% endfor %}
        "#},
    )
    .unwrap();
    write_page(
        root.path(),
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            updated = "2026-03-13T22:36:00Z"
            tags = ["C++", "<script>"]
            +++
            Body
        "#},
    );

    build(root.path(), BuildOptions::default()).unwrap();

    let html = fs::read_to_string(root.path().join("public/posts/note/hello/index.html")).unwrap();
    assert!(html.contains(r#"<time datetime="2026-03-14T06:36:00+08:00">2026-03-14</time>"#));
    assert!(
        html.contains(r#"<a href="https://example.com/tags/c%2B%2B/">C++</a>"#),
        "html:\n{html}"
    );
    assert!(html.contains("&lt;script&gt;</a>"));
    assert!(!html.contains("<script>"));
}

#[test]
fn build_heading_numbering_is_per_page() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            base_url = "https://example.com"
            title = "Test Site"

            [params]
            heading_numbering = true
        "#},
    )
    .unwrap();
    copy_templates(&root.path().join("templates"));
    let pages = [
        (
            "numbered",
            "heading_numbering = true",
            "",
            "",
            Some(("1", "1.1")),
        ),
        (
            "continued",
            "heading_numbering = true",
            " {numbering-start=2}",
            "",
            Some(("2", "2.1")),
        ),
        (
            "child",
            "heading_numbering = true",
            " {numbering-start=2}",
            " {numbering-start=0}",
            Some(("2", "2.0")),
        ),
        (
            "another",
            "heading_numbering = true",
            "",
            "",
            Some(("1", "1.1")),
        ),
        (
            "disabled",
            "heading_numbering = false",
            " {numbering-start=bad}",
            "",
            None,
        ),
        ("default", "", " {numbering-start=bad}", "", None),
    ];
    for (slug, setting, root_attribute, child_attribute, _) in pages {
        write_page(
            root.path(),
            &format!("posts/{slug}"),
            &formatdoc! {r#"
                +++
                title = "Post"
                {setting}
                +++
                ## Section{root_attribute}
                ### Detail{child_attribute}
            "#},
        );
    }
    build(root.path(), BuildOptions::default()).unwrap();
    for (slug, _, _, _, numbers) in pages {
        let html = fs::read_to_string(root.path().join(format!("public/posts/{slug}/index.html")))
            .unwrap();
        if let Some((root, child)) = numbers {
            for (id, number, title, level) in [
                ("section", root, "Section", 2),
                ("detail", child, "Detail", 3),
            ] {
                assert!(html.contains(&format!(
                    r#"<h{level} id="{id}"><span class="heading-number">{number}</span> {title}</h{level}>"#
                )));
                assert!(html.contains(&format!(
                    r##"href="#{id}"><span class="heading-number">{number}</span> {title}</a>"##
                )));
            }
        } else {
            assert!(html.contains(r#"<h2 id="section">Section</h2>"#));
            assert!(!html.contains("heading-number"));
        }
    }
}

#[test]
fn build_validates_render_options_only_for_content() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("config.toml"),
        indoc! {r#"
            [params]
            emojis = "yes"
        "#},
    )
    .unwrap();
    let templates = root.path().join("templates");
    copy_templates(&templates);
    fs::write(templates.join("home.html"), "<h1>Empty site</h1>").unwrap();

    build(root.path(), BuildOptions::default()).unwrap();

    assert_eq!(
        fs::read_to_string(root.path().join("public/index.html")).unwrap(),
        "<h1>Empty site</h1>"
    );

    write_page(
        root.path(),
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            Body
        "#},
    );

    let err = build(root.path(), BuildOptions::default()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "failed to parse render options from [params]"
    );
    assert!(format!("{err:#}").contains("invalid type: string \"yes\", expected a boolean"));
    assert!(!root.path().join("public/posts/hello/index.html").exists());
}
