use super::support::{listing_site, xml_texts};

// ── build: RSS feeds ──

#[test]
fn build_generates_rss_feeds() {
    let site = listing_site();

    site.build().unwrap();

    for (path, channel, items) in [
        (
            "index.xml",
            "",
            vec![
                "posts/review/latest",
                "posts/note/newer",
                "posts/note/older",
            ],
        ),
        (
            "posts/index.xml",
            "posts/",
            vec![
                "posts/review/latest",
                "posts/note/newer",
                "posts/note/older",
            ],
        ),
        (
            "posts/note/index.xml",
            "posts/note/",
            vec!["posts/note/newer", "posts/note/older"],
        ),
        (
            "posts/review/index.xml",
            "posts/review/",
            vec!["posts/review/latest"],
        ),
        (
            "tags/rust/index.xml",
            "tags/rust/",
            vec!["about", "posts/review/latest", "posts/note/older"],
        ),
        (
            "tags/testing/index.xml",
            "tags/testing/",
            vec!["posts/note/newer"],
        ),
    ] {
        let expected: Vec<_> = [format!("https://example.com/{channel}")]
            .into_iter()
            .chain(
                items
                    .iter()
                    .map(|path| format!("https://example.com/{path}/")),
            )
            .collect();
        assert_eq!(
            xml_texts(&site.read(path), "link"),
            expected,
            "incorrect membership or order in {path}"
        );
    }

    assert_eq!(xml_texts(&site.read("index.xml"), "title")[0], "Test Site");
}
