//! Layered internationalization (i18n) strings.
//!
//! Strings resolve from three TOML tables, in decreasing precedence:
//!
//! 1. `<site_root>/i18n/<language>.toml` — site-level overrides
//! 2. `<theme>/i18n/<language>.toml` — theme strings for the active language
//! 3. `<theme>/i18n/en.toml` — theme English fallback

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::content::read_optional;

/// BCP 47 language tag selecting translation files. Tags are limited to ASCII letters, digits
/// and hyphens, starting with a letter, because they are interpolated into file names.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String")]
pub struct Language(String);

impl Language {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Language {
    fn default() -> Self {
        Self("en".to_owned())
    }
}

impl TryFrom<String> for Language {
    type Error = String;

    fn try_from(tag: String) -> Result<Self, String> {
        if tag.starts_with(|c: char| c.is_ascii_alphabetic())
            && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            Ok(Self(tag))
        } else {
            Err(format!(
                "invalid language tag `{tag}`: expected BCP 47 shape (ASCII letters / digits / hyphens)"
            ))
        }
    }
}

impl FromStr for Language {
    type Err = String;

    fn from_str(tag: &str) -> Result<Self, String> {
        tag.to_owned().try_into()
    }
}

/// Merged i18n strings for a single active language.
#[derive(Debug, Clone)]
pub struct I18n {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    strings: HashMap<String, String>,
    /// Key and warning pairs already emitted by [`I18n::t_interp`], so each one is logged once.
    /// `t()` misses are not warnings. See [`I18n::t`].
    warned: Mutex<HashSet<(String, Warning<String>)>>,
}

impl I18n {
    /// Loads and merges `<theme>/i18n/{en,<language>}.toml` and `<site_root>/i18n/<language>.toml`.
    ///
    /// Precedence: site override → theme active-language → theme English. If the theme has no
    /// `i18n/` directory, site-only i18n is allowed.
    ///
    /// # Errors
    ///
    /// Returns an error if locale files cannot be read, a theme with locale files lacks
    /// `en.toml`, or a loaded file is not a flat table of strings.
    pub fn load(site_root: &Path, theme_dir: Option<&Path>, language: &Language) -> Result<Self> {
        let file_name = format!("{}.toml", language.as_str());
        let mut strings: HashMap<String, String> = HashMap::new();

        if let Some(theme_dir) = theme_dir {
            let theme_i18n_dir = theme_dir.join("i18n");
            if theme_i18n_dir.is_dir() {
                ensure_english_fallback(&theme_i18n_dir)?;
                merge_file(&mut strings, &theme_i18n_dir.join("en.toml"))?;
                if language.as_str() != "en" {
                    merge_file(&mut strings, &theme_i18n_dir.join(&file_name))?;
                }
            }
        }

        merge_file(&mut strings, &site_root.join("i18n").join(&file_name))?;

        Ok(Self {
            inner: Arc::new(Inner {
                strings,
                warned: Mutex::new(HashSet::new()),
            }),
        })
    }

    /// Looks up a string by key, or returns the key itself on miss.
    #[must_use]
    pub fn t<'a>(&'a self, key: &str) -> Cow<'a, str> {
        if let Some(value) = self.inner.strings.get(key) {
            return Cow::Borrowed(value);
        }
        Cow::Owned(key.to_owned())
    }

    /// Looks up a string by key and interpolates Python-style `{name}` placeholders from `args`.
    ///
    /// `{{` and `}}` render as literal braces. Missing placeholders substitute an empty string and
    /// emit a warning. An unclosed `{` emits a warning and renders the partial literal as-is.
    #[must_use]
    pub fn t_interp(&self, key: &str, args: &BTreeMap<&str, impl AsRef<str>>) -> String {
        let template = self.t(key);
        interpolate(&template, args, |warning| self.warn_once(key, warning))
    }

    fn warn_once(&self, key: &str, warning: Warning<&str>) {
        let mut warned = self.inner.warned.lock().expect("i18n warned poisoned");
        if !warned.insert((key.to_owned(), warning.map(str::to_owned))) {
            return;
        }
        // Drop the guard before calling into `tracing`, since the subscriber may run arbitrary code
        // and holding the lock across it risks a re-entrant deadlock.
        drop(warned);
        match warning {
            Warning::MissingPlaceholder(name) => {
                tracing::warn!(key, name, "missing placeholder for i18n key");
            }
            Warning::UnclosedPlaceholder(partial) => {
                tracing::warn!(key, partial, "unclosed placeholder in i18n key");
            }
        }
    }
}

// ── Loading helpers ──

