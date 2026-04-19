//! Layered internationalization (i18n) strings.
//!
//! Strings resolve from three TOML tables, in decreasing precedence:
//!
//! 1. `<site_root>/i18n/<language>.toml` — site-level overrides
//! 2. `<theme>/i18n/<language>.toml` — theme strings for the active language
//! 3. `<theme>/i18n/en.toml` — theme English fallback
//!
//! Every i18n table must declare `date_format` (a strftime format string)
//! somewhere in the merge chain. `date_format` is extracted into a dedicated
//! field rather than left in the string map, so the `localdate` filter and
//! other callers read it via [`I18n::date_format`] rather than `t()`.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};

const DATE_FORMAT_KEY: &str = "date_format";

/// Merged i18n strings for a single active language, plus the resolved
/// strftime `date_format`.
#[derive(Debug, Clone)]
pub struct I18n {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// Merged strings for the active language (excluding `date_format`).
    strings: HashMap<String, String>,
    /// Resolved strftime `date_format` for the active language.
    date_format: String,
    /// Active BCP 47 language tag.
    language: String,
    /// Warnings we've already emitted, to keep the log from spamming.
    warned: Mutex<HashSet<WarnKey>>,
}

/// Deduplication key for warnings emitted by [`I18n::t`] and
/// [`I18n::t_interp`]. Each unique variant is logged once per `I18n`
/// instance regardless of how many pages trigger it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum WarnKey {
    /// `t(key)` miss.
    MissingKey(String),
    /// Placeholder `{name}` missing from `t_interp` args for `key`.
    MissingPlaceholder { key: String, name: String },
    /// Unclosed `{` in the value for `key`.
    UnclosedPlaceholder { key: String, partial: String },
}

impl I18n {
    /// Loads i18n tables from `<theme>/i18n/{en,<language>}.toml` and
    /// `<site_root>/i18n/<language>.toml` and merges them.
    ///
    /// Precedence (highest to lowest): site override → theme active-language
    /// → theme English. If the theme has no `i18n/` directory at all,
    /// site-only i18n is allowed.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - a theme i18n directory exists with any `*.toml` other than `en.toml`
    ///   but `en.toml` is missing
    /// - any loaded file is not a flat table of string values
    /// - `date_format` is missing after merging, when any i18n file was loaded
    pub fn load(site_root: &Path, theme_dir: Option<&Path>, language: &str) -> Result<Self> {
        // Paths below interpolate `language` into filenames — guard against
        // traversal or oddly-shaped tags before anything touches the FS.
        if !language
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            || !language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            bail!(
                "invalid language tag `{language}`: expected BCP 47 shape (ASCII letters / digits / hyphens)",
            );
        }

        let mut strings: HashMap<String, String> = HashMap::new();
        let mut any_file_loaded = false;

        if let Some(theme_dir) = theme_dir {
            let theme_i18n_dir = theme_dir.join("i18n");
            if theme_i18n_dir.is_dir() {
                let en_path = theme_i18n_dir.join("en.toml");
                let has_other = theme_has_non_english_toml(&theme_i18n_dir)?;
                if en_path.exists() {
                    merge_from_file(&mut strings, &en_path)?;
                    any_file_loaded = true;
                } else if has_other {
                    bail!(
                        "theme i18n directory {} is missing required en.toml fallback",
                        theme_i18n_dir.display(),
                    );
                }

                if language != "en" {
                    let lang_path = theme_i18n_dir.join(format!("{language}.toml"));
                    if lang_path.exists() {
                        merge_from_file(&mut strings, &lang_path)?;
                        any_file_loaded = true;
                    }
                }
            }
        }

        let site_path = site_root.join("i18n").join(format!("{language}.toml"));
        if site_path.exists() {
            merge_from_file(&mut strings, &site_path)?;
            any_file_loaded = true;
        }

