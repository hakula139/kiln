use std::collections::HashSet;

use scraper::Html;

#[derive(Default)]
pub(super) struct PageIds {
    used: HashSet<String>,
}

impl PageIds {
    pub(super) fn reserve(&mut self, id: &str) {
        self.used.insert(id.to_owned());
    }

    pub(super) fn reserve_html(&mut self, html: &str) {
        let fragment = Html::parse_fragment(html);
        for node in fragment.tree.root().descendants() {
            if let Some(id) = node
                .value()
                .as_element()
                .and_then(|element| element.attr("id"))
            {
                self.reserve(id);
            }
        }
    }

    pub(super) fn allocate(&mut self, id: &str) -> String {
        if self.used.insert(id.to_owned()) {
            return id.to_owned();
        }
        let mut suffix = 1;
        loop {
            let candidate = format!("{id}-{suffix}");
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
            suffix += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PageIds::reserve_html ──

    #[test]
    fn reserve_html_decodes_attributes_and_ignores_text() {
        let mut ids = PageIds::default();
        ids.reserve_html(indoc::indoc! {r#"
            <div ID=shared></div><span id='fn&#45;a'></span><p id="quoted"></p>
            <!-- <i id="comment"></i> -->
            <script>const text = '<i id="script"></i>';</script>
            <style>p::after { content: '<i id="style"></i>'; }</style>
            <textarea><i id="textarea"></i></textarea>
            <code>&lt;i id="code"&gt;</code>
            <svg><title/><g id="svg"></g></svg>
        "#});
        for reserved in ["shared", "fn-a", "quoted", "svg"] {
            assert_eq!(ids.allocate(reserved), format!("{reserved}-1"));
        }
        for literal in ["comment", "script", "style", "textarea", "code"] {
            assert_eq!(ids.allocate(literal), literal);
        }
    }

    // ── PageIds::allocate ──

    #[test]
    fn allocate_skips_authored_and_generated_suffixes() {
        let mut ids = PageIds::default();
        ids.reserve("name-1");
        assert_eq!(ids.allocate("name"), "name");
        assert_eq!(ids.allocate("name"), "name-2");
        assert_eq!(ids.allocate("name-2"), "name-2-1");
    }
}
