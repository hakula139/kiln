use pulldown_cmark::HeadingLevel;

use crate::html::escape;

#[derive(Default)]
pub(super) struct HeadingNumbers {
    levels: Vec<(HeadingLevel, usize)>,
}

impl HeadingNumbers {
    /// Advances the outline number for `level`, collapsing skipped heading levels.
    pub(super) fn next(&mut self, level: HeadingLevel) -> String {
        let mut number = 1;
        while self
            .levels
            .last()
            .is_some_and(|(previous, _)| *previous >= level)
        {
            if let Some((_, previous)) = self.levels.pop() {
                number = previous + 1;
            }
        }
        self.levels.push((level, number));
        self.levels
            .iter()
            .map(|(_, number)| number.to_string())
            .collect::<Vec<_>>()
            .join(".")
    }
}

pub(super) fn render_number(number: Option<&str>) -> String {
    number.map_or_else(String::new, |number| {
        format!(r#"<span class="heading-number">{}</span> "#, escape(number))
    })
}
