use std::fmt::Write;

use pulldown_cmark::HeadingLevel;

use super::escape_html;

/// A single entry in the table of contents, collected during heading rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// Heading level (H1–H6).
    pub level: HeadingLevel,
    /// The slugified ID attribute for this heading.
    pub id: String,
    /// The plain-text title of the heading.
    pub title: String,
}

/// Renders a list of `TocEntry` values into a `<nav>` HTML structure with
/// nested `<ul>` / `<li>` / `<a>` elements.
///
/// Heading levels are normalized so the smallest level in the input becomes
/// depth 1, avoiding empty outer wrappers when content starts at H2 or deeper.
///
/// Returns an empty string if `entries` is empty.
#[must_use]
pub fn render_toc_html(entries: &[TocEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }

    let min_level = entries.iter().map(|e| e.level as u8).min().unwrap_or(1);

    let mut html = String::from("<nav class=\"toc\">\n");
    let mut depth: u8 = 0;

    for entry in entries {
        let target = entry.level as u8 - min_level + 1;

        if target <= depth {
            // Close deeper levels.
            while depth > target {
                indent(&mut html, depth * 2);
                html.push_str("</li>\n");
                indent(&mut html, depth * 2 - 1);
                html.push_str("</ul>\n");
                depth -= 1;
            }
            // Close sibling at target depth.
            indent(&mut html, depth * 2);
            html.push_str("</li>\n");
        }

        // Open new levels. When skipping heading levels (e.g., H2 → H4),
        // emit wrapper <li> elements at intermediate depths so that nested
        // <ul> elements always appear inside a <li> (required by HTML spec).
        while depth < target {
            indent(&mut html, depth * 2 + 1);
            html.push_str("<ul>\n");
            depth += 1;
            if depth < target {
                indent(&mut html, depth * 2);
                html.push_str("<li>\n");
            }
        }

        // Emit entry.
        indent(&mut html, depth * 2);
        let _ = writeln!(
            html,
            "<li><a href=\"#{}\">{}</a>",
            escape_html(&entry.id),
            escape_html(&entry.title),
        );
    }

    // Close all remaining levels.
    while depth > 0 {
        indent(&mut html, depth * 2);
        html.push_str("</li>\n");
        indent(&mut html, depth * 2 - 1);
        html.push_str("</ul>\n");
        depth -= 1;
    }

    html.push_str("</nav>\n");
    html
}