fn ensure_english_fallback(dir: &Path) -> Result<()> {
    let mut has_locales = false;
    let mut has_english = false;
    for entry in fs::read_dir(dir)
        .with_context(|| format!("failed to read i18n directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
        {
            has_locales = true;
            has_english |= entry.file_name() == "en.toml";
        }
    }
    ensure!(
        !has_locales || has_english,
        "theme i18n directory {} is missing required en.toml fallback",
        dir.display(),
    );
    Ok(())
}

/// Merges the strings of a locale file, if it exists, over those already loaded.
fn merge_file(strings: &mut HashMap<String, String>, path: &Path) -> Result<()> {
    let Some(contents) = read_optional(path)? else {
        return Ok(());
    };
    let table: toml::Table = toml::from_str(&contents)
        .with_context(|| format!("failed to parse i18n file {}", path.display()))?;

    for (key, value) in table {
        let toml::Value::String(value) = value else {
            bail!(
                "i18n key `{key}` in {} must be a string, found `{}`",
                path.display(),
                value.type_str(),
            );
        };
        strings.insert(key, value);
    }
    Ok(())
}

// ── Interpolation ──

/// Non-fatal interpolation problem, carrying the placeholder name or the unclosed partial text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Warning<S> {
    MissingPlaceholder(S),
    UnclosedPlaceholder(S),
}

impl<S> Warning<S> {
    fn map<T>(self, f: impl FnOnce(S) -> T) -> Warning<T> {
        match self {
            Self::MissingPlaceholder(name) => Warning::MissingPlaceholder(f(name)),
            Self::UnclosedPlaceholder(partial) => Warning::UnclosedPlaceholder(f(partial)),
        }
    }
}

