use indoc::{formatdoc, indoc};
use jiff::Timestamp;

use crate::html::{self, writeln_indented};
use crate::url::BaseUrl;

/// A single URL entry in the sitemap.
#[derive(Debug)]
pub(super) struct SitemapEntry {
    pub(super) loc: String,
    pub(super) lastmod: Option<Timestamp>,
}

/// Renders a `sitemap.xml` with one `<url>` per entry.
pub(super) fn render_sitemap(entries: &[SitemapEntry]) -> String {
    let mut xml = String::from(indoc! {r#"
        <?xml version="1.0" encoding="utf-8" standalone="yes"?>
        <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
    "#});

    for entry in entries {
        writeln_indented!(&mut xml, 1, "<url>");
        writeln_indented!(&mut xml, 2, "<loc>{}</loc>", html::escape(&entry.loc));

        if let Some(lastmod) = entry.lastmod {
            writeln_indented!(&mut xml, 2, "<lastmod>{lastmod}</lastmod>");
        }

        writeln_indented!(&mut xml, 1, "</url>");
    }

    xml.push_str("</urlset>\n");
    xml
}

/// Renders a `robots.txt` that allows every path and points to the sitemap.
pub(super) fn render_robots(base_url: &BaseUrl) -> String {
    formatdoc! {"
        User-agent: *
        Allow: /

        Sitemap: {}
    ", base_url.site_url("sitemap.xml")}
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── render_sitemap ──

    #[test]
    fn render_sitemap_renders_entries_and_optional_dates() {
        let entries = [
            SitemapEntry {
                loc: "https://example.com/tags/c&c++/".into(),
                lastmod: None,
            },
            SitemapEntry {
                loc: "https://example.com/posts/hello/".into(),
                lastmod: Some("2026-03-15T10:00:00+08:00".parse().unwrap()),
            },
        ];

        assert_eq!(
            render_sitemap(&entries),
            indoc! {r#"
                <?xml version="1.0" encoding="utf-8" standalone="yes"?>
                <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                  <url>
                    <loc>https://example.com/tags/c&amp;c++/</loc>
                  </url>
                  <url>
                    <loc>https://example.com/posts/hello/</loc>
                    <lastmod>2026-03-15T02:00:00Z</lastmod>
                  </url>
                </urlset>
            "#}
        );
    }

    #[test]
    fn render_sitemap_empty() {
        assert_eq!(
            render_sitemap(&[]),
            indoc! {r#"
                <?xml version="1.0" encoding="utf-8" standalone="yes"?>
                <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                </urlset>
            "#}
        );
    }

    // ── render_robots ──

    #[test]
    fn render_robots_points_to_sitemap() {
        assert_eq!(
            render_robots(&"https://example.com/blog".parse().unwrap()),
            indoc! {"
                User-agent: *
                Allow: /

                Sitemap: https://example.com/blog/sitemap.xml
            "}
        );
    }
}
