use std::path::{Component, Path};

use minijinja::value::Kwargs;

use crate::i18n::I18n;
use crate::render::assets::{AssetsHandle, LoadStrategy, ScriptTag};

// ── Date / Time ──

/// Returns the current local timestamp as an ISO 8601 string.
pub(super) fn tpl_now() -> String {
    jiff::Zoned::now().to_string()
}

// ── File IO ──

/// Reads a file relative to the directive's `source_dir`. Rejects `..`, absolute, and rooted
/// path components to keep reads inside the page's source directory.
pub(super) fn tpl_read_file(
    state: &minijinja::State,
    filename: &str,
) -> std::result::Result<String, minijinja::Error> {
    let source_dir = state
        .lookup("source_dir")
        .filter(|v| !v.is_none() && !v.is_undefined())
        .ok_or_else(|| {
            minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "read_file requires source_dir in directive context",
            )
        })?;

    let source_dir = source_dir.as_str().ok_or_else(|| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "source_dir must be a string",
        )
    })?;

    let rel = Path::new(filename);
    for component in rel.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(..)
        ) {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                format!("path traversal not allowed: {filename}"),
            ));
        }
    }

    let path = Path::new(source_dir).join(rel);
    std::fs::read_to_string(&path).map_err(|e| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            format!("failed to read {}: {e}", path.display()),
        )
    })
}

/// Parses CSV text into a list of rows; each row is a list of field strings.
pub(super) fn tpl_parse_csv(text: &str) -> std::result::Result<minijinja::Value, minijinja::Error> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(text.as_bytes());

    let rows: Vec<minijinja::Value> = reader
        .records()
        .map(|r| {
            let record = r.map_err(|e| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("CSV parse error: {e}"),
                )
            })?;
            Ok(minijinja::Value::from(
                record
                    .iter()
                    .map(|field| minijinja::Value::from(field.to_string()))
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<std::result::Result<_, minijinja::Error>>()?;

    Ok(minijinja::Value::from(rows))
}

// ── i18n ──

/// Looks up an i18n string and interpolates kwargs into Python-style `{name}` placeholders.
pub(super) fn tpl_t(
    i18n: &I18n,
    key: &str,
    kwargs: &Kwargs,
) -> std::result::Result<String, minijinja::Error> {
    let arg_names: Vec<&str> = kwargs.args().collect();
    if arg_names.is_empty() {
        return Ok(i18n.t(key).into_owned());
    }

    // Materialize owned strings: minijinja `Value`s don't borrow from `kwargs`, but `t_interp`
    // takes borrowed `&str`. Map `none` / undefined to empty so missing fields don't render
    // as the literal text `"none"`.
    let mut owned: Vec<(&str, String)> = Vec::with_capacity(arg_names.len());
    for name in arg_names {
        let value: minijinja::Value = kwargs.get(name)?;
        let stringified = if value.is_none() || value.is_undefined() {
            String::new()
        } else {
            value.to_string()
        };
        owned.push((name, stringified));
    }
    let args: std::collections::BTreeMap<&str, &str> = owned
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    Ok(i18n.t_interp(key, &args))
}

// ── Asset Registration ──

/// Registers a `<script>` for the current page on the per-render [`AssetsHandle`]. Returns the
/// empty string so the call can be used as a statement.
///
/// Accepts `load="defer"` (default), `"async"`, or `"sync"`, and `module=true` for
/// `type="module"`. Re-registering the same `(url, load, module)` is a no-op; re-registering the
/// same URL with different attributes is an error.
pub(super) fn tpl_register_script(
    state: &minijinja::State,
    url: &str,
    kwargs: &Kwargs,
) -> std::result::Result<&'static str, minijinja::Error> {
    let assets_value = state
        .lookup("__assets")
        .filter(|v| !v.is_undefined() && !v.is_none())
        .ok_or_else(|| {
            minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "register_script requires page asset context — \
                 only callable from directive templates",
            )
        })?;

    let handle = assets_value
        .downcast_object::<AssetsHandle>()
        .ok_or_else(|| {
            minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "__assets is not a recognized asset handle",
            )
        })?;

    let load_str: Option<String> = kwargs.get("load")?;
    let module: bool = kwargs.get::<Option<bool>>("module")?.unwrap_or(false);
    kwargs.assert_all_used()?;

    let load = match load_str.as_deref() {
        None => LoadStrategy::Defer,
        Some(s) => s.parse::<LoadStrategy>().map_err(|_| {
            minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                format!(
                    r#"register_script: load must be one of "defer", "async", "sync"; got "{s}""#,
                ),
            )
        })?,
    };

    handle
        .lock()
        .register_script(ScriptTag {
            url: url.to_owned(),
            load,
            module,
        })
        .map_err(|e| {
            minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
        })?;

    Ok("")
}
