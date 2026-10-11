use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use itertools::Itertools;
use pulldown_cmark::HeadingLevel;

use crate::html::escape;

/// Heading attribute that restarts the outline number at the heading's level.
pub(super) const NUMBERING_START: &str = "numbering-start";

#[derive(Default)]
pub(super) struct HeadingNumbers {
    levels: Vec<(HeadingLevel, usize)>,
    used: HashSet<String>,
}

impl HeadingNumbers {
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
                    format!("heading ID {id}: {NUMBERING_START} must be a non-negative integer, got {start:?}")
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
        } else {
            1
        };
        self.levels.push((level, number));
        let number = self.levels.iter().map(|(_, number)| number).join(".");

        if !self.used.insert(number.clone()) {
            bail!("heading numbering produces duplicate number {number} at heading ID {id}");
        }
        Ok(number)
    }
}

pub(super) fn render_number(number: &str) -> String {
    format!(r#"<span class="heading-number">{}</span> "#, escape(number))
}
