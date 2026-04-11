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
pub fn generate_sitemap(entries: &[SitemapEntry]) -> String {
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
    fn generate_sitemap_basic() {
        let entries = vec![
            SitemapEntry {
                loc: "https://example.com/".into(),
                lastmod: None,
            },
            SitemapEntry {
                loc: "https://example.com/posts/hello/".into(),
                lastmod: Some("2026-03-15T10:00:00+00:00".into()),
            },
        ];

        let xml = generate_sitemap(&entries);

        assert!(xml.starts_with(r#"<?xml version="1.0""#));
        assert!(xml.contains("<loc>https://example.com/</loc>"));
        assert!(xml.contains("<loc>https://example.com/posts/hello/</loc>"));
        assert!(xml.contains("<lastmod>2026-03-15T10:00:00+00:00</lastmod>"));
        assert!(xml.ends_with("</urlset>\n"));
    }

    #[test]
    fn generate_sitemap_escapes_urls() {
        let entries = vec![SitemapEntry {
            loc: "https://example.com/tags/c&c++/".into(),
            lastmod: None,
        }];

        let xml = generate_sitemap(&entries);

        assert!(
            xml.contains("<loc>https://example.com/tags/c&amp;c++/</loc>"),
            "should escape ampersand in URL, xml:\n{xml}"
        );
    }

    #[test]
    fn generate_sitemap_empty() {
        let xml = generate_sitemap(&[]);

        assert!(xml.starts_with(r#"<?xml version="1.0""#));
        assert!(xml.contains("<urlset"));
        assert!(xml.ends_with("</urlset>\n"));
        assert!(!xml.contains("<url>"));
    }

    #[test]
    fn generate_sitemap_omits_lastmod_when_none() {
        let entries = vec![SitemapEntry {
            loc: "https://example.com/about/".into(),
            lastmod: None,
        }];

        let xml = generate_sitemap(&entries);

        assert!(!xml.contains("<lastmod>"));
    }

    // ── generate_robots_txt ──

    #[test]
    fn generate_robots_txt_basic() {
        let txt = generate_robots_txt("https://example.com");
        assert_eq!(
            txt,
            indoc! {"
                User-agent: *
                Allow: /

                Sitemap: https://example.com/sitemap.xml
            "},
        );
    }

    #[test]
    fn generate_robots_txt_strips_trailing_slash() {
        let txt = generate_robots_txt("https://example.com/");
        assert!(
            txt.contains("Sitemap: https://example.com/sitemap.xml"),
            "should not double-slash, txt:\n{txt}"
        );
    }
}
