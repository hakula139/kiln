use indoc::indoc;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::html::{self, writeln_indented};
use crate::template::vars::PageSummary;

/// RSS channel metadata.
#[derive(Debug)]
pub struct Channel {
    pub title: String,
    pub link: String,
    pub feed_url: String,
    pub description: String,
    pub language: String,
    pub last_build_date: Option<String>,
}

/// Default number of items per feed.
pub const DEFAULT_FEED_LIMIT: usize = 20;

/// Generates an RSS 2.0 XML feed from a channel description and page entries.
///
/// Items are included in the order given — callers should pre-sort by date
/// descending (newest first). The feed limits output to `limit` items.
#[must_use]
pub fn generate_rss(channel: &Channel, items: &[PageSummary], limit: usize) -> String {
    let mut xml = String::from(indoc! {r#"
        <?xml version="1.0" encoding="utf-8" standalone="yes"?>
        <rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
          <channel>
    "#});

    write_escaped_element(&mut xml, 2, "title", &channel.title);
    write_escaped_element(&mut xml, 2, "link", &channel.link);
    write_escaped_element(&mut xml, 2, "description", &channel.description);
    write_escaped_element(&mut xml, 2, "language", &channel.language);
    writeln_indented!(
        &mut xml,
        2,
        r#"<atom:link href="{}" rel="self" type="application/rss+xml" />"#,
        html::escape(&channel.feed_url),
    );

    if let Some(date) = channel.last_build_date.as_deref() {
        write_escaped_element(&mut xml, 2, "lastBuildDate", date);
    }

    for item in items.iter().take(limit) {
        writeln_indented!(&mut xml, 2, "<item>");
        write_escaped_element(&mut xml, 3, "title", &item.title);
        write_escaped_element(&mut xml, 3, "link", &item.url);

        if !item.description.is_empty() {
            write_escaped_element(&mut xml, 3, "description", &item.description);
        }

        if let Some(ref date) = item.date
            && let Some(rfc2822) = iso_to_rfc2822(date)
        {
            write_escaped_element(&mut xml, 3, "pubDate", &rfc2822);
        }

        writeln_indented!(
            &mut xml,
            3,
            r#"<guid isPermaLink="true">{}</guid>"#,
            html::escape(&item.url),
        );
        writeln_indented!(&mut xml, 2, "</item>");
    }

    xml.push_str(indoc! {"
          </channel>
        </rss>
    "});
    xml
}

/// Formats a `Timestamp` as RFC 2822 (e.g., `Mon, 02 Jan 2006 15:04:05 +0000`).
#[must_use]
pub fn format_rfc2822(ts: Timestamp) -> String {
    ts.to_zoned(TimeZone::UTC)
        .strftime("%a, %d %b %Y %H:%M:%S %z")
        .to_string()
}

// ── Helpers ──

fn write_escaped_element(xml: &mut String, level: u8, tag: &str, content: &str) {
    writeln_indented!(xml, level, "<{tag}>{}</{tag}>", html::escape(content));
}

/// Converts an ISO 8601 date string to RFC 2822 format for RSS `<pubDate>`.
///
/// Returns `None` if the input cannot be parsed.
fn iso_to_rfc2822(iso: &str) -> Option<String> {
    let ts: Timestamp = iso.parse().ok()?;
    Some(format_rfc2822(ts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_summary(title: &str, url: &str, date: Option<&str>) -> PageSummary {
        PageSummary {
            title: title.into(),
            url: url.into(),
            date: date.map(String::from),
            description: String::new(),
            featured_image: None,
            tags: Vec::new(),
            section: None,
        }
    }

    // ── generate_rss ──

    #[test]
    fn generate_rss_basic() {
        let channel = Channel {
            title: "Test Site".into(),
            link: "https://example.com/".into(),
            feed_url: "https://example.com/index.xml".into(),
            description: "A test site".into(),
            language: "en".into(),
            last_build_date: None,
        };
        let items = vec![
            make_summary(
                "Post A",
                "https://example.com/post-a/",
                Some("2026-03-15T10:00:00Z"),
            ),
            make_summary(
                "Post B",
                "https://example.com/post-b/",
                Some("2026-03-10T08:00:00Z"),
            ),
        ];

        let xml = generate_rss(&channel, &items, DEFAULT_FEED_LIMIT);

        assert!(xml.starts_with(r#"<?xml version="1.0""#));
        assert!(xml.contains("<title>Test Site</title>"));
        assert!(xml.contains("<link>https://example.com/</link>"));
        assert!(xml.contains("<description>A test site</description>"));
        assert!(xml.contains("<language>en</language>"));
        assert!(xml.contains("<title>Post A</title>"));
        assert!(xml.contains("<link>https://example.com/post-a/</link>"));
        assert!(xml.contains("<pubDate>Sun, 15 Mar 2026 10:00:00 +0000</pubDate>"));
        assert!(xml.contains(r#"<guid isPermaLink="true">https://example.com/post-b/</guid>"#));
    }

    #[test]
    fn generate_rss_escapes_special_chars() {
        let channel = Channel {
            title: "A & B <Site>".into(),
            link: "https://example.com/".into(),
            feed_url: "https://example.com/index.xml".into(),
            description: String::new(),
            language: "en".into(),
            last_build_date: None,
        };
        let items = vec![make_summary(
            r#"Post "with" <tags>"#,
            "https://example.com/post/",
            None,
        )];

        let xml = generate_rss(&channel, &items, DEFAULT_FEED_LIMIT);

        assert!(
            xml.contains("<title>A &amp; B &lt;Site&gt;</title>"),
            "should escape channel title, xml:\n{xml}"
        );
        assert!(
            xml.contains("<title>Post &quot;with&quot; &lt;tags&gt;</title>"),
            "should escape item title, xml:\n{xml}"
        );
    }

    #[test]
    fn generate_rss_respects_limit() {
        let channel = Channel {
            title: "Site".into(),
            link: "https://example.com/".into(),
            feed_url: "https://example.com/index.xml".into(),
            description: String::new(),
            language: "en".into(),
            last_build_date: None,
        };
        let items: Vec<_> = (1..=5)
            .map(|i| {
                make_summary(
                    &format!("Post {i}"),
                    &format!("https://example.com/{i}/"),
                    None,
                )
            })
            .collect();

        let xml = generate_rss(&channel, &items, 3);

        assert!(xml.contains("Post 1"));
        assert!(xml.contains("Post 3"));
        assert!(!xml.contains("Post 4"), "should stop at limit");
    }

    #[test]
    fn generate_rss_includes_atom_self_link() {
        let channel = Channel {
            title: "Site".into(),
            link: "https://example.com/".into(),
            feed_url: "https://example.com/index.xml".into(),
            description: String::new(),
            language: "en".into(),
            last_build_date: None,
        };

        let xml = generate_rss(&channel, &[], DEFAULT_FEED_LIMIT);

        assert!(
            xml.contains(r#"<atom:link href="https://example.com/index.xml" rel="self""#),
            "should include atom:link self reference, xml:\n{xml}"
        );
    }

    #[test]
    fn generate_rss_with_last_build_date() {
        let channel = Channel {
            title: "Site".into(),
            link: "https://example.com/".into(),
            feed_url: "https://example.com/index.xml".into(),
            description: String::new(),
            language: "en".into(),
            last_build_date: Some("Sun, 15 Mar 2026 10:00:00 +0000".into()),
        };

        let xml = generate_rss(&channel, &[], DEFAULT_FEED_LIMIT);

        assert!(xml.contains("<lastBuildDate>Sun, 15 Mar 2026 10:00:00 +0000</lastBuildDate>"));
    }

    #[test]
    fn generate_rss_omits_empty_description() {
        let channel = Channel {
            title: "Site".into(),
            link: "https://example.com/".into(),
            feed_url: "https://example.com/index.xml".into(),
            description: String::new(),
            language: "en".into(),
            last_build_date: None,
        };
        let items = vec![make_summary("Post", "https://example.com/post/", None)];

        let xml = generate_rss(&channel, &items, DEFAULT_FEED_LIMIT);

        let item_section = xml.split("<item>").nth(1).unwrap();
        assert!(
            !item_section.contains("<description>"),
            "should omit empty description from item, xml:\n{xml}"
        );
    }

    #[test]
    fn generate_rss_includes_item_description() {
        let channel = Channel {
            title: "Site".into(),
            link: "https://example.com/".into(),
            feed_url: "https://example.com/index.xml".into(),
            description: String::new(),
            language: "en".into(),
            last_build_date: None,
        };
        let mut item = make_summary("Post", "https://example.com/post/", None);
        item.description = "A summary of the post".into();

        let xml = generate_rss(&channel, &[item], DEFAULT_FEED_LIMIT);

        assert!(
            xml.contains("<description>A summary of the post</description>"),
            "should include non-empty description, xml:\n{xml}"
        );
    }

    #[test]
    fn generate_rss_omits_pub_date_without_date() {
        let channel = Channel {
            title: "Site".into(),
            link: "https://example.com/".into(),
            feed_url: "https://example.com/index.xml".into(),
            description: String::new(),
            language: "en".into(),
            last_build_date: None,
        };
        let items = vec![make_summary("Post", "https://example.com/post/", None)];

        let xml = generate_rss(&channel, &items, DEFAULT_FEED_LIMIT);

        let item_section = xml.split("<item>").nth(1).unwrap();
        assert!(
            !item_section.contains("<pubDate>"),
            "should omit pubDate for undated item, xml:\n{xml}"
        );
    }

    #[test]
    fn generate_rss_omits_pub_date_with_invalid_date() {
        let channel = Channel {
            title: "Site".into(),
            link: "https://example.com/".into(),
            feed_url: "https://example.com/index.xml".into(),
            description: String::new(),
            language: "en".into(),
            last_build_date: None,
        };
        let items = vec![make_summary(
            "Post",
            "https://example.com/post/",
            Some("not-a-date"),
        )];

        let xml = generate_rss(&channel, &items, DEFAULT_FEED_LIMIT);

        let item_section = xml.split("<item>").nth(1).unwrap();
        assert!(
            !item_section.contains("<pubDate>"),
            "should omit pubDate for unparsable date, xml:\n{xml}"
        );
    }

    // ── iso_to_rfc2822 ──

    #[test]
    fn iso_to_rfc2822_valid() {
        assert_eq!(
            iso_to_rfc2822("2026-01-02T15:04:05Z"),
            Some("Fri, 02 Jan 2026 15:04:05 +0000".into()),
        );
    }

    #[test]
    fn iso_to_rfc2822_with_offset() {
        let result = iso_to_rfc2822("2026-01-02T23:04:05+08:00");
        assert_eq!(
            result,
            Some("Fri, 02 Jan 2026 15:04:05 +0000".into()),
            "should convert to UTC"
        );
    }

    #[test]
    fn iso_to_rfc2822_invalid_returns_none() {
        assert!(iso_to_rfc2822("not-a-date").is_none());
    }

    // ── format_rfc2822 ──

    #[test]
    fn format_rfc2822_utc() {
        let ts: Timestamp = "2026-03-15T10:30:00Z".parse().unwrap();
        let formatted = format_rfc2822(ts);
        assert_eq!(formatted, "Sun, 15 Mar 2026 10:30:00 +0000");
    }
}
