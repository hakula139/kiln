use std::fs;

use kiln::build::{BuildOptions, build};

use super::support::write_listing_site;

// ── build: RSS feeds ──

#[test]
fn build_generates_rss_feeds() {
    let root = tempfile::tempdir().unwrap();
    write_listing_site(root.path());

    build(root.path(), BuildOptions::default()).unwrap();

    let output_dir = root.path().join("public");
    for (path, expected) in [
        (
            "index.xml",
            vec![
                "posts/review/latest",
                "posts/note/newer",
                "posts/note/older",
            ],
        ),
        (
            "posts/index.xml",
            vec![
                "posts/review/latest",
                "posts/note/newer",
                "posts/note/older",
            ],
        ),
        (
            "posts/note/index.xml",
            vec!["posts/note/newer", "posts/note/older"],
        ),
        ("posts/review/index.xml", vec!["posts/review/latest"]),
        (
            "tags/rust/index.xml",
            vec!["about", "posts/review/latest", "posts/note/older"],
        ),
        ("tags/testing/index.xml", vec!["posts/note/newer"]),
    ] {
        let xml = fs::read_to_string(output_dir.join(path)).unwrap();
        let links: Vec<_> = xml
            .split("<item>")
            .skip(1)
            .map(|item| {
                item.split_once("<link>")
                    .unwrap()
                    .1
                    .split_once("</link>")
                    .unwrap()
                    .0
            })
            .collect();
        let expected: Vec<_> = expected
            .iter()
            .map(|path| format!("https://example.com/{path}/"))
            .collect();
        assert_eq!(links, expected, "incorrect membership or order in {path}");
    }

    let main_feed = fs::read_to_string(output_dir.join("index.xml")).unwrap();
    assert!(main_feed.contains("<title>Test Site</title>"));
}
