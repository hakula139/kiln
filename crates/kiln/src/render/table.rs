use pulldown_cmark::{Alignment, Event, Tag, TagEnd};
use unicode_width::UnicodeWidthStr;

/// Adds `class="nowrap"` to every cell of a table column whose widest cell fits within
/// `max_width` terminal columns. Only text, inline code, and math source count.
pub(super) fn stamp_nowrap(events: &mut [Event<'_>], max_width: usize) {
    let mut stamps = Vec::new();
    let mut table = None;
    for (index, event) in events.iter().enumerate() {
        match event {
            Event::Start(Tag::Table(alignments)) => {
                table = Some(TableLayout::new(alignments.clone()));
            }
            Event::End(TagEnd::Table) => {
                if let Some(table) = table.take() {
                    stamps.extend(table.nowrap_cells(max_width));
                }
            }
            _ => {
                if let Some(table) = &mut table {
                    table.observe(event, index);
                }
            }
        }
    }
    for (index, html) in stamps {
        events[index] = Event::Html(html.into());
    }
}

struct TableLayout {
    alignments: Vec<Alignment>,
    column_widths: Vec<usize>,
    cells: Vec<Cell>,
    in_head: bool,
    column: usize,
    cell_text: String,
}

struct Cell {
    event_index: usize,
    column: usize,
    is_head: bool,
}

impl TableLayout {
    fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            column_widths: vec![0; alignments.len()],
            alignments,
            cells: Vec::new(),
            in_head: false,
            column: 0,
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
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) => self.cell_text.push_str(t),
            _ => {}
        }
    }

    /// Yields raw HTML replacing each nowrap cell's `Start(TableCell)`. The `End(TableCell)` events
    /// stay untouched, so pulldown-cmark still closes each cell from its own head / body state.
    fn nowrap_cells(self, max_width: usize) -> impl Iterator<Item = (usize, String)> {
        self.cells
            .into_iter()
            .filter(move |cell| self.column_widths[cell.column] <= max_width)
            .map(move |cell| {
                let tag = if cell.is_head { "th" } else { "td" };
                let style = match self.alignments[cell.column] {
                    Alignment::None => "",
                    Alignment::Left => r#" style="text-align: left""#,
                    Alignment::Center => r#" style="text-align: center""#,
                    Alignment::Right => r#" style="text-align: right""#,
                };
                (
                    cell.event_index,
                    format!(r#"<{tag} class="nowrap"{style}>"#),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use pulldown_cmark::{Options, Parser};

    use super::*;

    // ── stamp_nowrap ──

    #[test]
    fn marks_every_cell_of_short_columns() {
        // 名称 peaks at 8 with `Cherries`, 产地 at 4, 价格 at 5, and 说明 at 12 (CJK = 2).
        let html = render(
            indoc! {"
                | 名称 | 产地 | 价格 | 说明 |
                | :--- | :--: | ---: | --- |
                | Cherries | 北方 | 12.50 | 红色的小水果 |
                | 香蕉 | 南方 | 3.00 | 黄色水果 |
            "},
            8,
        );

        let expected = indoc! {r#"
            <table><thead><tr><th class="nowrap" style="text-align: left">名称</th><th class="nowrap" style="text-align: center">产地</th><th class="nowrap" style="text-align: right">价格</th><th>说明</th></tr></thead><tbody>
            <tr><td class="nowrap" style="text-align: left">Cherries</td><td class="nowrap" style="text-align: center">北方</td><td class="nowrap" style="text-align: right">12.50</td><td>红色的小水果</td></tr>
            <tr><td class="nowrap" style="text-align: left">香蕉</td><td class="nowrap" style="text-align: center">南方</td><td class="nowrap" style="text-align: right">3.00</td><td>黄色水果</td></tr>
            </tbody></table>
        "#};
        assert_eq!(html, expected);
    }

    #[test]
    fn measures_visible_text_and_math_source() {
        // Visible text is `ab 🎮 cd x2`: the link target, code backticks, math delimiters, and
        // footnote label contribute nothing, and the emoji counts as 2.
        let md = indoc! {"
            | Name |
            | ---- |
            | [ab](https://example.com/a-very-long-url) 🎮 `cd` $x2$[^note] |

            [^note]: A footnote.
        "};
        assert!(
            render(md, 11).contains(r#"<td class="nowrap"><a href="#),
            "cell at the threshold should be nowrap"
        );
        assert!(
            render(md, 10).contains("<td><a href="),
            "cell one column over the threshold should wrap"
        );
    }

    #[test]
    fn follows_padded_ragged_rows() {
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
    fn resets_per_table() {
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

    fn render(content: &str, max_width: usize) -> String {
        let options = Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_MATH;
        let mut events: Vec<_> = Parser::new_ext(content, options).collect();
        stamp_nowrap(&mut events, max_width);
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, events.into_iter());
        html
    }
}
