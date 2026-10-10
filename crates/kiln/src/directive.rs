pub(crate) mod callout;
pub(crate) mod div;
pub(crate) mod parser;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;

use serde::Serialize;

use self::callout::Callout;
use crate::attrs::{Identity, PandocAttrs};

/// A `:::`-fenced directive block extracted from content.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DirectiveBlock {
    /// Directive name, empty for an anonymous `::: {...}` block.
    pub name: String,
    pub args: DirectiveArgs,
    /// Byte range of the body between the fences, excluding its final line ending. For nested
    /// directives, the outer body contains the inner fences verbatim.
    pub body: Range<usize>,
    /// Byte range from the opening fence through the closing fence.
    pub range: Range<usize>,
}

/// Owned arguments from a directive's `{...}` attribute block.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DirectiveArgs {
    pub positional: Vec<String>,
    /// Named values, where a repeated key keeps its last value.
    pub named: BTreeMap<String, String>,
    pub identity: Identity,
}

impl From<PandocAttrs<'_>> for DirectiveArgs {
    fn from(attrs: PandocAttrs<'_>) -> Self {
        Self {
            identity: attrs.identity(),
            positional: attrs.positional.into_iter().map(Cow::into_owned).collect(),
            named: attrs
                .named
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value.into_owned()))
                .collect(),
        }
    }
}

/// Variables passed to a `directives/<name>.html` template.
#[derive(Debug, Serialize)]
pub(crate) struct DirectiveContext<'a> {
    pub name: &'a str,
    pub positional_args: &'a [String],
    pub named_args: &'a BTreeMap<String, String>,
    #[serde(flatten)]
    pub identity: &'a Identity,
    pub body_html: &'a str,
    /// Validated `type`, `title` and `open` values of a `callout` directive.
    #[serde(flatten)]
    pub callout: Option<Callout<'a>>,
    pub source_dir: Option<Cow<'a, str>>,
    #[serde(rename = "__page_url")]
    pub page_url: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::parse_pandoc_attrs;

    // ── DirectiveArgs::from ──

    #[test]
    fn directive_args_from_pandoc_attrs_owns_all_values() {
        let args = DirectiveArgs::from(parse_pandoc_attrs(
            r#"#my-id .wide "He said \"hi\"" bare server="example" type=song"#,
        ));
        assert_eq!(
            args,
            DirectiveArgs {
                positional: vec![r#"He said "hi""#.into(), "bare".into()],
                named: BTreeMap::from([
                    ("server".into(), "example".into()),
                    ("type".into(), "song".into()),
                ]),
                identity: Identity {
                    id: Some("my-id".into()),
                    classes: vec!["wide".into()],
                },
            }
        );
    }

    #[test]
    fn directive_args_from_pandoc_attrs_duplicate_named_last_wins() {
        let args = DirectiveArgs::from(parse_pandoc_attrs(r#"key="first" key="second""#));
        assert_eq!(
            args.named,
            BTreeMap::from([("key".into(), "second".into())])
        );
    }
}