fn indent(html: &mut String, level: u8) {
    for _ in 0..level {
        html.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use pulldown_cmark::HeadingLevel;

    use super::*;

    #[test]
    fn empty_entries() {
        assert_eq!(render_toc_html(&[]), "");
    }

    #[test]
    fn single_entry() {
        let entries = vec![TocEntry {
            level: HeadingLevel::H2,
            id: "hello".into(),
            title: "Hello".into(),
        }];
        assert_eq!(
            render_toc_html(&entries),
            indoc! {r##"
                <nav class="toc">
                  <ul>
                    <li><a href="#hello">Hello</a>
                    </li>
                  </ul>
                </nav>
            "##}
        );
    }

    #[test]
    fn nested_headings() {
        let entries = vec![
            TocEntry {
                level: HeadingLevel::H2,
                id: "intro".into(),
                title: "Intro".into(),
            },
            TocEntry {
                level: HeadingLevel::H3,
                id: "detail-a".into(),
                title: "Detail A".into(),
            },
            TocEntry {
                level: HeadingLevel::H3,
                id: "detail-b".into(),
                title: "Detail B".into(),
            },
            TocEntry {
                level: HeadingLevel::H2,
                id: "conclusion".into(),
                title: "Conclusion".into(),
            },
        ];
        assert_eq!(
            render_toc_html(&entries),
            indoc! {r##"
                <nav class="toc">
                  <ul>
                    <li><a href="#intro">Intro</a>
                      <ul>
                        <li><a href="#detail-a">Detail A</a>
                        </li>
                        <li><a href="#detail-b">Detail B</a>
                        </li>
                      </ul>
                    </li>
                    <li><a href="#conclusion">Conclusion</a>
                    </li>
                  </ul>
                </nav>
            "##}
        );
    }

    #[test]
    fn skipped_levels() {
        // H2 then H4 — intermediate <ul> levels get wrapper <li> elements.
        let entries = vec![
            TocEntry {
                level: HeadingLevel::H2,
                id: "top".into(),
                title: "Top".into(),
            },
            TocEntry {
                level: HeadingLevel::H4,
                id: "deep".into(),
                title: "Deep".into(),
            },
        ];
        assert_eq!(
            render_toc_html(&entries),
            indoc! {r##"
                <nav class="toc">
                  <ul>
                    <li><a href="#top">Top</a>
                      <ul>
                        <li>
                          <ul>
                            <li><a href="#deep">Deep</a>
                            </li>
                          </ul>
                        </li>
                      </ul>
                    </li>
                  </ul>
                </nav>
            "##}
        );
    }

    #[test]
    fn deeper_heading_first() {
        // H3 then H2 — deeper heading before the minimum level.
        let entries = vec![
            TocEntry {
                level: HeadingLevel::H3,
                id: "detail".into(),
                title: "Detail".into(),
            },
            TocEntry {
                level: HeadingLevel::H2,
                id: "overview".into(),
                title: "Overview".into(),
            },
        ];
        assert_eq!(
            render_toc_html(&entries),
            indoc! {r##"
                <nav class="toc">
                  <ul>
                    <li>
                      <ul>
                        <li><a href="#detail">Detail</a>
                        </li>
                      </ul>
                    </li>
                    <li><a href="#overview">Overview</a>
                    </li>
                  </ul>
                </nav>
            "##}
        );
    }

    #[test]
    fn deep_nesting_round_trip() {
        // H2 → H3 → H4 → H2: verifies all intermediate levels close correctly.
        let entries = vec![
            TocEntry {
                level: HeadingLevel::H2,
                id: "a".into(),
                title: "A".into(),
            },
            TocEntry {
                level: HeadingLevel::H3,
                id: "b".into(),
                title: "B".into(),
            },
            TocEntry {
                level: HeadingLevel::H4,
                id: "c".into(),
                title: "C".into(),
            },
            TocEntry {
                level: HeadingLevel::H2,
                id: "d".into(),
                title: "D".into(),
            },
        ];
        assert_eq!(
            render_toc_html(&entries),
            indoc! {r##"
                <nav class="toc">
                  <ul>
                    <li><a href="#a">A</a>
                      <ul>
                        <li><a href="#b">B</a>
                          <ul>
                            <li><a href="#c">C</a>
                            </li>
                          </ul>
                        </li>
                      </ul>
                    </li>
                    <li><a href="#d">D</a>
                    </li>
                  </ul>
                </nav>
            "##}
        );
    }

    #[test]
    fn h3_first_normalizes_to_top_level() {
        // When content starts at H3, normalization makes it depth 1.
        let entries = vec![TocEntry {
            level: HeadingLevel::H3,
            id: "only".into(),
            title: "Only".into(),
        }];
        assert_eq!(
            render_toc_html(&entries),
            indoc! {r##"
                <nav class="toc">
                  <ul>
                    <li><a href="#only">Only</a>
                    </li>
                  </ul>
                </nav>
            "##}
        );
    }

    #[test]
    fn flat_same_level() {
        let entries = vec![
            TocEntry {
                level: HeadingLevel::H2,
                id: "a".into(),
                title: "A".into(),
            },
            TocEntry {
                level: HeadingLevel::H2,
                id: "b".into(),
                title: "B".into(),
            },
            TocEntry {
                level: HeadingLevel::H2,
                id: "c".into(),
                title: "C".into(),
            },
        ];
        assert_eq!(
            render_toc_html(&entries),
            indoc! {r##"
                <nav class="toc">
                  <ul>
                    <li><a href="#a">A</a>
                    </li>
                    <li><a href="#b">B</a>
                    </li>
                    <li><a href="#c">C</a>
                    </li>
                  </ul>
                </nav>
            "##}
        );
    }

    #[test]
    fn title_with_html_chars() {
        let entries = vec![TocEntry {
            level: HeadingLevel::H2,
            id: "generics".into(),
            title: "Vec<T> & Friends".into(),
        }];
        let html = render_toc_html(&entries);
        assert!(
            html.contains("Vec&lt;T&gt; &amp; Friends"),
            "should escape HTML in titles"
        );
    }

    #[test]
    fn id_with_html_chars() {
        let entries = vec![TocEntry {
            level: HeadingLevel::H2,
            id: "foo&bar".into(),
            title: "Foo".into(),
        }];
        let html = render_toc_html(&entries);
        assert!(
            html.contains("href=\"#foo&amp;bar\""),
            "should escape HTML in href, html: {html}"
        );
    }
}