        // No i18n files at all is legal — callers that don't localize still
        // need a working `localdate` filter, so fall back to an ISO-ish
        // default. Once any file is loaded, `date_format` must be declared
        // explicitly so themes and sites can't accidentally inherit a
        // format that doesn't match their locale.
        let date_format = match strings.remove(DATE_FORMAT_KEY) {
            Some(value) => value,
            None if any_file_loaded => {
                bail!(
                    "i18n tables for language `{language}` are missing required `{DATE_FORMAT_KEY}` key",
                );
            }
            None => "%Y-%m-%d".to_owned(),
        };

        Ok(Self {
            inner: Arc::new(Inner {
                strings,
                date_format,
                language: language.to_owned(),
                warned: Mutex::new(HashSet::new()),
            }),
        })
    }

    /// The resolved strftime `date_format` string for the active language.
    #[must_use]
    pub fn date_format(&self) -> &str {
        &self.inner.date_format
    }

    /// The active BCP 47 language tag (e.g., `"en"`, `"zh-Hans"`).
    #[must_use]
    pub fn language(&self) -> &str {
        &self.inner.language
    }

    /// Looks up a string by key.
    ///
    /// On miss, emits `tracing::warn!` once per key per `I18n` instance.
    /// Returns `«missing:<key>»` when `KILN_DEV` is set (non-empty), or the
    /// key literal otherwise, so missing strings are visible in template
    /// output without crashing the build.
    #[must_use]
    pub fn t<'a>(&'a self, key: &str) -> Cow<'a, str> {
        if let Some(value) = self.inner.strings.get(key) {
            return Cow::Borrowed(value);
        }
        self.warn_once(WarnKey::MissingKey(key.to_owned()));
        // Miss path always allocates: borrowing `key` here would tie the
        // returned `Cow` to the caller's stack, forcing every call site to
        // immediately clone. Owning here keeps the hit path zero-copy
        // without punishing the miss path further.
        Cow::Owned(render_miss(key, kiln_dev_enabled()).into_owned())
    }

    /// Looks up a string by key and interpolates Python-style `{name}`
    /// placeholders from `args`.
    ///
    /// `{{` renders as a literal `{`, `}}` renders as a literal `}`. Missing
    /// placeholders substitute an empty string and emit a warning. An
    /// unclosed `{` emits a warning and renders the partial literal as-is.
    #[must_use]
    pub fn t_interp(&self, key: &str, args: &BTreeMap<&str, &str>) -> String {
        let template = self.t(key);
        interpolate(&template, args, |warning| self.emit_warning(key, warning))
    }

    fn warn_once(&self, warning: WarnKey) {
        let mut warned = self.inner.warned.lock().expect("i18n warned poisoned");
        if !warned.insert(warning.clone()) {
            return;
        }
        // Drop the guard before calling into `tracing`: the subscriber may
        // run arbitrary code, and holding the lock across it risks a
        // re-entrant deadlock and needlessly extends the critical section.
        drop(warned);
        match warning {
            WarnKey::MissingKey(key) => tracing::warn!(key, "missing i18n key"),
            WarnKey::MissingPlaceholder { key, name } => {
                tracing::warn!(key, name, "missing placeholder for i18n key");
            }
            WarnKey::UnclosedPlaceholder { key, partial } => {
                tracing::warn!(key, partial, "unclosed placeholder in i18n key");
            }
        }
    }

    fn emit_warning(&self, key: &str, warning: InterpolateWarning<'_>) {
        let warn_key = match warning {
            InterpolateWarning::MissingPlaceholder(name) => WarnKey::MissingPlaceholder {
                key: key.to_owned(),
                name: name.to_owned(),
            },
            InterpolateWarning::UnclosedPlaceholder(partial) => WarnKey::UnclosedPlaceholder {
                key: key.to_owned(),
                partial: partial.to_owned(),
            },
        };
        self.warn_once(warn_key);
    }
}

// ── Miss rendering ──

/// Returns the rendered value for a missing i18n key.
///
/// Factored out of `I18n::t` so tests can exercise dev mode without touching
/// the process environment (which would conflict with `unsafe_code = forbid`).
fn render_miss(key: &str, dev_mode: bool) -> Cow<'_, str> {
    if dev_mode {
        Cow::Owned(format!("«missing:{key}»"))
    } else {
        Cow::Borrowed(key)
    }
}

