use std::fs;

use indoc::indoc;
use scraper::{Html, Selector};

use kiln::build::{BuildOptions, build};
use kiln::content::discovery::discover_content;
use kiln::convert::convert;

#[path = "support/filesystem.rs"]
mod filesystem;

use filesystem::write_test_file;

// ── convert ──

#[test]
fn convert_build_preserves_callout_and_bundle_ownership() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    write_test_file(
        &source,
        "content/posts/parent/index.MD",
        indoc! {r#"
            ---
            title: Parent
            ---
            Before {{< admonition type="warning" title="A \"quoted\" title" open=false >}}Keep **this body**.{{< /admonition >}} After
        "#},
    );
    write_test_file(
        &source,
        "content/posts/parent/images/public.txt",
        "public asset",
    );
    write_test_file(
        &source,
        "content/posts/parent/draft/index.MD",
        indoc! {r"
            ---
            title: Draft
            draft: true
            ---
            Private body
        "},
    );
    write_test_file(
        &source,
        "content/posts/parent/draft/secret.pdf",
        "private asset",
    );
    write_test_file(
        &source,
        "content/posts/parent/child/index.md",
        indoc! {r"
            ---
            title: Child
            ---
            Public child
        "},
    );
    write_test_file(
        &source,
        "content/posts/parent/child/child.txt",
        "child asset",
    );
    write_test_file(
        &source,
        "content/posts/parent/NOTES.MD",
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
    write_test_file(
        &destination,
        "templates/post.html",
        "<article>{{ content | safe }}</article>",
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
fn convert_overlapping_roots_return_error_without_writes() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("site");
    write_test_file(&source, "content/post.md", "Source body");
    for destination in [
        source.clone(),
        source.join("nested"),
        root.path().to_owned(),
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
fn convert_malformed_frontmatter_returns_error_without_output_file() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    for content in [
        "---\ntitle: Missing close",
        "---broken\ntitle: Invalid opening\n---",
    ] {
        write_test_file(&source, "content/post.md", content);
        let error = convert(&source, &destination).unwrap_err();
        assert!(
            error.to_string().contains("malformed YAML frontmatter"),
            "{error}"
        );
        assert!(!destination.join("content/post.md").exists());
    }
}

#[cfg(unix)]
#[test]
fn convert_source_file_symlink_copies_contents() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    fs::create_dir_all(source.join("content")).unwrap();
    fs::write(root.path().join("outside.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(
        root.path().join("outside.txt"),
        source.join("content/link.txt"),
    )
    .unwrap();
    convert(&source, &destination).unwrap();
    assert_eq!(
        fs::read_to_string(destination.join("content/link.txt")).unwrap(),
        "outside"
    );
    assert!(
        !fs::symlink_metadata(destination.join("content/link.txt"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn convert_destination_symlink_escape_returns_error() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    let outside = root.path().join("outside");
    write_test_file(&source, "content/new.txt", "new");
    fs::create_dir_all(&destination).unwrap();
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, destination.join("content")).unwrap();
    let error = convert(&source, &destination).unwrap_err();
    assert!(error.to_string().contains("escapes its root"), "{error}");
    assert!(!outside.join("new.txt").exists());
}
