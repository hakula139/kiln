use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;

use indoc::indoc;
use scraper::{Html, Selector};

use kiln::build::{BuildOptions, build};
use kiln::content::discovery::discover_content;
use kiln::convert::convert;

use super::support::TestSite;

// ── convert: library ──

#[test]
fn convert_build_preserves_callout_and_bundle_ownership() {
    let site = TestSite::empty();
    let (source, destination) = (site.path("source"), site.path("destination"));
    site.write(
        "source/content/posts/parent/index.MD",
        indoc! {r#"
            ---
            title: Parent
            ---
            Before {{< admonition type="warning" title="A \"quoted\" title" open=false >}}Keep **this body**.{{< /admonition >}} After
        "#},
    )
    .write(
        "source/content/posts/parent/images/public.txt",
        "public asset",
    )
    .write(
        "source/content/posts/parent/draft/index.MD",
        indoc! {r"
            ---
            title: Draft
            draft: true
            ---
            Private body
        "},
    )
    .write(
        "source/content/posts/parent/draft/secret.pdf",
        "private asset",
    )
    .write(
        "source/content/posts/parent/child/index.md",
        indoc! {r"
            ---
            title: Child
            ---
            Public child
        "},
    )
    .write(
        "source/content/posts/parent/child/child.txt",
        "child asset",
    )
    .write(
        "source/content/posts/parent/NOTES.MD",
        "Unpublished notes",
    );
    convert(&source, &destination).unwrap();
    let discovered = discover_content(&destination).unwrap();
    let parent = discovered
        .pages
        .iter()
        .find(|page| page.frontmatter.title == "Parent")
        .unwrap();
    assert_eq!(
        parent.assets,
        [destination.join("content/posts/parent/images/public.txt")]
    );
    let child = discovered
        .pages
        .iter()
        .find(|page| page.frontmatter.title == "Child")
        .unwrap();
    assert_eq!(
        child.assets,
        [destination.join("content/posts/parent/child/child.txt")]
    );
    site.write(
        "destination/templates/post.html",
        "<article>{{ content | safe }}</article>",
    )
    .write(
        "destination/templates/directives/callout.html",
        r#"<details class="callout {{ type }}"{% if open %} open{% endif %}><summary>{{ title }}</summary>{{ body_html | safe }}</details>"#,
    );
    build(&destination, BuildOptions::default()).unwrap();

    let output = destination.join("public");
    let html = fs::read_to_string(output.join("posts/parent/index.html")).unwrap();
    assert_converted_callout(&html);
    assert_eq!(
        fs::read_to_string(output.join("posts/parent/images/public.txt")).unwrap(),
        "public asset"
    );
    assert_eq!(
        fs::read_to_string(output.join("posts/parent/child/child.txt")).unwrap(),
        "child asset"
    );
    assert!(!output.join("posts/parent/draft").exists());
    assert!(!output.join("posts/parent/NOTES.MD").exists());
    assert!(!output.join("posts/parent/index.MD").exists());
}

fn assert_converted_callout(html: &str) {
    let document = Html::parse_document(html);
    let details = document
        .select(&Selector::parse("details.callout.warning").unwrap())
        .next()
        .unwrap();
    assert!(details.value().attr("open").is_none());
    let title = details
        .select(&Selector::parse("summary").unwrap())
        .next()
        .unwrap();
    assert_eq!(title.text().collect::<String>(), "A \"quoted\" title");
    let body = details
        .select(&Selector::parse("strong").unwrap())
        .next()
        .unwrap();
    assert_eq!(body.text().collect::<String>(), "this body");
    let text = document.root_element().text().collect::<String>();
    assert!(text.contains("Before"), "{text}");
    assert!(text.contains("After"), "{text}");
}

#[test]
fn convert_overlapping_roots_without_writes_returns_error() {
    let site = TestSite::empty();
    site.write("site/content/post.md", "Source body");
    let source = site.path("site");
    for destination in [
        source.clone(),
        source.join("nested"),
        site.root().to_owned(),
    ] {
        let error = convert(&source, &destination).unwrap_err();
        assert!(error.to_string().contains("must not overlap"), "{error}");
    }
    assert_eq!(
        fs::read_to_string(source.join("content/post.md")).unwrap(),
        "Source body"
    );
    assert!(!source.join("nested").exists());
}

#[test]
fn convert_malformed_frontmatter_without_output_file_returns_error() {
    let site = TestSite::empty();
    let (source, destination) = (site.path("source"), site.path("destination"));
    for content in [
        indoc! {"
            ---
            title: Missing close
        "}
        .trim_end(),
        indoc! {"
            ---broken
            title: Invalid opening
            ---
        "},
    ] {
        site.write("source/content/post.md", content);
        let error = convert(&source, &destination).unwrap_err();
        assert!(
            error.to_string().contains("malformed YAML frontmatter"),
            "{content:?}: {error}"
        );
        assert!(!destination.join("content/post.md").exists());
    }
}

#[cfg(unix)]
#[test]
fn convert_source_file_symlink_copies_contents() {
    let site = TestSite::empty();
    let (source, destination) = (site.path("source"), site.path("destination"));
    site.write("outside.txt", "outside");
    fs::create_dir_all(source.join("content")).unwrap();
    symlink(site.path("outside.txt"), source.join("content/link.txt")).unwrap();

    convert(&source, &destination).unwrap();

    let converted = destination.join("content/link.txt");
    assert_eq!(fs::read_to_string(&converted).unwrap(), "outside");
    assert!(
        !fs::symlink_metadata(converted)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn convert_destination_symlink_escape_returns_error() {
    for directory in ["content", "content/sub"] {
        let site = TestSite::empty();
        let (source, destination) = (site.path("source"), site.path("destination"));
        let outside = site.path("outside");
        let link = destination.join(directory);
        site.write(&format!("source/{directory}/new.txt"), "new");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, &link).unwrap();

        let error = convert(&source, &destination).unwrap_err();

        assert!(error.to_string().contains("escapes its root"), "{error}");
        assert!(!outside.join("new.txt").exists());
    }
}
