use std::collections::HashMap;
use std::fmt::Write;

use pulldown_cmark::{Event, Tag, TagEnd};
use unicase::UniCase;

use super::Spanned;
use super::page_ids::PageIds;
use crate::html::{escape, writeln_indented};
use crate::text::slugify_or;

/// Footnotes reachable from a document body, in first-reference order.
#[derive(Default)]
pub(super) struct Notes {
    notes: Vec<Note>,
    numbers: HashMap<String, usize>,
}

struct Note {
    key: String,
    body: Vec<Spanned>,
    reference_count: usize,
    id: String,
    references: Vec<String>,
}

impl Notes {
    /// Removes footnote definitions from `events`, returning the body and its reachable notes.
    pub(super) fn extract(mut events: impl Iterator<Item = Spanned>) -> (Vec<Spanned>, Self) {
        let mut definitions = HashMap::new();
        let (body, _) = extract_definitions(&mut events, &mut definitions, false);
        let mut notes = Self::default();
        notes.discover_references(reference_keys(&body), &mut definitions);
        let mut index = 0;
        while index < notes.notes.len() {
            let keys = reference_keys(&notes.notes[index].body);
            notes.discover_references(keys, &mut definitions);
            index += 1;
        }
        (body, notes)
    }

    fn discover_references(
        &mut self,
        keys: Vec<String>,
        definitions: &mut HashMap<String, Vec<Spanned>>,
    ) {
        for key in keys {
            if let Some(body) = definitions.remove(&key) {
                self.numbers.insert(key.clone(), self.notes.len());
                self.notes.push(Note {
                    key,
                    body,
                    reference_count: 1,
                    id: String::new(),
                    references: Vec::new(),
                });
            } else {
                self.notes[self.numbers[&key]].reference_count += 1;
            }
        }
    }

    /// Yields the note bodies in reference order.
    pub(super) fn events(&self) -> impl Iterator<Item = &Spanned> {
        self.notes.iter().flat_map(|note| &note.body)
    }

    /// Allocates note and backlink IDs and replaces reference events in `body` and the notes.
    pub(super) fn allocate_ids(&mut self, body: &mut [Spanned], ids: &mut PageIds, scope: usize) {
        let mut references: Vec<_> = self
            .notes
            .iter_mut()
            .enumerate()
            .map(|(index, note)| {
                let label = slugify_or(&note.key, "note");
                let prefix = if scope == 0 {
                    label
                } else {
                    format!("{scope}-{label}")
                };
                note.id = ids.allocate(&format!("fn-{prefix}"));
                note.references = (1..=note.reference_count)
                    .map(|n| ids.allocate(&format!("fnref-{prefix}-{n}")))
                    .collect();
                note.references
                    .iter()
                    .map(|reference| render_reference(&note.id, reference, index + 1))
                    .collect::<Vec<_>>()
                    .into_iter()
            })
            .collect();
        for (event, _) in body
            .iter_mut()
            .chain(self.notes.iter_mut().flat_map(|note| &mut note.body))
        {
            if let Event::FootnoteReference(name) = event
                && let Some(html) = references[self.numbers[&footnote_key(name)]].next()
            {
                *event = Event::InlineHtml(html.into());
            }
        }
    }

