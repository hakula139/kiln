use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Component, Path};

use minijinja::value::{Kwargs, Value};
use minijinja::{Error, ErrorKind, State};

use crate::i18n::I18n;
use crate::render::assets::{AssetsHandle, LoadStrategy, ScriptTag};
use crate::static_assets::{StaticAssetManifest, is_external};
use crate::url::site_asset_url;

// ── Date / Time ──

/// Returns the current local timestamp as an ISO 8601 string.
pub(super) fn tpl_now() -> String {
    jiff::Zoned::now().to_string()
}

// ── File IO ──

/// Reads a file relative to the directive's `source_dir`. Rejects `..`, absolute, and rooted
/// path components. Symlinks may refer to files outside the source directory.
pub(super) fn tpl_read_file(state: &State, filename: &str) -> Result<String, Error> {
    let source_dir = state
        .lookup("source_dir")
        .filter(|v| !v.is_none() && !v.is_undefined())
        .ok_or_else(|| invalid("read_file requires source_dir in directive context"))?;

    // `DirectiveContext.source_dir` is the only producer of this value, so when present it is
    // always a string.
    let source_dir = source_dir.as_str().unwrap_or_default();

    let rel = Path::new(filename);
    if rel.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(..)
        )
    }) {
        return Err(invalid(format!("path traversal not allowed: {filename}")));
    }

    let path = Path::new(source_dir).join(rel);
    std::fs::read_to_string(&path)
        .map_err(|e| invalid(format!("failed to read {}: {e}", path.display())))
}

/// Parses CSV text into a list of rows, each a list of field strings.
pub(super) fn tpl_parse_csv(text: &str) -> Result<Value, Error> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(text.as_bytes());

    let rows: Vec<Value> = reader
        .records()
        .map(|r| {
            let record = r.map_err(|e| invalid(format!("CSV parse error: {e}")))?;
            Ok(Value::from_iter(record.iter().map(Value::from)))
        })
        .collect::<Result<_, Error>>()?;

    Ok(Value::from(rows))
}

// ── i18n ──

