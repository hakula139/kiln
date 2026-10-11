use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use swc_common::SourceMap;
use swc_common::sync::Lrc;
use swc_ecma_ast::{CallExpr, Callee, ExportAll, Expr, ImportDecl, Lit, NamedExport, Str};
use swc_ecma_visit::{Visit, VisitWith};

use super::{StaticAssetManifest, is_passthrough, site_key, site_path};
use crate::minify::{AssetKind, asset_kind, parse_js};
use crate::url::join_site_url;

/// Module specifiers requested by a script.
#[derive(Debug, PartialEq, Eq)]
enum ScriptImports {
    /// Every requested module is named by a string literal.
    Literal(Vec<String>),
    /// A module specifier cannot be read as a literal URL: a computed `import()` or a specifier
    /// that is not valid UTF-8.
    Computed,
}

/// Renames scripts from `assets/` and page bundles to their content-addressed names when no
/// published script imports them and they import nothing.
///
/// Module specifiers resolve by URL, so renaming an importer or an import target would break the
/// import. A computed `import()` may load any script, so it keeps every script's source name.
///
/// # Errors
///
/// Returns an error if a script cannot be read or renamed.
pub(crate) fn fingerprint_scripts(
    manifest: &mut StaticAssetManifest,
    scripts: &BTreeMap<PathBuf, bool>,
    output_dir: &Path,
    prefix: &str,
) -> Result<()> {
    let mut linked = BTreeSet::new();
    for path in scripts.keys() {
        let file = output_dir.join(path);
        let bytes =
            fs::read(&file).with_context(|| format!("failed to read script {}", file.display()))?;
        let imports = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|source| parse_imports(source, path));
        let specifiers = match imports {
            Some(ScriptImports::Literal(specifiers)) if specifiers.is_empty() => continue,
            Some(ScriptImports::Literal(specifiers)) => specifiers,
            Some(ScriptImports::Computed) => return Ok(()),
            None => Vec::new(),
        };
        let url = join_site_url(prefix, &site_path(path));
        linked.insert(site_path(path));
        linked.extend(
            specifiers
                .iter()
                .filter(|specifier| !is_passthrough(specifier))
                .filter_map(|specifier| site_key(specifier, &url, prefix)),
        );
    }

    for (path, _) in scripts
        .iter()
        .filter(|(path, addressed)| **addressed && !linked.contains(&site_path(path)))
    {
        manifest.fingerprint(output_dir, path)?;
    }
    Ok(())
}

/// Returns the module specifiers a script requests, or `None` if it cannot be parsed.
fn parse_imports(source: &str, path: &Path) -> Option<ScriptImports> {
    let is_module = asset_kind(path) == Some(AssetKind::Module);
    let program = parse_js(&Lrc::new(SourceMap::default()), source, is_module).ok()?;
    let mut collector = ImportCollector::default();
    program.visit_with(&mut collector);
    if collector.computed {
        return Some(ScriptImports::Computed);
    }
    collector.specifiers.sort_unstable();
    Some(ScriptImports::Literal(collector.specifiers))
}

#[derive(Default)]
struct ImportCollector {
    specifiers: Vec<String>,
    computed: bool,
}

impl ImportCollector {
    fn push(&mut self, specifier: &Str) {
        match specifier.value.as_str() {
            Some(value) => self.specifiers.push(value.to_owned()),
            None => self.computed = true,
        }
    }
}

impl Visit for ImportCollector {
    fn visit_import_decl(&mut self, import: &ImportDecl) {
        self.push(&import.src);
    }

    fn visit_export_all(&mut self, export: &ExportAll) {
        self.push(&export.src);
    }

    fn visit_named_export(&mut self, export: &NamedExport) {
        if let Some(src) = &export.src {
            self.push(src);
        }
    }

