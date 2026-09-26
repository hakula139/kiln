use pulldown_cmark::{Alignment, Event, Tag, TagEnd};
use unicode_width::UnicodeWidthStr;

/// Adds `class="nowrap"` to every cell of a table column whose widest cell fits within
/// `max_width` terminal columns. Only text, inline code, and math source count.
pub(super) struct TableNowrap {
    max_width: usize,
    table: Option<TableLayout>,
}

struct TableLayout {
    alignments: Vec<Alignment>,
    column_widths: Vec<usize>,
    cells: Vec<Cell>,
    in_head: bool,
    column: usize,
    image_depth: usize,
    cell_text: String,
}

struct Cell {
    event_index: usize,
    column: usize,
    is_head: bool,
}

impl TableNowrap {
    pub fn new(max_width: usize) -> Self {
        Self {
            max_width,
            table: None,
        }
    }

    /// Records `event`, which the caller pushes onto `events` next.
    pub fn observe(&mut self, event: &Event<'_>, events: &mut [Event<'_>]) {
        match event {
            Event::Start(Tag::Table(alignments)) => {
                self.table = Some(TableLayout::new(alignments.clone()));
            }
            Event::End(TagEnd::Table) => {
                if let Some(table) = self.table.take() {
                    table.stamp_nowrap(self.max_width, events);
                }
            }
            _ => {
                if let Some(table) = &mut self.table {
                    table.observe(event, events.len());
                }
            }
        }
    }
}

impl TableLayout {
    fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            column_widths: vec![0; alignments.len()],
            alignments,
            cells: Vec::new(),
            in_head: false,
            column: 0,
            image_depth: 0,
            cell_text: String::new(),
        }
    }

    fn observe(&mut self, event: &Event<'_>, event_index: usize) {
        match event {
            Event::Start(Tag::TableHead) => {
                self.in_head = true;
                self.column = 0;
            }
            Event::End(TagEnd::TableHead) => self.in_head = false,
            Event::Start(Tag::TableRow) => self.column = 0,
            Event::Start(Tag::TableCell) => {
                self.cells.push(Cell {
                    event_index,
                    column: self.column,
                    is_head: self.in_head,
                });
                self.cell_text.clear();
            }
            Event::End(TagEnd::TableCell) => {
                let width = self.cell_text.trim().width();
                self.column_widths[self.column] = self.column_widths[self.column].max(width);
                self.column += 1;
            }
            Event::Start(Tag::Image { .. }) => self.image_depth += 1,
            Event::End(TagEnd::Image) => self.image_depth -= 1,
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) if self.image_depth == 0 => {
                self.cell_text.push_str(t);
            }
            _ => {}
        }
    }

    /// Swaps each nowrap cell's `Start(TableCell)` for raw HTML. The `End(TableCell)` events stay
    /// untouched, so pulldown-cmark still closes each cell from its own head / body state.
    fn stamp_nowrap(self, max_width: usize, events: &mut [Event<'_>]) {
        for cell in &self.cells {
            if self.column_widths[cell.column] > max_width {
                continue;
            }
            let tag = if cell.is_head { "th" } else { "td" };
            let style = match self.alignments[cell.column] {
                Alignment::None => "",
                Alignment::Left => r#" style="text-align: left""#,
                Alignment::Center => r#" style="text-align: center""#,
                Alignment::Right => r#" style="text-align: right""#,
            };
            events[cell.event_index] =
                Event::Html(format!(r#"<{tag} class="nowrap"{style}>"#).into());
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use pulldown_cmark::{Options, Parser};

    use super::*;

    fn render(content: &str, max_width: usize) -> String {
        let options = Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_MATH;
        let mut nowrap = TableNowrap::new(max_width);
        let mut events = Vec::new();
        for event in Parser::new_ext(content, options) {
            nowrap.observe(&event, &mut events);
            events.push(event);
        }
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, events.into_iter());
        html
    }

    // ── TableNowrap::observe ──

    #[test]
    fn observe_marks_every_cell_of_short_columns() {
        // 名称 peaks at 8 with `Cherries`, 产地 at 4, and 说明 at 12 (CJK = 2).
        let html = render(
            indoc! {"
                | 名称 | 产地 | 说明 |
                | :--- | :--: | ---: |
                | Cherries | 北方 | 红色的小水果 |
                | 香蕉 | 南方 | 黄色水果 |
            "},
            8,
        );

        let expected = indoc! {r#"
            <table><thead><tr><th class="nowrap" style="text-align: left">名称</th><th class="nowrap" style="text-align: center">产地</th><th style="text-align: right">说明</th></tr></thead><tbody>
            <tr><td class="nowrap" style="text-align: left">Cherries</td><td class="nowrap" style="text-align: center">北方</td><td style="text-align: right">红色的小水果</td></tr>
            <tr><td class="nowrap" style="text-align: left">香蕉</td><td class="nowrap" style="text-align: center">南方</td><td style="text-align: right">黄色水果</td></tr>
            </tbody></table>
        "#};
        assert_eq!(html, expected);
    }

    #[test]
    fn observe_measures_visible_text_and_math_source() {
        // Visible text is `ab 🎮 cd  x2`: the link target, image alt text, code backticks, math
        // delimiters, and footnote label contribute nothing, and the emoji counts as 2.
        let md = indoc! {"
            | Name |
            | ---- |
            | [ab](https://example.com/a-very-long-url) 🎮 `cd` ![long alt text](a.png) $x2$[^note] |

            [^note]: A footnote.
        "};
        assert!(
            render(md, 12).contains(r#"<td class="nowrap"><a href="#),
            "cell at the threshold should be nowrap"
        );
        assert!(
            render(md, 11).contains("<td><a href="),
            "cell one column over the threshold should wrap"
        );
    }

    #[test]
    fn observe_follows_padded_ragged_rows() {
        // pulldown-cmark pads short rows and drops cells beyond the header's column count.
        let html = render(
            indoc! {"
                | A | B |
                | - | - |
                | a |
                | a | a long cell | dropped |
            "},
            4,
        );
        assert!(
            html.contains(r#"<tr><td class="nowrap">a</td><td></td></tr>"#),
            "padded cell should follow its wrapping column, html:\n{html}"
        );
        assert!(
            html.contains(r#"<tr><td class="nowrap">a</td><td>a long cell</td></tr>"#),
            "dropped cell should not shift columns, html:\n{html}"
        );
    }

    #[test]
    fn observe_resets_per_table() {
        let html = render(
            indoc! {"
                | A |
                | - |
                | a long cell |

                | B |
                | - |
                | b |
            "},
            4,
        );
        assert!(
            html.contains("<th>A</th>") && html.contains(r#"<th class="nowrap">B</th>"#),
            "column widths should not leak between tables, html:\n{html}"
        );
    }
}
