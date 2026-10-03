use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, bail};
use pulldown_cmark::HeadingLevel;

use crate::content::frontmatter::HeadingNumbering;
use crate::html::escape;

pub(super) struct HeadingNumbers {
    start: usize,
    starts: BTreeMap<String, usize>,
    levels: Vec<(HeadingLevel, usize)>,
    used: HashSet<String>,
}

impl HeadingNumbers {
    pub(super) fn new(settings: &HeadingNumbering) -> Self {
        Self {
            start: settings.start,
            starts: settings.starts.clone(),
            levels: Vec::new(),
            used: HashSet::new(),
        }
    }

    /// Advances the outline number for `level`, collapsing skipped heading levels.
    pub(super) fn next(&mut self, level: HeadingLevel, id: &str) -> Result<String> {
        let mut number = if self.levels.is_empty() {
            self.start
        } else {
            1
        };
        while self
            .levels
            .last()
            .is_some_and(|(previous, _)| *previous >= level)
        {
            if let Some((_, previous)) = self.levels.pop() {
                number = previous + 1;
            }
        }
        if let Some(start) = self.starts.remove(id) {
            number = start;
        }
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

    pub(super) fn validate_starts(&self) -> Result<()> {
        if !self.starts.is_empty() {
            bail!(
                "heading numbering starts refer to unknown heading IDs: {}",
                self.starts.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        }
        Ok(())
    }
}

pub(super) fn render_number(number: Option<&str>) -> String {
    number.map_or_else(String::new, |number| {
        format!(r#"<span class="heading-number">{}</span> "#, escape(number))
    })
}
