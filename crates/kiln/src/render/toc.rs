use pulldown_cmark::HeadingLevel;

use super::heading::render_number;
use crate::html::{escape, writeln_indented};

/// A heading collected at parse time for the table of contents.
pub(super) struct TocEntry {
    pub(super) level: HeadingLevel,
    /// The optional hierarchical number, separate from the authored title.
    pub(super) number: Option<String>,
    /// The authored ID, or a deduplicated slug of the title once IDs are allocated.
    pub(super) id: String,
    pub(super) title: String,
}

/// Renders `TocEntry` values into a nested `<nav>` / `<ul>` / `<li>` / `<a>` structure.
///
/// Heading levels are normalized so the smallest level becomes depth 1, avoiding empty outer
/// wrappers when content starts at H2 or deeper. Returns an empty string if `entries` is empty.
#[must_use]
pub(super) fn render_toc_html(entries: &[TocEntry]) -> String {
    let Some(min_level) = entries.iter().map(|entry| entry.level as u8).min() else {
        return String::new();
    };

    let mut html = String::new();
    writeln_indented!(&mut html, 0, r#"<nav class="toc">"#);

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
            entry
                .number
                .as_deref()
                .map(render_number)
                .unwrap_or_default(),
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

    fn entry(level: HeadingLevel, id: &str, title: &str) -> TocEntry {
        TocEntry {
            level,
            number: None,
            id: id.to_owned(),
            title: title.to_owned(),
        }
    }

    // ── render_toc_html ──

    #[test]
    fn render_toc_html_nested_headings() {
        let entries = [
            entry(HeadingLevel::H2, "intro", "Intro"),
            entry(HeadingLevel::H3, "detail-a", "Detail A"),
            entry(HeadingLevel::H3, "detail-b", "Detail B"),
            entry(HeadingLevel::H2, "conclusion", "Conclusion"),
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
    fn render_toc_html_deep_nesting_round_trip_from_h3() {
        // H3 → H4 → H5 → H3: normalizes to depth 1 and closes every intermediate level.
        let entries = [
            entry(HeadingLevel::H3, "a", "A"),
            entry(HeadingLevel::H4, "b", "B"),
            entry(HeadingLevel::H5, "c", "C"),
            entry(HeadingLevel::H3, "d", "D"),
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
        let entries = [
            entry(HeadingLevel::H2, "top", "Top"),
            entry(HeadingLevel::H4, "deep", "Deep"),
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
        let entries = [
            entry(HeadingLevel::H3, "detail", "Detail"),
            entry(HeadingLevel::H2, "overview", "Overview"),
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
        let html = render_toc_html(&[entry(HeadingLevel::H2, "foo&bar", "Vec<T> & Friends")]);
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