/// Looks up an i18n string and interpolates kwargs into Python-style `{name}` placeholders.
pub(super) fn tpl_t(i18n: &I18n, key: &str, kwargs: &Kwargs) -> Result<String, Error> {
    // `none` and undefined values render as empty strings instead of the literal text "none".
    let args = kwargs
        .args()
        .map(|name| {
            let value: Value = kwargs.get(name)?;
            let text = if value.is_none() || value.is_undefined() {
                String::new()
            } else {
                value.to_string()
            };
            Ok((name, text))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    Ok(i18n.t_interp(key, &args))
}

// ── Static Assets ──

/// Resolves a site-root-relative or page-relative path to its published asset URL.
pub(super) fn tpl_asset_url(
    state: &State,
    manifest: &StaticAssetManifest,
    deployment_prefix: &str,
    url: &str,
) -> Result<String, Error> {
    let page_url = state.lookup("__page_url");
    let page_url = page_url.as_ref().and_then(Value::as_str);
    let base = if url.starts_with('/') || is_external(url) {
        "/"
    } else {
        page_url.ok_or_else(|| invalid("asset_url requires a page URL for relative paths"))?
    };
    manifest
        .resolve(
            &site_asset_url(url, deployment_prefix),
            base,
            deployment_prefix,
        )
        .ok_or_else(|| invalid(format!("asset_url: static asset not found: {url}")))
}

// ── Asset Registration ──

/// Registers a `<script>` for the current page on the per-render [`AssetsHandle`]. Returns the
/// empty string so the call can be used as a statement.
///
/// Accepts `load="defer"` (default), `"async"`, or `"sync"`, and `module=true` for
/// `type="module"`. Modules reject `load="sync"`. Re-registering the same tag is a no-op, while
/// re-registering the same URL with different attributes is an error.
pub(super) fn tpl_register_script(
    state: &State,
    url: &str,
    kwargs: &Kwargs,
) -> Result<&'static str, Error> {
    let assets_value = state
        .lookup("__assets")
        .filter(|v| !v.is_undefined() && !v.is_none())
        .ok_or_else(|| {
            invalid(
                "register_script requires page asset context: \
                 only callable from directive templates",
            )
        })?;

    let handle = assets_value
        .downcast_object::<AssetsHandle>()
        .ok_or_else(|| invalid("__assets is not a recognized asset handle"))?;

    let load_str: Option<String> = kwargs.get("load")?;
    let module: bool = kwargs.get::<Option<bool>>("module")?.unwrap_or(false);
    kwargs.assert_all_used()?;

    let load = match load_str.as_deref() {
        None => LoadStrategy::Defer,
        Some(s) => s.parse::<LoadStrategy>().map_err(|_| {
            invalid(format!(
                r#"register_script: load must be one of "defer", "async", "sync"; got "{s}""#,
            ))
        })?,
    };

    handle
        .lock()
        .register_script(ScriptTag {
            url: url.to_owned(),
            load,
            module,
        })
        .map_err(|e| invalid(e.to_string()))?;

    Ok("")
}

fn invalid(message: impl Into<Cow<'static, str>>) -> Error {
    Error::new(ErrorKind::InvalidOperation, message)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::indoc;
    use minijinja::context;

    use super::*;
    use crate::template::TemplateEngine;
    use crate::test_utils::{test_engine, test_i18n};

    // ── tpl_now ──

    #[test]
    fn tpl_now_returns_current_timestamp() {
        let before = jiff::Timestamp::now();
        let result = render("{{ now() }}", ()).unwrap();
        let after = jiff::Timestamp::now();

        let timestamp = result.parse::<jiff::Zoned>().unwrap().timestamp();
        assert!(timestamp >= before && timestamp <= after, "{result}");
    }

    // ── tpl_read_file ──

    #[test]
    fn tpl_read_file_reads_relative_to_source_dir() {
        let source = tempfile::tempdir().unwrap();
        let contents = indoc! {"
            A,B
            1,2
        "};
        fs::write(source.path().join("scores.csv"), contents).unwrap();

        assert_eq!(
            read_file("scores.csv", Some(source.path())).unwrap(),
            contents
        );
    }

    #[test]
    fn tpl_read_file_rejects_parent_and_absolute_paths() {
        let source = tempfile::tempdir().unwrap();
        let source_dir = source.path().join("subdir");
        fs::create_dir(&source_dir).unwrap();
        let outside = source.path().join("outside.txt");
        fs::write(&outside, "outside").unwrap();

        for filename in ["../outside.txt", outside.to_str().unwrap()] {
            let error = read_file(filename, Some(&source_dir)).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
            assert!(
                error.to_string().contains("path traversal not allowed"),
                "{filename}: {error}"
            );
        }
    }

    #[test]
    fn tpl_read_file_missing_source_dir_or_file_returns_error() {
        let source = tempfile::tempdir().unwrap();
        for (source_dir, expected) in [
            (None, "read_file requires source_dir"),
            (Some(source.path()), "failed to read"),
        ] {
            let error = read_file("missing.csv", source_dir).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn tpl_read_file_follows_external_symlink() {
        let source = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::write(external.path().join("data.txt"), "external <data>").unwrap();
        std::os::unix::fs::symlink(
            external.path().join("data.txt"),
            source.path().join("data.txt"),
        )
        .unwrap();

        assert_eq!(
            read_file("data.txt", Some(source.path())).unwrap(),
            "external <data>"
        );
    }

    // ── tpl_parse_csv ──

    #[test]
    fn tpl_parse_csv_basic() {
        let rows = tpl_parse_csv(indoc! {"
            A,B
            1,2
            3,4
        "})
        .unwrap();

        assert_eq!(
            rows,
            Value::from_serialize([["A", "B"], ["1", "2"], ["3", "4"]])
        );
    }

    #[test]
    fn tpl_parse_csv_quoted_fields() {
        let rows = tpl_parse_csv(indoc! {r#"
            name,value
            "field with, comma","has ""quotes"""
        "#})
        .unwrap();

        assert_eq!(
            rows,
            Value::from_serialize([["name", "value"], ["field with, comma", r#"has "quotes""#],])
        );
    }

    #[test]
    fn tpl_parse_csv_empty_input() {
        assert_eq!(tpl_parse_csv("").unwrap(), Value::from(Vec::<Value>::new()));
    }

    #[test]
    fn tpl_parse_csv_unequal_lengths_returns_error() {
        let error = tpl_parse_csv(indoc! {"
            a,b
            only-one
        "})
        .unwrap_err();

        assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        assert!(error.detail().unwrap().starts_with("CSV parse error: "));
    }

    // ── tpl_t ──

    #[test]
    fn tpl_t_looks_up_key_or_returns_key_literal() {
        assert_eq!(render(r#"{{ t("all_posts") }}"#, ()).unwrap(), "All Posts");
        assert_eq!(
            render(r#"{{ t("not_defined_anywhere") }}"#, ()).unwrap(),
            "not_defined_anywhere"
        );
    }

    #[test]
    fn tpl_t_interpolates_keyword_arguments() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("i18n")).unwrap();
        fs::write(
            dir.path().join("i18n").join("en.toml"),
            r#"greeting = "Hi {name}, {count}!""#,
        )
        .unwrap();
        let i18n = I18n::load(Path::new("/nonexistent"), Some(dir.path()), "en").unwrap();

        let templates = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new(Some(templates.path()), None, &i18n).unwrap();
        for (template, expected) in [
            (
                r#"{{ t("greeting", name="Alex", count=2) }}"#,
                "Hi Alex, 2!",
            ),
            (r#"{{ t("greeting", name=none) }}"#, "Hi , !"),
        ] {
            let result = engine.env.render_str(template, ()).unwrap();
            assert_eq!(result, expected, "template: {template}");
        }
    }

    // ── tpl_asset_url ──

    #[test]
    fn tpl_asset_url_renders_prefixed_and_encoded_manifest_urls() {
        let static_dir = tempfile::tempdir().unwrap();
        fs::create_dir(static_dir.path().join("blog")).unwrap();
        for (path, content) in [
            ("shared.css", "abc"),
            ("blog/shared.css", "different"),
            ("shared.js", "abc"),
            ("page script.js", "abc"),
        ] {
            fs::write(static_dir.path().join(path), content).unwrap();
        }

        let manifest = StaticAssetManifest::build(static_dir.path()).unwrap();
        let templates = tempfile::tempdir().unwrap();
        let engine = TemplateEngine::new_with_assets(
            Some(templates.path()),
            None,
            &test_i18n(),
            "/blog",
            &manifest,
        )
        .unwrap();

        assert_eq!(
            engine
                .env
                .render_str(
                    indoc! {r#"
                        <link rel="stylesheet" href="{{ asset_url('/shared.css') | safe }}">
                        <link rel="stylesheet" href="{{ asset_url('/blog/shared.css') | safe }}">
                        <script src="{{ asset_url('/shared.js') | safe }}"></script>
                        <script src="{{ asset_url('/page%20script.js') | safe }}"></script>
                    "#},
                    (),
                )
                .unwrap(),
            indoc! {r#"
                <link rel="stylesheet" href="/blog/shared.ba7816bf8f01.css">
                <link rel="stylesheet" href="/blog/blog/shared.9d6f965ac832.css">
                <script src="/blog/shared.ba7816bf8f01.js"></script>
                <script src="/blog/page%20script.ba7816bf8f01.js"></script>
            "#}
            .trim_end(),
        );
    }

    #[test]
    fn tpl_asset_url_missing_file_or_page_context_returns_error() {
        for (template, expected) in [
            (
                r"{{ asset_url('/missing.png') }}",
                "static asset not found: /missing.png",
            ),
            (
                r"{{ asset_url('image.png') }}",
                "requires a page URL for relative paths",
            ),
        ] {
            let error = render(template, ()).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    // ── tpl_register_script ──

    #[test]
    fn tpl_register_script_records_and_deduplicates_default_deferred_tag() {
        let assets = AssetsHandle::default();
        for _ in 0..2 {
            assert_eq!(
                register(r#"{{ register_script("/js/widget.js") }}<widget>"#, &assets).unwrap(),
                "<widget>",
                "register_script must return an empty string"
            );
        }
        assert_eq!(
            assets.snapshot().scripts(),
            [ScriptTag {
                url: "/js/widget.js".into(),
                load: LoadStrategy::Defer,
                module: false,
            }]
        );
    }

    #[test]
    fn tpl_register_script_honors_load_async_kwarg() {
        let assets = AssetsHandle::default();
        register(
            r#"{{ register_script("/js/widget.js", load="async") }}"#,
            &assets,
        )
        .unwrap();

        assert_eq!(
            assets.snapshot().scripts(),
            [ScriptTag {
                url: "/js/widget.js".into(),
                load: LoadStrategy::Async,
                module: false,
            }]
        );
    }

    #[test]
    fn tpl_register_script_invalid_declaration_returns_error() {
        for (template, message) in [
            (
                r#"{{ register_script("/x.js", load="sync", module=true) }}"#,
                "cannot use synchronous loading",
            ),
            (
                r#"{{ register_script("/x.js") }}{{ register_script("/x.js", module=true) }}"#,
                "Pick one set of attributes per URL",
            ),
            (r#"{{ register_script("/x.js", bogus=true) }}"#, "bogus"),
            (
                r#"{{ register_script("/x.js", load="eager") }}"#,
                r#"load must be one of "defer", "async", "sync"; got "eager""#,
            ),
            (
                r#"{{ register_script("/x.js", module="yes") }}"#,
                "cannot convert",
            ),
        ] {
            let assets = AssetsHandle::default();
            let error = register(template, &assets).unwrap_err();
            assert!(error.to_string().contains(message), "{template}: {error}");
        }
    }

    #[test]
    fn tpl_register_script_without_asset_handle_returns_error() {
        for (ctx, message) in [
            (Value::UNDEFINED, "only callable from directive templates"),
            (
                context! { __assets => "not a handle" },
                "__assets is not a recognized asset handle",
            ),
        ] {
            let error = render(r#"{{ register_script("/x.js") }}"#, ctx).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
        }
    }

    fn render(template: &str, ctx: impl serde::Serialize) -> Result<String, Error> {
        test_engine().env.render_str(template, ctx)
    }

    fn read_file(filename: &str, source_dir: Option<&Path>) -> Result<String, Error> {
        render(
            "{{ read_file(filename) }}",
            context! {
                filename,
                source_dir => source_dir.map(|dir| dir.to_string_lossy()),
            },
        )
    }

    fn register(template: &str, assets: &AssetsHandle) -> Result<String, Error> {
        render(
            template,
            context! { __assets => Value::from_object(assets.clone()) },
        )
    }
}
