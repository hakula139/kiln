use indoc::{formatdoc, indoc};

use crate::html::{self, writeln_indented};

/// A single URL entry in the sitemap.
#[derive(Debug)]
pub struct SitemapEntry {
    pub loc: String,
    pub lastmod: Option<String>,
}

/// Generates an XML sitemap from a list of URL entries.
#[must_use]
pub fn generate_sitemap<'a>(entries: impl IntoIterator<Item = &'a SitemapEntry>) -> String {
    let mut xml = String::from(indoc! {r#"
        <?xml version="1.0" encoding="utf-8" standalone="yes"?>
        <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
    "#});

    for entry in entries {
        writeln_indented!(&mut xml, 1, "<url>");
        writeln_indented!(&mut xml, 2, "<loc>{}</loc>", html::escape(&entry.loc));

        if let Some(ref lastmod) = entry.lastmod {
            writeln_indented!(&mut xml, 2, "<lastmod>{}</lastmod>", html::escape(lastmod));
        }

        writeln_indented!(&mut xml, 1, "</url>");
    }

    xml.push_str("</urlset>\n");
    xml
}

/// Generates a `robots.txt` file pointing to the sitemap.
#[must_use]
pub fn generate_robots_txt(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    formatdoc! {"
        User-agent: *
        Allow: /

        Sitemap: {base}/sitemap.xml
    "}
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── generate_sitemap ──

    #[test]
    fn generate_sitemap_renders_entries_and_optional_dates() {
        let entries = [
            SitemapEntry {
                loc: "https://example.com/tags/c&c++/".into(),
                lastmod: None,
            },
            SitemapEntry {
                loc: "https://example.com/posts/hello/".into(),
                lastmod: Some("2026-03-15T10:00:00+00:00".into()),
            },
        ];

        assert_eq!(
            generate_sitemap(&entries),
            indoc! {r#"
                <?xml version="1.0" encoding="utf-8" standalone="yes"?>
                <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                  <url>
                    <loc>https://example.com/tags/c&amp;c++/</loc>
                  </url>
                  <url>
                    <loc>https://example.com/posts/hello/</loc>
                    <lastmod>2026-03-15T10:00:00+00:00</lastmod>
                  </url>
                </urlset>
            "#}
        );
    }

    #[test]
    fn generate_sitemap_empty() {
        assert_eq!(
            generate_sitemap(&[]),
            indoc! {r#"
                <?xml version="1.0" encoding="utf-8" standalone="yes"?>
                <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
                </urlset>
            "#}
        );
    }

    // ── generate_robots_txt ──

    #[test]
    fn generate_robots_txt_normalizes_base_url() {
        for base in ["https://example.com", "https://example.com/"] {
            assert_eq!(
                generate_robots_txt(base),
                indoc! {"
                    User-agent: *
                    Allow: /

                    Sitemap: https://example.com/sitemap.xml
                "}
            );
        }
    }
}