    /// Appends the endnotes with backlinks to `html` after [`Self::allocate_ids`].
    pub(super) fn render(self, html: &mut String, mut render: impl FnMut(Vec<Spanned>) -> String) {
        if self.notes.is_empty() {
            return;
        }
        writeln_indented!(
            html,
            0,
            r#"<section class="footnotes" role="doc-endnotes">"#
        );
        writeln_indented!(html, 1, "<ol>");
        for note in self.notes {
            writeln_indented!(html, 2, r#"<li id="{}">"#, escape(&note.id));
            let ends_in_paragraph =
                matches!(note.body.last(), Some((Event::End(TagEnd::Paragraph), _)));
            let body = render(note.body);
            let backlinks = render_backlinks(&note.references);
            if ends_in_paragraph && let Some(body) = body.strip_suffix("</p>\n") {
                html.push_str(body);
                _ = writeln!(html, " {backlinks}</p>");
            } else {
                html.push_str(&body);
                _ = writeln!(html, "<p>{backlinks}</p>");
            }
            writeln_indented!(html, 2, "</li>");
        }
        writeln_indented!(html, 1, "</ol>");
        writeln_indented!(html, 0, "</section>");
    }
}

fn extract_definitions(
    events: &mut impl Iterator<Item = Spanned>,
    definitions: &mut HashMap<String, Vec<Spanned>>,
    nested: bool,
) -> (Vec<Spanned>, bool) {
    let mut output = Vec::new();
    let mut removed = false;
    while let Some((event, range)) = events.next() {
        match event {
            Event::Start(Tag::FootnoteDefinition(name)) => {
                let key = footnote_key(&name);
                let first = !definitions.contains_key(&key);
                definitions.entry(key.clone()).or_default();
                let (body, _) = extract_definitions(events, definitions, true);
                if first {
                    definitions.insert(key, body);
                }
                removed = true;
            }
            Event::Start(tag) => {
                let (mut body, changed) = extract_definitions(events, definitions, true);
                removed |= changed;
                let empty = body.len() == 1;
                if !(changed
                    && empty
                    && matches!(tag, Tag::BlockQuote(_) | Tag::List(_) | Tag::Item))
                {
                    output.push((Event::Start(tag), range));
                    output.append(&mut body);
                }
            }
            Event::End(TagEnd::FootnoteDefinition) => break,
            event @ Event::End(_) if nested => {
                output.push((event, range));
                break;
            }
            event => output.push((event, range)),
        }
    }
    (output, removed)
}

fn reference_keys(events: &[Spanned]) -> Vec<String> {
    events
        .iter()
        .filter_map(|(event, _)| match event {
            Event::FootnoteReference(name) => Some(footnote_key(name)),
            _ => None,
        })
        .collect()
}

fn footnote_key(name: &str) -> String {
    UniCase::new(name).to_folded_case()
}

fn render_reference(id: &str, reference: &str, number: usize) -> String {
    format!(
        r##"<sup class="footnote-reference" id="{}"><a href="#{}" role="doc-noteref">{number}</a></sup>"##,
        escape(reference),
        escape(id)
    )
}

fn render_backlinks(references: &[String]) -> String {
    let mut html = String::new();
    for (index, reference) in references.iter().enumerate() {
        if index > 0 {
            html.push(' ');
        }
        // U+FE0E keeps the return arrow in text presentation on platforms that default to emoji.
        _ = write!(
            &mut html,
            r##"<a class="footnote-backref" href="#{}" role="doc-backlink">↩&#xFE0E;"##,
            escape(reference)
        );
        if references.len() > 1 {
            _ = write!(&mut html, "<sup>{}</sup>", index + 1);
        }
        html.push_str("</a>");
    }
    html
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use pulldown_cmark::{Options, Parser};

    use super::*;

    // ── render ──

    #[test]
    fn renders_reference_and_endnote_list() {
        let html = render(indoc! {"
            Text[^a].

            [^a]: Note.

            After.
        "});
        assert_eq!(
            html,
            indoc! {r##"
                <p>Text<sup class="footnote-reference" id="fnref-a-1"><a href="#fn-a" role="doc-noteref">1</a></sup>.</p>
                <p>After.</p>
                <section class="footnotes" role="doc-endnotes">
                  <ol>
                    <li id="fn-a">
                <p>Note. <a class="footnote-backref" href="#fnref-a-1" role="doc-backlink">↩&#xFE0E;</a></p>
                    </li>
                  </ol>
                </section>
            "##},
        );
    }

    #[test]
    fn multiple_references_get_numbered_backlinks() {
        let html = render(indoc! {"
            First[^a] and second[^a].

            [^a]: Shared.
        "});
        assert!(
            html.contains(r#"id="fnref-a-1""#) && html.contains(r#"id="fnref-a-2""#),
            "each reference needs its own anchor, html:\n{html}",
        );
        assert!(
            html.contains(concat!(
                r##"<a class="footnote-backref" href="#fnref-a-1" role="doc-backlink">↩&#xFE0E;<sup>1</sup></a> "##,
                r##"<a class="footnote-backref" href="#fnref-a-2" role="doc-backlink">↩&#xFE0E;<sup>2</sup></a></p>"##,
            )),
            "html:\n{html}",
        );
    }

    #[test]
    fn numbers_by_first_reference() {
        let html = render(indoc! {"
            [^b]: Bee.

            [^a]: Ay.

            One[^a], two[^b], again[^a].
        "});
        assert!(html.contains(r##"<a href="#fn-a" role="doc-noteref">1</a>"##));
        assert!(html.contains(r##"<a href="#fn-b" role="doc-noteref">2</a>"##));
        let a = html.find(r#"<li id="fn-a">"#).expect("fn-a listed");
        let b = html.find(r#"<li id="fn-b">"#).expect("fn-b listed");
        assert!(a < b, "list follows reference order, html:\n{html}");
    }

    #[test]
    fn matches_names_case_insensitively() {
        let html = render(indoc! {"
            Upper[^Note] and lower[^note].

            [^NOTE]: Body.
        "});
        assert!(html.contains(r##"href="#fn-note""##), "html:\n{html}");
        assert!(html.contains(r#"<li id="fn-note">"#), "html:\n{html}");
        assert!(html.contains(r#"id="fnref-note-2""#), "html:\n{html}");
    }

    #[test]
    fn unicode_equivalent_labels_share_one_note() {
        let html = render(indoc! {"
            One[^STRASSE], two[^straße], three[^Σ], four[^ς].

            [^straße]: Street.

            [^σ]: Sigma.
        "});
        assert_eq!(html.matches("<li id=").count(), 2);
        assert!(html.contains(r##"href="#fn-strasse" role="doc-noteref">1</a>"##));
        assert!(html.contains(r#"id="fnref-strasse-2""#));
        assert!(html.contains(r#"id="fnref-σ-2""#));
    }

    #[test]
    fn backlinks_join_last_paragraph() {
        let html = render(indoc! {"
            Text[^a].

            [^a]:
                First.

                Second.
        "});
        assert!(
            html.contains(
                "<p>First.</p>\n<p>Second. <a class=\"footnote-backref\" href=\"#fnref-a-1\""
            ),
            "html:\n{html}",
        );
    }

    #[test]
    fn backlinks_follow_non_paragraph_ending() {
        let html = render(indoc! {"
            Text[^a].

            [^a]:
                Intro.

                - item
        "});
        assert!(
            html.contains("</ul>\n<p><a class=\"footnote-backref\" href=\"#fnref-a-1\""),
            "html:\n{html}",
        );
    }

    #[test]
    fn renders_nested_reference() {
        let html = render(indoc! {"
            Text[^a].

            [^a]: See[^b].

            [^b]: Inner.
        "});
        assert!(
            html.contains(r#"<p>See<sup class="footnote-reference" id="fnref-b-1">"#),
            "html:\n{html}",
        );
        assert!(html.contains(r#"<li id="fn-b">"#), "html:\n{html}");
    }

    #[test]
    fn counts_only_reachable_first_definitions() {
        let html = render(indoc! {"
            [^unused]: Hidden[^b].

            [^a]: First[^b].

            [^A]: Duplicate[^c].

            Body[^a], then[^b].

            [^b]: Back[^a].

            [^c]: Unreachable.
        "});
        assert!(html.contains(r##"href="#fn-a" role="doc-noteref">1</a>"##));
        assert!(html.contains(r##"href="#fn-b" role="doc-noteref">2</a>"##));
        for hidden in ["Hidden", "Duplicate", "Unreachable", "fn-c", "fnref-b-3"] {
            assert!(!html.contains(hidden), "{hidden}: {html}");
        }
        for reference in ["fnref-a-1", "fnref-a-2", "fnref-b-1", "fnref-b-2"] {
            assert_eq!(html.matches(&format!(r#"id="{reference}""#)).count(), 1);
            assert_eq!(
                html.matches(&format!(r##"href="#{reference}""##)).count(),
                1
            );
        }
    }

    #[test]
    fn nested_duplicate_keeps_first_definition() {
        let html = render(indoc! {"
            Body[^a].

            [^a]: Outer.

                [^A]: Inner.
        "});
        assert!(html.contains("Outer."));
        assert!(!html.contains("Inner."));
        assert_eq!(html.matches("<li id=").count(), 1);
    }

    #[test]
    fn self_reference_terminates() {
        let html = render(indoc! {"
            Body[^a].

            [^a]: Self[^a].
        "});
        assert_eq!(html.matches("<li id=").count(), 1);
        assert!(html.contains(r#"id="fnref-a-2""#));
        assert!(html.contains(r##"href="#fnref-a-2""##));
    }

    #[test]
    fn prunes_only_containers_emptied_by_definitions() {
        let html = render(indoc! {"
            Body[^a][^b].

            > [^a]: Quote note.

            - [^b]: List note.

            >

            - Kept.
        "});
        assert_eq!(html.matches("<blockquote>").count(), 1);
        assert_eq!(html.matches("<ul>").count(), 1);
        assert!(html.contains("<li>Kept.</li>"));
        assert!(html.contains("Quote note."));
        assert!(html.contains("List note."));
    }

    #[test]
    fn slugs_unsafe_labels_without_collisions() {
        let html = render(indoc! {"
            Body[^a b][^a-b][^a%20b][^a#b][^a~20b].

            [^a b]: Space.
            [^a-b]: Dash.
            [^a%20b]: Percent.
            [^a#b]: Hash.
            [^a~20b]: Tilde.
        "});
        for label in ["a-b", "a-b-1", "a-20b", "a-b-2", "a~20b"] {
            assert!(
                html.contains(&format!(r#"id="fn-{label}""#)),
                "{label}: {html}"
            );
            assert!(html.contains(&format!(r##"href="#fn-{label}""##)));
        }
    }

    #[test]
    fn replaces_quotes_in_ids() {
        let html = render(indoc! {r#"
            Text[^a"b].

            [^a"b]: Note.
        "#});
        assert!(html.contains(r#"id="fn-a-b""#), "html:\n{html}");
        assert!(!html.contains(r#"a"b"#), "html:\n{html}");
    }

    #[test]
    fn punctuation_labels_have_nonempty_ids() {
        let html = render(indoc! {"
            Body[^!][^?].

            [^!]: First.
            [^?]: Second.
        "});
        assert!(html.contains(r#"id="fn-note""#));
        assert!(html.contains(r#"id="fn-note-1""#));
        assert!(html.contains(r##"href="#fnref-note-1-1""##));
    }

    #[test]
    fn without_references_is_identity() {
        let html = render("Plain [^missing] text.");
        assert_eq!(html, "<p>Plain [^missing] text.</p>\n");
    }

    fn render(content: &str) -> String {
        let events = Parser::new_ext(content, Options::ENABLE_FOOTNOTES)
            .into_offset_iter()
            .map(|(event, range)| (event.into_static(), range));
        let (mut body, mut notes) = Notes::extract(events);
        notes.allocate_ids(&mut body, &mut PageIds::default(), 0);
        let mut html = push_html(body);
        notes.render(&mut html, push_html);
        html
    }

    fn push_html(events: Vec<Spanned>) -> String {
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, events.into_iter().map(|(event, _)| event));
        html
    }
}
