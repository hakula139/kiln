use std::path::Path;

use anyhow::Result;
use indoc::indoc;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::BuildContext;
use super::bucket::Listing;
use super::listing::{PreparedPage, newest};
use crate::html::{self, writeln_indented};
use crate::output::write_output;
use crate::url::directory_url;

/// Maximum number of items in each feed.
const FEED_LIMIT: usize = 20;

/// RSS channel metadata.
#[derive(Debug)]
struct Channel<'a> {
    title: &'a str,
    link: &'a str,
    feed_url: &'a str,
    description: &'a str,
    language: &'a str,
    last_build_date: Option<Timestamp>,
}

/// Generates the site-wide RSS feed at `index.xml` plus one per listing bucket
/// (all-posts, per-section, per-tag).
pub(super) fn build_feeds(
    ctx: &BuildContext,
    listed_posts: &[&PreparedPage],
    buckets: &[Listing<'_>],
    output_dir: &Path,
) -> Result<()> {
    write_feed(
        ctx,
        &ctx.config.title,
        Path::new(""),
        listed_posts,
        output_dir,
    )?;
    for bucket in buckets {
        let title = format!("{} - {}", bucket.name, ctx.config.title);
        write_feed(ctx, &title, &bucket.path(), &bucket.pages, output_dir)?;
    }
    Ok(())
}

/// Writes the feed for the listing at `directory` into its `index.xml`.
fn write_feed(
    ctx: &BuildContext,
    title: &str,
    directory: &Path,
    pages: &[&PreparedPage],
    output_dir: &Path,
) -> Result<()> {
    let link = ctx.config.base_url.site_url(&directory_url(directory));
    let channel = Channel {
        title,
        link: &link,
        feed_url: &format!("{link}index.xml"),
        description: &ctx.config.description,
        language: ctx.config.language.as_str(),
        last_build_date: newest(pages),
    };
    write_output(
        &output_dir.join(directory).join("index.xml"),
        &generate_rss(&channel, pages),
    )
}

/// Generates an RSS 2.0 document listing the first [`FEED_LIMIT`] pages in the given order.
fn generate_rss(channel: &Channel<'_>, pages: &[&PreparedPage]) -> String {
    let mut xml = String::from(indoc! {r#"
        <?xml version="1.0" encoding="utf-8" standalone="yes"?>
        <rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
          <channel>
    "#});

    write_escaped_element(&mut xml, 2, "title", channel.title);
    write_escaped_element(&mut xml, 2, "link", channel.link);
    write_escaped_element(&mut xml, 2, "description", channel.description);
    write_escaped_element(&mut xml, 2, "language", channel.language);
    writeln_indented!(
        &mut xml,
        2,
        r#"<atom:link href="{}" rel="self" type="application/rss+xml" />"#,
        html::escape(channel.feed_url),
    );

    if let Some(date) = channel.last_build_date {
        write_escaped_element(&mut xml, 2, "lastBuildDate", &format_rfc2822(date));
    }

    for page in pages.iter().take(FEED_LIMIT) {
        let summary = &page.summary;
        writeln_indented!(&mut xml, 2, "<item>");
        write_escaped_element(&mut xml, 3, "title", &summary.title);
        write_escaped_element(&mut xml, 3, "link", &summary.url);

        if !summary.description.is_empty() {
            write_escaped_element(&mut xml, 3, "description", &summary.description);
        }

        if let Some(date) = page.published {
            write_escaped_element(&mut xml, 3, "pubDate", &format_rfc2822(date));
        }

        writeln_indented!(
            &mut xml,
            3,
            r#"<guid isPermaLink="true">{}</guid>"#,
            html::escape(&summary.url),
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
fn format_rfc2822(ts: Timestamp) -> String {
    ts.to_zoned(TimeZone::UTC)
        .strftime("%a, %d %b %Y %H:%M:%S %z")
        .to_string()
}

fn write_escaped_element(xml: &mut String, level: u8, tag: &str, content: &str) {
    writeln_indented!(xml, level, "<{tag}>{}</{tag}>", html::escape(content));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::listing::prepared_page;

    // ── generate_rss ──

    #[test]
    fn generate_rss_renders_channel_and_optional_item_fields() {
        let mut dated = prepared_page("Post A & B", Some("2026-03-15T10:00:00Z"), None);
        dated.summary.description = "A <summary>".into();
        let undated = prepared_page("Post C", None, None);

        assert_eq!(
            generate_rss(
                &Channel {
                    last_build_date: Some("2026-03-16T08:30:00Z".parse().unwrap()),
                    ..channel()
                },
                &[&dated, &undated]
            ),
            indoc! {r#"
                <?xml version="1.0" encoding="utf-8" standalone="yes"?>
                <rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
                  <channel>
                    <title>Site &quot;A&quot; &amp; B</title>
                    <link>https://example.com/</link>
                    <description>Example description</description>
                    <language>en</language>
                    <atom:link href="https://example.com/index.xml" rel="self" type="application/rss+xml" />
                    <lastBuildDate>Mon, 16 Mar 2026 08:30:00 +0000</lastBuildDate>
                    <item>
                      <title>Post A &amp; B</title>
                      <link>/Post A &amp; B/</link>
                      <description>A &lt;summary&gt;</description>
                      <pubDate>Sun, 15 Mar 2026 10:00:00 +0000</pubDate>
                      <guid isPermaLink="true">/Post A &amp; B/</guid>
                    </item>
                    <item>
                      <title>Post C</title>
                      <link>/Post C/</link>
                      <guid isPermaLink="true">/Post C/</guid>
                    </item>
                  </channel>
                </rss>
            "#}
        );
    }

    #[test]
    fn generate_rss_truncates_to_feed_limit() {
        let pages: Vec<_> = (1..=FEED_LIMIT + 1)
            .map(|number| prepared_page(&format!("Post {number}"), None, None))
            .collect();
        let refs: Vec<_> = pages.iter().collect();

        let xml = generate_rss(&channel(), &refs);

        assert_eq!(xml.matches("<item>").count(), FEED_LIMIT);
        assert!(xml.contains(&format!("<title>Post {FEED_LIMIT}</title>")));
        assert!(!xml.contains(&format!("<title>Post {}</title>", FEED_LIMIT + 1)));
    }

    fn channel() -> Channel<'static> {
        Channel {
            title: r#"Site "A" & B"#,
            link: "https://example.com/",
            feed_url: "https://example.com/index.xml",
            description: "Example description",
            language: "en",
            last_build_date: None,
        }
    }
}