/// Interpolates `template` by the rules of [`I18n::t_interp`], reporting its warnings via `warn`.
fn interpolate(
    template: &str,
    args: &BTreeMap<&str, impl AsRef<str>>,
    mut warn: impl FnMut(Warning<&str>),
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        match ch {
            '{' => {
                if let Some(&(_, '{')) = chars.peek() {
                    chars.next();
                    out.push('{');
                    continue;
                }
                let mut name = String::new();
                let mut closed = false;
                for (_, c) in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    name.push(c);
                }
                if !closed {
                    warn(Warning::UnclosedPlaceholder(&name));
                    out.push('{');
                    out.push_str(&name);
                    break;
                }
                if let Some(value) = args.get(name.as_str()) {
                    out.push_str(value.as_ref());
                } else {
                    warn(Warning::MissingPlaceholder(&name));
                }
            }
            '}' => {
                // `}}` is the escape for a literal `}`, and a stray `}` renders literally too.
                // Either way we emit one `}`, consuming the second brace only when present.
                if let Some(&(_, '}')) = chars.peek() {
                    chars.next();
                }
                out.push('}');
            }
            _ => out.push(ch),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::test_utils::write_test_file;

    // ── load ──

    #[test]
    fn merges_site_over_theme_language_over_theme_english() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();

        write_test_file(
            theme.path(),
            "i18n/en.toml",
            indoc! {r#"
                all_posts = "All Posts"
                back_to_top = "Back to Top"
                only_in_en = "from en"
            "#},
        );
        write_test_file(
            theme.path(),
            "i18n/zh-Hans.toml",
            indoc! {r#"
                all_posts = "全部文章"
                back_to_top = "回到顶部"
                translated_only = "本地化"
            "#},
        );
        write_test_file(
            site.path(),
            "i18n/zh-Hans.toml",
            indoc! {r#"
                back_to_top = "回顶"
            "#},
        );

        let i18n = I18n::load(site.path(), Some(theme.path()), &lang("zh-Hans")).unwrap();
        assert_eq!(i18n.t("back_to_top").as_ref(), "回顶");
        assert_eq!(i18n.t("all_posts").as_ref(), "全部文章");
        assert_eq!(i18n.t("only_in_en").as_ref(), "from en");
        assert_eq!(i18n.t("translated_only").as_ref(), "本地化");
    }

    #[test]
    fn english_uses_en_toml_as_sole_source() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_test_file(
            theme.path(),
            "i18n/en.toml",
            indoc! {r#"
                all_posts = "All Posts"
            "#},
        );

        let i18n = I18n::load(site.path(), Some(theme.path()), &lang("en")).unwrap();
        assert_eq!(i18n.t("all_posts").as_ref(), "All Posts");
    }

    #[test]
    fn missing_language_falls_back_to_english() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_test_file(theme.path(), "i18n/en.toml", r#"greeting = "Hello""#);

        let i18n = I18n::load(site.path(), Some(theme.path()), &lang("fr")).unwrap();
        assert_eq!(i18n.t("greeting"), "Hello");
    }

    #[test]
    fn loads_site_strings_without_theme_i18n() {
        let theme = tempfile::tempdir().unwrap();
        for theme_dir in [Some(theme.path()), None] {
            let site = tempfile::tempdir().unwrap();
            write_test_file(
                site.path(),
                "i18n/zh-Hans.toml",
                indoc! {r#"
                    hello = "你好"
                "#},
            );

            let i18n = I18n::load(site.path(), theme_dir, &lang("zh-Hans")).unwrap();
            assert_eq!(i18n.t("hello").as_ref(), "你好", "{theme_dir:?}");
        }
    }

    #[test]
    fn ignores_non_toml_entries() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_test_file(theme.path(), "i18n/README.md", "translator notes");
        fs::create_dir_all(theme.path().join("i18n/.backup")).unwrap();

        let i18n = I18n::load(site.path(), Some(theme.path()), &lang("en")).unwrap();
        assert_eq!(i18n.t("greeting"), "greeting");
    }

    #[test]
    fn rejects_theme_locales_without_en_toml() {
        for locale in ["i18n/En.toml", "i18n/zh-Hans.toml"] {
            let site = tempfile::tempdir().unwrap();
            let theme = tempfile::tempdir().unwrap();
            write_test_file(theme.path(), locale, r#"greeting = "Hello""#);

            let error = I18n::load(site.path(), Some(theme.path()), &lang("zh-Hans")).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("missing required en.toml fallback"),
                "{locale}: {error}"
            );
        }
    }

    #[test]
    fn rejects_non_string_values() {
        for contents in [r#"nested = { key = "value" }"#, "count = 42"] {
            let site = tempfile::tempdir().unwrap();
            let theme = tempfile::tempdir().unwrap();
            write_test_file(theme.path(), "i18n/en.toml", contents);

            let error = I18n::load(site.path(), Some(theme.path()), &lang("en")).unwrap_err();
            assert!(
                error.to_string().contains("must be a string"),
                "{contents}: {error}"
            );
        }
    }

    #[test]
    fn rejects_malformed_toml() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_test_file(theme.path(), "i18n/en.toml", "key = \n");

        let err = I18n::load(site.path(), Some(theme.path()), &lang("en"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to parse i18n file"),
            "should report parse failure, got: {err}"
        );
    }

    #[test]
    fn reports_unreadable_theme_and_site_locales() {
        for in_theme in [true, false] {
            let site = tempfile::tempdir().unwrap();
            let theme = tempfile::tempdir().unwrap();
            write_test_file(theme.path(), "i18n/en.toml", "");
            let locale_dir = if in_theme { theme.path() } else { site.path() }.join("i18n");
            let locale = locale_dir.join("fr.toml");
            fs::create_dir_all(&locale).unwrap();

            let err = I18n::load(site.path(), Some(theme.path()), &lang("fr"))
                .unwrap_err()
                .to_string();
            assert_eq!(err, format!("failed to read {}", locale.display()));
        }
    }

    // ── t ──

    #[test]
    fn returns_translation_for_known_key() {
        let i18n = make_i18n(&[("greeting", "Hello")]);
        assert_eq!(i18n.t("greeting").as_ref(), "Hello");
    }

    #[test]
    fn returns_missing_key_without_warning() {
        let i18n = make_i18n(&[]);
        assert_eq!(i18n.t("missing_key").as_ref(), "missing_key");
        assert!(
            i18n.inner.warned.lock().unwrap().is_empty(),
            "the literal key fallback must not warn"
        );
    }

    // ── t_interp ──

    #[test]
    fn substitutes_placeholders_and_ignores_unused_args() {
        let i18n = make_i18n(&[("counter", "Page {current} of {total}")]);
        let mut args = BTreeMap::new();
        args.insert("current", "2");
        args.insert("total", "5");
        args.insert("unused", "nope");
        assert_eq!(i18n.t_interp("counter", &args), "Page 2 of 5");
    }

    #[test]
    fn unescapes_doubled_braces_and_keeps_stray_close() {
        let i18n = make_i18n(&[("set", "{{literal}} and {name} }")]);
        let mut args = BTreeMap::new();
        args.insert("name", "X");
        assert_eq!(i18n.t_interp("set", &args), "{literal} and X }");
    }

    #[test]
    fn missing_arg_renders_empty_and_warns_once() {
        let i18n = make_i18n(&[("hello", "Hi {who}!")]);
        let args = BTreeMap::<&str, &str>::new();
        for _ in 0..2 {
            assert_eq!(i18n.t_interp("hello", &args), "Hi !");
        }
        let warned = i18n.inner.warned.lock().unwrap();
        assert_eq!(warned.len(), 1);
        assert!(warned.contains(&(
            "hello".to_owned(),
            Warning::MissingPlaceholder("who".to_owned())
        )));
    }

    #[test]
    fn unclosed_brace_renders_partial_and_warns_once() {
        let i18n = make_i18n(&[("bad", "start {unclosed tail")]);
        let args = BTreeMap::<&str, &str>::new();
        for _ in 0..2 {
            assert_eq!(i18n.t_interp("bad", &args), "start {unclosed tail");
        }
        let warned = i18n.inner.warned.lock().unwrap();
        assert_eq!(warned.len(), 1);
        assert!(warned.contains(&(
            "bad".to_owned(),
            Warning::UnclosedPlaceholder("unclosed tail".to_owned())
        )));
    }

    fn lang(tag: &str) -> Language {
        tag.parse().unwrap()
    }

    fn make_i18n(pairs: &[(&str, &str)]) -> I18n {
        let strings = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        I18n {
            inner: Arc::new(Inner {
                strings,
                warned: Mutex::new(HashSet::new()),
            }),
        }
    }
}