fn kiln_dev_enabled() -> bool {
    std::env::var("KILN_DEV").is_ok_and(|v| !v.is_empty())
}

// ── Loading helpers ──

fn theme_has_non_english_toml(dir: &Path) -> Result<bool> {
    for entry in fs::read_dir(dir)
        .with_context(|| format!("failed to read i18n directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        let is_toml = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"));
        if !is_toml {
            continue;
        }
        // Compare the stem (not the full filename) case-insensitively so a
        // theme shipping `En.toml` on a case-insensitive filesystem isn't
        // spuriously flagged as missing the `en.toml` fallback.
        let is_en = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("en"));
        if !is_en {
            return Ok(true);
        }
    }
    Ok(false)
}

fn merge_from_file(strings: &mut HashMap<String, String>, path: &Path) -> Result<()> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read i18n file {}", path.display()))?;
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

/// Non-fatal signal emitted by [`interpolate`] for the caller to dedup and
/// log as it sees fit. Both variants borrow from the template / args.
#[derive(Debug, Clone, Copy)]
enum InterpolateWarning<'a> {
    MissingPlaceholder(&'a str),
    UnclosedPlaceholder(&'a str),
}

/// Interpolates `{name}` placeholders from `args` into `template`.
///
/// `{{` / `}}` escape to literal braces. Missing placeholders substitute
/// empty string and report via `warn`. Unclosed `{` reports via `warn` and
/// renders the remaining text as-is.
fn interpolate(
    template: &str,
    args: &BTreeMap<&str, &str>,
    mut warn: impl FnMut(InterpolateWarning<'_>),
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
                // Collect placeholder name up to the next `}`.
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
                    warn(InterpolateWarning::UnclosedPlaceholder(&name));
                    out.push('{');
                    out.push_str(&name);
                    break;
                }
                if let Some(value) = args.get(name.as_str()) {
                    out.push_str(value);
                } else {
                    warn(InterpolateWarning::MissingPlaceholder(&name));
                }
            }
            '}' => {
                // `}}` is the escape for a literal `}`, and a stray `}`
                // renders literally too — either way we emit one `}`,
                // consuming the second brace only when present.
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

    // ── load ──

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn load_language_en_loads_en_toml_as_sole_source() {
        // When `language == "en"`, the resolver deliberately skips the
        // second `{language}.toml` open attempt (that file would be
        // identical to the `en.toml` already merged).
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_file(
            &theme.path().join("i18n/en.toml"),
            indoc! {r#"
                date_format = "%Y-%m-%d"
                all_posts = "All Posts"
            "#},
        );

        let i18n = I18n::load(site.path(), Some(theme.path()), "en").unwrap();
        assert_eq!(i18n.date_format(), "%Y-%m-%d");
        assert_eq!(i18n.t("all_posts").as_ref(), "All Posts");
    }

    #[test]
    fn load_date_format_from_theme_lang_file_overrides_en() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_file(
            &theme.path().join("i18n/en.toml"),
            indoc! {r#"
                date_format = "%Y-%m-%d"
                shared = "shared en"
            "#},
        );
        write_file(
            &theme.path().join("i18n/fr.toml"),
            indoc! {r#"
                date_format = "%d/%m/%Y"
                only_in_fr = "fr-only value"
            "#},
        );

        let i18n = I18n::load(site.path(), Some(theme.path()), "fr").unwrap();
        assert_eq!(i18n.date_format(), "%d/%m/%Y");
        assert_eq!(i18n.t("only_in_fr").as_ref(), "fr-only value");
        assert_eq!(i18n.t("shared").as_ref(), "shared en");
    }

    #[test]
    fn load_site_date_format_overrides_theme() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_file(
            &theme.path().join("i18n/en.toml"),
            indoc! {r#"
                date_format = "%Y-%m-%d"
                theme_only = "from theme"
            "#},
        );
        write_file(
            &site.path().join("i18n/en.toml"),
            indoc! {r#"
                date_format = "%B %d, %Y"
            "#},
        );

        let i18n = I18n::load(site.path(), Some(theme.path()), "en").unwrap();
        assert_eq!(i18n.date_format(), "%B %d, %Y");
        assert_eq!(
            i18n.t("theme_only").as_ref(),
            "from theme",
            "non-overridden theme keys should still resolve",
        );
    }

    #[test]
    fn load_merges_precedence_site_over_theme_lang_over_theme_en() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();

        write_file(
            &theme.path().join("i18n/en.toml"),
            indoc! {r#"
                date_format = "%Y-%m-%d"
                all_posts = "All Posts"
                back_to_top = "Back to Top"
                only_in_en = "from en"
            "#},
        );
        write_file(
            &theme.path().join("i18n/zh-Hans.toml"),
            indoc! {r#"
                all_posts = "全部文章"
                back_to_top = "回到顶部"
            "#},
        );
        write_file(
            &site.path().join("i18n/zh-Hans.toml"),
            indoc! {r#"
                back_to_top = "回顶"
            "#},
        );

        let i18n = I18n::load(site.path(), Some(theme.path()), "zh-Hans").unwrap();
        // Site wins.
        assert_eq!(i18n.t("back_to_top").as_ref(), "回顶");
        // Theme lang wins over theme en when site has no override.
        assert_eq!(i18n.t("all_posts").as_ref(), "全部文章");
        // Theme en is the ultimate fallback.
        assert_eq!(i18n.t("only_in_en").as_ref(), "from en");
        // Date format came from theme en.
        assert_eq!(i18n.date_format(), "%Y-%m-%d");
        assert_eq!(i18n.language(), "zh-Hans");
    }

    #[test]
    fn load_site_only_without_theme_i18n_dir() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();

        write_file(
            &site.path().join("i18n/en.toml"),
            indoc! {r#"
                date_format = "%F"
                greeting = "Hello"
            "#},
        );

        let i18n = I18n::load(site.path(), Some(theme.path()), "en").unwrap();
        assert_eq!(i18n.date_format(), "%F");
        assert_eq!(i18n.t("greeting").as_ref(), "Hello");
    }

    #[test]
    fn load_site_only_without_theme() {
        let site = tempfile::tempdir().unwrap();
        write_file(
            &site.path().join("i18n/zh-Hans.toml"),
            indoc! {r#"
                date_format = "%Y年%m月%d日"
            "#},
        );

        let i18n = I18n::load(site.path(), None, "zh-Hans").unwrap();
        assert_eq!(i18n.date_format(), "%Y年%m月%d日");
    }

    #[test]
    fn load_ignores_non_toml_entries_in_i18n_dir() {
        // Non-TOML files (READMEs, editor swap files, etc.) and
        // subdirectories must be skipped without tripping the
        // "missing en.toml fallback" check.
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_file(
            &theme.path().join("i18n/en.toml"),
            indoc! {r#"
                date_format = "%Y-%m-%d"
                greeting = "Hi"
            "#},
        );
        write_file(&theme.path().join("i18n/README.md"), "translator notes");
        fs::create_dir_all(theme.path().join("i18n/.backup")).unwrap();

        let i18n = I18n::load(site.path(), Some(theme.path()), "en").unwrap();
        assert_eq!(i18n.t("greeting").as_ref(), "Hi");
    }

    #[test]
    fn load_with_no_files_falls_back_to_hardcoded_date_format() {
        // The hardcoded ISO fallback is meaningful here because no files
        // were loaded — `strings` is empty, so `t()` always misses, and the
        // date format can only have come from the hardcoded default.
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();

        let i18n = I18n::load(site.path(), Some(theme.path()), "en").unwrap();
        assert_eq!(i18n.date_format(), "%Y-%m-%d");
        assert_eq!(
            i18n.t("anything").as_ref(),
            "anything",
            "strings map must be empty when no files contributed",
        );
    }

    #[test]
    fn load_theme_en_file_with_different_case_is_still_recognized() {
        // On a case-sensitive filesystem `En.toml` and `en.toml` are
        // distinct files; on case-insensitive filesystems they collide.
        // In both cases `En.toml` should count as the English fallback
        // and not trip the "missing en.toml" bail.
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_file(
            &theme.path().join("i18n/En.toml"),
            indoc! {r#"
                date_format = "%Y-%m-%d"
                greeting = "Hello"
            "#},
        );

        // Loading with `language == "en"` tries to open `en.toml`; on a
        // case-insensitive FS that resolves to `En.toml`, on a
        // case-sensitive FS neither file is read but the presence of
        // `En.toml` must not trigger the "missing en.toml fallback" bail.
        let result = I18n::load(site.path(), Some(theme.path()), "en");
        assert!(result.is_ok(), "got error: {:?}", result.err());
    }

    #[test]
    fn load_missing_en_fallback_returns_error() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_file(
            &theme.path().join("i18n/zh-Hans.toml"),
            indoc! {r#"
                date_format = "%Y-%m-%d"
            "#},
        );

        let err = I18n::load(site.path(), Some(theme.path()), "zh-Hans")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("missing required en.toml fallback"),
            "should report missing en.toml, got: {err}"
        );
    }

    #[test]
    fn load_missing_date_format_returns_error() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_file(
            &theme.path().join("i18n/en.toml"),
            indoc! {r#"
                all_posts = "All Posts"
            "#},
        );

        let err = I18n::load(site.path(), Some(theme.path()), "en")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("missing required `date_format`"),
            "should report missing date_format, got: {err}"
        );
    }

    #[test]
    fn load_nested_table_returns_error() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_file(
            &theme.path().join("i18n/en.toml"),
            indoc! {r#"
                date_format = "%Y-%m-%d"

                [nested]
                key = "value"
            "#},
        );

        let err = I18n::load(site.path(), Some(theme.path()), "en")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("must be a string"),
            "should reject nested table, got: {err}"
        );
    }

    #[test]
    fn load_integer_value_returns_error() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_file(
            &theme.path().join("i18n/en.toml"),
            indoc! {r#"
                date_format = "%Y-%m-%d"
                count = 42
            "#},
        );

        let err = I18n::load(site.path(), Some(theme.path()), "en")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("must be a string"),
            "should reject integer value, got: {err}"
        );
    }

    #[test]
    fn load_malformed_toml_returns_error() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();
        write_file(&theme.path().join("i18n/en.toml"), "key = \n");

        let err = I18n::load(site.path(), Some(theme.path()), "en")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("failed to parse i18n file"),
            "should report parse failure, got: {err}"
        );
    }

    #[test]
    fn load_invalid_language_returns_error() {
        let site = tempfile::tempdir().unwrap();
        let theme = tempfile::tempdir().unwrap();

        for bad in ["../etc/passwd", "", "-en", "1en", "en_US", "en/US"] {
            let err = I18n::load(site.path(), Some(theme.path()), bad)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("invalid language tag"),
                "should reject {bad:?}, got: {err}",
            );
        }
    }

    // ── t ──

    fn make_i18n(pairs: &[(&str, &str)]) -> I18n {
        let mut strings = HashMap::new();
        for (k, v) in pairs {
            strings.insert((*k).to_owned(), (*v).to_owned());
        }
        I18n {
            inner: Arc::new(Inner {
                strings,
                date_format: "%Y-%m-%d".to_owned(),
                language: "en".to_owned(),
                warned: Mutex::new(HashSet::new()),
            }),
        }
    }

    #[test]
    fn t_hit_returns_value() {
        let i18n = make_i18n(&[("greeting", "Hello")]);
        assert_eq!(i18n.t("greeting").as_ref(), "Hello");
    }

    #[test]
    fn t_warns_once_per_key_independently() {
        let i18n = make_i18n(&[]);
        for _ in 0..3 {
            let _ = i18n.t("key_alpha");
        }
        for _ in 0..3 {
            let _ = i18n.t("key_beta");
        }
        let warned = i18n.inner.warned.lock().unwrap();
        assert_eq!(warned.len(), 2);
        assert!(warned.contains(&WarnKey::MissingKey("key_alpha".to_owned())));
        assert!(warned.contains(&WarnKey::MissingKey("key_beta".to_owned())));
    }

    // ── t_interp ──

    #[test]
    fn t_interp_simple_substitution() {
        let i18n = make_i18n(&[("hello", "Hello, {name}!")]);
        let mut args = BTreeMap::new();
        args.insert("name", "Alex");
        assert_eq!(i18n.t_interp("hello", &args), "Hello, Alex!");
    }

    #[test]
    fn t_interp_multiple_placeholders() {
        let i18n = make_i18n(&[("counter", "Page {current} of {total}")]);
        let mut args = BTreeMap::new();
        args.insert("current", "2");
        args.insert("total", "5");
        assert_eq!(i18n.t_interp("counter", &args), "Page 2 of 5");
    }

    #[test]
    fn t_interp_double_brace_escapes() {
        let i18n = make_i18n(&[("set", "{{literal}} and {name}")]);
        let mut args = BTreeMap::new();
        args.insert("name", "X");
        assert_eq!(i18n.t_interp("set", &args), "{literal} and X");
    }

    #[test]
    fn t_interp_closing_brace_escape_and_stray() {
        let i18n = make_i18n(&[("braces", "a}}b }")]);
        let args = BTreeMap::new();
        assert_eq!(i18n.t_interp("braces", &args), "a}b }");
    }

    #[test]
    fn t_interp_missing_arg_substitutes_empty() {
        let i18n = make_i18n(&[("hello", "Hi {who}!")]);
        let args = BTreeMap::new();
        assert_eq!(i18n.t_interp("hello", &args), "Hi !");
    }

    #[test]
    fn t_interp_missing_arg_warns_once_across_many_calls() {
        let i18n = make_i18n(&[("hello", "Hi {who}!")]);
        let args = BTreeMap::new();
        for _ in 0..500 {
            let _ = i18n.t_interp("hello", &args);
        }
        let warned = i18n.inner.warned.lock().unwrap();
        assert_eq!(warned.len(), 1);
        assert!(warned.contains(&WarnKey::MissingPlaceholder {
            key: "hello".to_owned(),
            name: "who".to_owned(),
        }));
    }

    #[test]
    fn t_interp_unclosed_brace_renders_partial() {
        let i18n = make_i18n(&[("bad", "start {unclosed tail")]);
        let args = BTreeMap::new();
        assert_eq!(i18n.t_interp("bad", &args), "start {unclosed tail");
    }

    #[test]
    fn t_interp_unclosed_brace_warns_once_across_many_calls() {
        let i18n = make_i18n(&[("bad", "start {unclosed tail")]);
        let args = BTreeMap::new();
        for _ in 0..500 {
            let _ = i18n.t_interp("bad", &args);
        }
        let warned = i18n.inner.warned.lock().unwrap();
        assert_eq!(warned.len(), 1);
        assert!(warned.contains(&WarnKey::UnclosedPlaceholder {
            key: "bad".to_owned(),
            partial: "unclosed tail".to_owned(),
        }));
    }

    #[test]
    fn t_interp_ignores_args_when_no_placeholder() {
        let i18n = make_i18n(&[("plain", "Just text.")]);
        let mut args = BTreeMap::new();
        args.insert("unused", "nope");
        assert_eq!(i18n.t_interp("plain", &args), "Just text.");
    }

    // ── render_miss ──

    #[test]
    fn render_miss_returns_key_when_dev_off() {
        // Exercise the miss rendering directly so the test doesn't depend on
        // the ambient `KILN_DEV` env var (touching process env requires
        // `unsafe` under Rust 2024, which is forbidden in this crate).
        assert_eq!(render_miss("missing_key", false).as_ref(), "missing_key");
    }

    #[test]
    fn render_miss_returns_marker_when_dev_on() {
        assert_eq!(
            render_miss("missing_key", true).as_ref(),
            "«missing:missing_key»",
        );
    }
}