    fn visit_call_expr(&mut self, call: &CallExpr) {
        if matches!(call.callee, Callee::Import(_)) {
            match call.args.first().map(|arg| &*arg.expr) {
                Some(Expr::Lit(Lit::Str(specifier))) => self.push(specifier),
                _ => self.computed = true,
            }
        }
        call.visit_children_with(self);
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    // ── fingerprint_scripts ──

    #[test]
    fn fingerprint_scripts_renames_only_unlinked_bundled_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let mut scripts = BTreeMap::new();
        for (path, source, addressed) in [
            ("assets/js/classic.js", "window.ready = true;", true),
            (
                "assets/js/app.js",
                "import { value } from './lib.js';",
                true,
            ),
            ("assets/js/lib.js", "export const value = 1;", true),
            ("assets/js/lazy.mjs", "export const lazy = 1;", true),
            ("assets/js/broken.js", "function () {", true),
            (
                "posts/a/chart.js",
                "import('/blog/assets/js/lazy.mjs');",
                true,
            ),
            ("vendor.js", "window.vendor = true;", false),
        ] {
            let file = dir.path().join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, source).unwrap();
            scripts.insert(PathBuf::from(path), addressed);
        }
        let mut manifest = StaticAssetManifest::new(scripts.keys().map(|path| (path, path)));

        fingerprint_scripts(&mut manifest, &scripts, dir.path(), "/blog/").unwrap();

        let classic = format!(
            "/assets/js/classic.{}.js",
            super::super::content_fingerprint(b"window.ready = true;")
        );
        assert_eq!(
            manifest.asset_url("/assets/js/classic.js").unwrap(),
            classic
        );
        assert!(dir.path().join(&classic[1..]).exists());
        assert!(!dir.path().join("assets/js/classic.js").exists());
        for path in [
            "/assets/js/app.js",
            "/assets/js/lib.js",
            "/assets/js/lazy.mjs",
            "/assets/js/broken.js",
            "/posts/a/chart.js",
            "/vendor.js",
        ] {
            assert_eq!(manifest.asset_url(path).unwrap(), path);
            assert!(dir.path().join(&path[1..]).exists(), "{path}");
        }
    }

    #[test]
    fn fingerprint_scripts_computed_import_keeps_source_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut scripts = BTreeMap::new();
        for (path, source) in [
            ("loader.js", "import(`./${name}.js`);"),
            ("plugin.js", "window.plugin = true;"),
        ] {
            fs::write(dir.path().join(path), source).unwrap();
            scripts.insert(PathBuf::from(path), true);
        }
        let mut manifest = StaticAssetManifest::new(scripts.keys().map(|path| (path, path)));

        fingerprint_scripts(&mut manifest, &scripts, dir.path(), "").unwrap();

        assert_eq!(manifest.asset_url("/plugin.js").unwrap(), "/plugin.js");
        assert!(dir.path().join("plugin.js").exists());
    }

    // ── parse_imports ──

    #[test]
    fn parse_imports_collects_static_and_literal_dynamic_specifiers() {
        let source = indoc! {r#"
            import { a } from "./a.js";
            export { b } from '../b.js';
            export * from "/c.js";
            import "./side-effect.js";
            const d = import('./d.js');
            const e = import('.\/e.js');
            const local = 1;
            export { local };
        "#};

        assert_eq!(
            parse_imports(source, Path::new("app.js")),
            Some(ScriptImports::Literal(vec![
                "../b.js".to_owned(),
                "./a.js".to_owned(),
                "./d.js".to_owned(),
                "./e.js".to_owned(),
                "./side-effect.js".to_owned(),
                "/c.js".to_owned(),
            ]))
        );
        assert_eq!(
            parse_imports("var x = 1; function f() { return x; }", Path::new("App.JS")),
            Some(ScriptImports::Literal(Vec::new()))
        );
    }

    #[test]
    fn parse_imports_computed_or_non_utf8_specifier_returns_computed() {
        for source in [
            "import(name);",
            "import(`./${name}.js`);",
            r#"import "\uD800";"#,
        ] {
            assert_eq!(
                parse_imports(source, Path::new("app.js")),
                Some(ScriptImports::Computed),
                "{source}"
            );
        }
    }

    #[test]
    fn parse_imports_parse_error_or_strict_module_violation_returns_none() {
        assert_eq!(parse_imports("function () {", Path::new("app.js")), None);
        assert_eq!(parse_imports("with (scope) {}", Path::new("lib.MJS")), None);
        assert_eq!(
            parse_imports("with (scope) {}", Path::new("lib.js")),
            Some(ScriptImports::Literal(Vec::new()))
        );
    }
}
