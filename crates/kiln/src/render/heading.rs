use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use pulldown_cmark::HeadingLevel;

use crate::content::frontmatter::HeadingNumbering;
use crate::html::escape;

pub(super) struct HeadingNumbers {
    start: usize,
    levels: Vec<(HeadingLevel, usize)>,
    used: HashSet<String>,
}

impl HeadingNumbers {
    pub(super) fn new(settings: &HeadingNumbering) -> Self {
        Self {
            start: settings.start,
            levels: Vec::new(),
            used: HashSet::new(),
        }
    }

    /// Advances the outline number, optionally resetting its current level while preserving parents.
    pub(super) fn next(
        &mut self,
        level: HeadingLevel,
        start: Option<&str>,
        id: &str,
    ) -> Result<String> {
        let start = start
            .map(|start| {
                start.parse::<usize>().with_context(|| {
                    format!("heading ID {id}: numbering-start must be a non-negative integer, got {start:?}")
                })
            })
            .transpose()?;

        let mut previous = None;
        while self
            .levels
            .last()
            .is_some_and(|(previous, _)| *previous >= level)
        {
            previous = self.levels.pop().map(|(_, number)| number);
        }

        let number = if let Some(start) = start {
            start
        } else if let Some(previous) = previous {
            previous.checked_add(1).with_context(|| {
                format!("heading ID {id}: heading numbering exceeds the maximum supported integer")
            })?
        } else if self.levels.is_empty() {
            self.start
        } else {
            1
        };
        self.levels.push((level, number));
        let number = self
            .levels
            .iter()
            .map(|(_, number)| number.to_string())
            .collect::<Vec<_>>()
            .join(".");

        if !self.used.insert(number.clone()) {
            bail!("heading numbering produces duplicate number {number} at heading ID {id}");
        }
        Ok(number)
    }
}

pub(super) fn render_number(number: Option<&str>) -> String {
    number.map_or_else(String::new, |number| {
        format!(r#"<span class="heading-number">{}</span> "#, escape(number))
    })
}
