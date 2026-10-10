use pulldown_cmark::HeadingLevel;

use super::heading::render_number;
use crate::html::{escape, writeln_indented};

/// A single entry in the table of contents, collected during heading rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    /// Heading level (H1–H6).
    pub level: HeadingLevel,
    /// The optional hierarchical number, separate from the authored title.
    pub number: Option<String>,
    /// The slugified ID attribute for this heading.
    pub id: String,
    /// The plain-text title of the heading.
    pub title: String,
}

/// Renders `TocEntry` values into a nested `<nav>` / `<ul>` / `<li>` / `<a>` structure.
///
/// Heading levels are normalized so the smallest level becomes depth 1, avoiding empty outer
/// wrappers when content starts at H2 or deeper. Returns an empty string if `entries` is empty.
#[must_use]
pub fn render_toc_html(entries: &[TocEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }

    let mut html = String::new();
    writeln_indented!(&mut html, 0, r#"<nav class="toc">"#);

    let min_level = entries.iter().map(|e| e.level as u8).min().unwrap_or(1);
    let mut depth: u8 = 0;

    for entry in entries {
        let target = entry.level as u8 - min_level + 1;

        if target <= depth {
            while depth > target {
                writeln_indented!(&mut html, depth * 2, "</li>");
                writeln_indented!(&mut html, depth * 2 - 1, "</ul>");
                depth -= 1;
            }
            writeln_indented!(&mut html, depth * 2, "</li>");
        }

        // When skipping heading levels (e.g., H2 → H4), emit wrapper <li> elements at intermediate
        // depths so that nested <ul> elements always appear inside a <li> (required by HTML spec).
        while depth < target {
            writeln_indented!(&mut html, depth * 2 + 1, "<ul>");
            depth += 1;
            if depth < target {
                writeln_indented!(&mut html, depth * 2, "<li>");
            }
        }

        writeln_indented!(
            &mut html,
            depth * 2,
            r##"<li><a href="#{}">{}{}</a>"##,
            escape(&entry.id),
            render_number(entry.number.as_deref()),
            escape(&entry.title),
        );
    }

    while depth > 0 {
        writeln_indented!(&mut html, depth * 2, "</li>");
        writeln_indented!(&mut html, depth * 2 - 1, "</ul>");
        depth -= 1;
    }

    writeln_indented!(&mut html, 0, "</nav>");
    html
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use pulldown_cmark::HeadingLevel;

    use super::*;

    // ── render_toc_html ──

    #[test]
    fn render_toc_html_single_entry() {
        for level in [HeadingLevel::H2, HeadingLevel::H3] {
            let entries = vec![TocEntry {
                level,
                number: None,
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
    }

    #[test]
    fn render_toc_html_flat_same_level() {
        let entries = vec![
            TocEntry {
                level: HeadingLevel::H2,
                number: None,
                id: "a".into(),
                title: "A".into(),
            },
            TocEntry {
                level: HeadingLevel::H2,
                number: None,
                id: "b".into(),
                title: "B".into(),
            },
            TocEntry {
                level: HeadingLevel::H2,
                number: None,
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
    fn render_toc_html_nested_headings() {
        let entries = vec![
            TocEntry {
                level: HeadingLevel::H2,
                number: None,
                id: "intro".into(),
                title: "Intro".into(),
            },
            TocEntry {
                level: HeadingLevel::H3,
                number: None,
                id: "detail-a".into(),
                title: "Detail A".into(),
            },
            TocEntry {
                level: HeadingLevel::H3,
                number: None,
                id: "detail-b".into(),
                title: "Detail B".into(),
            },
            TocEntry {
                level: HeadingLevel::H2,
                number: None,
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
    fn render_toc_html_deep_nesting_round_trip() {
        // H2 → H3 → H4 → H2: verifies all intermediate levels close correctly.
        let entries = vec![
            TocEntry {
                level: HeadingLevel::H2,
                number: None,
                id: "a".into(),
                title: "A".into(),
            },
            TocEntry {
                level: HeadingLevel::H3,
                number: None,
                id: "b".into(),
                title: "B".into(),
            },
            TocEntry {
                level: HeadingLevel::H4,
                number: None,
                id: "c".into(),
                title: "C".into(),
            },
            TocEntry {
                level: HeadingLevel::H2,
                number: None,
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
    fn render_toc_html_skipped_levels() {
        // H2 then H4: intermediate <ul> levels get wrapper <li> elements.
        let entries = vec![
            TocEntry {
                level: HeadingLevel::H2,
                number: None,
                id: "top".into(),
                title: "Top".into(),
            },
            TocEntry {
                level: HeadingLevel::H4,
                number: None,
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
    fn render_toc_html_deeper_heading_first() {
        let entries = vec![
            TocEntry {
                level: HeadingLevel::H3,
                number: None,
                id: "detail".into(),
                title: "Detail".into(),
            },
            TocEntry {
                level: HeadingLevel::H2,
                number: None,
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
    fn render_toc_html_escapes_title_and_id() {
        let entries = vec![TocEntry {
            level: HeadingLevel::H2,
            number: None,
            id: "foo&bar".into(),
            title: "Vec<T> & Friends".into(),
        }];
        let html = render_toc_html(&entries);
        assert!(html.contains(r##"href="#foo&amp;bar""##), "{html}");
        assert!(
            html.contains("Vec&lt;T&gt; &amp; Friends"),
            "should escape HTML in titles, html:\n{html}"
        );
    }

    #[test]
    fn render_toc_html_empty_entries_returns_empty() {
        assert_eq!(render_toc_html(&[]), "");
    }
}
