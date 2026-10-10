use indoc::indoc;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::html::{self, writeln_indented};

/// Borrowed RSS item metadata with an unformatted publication timestamp.
#[derive(Debug)]
pub struct FeedItem<'a> {
    pub title: &'a str,
    pub url: &'a str,
    pub description: &'a str,
    pub published: Option<Timestamp>,
}

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

pub const DEFAULT_FEED_LIMIT: usize = 20;

/// Generates an RSS 2.0 XML feed from a channel description and page entries.
///
/// Items are included in the order given (callers pre-sort by date descending). Output is
/// limited to `limit` items.
#[must_use]
pub fn generate_rss<'a>(
    channel: &Channel,
    items: impl IntoIterator<Item = FeedItem<'a>>,
    limit: usize,
) -> String {
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

    for item in items.into_iter().take(limit) {
        writeln_indented!(&mut xml, 2, "<item>");
        write_escaped_element(&mut xml, 3, "title", item.title);
        write_escaped_element(&mut xml, 3, "link", item.url);

        if !item.description.is_empty() {
            write_escaped_element(&mut xml, 3, "description", item.description);
        }

        if let Some(date) = item.published {
            let rfc2822 = format_rfc2822(date);
            write_escaped_element(&mut xml, 3, "pubDate", &rfc2822);
        }

        writeln_indented!(
            &mut xml,
            3,
            r#"<guid isPermaLink="true">{}</guid>"#,
            html::escape(item.url),
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── generate_rss ──

    #[test]
    fn generate_rss_basic() {
        let channel = Channel {
            title: "Test Site".into(),
            link: "https://example.com/".into(),
            feed_url: "https://example.com/index.xml".into(),
            description: "A test site".into(),
            language: "en".into(),
            last_build_date: Some("Sun, 15 Mar 2026 10:00:00 +0000".into()),
        };
        let mut items = vec![
            make_item(
                "Post A",
                "https://example.com/post-a/",
                Some("2026-03-15T10:00:00Z"),
            ),
            make_item("Post B", "https://example.com/post-b/", None),
        ];

        items[0].description = "A summary of the post";

        let xml = generate_rss(&channel, items, DEFAULT_FEED_LIMIT);

        assert!(first_item(&xml).contains("<description>A summary of the post</description>"));
        let second_item = xml.split("<item>").nth(2).unwrap();
        assert!(!second_item.contains("<description>"));
        assert!(!second_item.contains("<pubDate>"));
        assert!(xml.starts_with(r#"<?xml version="1.0""#));
        assert!(xml.contains("<title>Test Site</title>"));
        assert!(xml.contains("<link>https://example.com/</link>"));
        assert!(xml.contains("<description>A test site</description>"));
        assert!(xml.contains("<language>en</language>"));
        assert!(xml.contains(r#"<atom:link href="https://example.com/index.xml" rel="self" type="application/rss+xml" />"#));
        assert!(xml.contains("<lastBuildDate>Sun, 15 Mar 2026 10:00:00 +0000</lastBuildDate>"));
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
        let items = vec![make_item(
            r#"Post "with" <tags>"#,
            "https://example.com/post/",
            None,
        )];

        let xml = generate_rss(&channel, items, DEFAULT_FEED_LIMIT);

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
        let items = [
            make_item("Post 1", "https://example.com/1/", None),
            make_item("Post 2", "https://example.com/2/", None),
            make_item("Post 3", "https://example.com/3/", None),
            make_item("Post 4", "https://example.com/4/", None),
        ];

        let xml = generate_rss(&channel, items, 3);

        let titles: Vec<_> = xml
            .split("<item>")
            .skip(1)
            .map(|item| {
                item.split("<title>")
                    .nth(1)
                    .unwrap()
                    .split("</title>")
                    .next()
                    .unwrap()
            })
            .collect();
        assert_eq!(titles, ["Post 1", "Post 2", "Post 3"]);
    }

    // ── format_rfc2822 ──

    #[test]
    fn format_rfc2822_utc() {
        let ts: Timestamp = "2026-03-15T10:30:00Z".parse().unwrap();
        let formatted = format_rfc2822(ts);
        assert_eq!(formatted, "Sun, 15 Mar 2026 10:30:00 +0000");
    }

    /// Extracts the inner content of the first `<item>...</item>` block in `xml`.
    fn first_item(xml: &str) -> &str {
        let start = xml.find("<item>").expect("xml should contain <item>") + "<item>".len();
        let end = xml[start..]
            .find("</item>")
            .expect("xml should contain </item>");
        &xml[start..start + end]
    }

    fn make_item<'a>(title: &'a str, url: &'a str, date: Option<&str>) -> FeedItem<'a> {
        FeedItem {
            title,
            url,
            description: "",
            published: date.map(|date| date.parse().unwrap()),
        }
    }
}
