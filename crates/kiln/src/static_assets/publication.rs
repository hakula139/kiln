use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::{StaticAssetManifest, is_media, rename_fingerprinted, site_path};
use crate::content::is_private;
use crate::content::page::Page;
use crate::minify::{AssetKind, asset_kind};
use crate::output::{copy_directory, copy_file};

#[derive(Default)]
struct PublishedFiles {
    paths: BTreeMap<PathBuf, PathBuf>,
    aliases: BTreeMap<PathBuf, PathBuf>,
}

/// Source → output paths recorded while publishing shared and owning-page assets.
pub(crate) struct PublishedAssets {
    shared_files: PublishedFiles,
    pages: BTreeMap<PathBuf, PublishedFiles>,
    /// Published files keyed by their source-named output paths.
    files: BTreeMap<PathBuf, PublishedFile>,
}

/// Whether kiln may publish a file under a content-addressed name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Naming {
    /// A file from `assets/` or a page bundle, which takes a content-addressed name when kiln can
    /// derive one from its final bytes.
    Addressable,
    /// A `static/` file, which keeps its source name.
    Fixed,
}

/// The file whose copy last wrote an output path.
struct PublishedFile {
    /// Source path, relative to the site root when it lies inside it.
    source: PathBuf,
    naming: Naming,
    /// Output path the file was published at.
    published: PathBuf,
}

/// Output files written so far, with the owner of each.
struct OutputFiles<'a> {
    root: &'a Path,
    output_dir: &'a Path,
    files: BTreeMap<PathBuf, PublishedFile>,
    /// Output paths keyed by the canonical path of the file each one names.
    canonical: BTreeMap<PathBuf, PathBuf>,
}

impl PublishedAssets {
    /// Copies theme, site, and page assets and records their paths relative to `output_dir`.
    ///
    /// Media from `assets/` and page bundles are published only under content-addressed names.
    ///
    /// # Errors
    ///
    /// Returns an error if an asset cannot be copied or renamed, if its source or output path
    /// cannot be resolved, or if two differently named outputs are one file on the filesystem.
    pub(crate) fn publish(
        root: &Path,
        theme_dir: Option<&Path>,
        pages: &[Page],
        output_dir: &Path,
    ) -> Result<Self> {
        let mut shared_files = PublishedFiles::default();
        let mut written = OutputFiles {
            root,
            output_dir,
            files: BTreeMap::new(),
            canonical: BTreeMap::new(),
        };
        for owner in theme_dir.into_iter().chain(std::iter::once(root)) {
            let assets = PublishedFiles::new(
                copy_directory(&owner.join("assets"), &output_dir.join("assets"), |name| {
                    !is_private(name)
                })?,
                output_dir,
            )?;
            written.record(&assets, Naming::Addressable)?;
            shared_files.extend(assets);
            let statics = PublishedFiles::new(
                copy_directory(&owner.join("static"), output_dir, |_| true)?,
                output_dir,
            )?;
            written.record(&statics, Naming::Fixed)?;
            shared_files.extend(statics);
        }

        let mut page_files = BTreeMap::new();
        for page in pages {
            let files = PublishedFiles::new(copy_page_assets(page, output_dir)?, output_dir)?;
            written.record(&files, Naming::Addressable)?;
            page_files.insert(page.source_path.clone(), files);
        }

        let mut files = written.files;
        for (path, file) in &mut files {
            if file.naming == Naming::Addressable && is_media(path) {
                file.published = rename_fingerprinted(output_dir, path)?;
            }
        }
        for published_files in std::iter::once(&mut shared_files).chain(page_files.values_mut()) {
            published_files.relocate(&files);
        }
        Ok(Self {
            shared_files,
            pages: page_files,
            files,
        })
    }

    /// Resolves a copied source path to its published path, preferring owning-page assets and
    /// explicit aliases.
    ///
    /// Returns `None` when the source is outside the page's and site's published files.
    ///
    /// # Errors
    ///
    /// Returns an error if the source path cannot be resolved.
    pub(crate) fn resolve(&self, page: Option<&Path>, source: &Path) -> Result<Option<&Path>> {
        let source = normalize_source(source)?;
        if let Some(files) = page.and_then(|page| self.pages.get(page))
            && let Some(path) = files.resolve(&source)?
        {
            return Ok(Some(path));
        }
        self.shared_files.resolve(&source)
    }

    /// Returns source-named output paths and the paths their files were published at.
    pub(crate) fn outputs(&self) -> impl Iterator<Item = (&PathBuf, &PathBuf)> {
        self.files
            .iter()
            .map(|(path, file)| (path, &file.published))
    }

    /// Returns published script paths with their naming.
    pub(crate) fn scripts(&self) -> impl Iterator<Item = (&Path, Naming)> {
        self.files
            .iter()
            .filter(|(path, _)| asset_kind(path).is_some_and(AssetKind::is_script))
            .map(|(path, file)| (path.as_path(), file.naming))
    }

    /// Rejects copied files that keep their source names under names the host cache rules for
    /// content-addressed files would match.
    ///
    /// A source-named image, font or script must not have a dotted stem, and a dotted `site` or
    /// `page` stylesheet under `assets/css/` is reserved for compiled stylesheets.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first such file's source.
    pub(crate) fn ensure_unaddressed_names(&self, manifest: &StaticAssetManifest) -> Result<()> {
        for (path, file) in &self.files {
            let source_named = manifest
                .paths
                .get(&site_path(path))
                .is_none_or(|published| published == path);
            let Some(hint) = source_named
                .then(|| reserved_name_hint(path, file.naming))
                .flatten()
            else {
                continue;
            };
            bail!(
                "{} publishes {} under its source name, which the immutable cache rule for \
                 content-addressed files would match. {hint}",
                file.source.display(),
                site_path(path),
            );
        }
        Ok(())
    }
}

/// Returns how to rename a source-named output that cache rules for content-addressed files
/// would match, or `None` when no rule matches it.
fn reserved_name_hint(path: &Path, naming: Naming) -> Option<&'static str> {
    let stem = path.file_stem()?.to_str()?;
    let (name, _) = stem.split_once('.')?;
    let is_script = asset_kind(path).is_some_and(AssetKind::is_script);
    if is_media(path) {
        Some(
            "Move it to assets/ to publish it under a content-addressed name, or remove the dots from its stem.",
        )
    } else if is_script && naming == Naming::Fixed {
        Some(
            "Remove the dots from its stem, or move it to assets/ to publish it under a content-addressed name.",
        )
    } else if is_script {
        Some(
            "Remove the dots from its stem, since scripts that import or are imported keep their source names.",
        )
    } else if asset_kind(path) == Some(AssetKind::Css)
        && path.parent().is_some_and(|parent| match name {
            "site" => parent == Path::new("assets/css"),
            "page" => parent.ends_with("assets/css"),
            _ => false,
        })
    {
        Some(
            "Rename it, since assets/css/site.*.css and page.*.css under any assets/css/ are reserved for compiled stylesheets.",
        )
    } else {
        None
    }
}

impl OutputFiles<'_> {
    /// Records published files over the files they overlay.
    ///
    /// # Errors
    ///
    /// Returns an error if an output file cannot be resolved, or if it is also an earlier output
    /// recorded under a different name.
    fn record(&mut self, files: &PublishedFiles, naming: Naming) -> Result<()> {
        for (source, output) in &files.paths {
            let source = source.strip_prefix(self.root).unwrap_or(source).to_owned();
            let canonical = self.output_dir.join(output).canonicalize()?;
            if let Some(existing) = self.canonical.get(&canonical)
                && existing != output
            {
                bail!(
                    "{} and {} publish {} and {}, which are one file on this filesystem. Rename \
                     one of them",
                    self.files[existing].source.display(),
                    source.display(),
                    site_path(existing),
                    site_path(output),
                );
            }
            self.canonical.insert(canonical, output.clone());
            self.files.insert(
                output.clone(),
                PublishedFile {
                    source,
                    naming,
                    published: output.clone(),
                },
            );
        }
        Ok(())
    }
}

fn copy_page_assets(page: &Page, output_dir: &Path) -> Result<BTreeMap<PathBuf, PathBuf>> {
    let mut files = BTreeMap::new();
    let Some(bundle) = page.bundle_dir() else {
        return Ok(files);
    };
    let destination = output_dir.join(&page.output_dir);
    for asset in &page.assets {
        let relative = asset
            .strip_prefix(bundle)
            .expect("bundle assets are discovered under the bundle directory");
        let target = destination.join(relative);
        copy_file(asset, &target)
            .with_context(|| format!("failed to copy asset {}", asset.display()))?;
        files.insert(asset.clone(), target);
    }
    Ok(files)
}

impl PublishedFiles {
    fn new(files: BTreeMap<PathBuf, PathBuf>, output_dir: &Path) -> Result<Self> {
        let mut published = Self::default();
        for (source, output) in files {
            let source = normalize_source(&source)?;
            let output = output.strip_prefix(output_dir)?.to_owned();
            // Tailwind resolves imported sources through symlinks before rebasing asset URLs.
            published
                .aliases
                .entry(source.canonicalize()?)
                .or_insert_with(|| output.clone());
            published.paths.insert(source, output);
        }
        Ok(published)
    }

    /// Replaces source-named output paths with the paths their files were published at.
    fn relocate(&mut self, files: &BTreeMap<PathBuf, PublishedFile>) {
        for output in self.paths.values_mut().chain(self.aliases.values_mut()) {
            // Every recorded output path has a published entry.
            output.clone_from(&files[output].published);
        }
    }

    fn extend(&mut self, files: Self) {
        self.paths.extend(files.paths);
        self.aliases.extend(files.aliases);
    }

    fn resolve(&self, source: &Path) -> Result<Option<&Path>> {
        if let Some(output) = self.paths.get(source) {
            return Ok(Some(output));
        }
        Ok(self
            .aliases
            .get(&source.canonicalize()?)
            .map(PathBuf::as_path))
    }
}

fn normalize_source(source: &Path) -> Result<PathBuf> {
    let canonical = source.canonicalize()?;
    let mut normalized = PathBuf::new();
    for component in std::path::absolute(source)?.components() {
        if component == Component::ParentDir {
            normalized.pop();
        } else {
            normalized.push(component);
        }
    }

    // Preserve the published alias only when lexical parent traversal keeps the same source.
    if normalized
        .canonicalize()
        .is_ok_and(|path| path == canonical)
    {
        Ok(normalized)
    } else {
        Ok(canonical)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use indoc::indoc;

    use super::*;
    use crate::content::discovery::discover_content;
    use crate::static_assets::content_fingerprint;

    // ── PublishedAssets::publish ──

    #[test]
    fn publish_preserves_site_static_and_page_precedence() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/example");
        let content = root.path().join("content");
        let output = root.path().join("public");
        for (path, value) in [
            (theme.join("assets/image.svg"), "theme asset"),
            (theme.join("static/assets/image.svg"), "theme static"),
            (root.path().join("assets/image.svg"), "site asset"),
            (root.path().join("static/assets/image.svg"), "site static"),
            (theme.join("assets/theme.svg"), "theme asset"),
            (theme.join("static/assets/theme.svg"), "theme static"),
            (theme.join("static/assets/site.svg"), "theme static"),
            (root.path().join("assets/site.svg"), "site asset"),
            (
                root.path().join("static/example/image.svg"),
                "site page image",
            ),
            (content.join("example/image.svg"), "page image"),
        ] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, value).unwrap();
        }
        let source = content.join("example/index.md");
        fs::write(
            &source,
            indoc! {r#"
                +++
                title = "Example"
                +++
            "#},
        )
        .unwrap();
        let pages = discover_content(root.path()).unwrap().pages;
        PublishedAssets::publish(root.path(), Some(&theme), &pages, &output).unwrap();

        for (path, value) in [
            ("assets/image.svg", "site static"),
            ("assets/theme.svg", "theme static"),
            ("assets/site.svg", "site asset"),
            ("example/image.svg", "page image"),
        ] {
            assert_eq!(
                fs::read_to_string(output.join(path)).unwrap(),
                value,
                "{path}"
            );
        }
    }

    #[test]
    fn publish_content_addresses_asset_and_bundle_media_only() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/example");
        let content = root.path().join("content");
        let output = root.path().join("public");
        for (path, value) in [
            (theme.join("assets/images/theme.png"), "theme"),
            (theme.join("assets/images/override.png"), "theme override"),
            (
                root.path().join("assets/images/override.png"),
                "site override",
            ),
            (root.path().join("assets/images/shadowed.png"), "asset"),
            (
                root.path().join("static/assets/images/shadowed.png"),
                "static",
            ),
            (root.path().join("assets/js/app.js"), "script"),
            (root.path().join("static/favicon.ico"), "icon"),
            (root.path().join("static/vendor.js"), "vendor"),
            (content.join("example/photo.webp"), "photo"),
        ] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, value).unwrap();
        }
        let source = content.join("example/index.md");
        fs::write(
            &source,
            indoc! {r#"
                +++
                title = "Example"
                +++
            "#},
        )
        .unwrap();
        let pages = discover_content(root.path()).unwrap().pages;

        let assets = PublishedAssets::publish(root.path(), Some(&theme), &pages, &output).unwrap();

        let hashed = |path: &str, value: &str| {
            let (stem, extension) = path.rsplit_once('.').unwrap();
            format!(
                "{stem}.{}.{extension}",
                content_fingerprint(value.as_bytes())
            )
        };
        let expected = [
            (
                "assets/images/override.png",
                hashed("assets/images/override.png", "site override"),
            ),
            (
                "assets/images/shadowed.png",
                "assets/images/shadowed.png".to_owned(),
            ),
            (
                "assets/images/theme.png",
                hashed("assets/images/theme.png", "theme"),
            ),
            ("assets/js/app.js", "assets/js/app.js".to_owned()),
            ("example/photo.webp", hashed("example/photo.webp", "photo")),
            ("favicon.ico", "favicon.ico".to_owned()),
            ("vendor.js", "vendor.js".to_owned()),
        ]
        .map(|(source, published)| (PathBuf::from(source), PathBuf::from(published)));
        assert_eq!(
            assets
                .outputs()
                .map(|(source, published)| (source.clone(), published.clone()))
                .collect::<BTreeMap<_, _>>(),
            BTreeMap::from(expected.clone())
        );
        let mut files: Vec<_> = walkdir::WalkDir::new(&output)
            .into_iter()
            .map(|entry| entry.unwrap())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| entry.path().strip_prefix(&output).unwrap().to_owned())
            .collect();
        files.sort();
        let mut published: Vec<_> = expected.into_iter().map(|(_, path)| path).collect();
        published.sort();
        assert_eq!(files, published);
        assert_eq!(
            assets
                .resolve(
                    Some(&content.join("example/index.md")),
                    &content.join("example/photo.webp")
                )
                .unwrap(),
            Some(Path::new(&hashed("example/photo.webp", "photo"))),
        );
        assert_eq!(
            assets
                .resolve(None, &theme.join("assets/images/override.png"))
                .unwrap(),
            Some(Path::new(&hashed(
                "assets/images/override.png",
                "site override"
            ))),
        );
    }

    #[test]
    fn publish_case_variant_outputs_returns_error() {
        let root = tempfile::tempdir().unwrap();
        let probe = root.path().join("CaseProbe");
        fs::write(&probe, "probe").unwrap();
        if !root.path().join("caseprobe").exists() {
            return;
        }
        let theme = root.path().join("themes/example");
        for (path, value) in [
            (theme.join("assets/images/Logo.png"), "theme"),
            (root.path().join("static/assets/images/logo.png"), "site"),
        ] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, value).unwrap();
        }

        let err =
            PublishedAssets::publish(root.path(), Some(&theme), &[], &root.path().join("public"))
                .err()
                .unwrap()
                .to_string();

        assert!(
            err.contains(&format!(
                "{} and {} publish /assets/images/Logo.png and /assets/images/logo.png",
                native_path("themes/example/assets/images/Logo.png"),
                native_path("static/assets/images/logo.png"),
            )),
            "got: {err}"
        );
    }

    // ── PublishedAssets::resolve ──

    #[cfg(unix)]
    #[test]
    fn resolve_respects_filesystem_parent_traversal_through_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let static_dir = root.path().join("static");
        let physical = root.path().join("external/nested");
        fs::create_dir_all(&static_dir).unwrap();
        fs::create_dir_all(&physical).unwrap();
        fs::write(static_dir.join("image.svg"), "logical sibling").unwrap();
        fs::write(
            physical.parent().unwrap().join("image.svg"),
            "physical sibling",
        )
        .unwrap();
        symlink(&physical, static_dir.join("linked")).unwrap();
        symlink(
            physical.parent().unwrap().join("image.svg"),
            static_dir.join("physical.svg"),
        )
        .unwrap();
        let output = root.path().join("public");
        let assets = PublishedAssets::publish(root.path(), None, &[], &output).unwrap();

        let destination = assets
            .resolve(None, &static_dir.join("linked/../image.svg"))
            .unwrap()
            .unwrap();
        assert_eq!(destination, Path::new("physical.svg"));
        assert_eq!(
            fs::read_to_string(output.join(destination)).unwrap(),
            "physical sibling"
        );
        assert_eq!(
            assets.resolve(None, &static_dir.join("image.svg")).unwrap(),
            Some(Path::new("image.svg"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_preserves_aliases_and_owning_page_scope() {
        let root = tempfile::tempdir().unwrap();
        let theme = root.path().join("themes/example");
        let content = root.path().join("content");
        let output = root.path().join("public");
        for directory in [
            root.path().join("static"),
            theme.join("static"),
            content.join("first/assets"),
            content.join("second"),
            root.path().join("_vendor"),
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        let static_image = root.path().join("_vendor/static.svg");
        let page_image = root.path().join("_vendor/page.svg");
        fs::write(&static_image, "static image").unwrap();
        fs::write(&page_image, "page image").unwrap();
        let theme_alias = theme.join("static/theme.svg");
        symlink(&static_image, &theme_alias).unwrap();
        symlink(&static_image, root.path().join("static/site.svg")).unwrap();
        symlink(&page_image, content.join("first/assets/first.svg")).unwrap();
        let page_alias = content.join("first/assets/second.svg");
        symlink(&page_image, &page_alias).unwrap();
        for name in ["first", "second"] {
            let source = content.join(name).join("index.md");
            fs::write(
                &source,
                indoc! {r#"
                    +++
                    title = "Example"
                    +++
                "#},
            )
            .unwrap();
        }
        let pages = discover_content(root.path()).unwrap().pages;

        let assets = PublishedAssets::publish(root.path(), Some(&theme), &pages, &output).unwrap();

        assert_eq!(
            assets.resolve(None, &static_image).unwrap(),
            Some(Path::new("site.svg")),
        );
        assert_eq!(
            assets.resolve(None, &theme_alias).unwrap(),
            Some(Path::new("theme.svg")),
        );
        assert_eq!(
            assets
                .resolve(Some(&pages[0].source_path), &page_image)
                .unwrap(),
            Some(Path::new("first/assets/first.svg")),
        );
        assert_eq!(
            assets
                .resolve(Some(&pages[0].source_path), &page_alias)
                .unwrap(),
            Some(Path::new("first/assets/second.svg")),
        );
        assert_eq!(assets.resolve(None, &page_image).unwrap(), None);
        assert_eq!(
            assets
                .resolve(Some(&pages[1].source_path), &page_image)
                .unwrap(),
            None,
        );
        assert_eq!(
            fs::read_to_string(output.join("site.svg")).unwrap(),
            "static image"
        );
        assert_eq!(
            fs::read_to_string(output.join("first/assets/second.svg")).unwrap(),
            "page image",
        );
    }

    // ── PublishedAssets::scripts ──

    #[test]
    fn scripts_marks_assets_and_bundle_scripts_addressable() {
        let root = tempfile::tempdir().unwrap();
        let assets = publish_files(
            root.path(),
            &[
                "assets/js/app.js",
                "assets/js/App.MJS",
                "assets/style.css",
                "static/vendor.js",
            ],
        );

        assert_eq!(
            assets.scripts().collect::<Vec<_>>(),
            [
                (Path::new("assets/js/App.MJS"), Naming::Addressable),
                (Path::new("assets/js/app.js"), Naming::Addressable),
                (Path::new("vendor.js"), Naming::Fixed),
            ]
        );
    }

    // ── PublishedAssets::ensure_unaddressed_names ──

    #[test]
    fn ensure_unaddressed_names_accepts_hashed_and_unreserved_names() {
        let root = tempfile::tempdir().unwrap();
        let assets = publish_files(
            root.path(),
            &[
                "assets/images/logo.dark.png",
                "assets/css/theme.min.css",
                "static/favicon.ico",
                "static/app.js",
                "static/site.min.css",
                "static/posts/a/assets/css/site.min.css",
                "static/data.v1.json",
            ],
        );

        assets
            .ensure_unaddressed_names(&StaticAssetManifest::new(assets.outputs()))
            .unwrap();
    }

    #[test]
    fn ensure_unaddressed_names_reserved_source_names_returns_error() {
        for (file, output, hint) in [
            (
                "static/logo.dark.png",
                "/logo.dark.png",
                "Move it to assets/",
            ),
            (
                "static/vendor.min.js",
                "/vendor.min.js",
                "Remove the dots from its stem, or move it to assets/",
            ),
            (
                "assets/js/app.min.js",
                "/assets/js/app.min.js",
                "scripts that import or are imported keep their source names",
            ),
            (
                "assets/css/site.min.css",
                "/assets/css/site.min.css",
                "reserved for compiled stylesheets",
            ),
            (
                "static/posts/a/assets/css/page.print.css",
                "/posts/a/assets/css/page.print.css",
                "reserved for compiled stylesheets",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let assets = publish_files(root.path(), &[file]);

            let err = assets
                .ensure_unaddressed_names(&StaticAssetManifest::new(assets.outputs()))
                .unwrap_err()
                .to_string();

            assert!(
                err.contains(&format!(
                    "{} publishes {output} under its source name",
                    native_path(file)
                )) && err.contains(hint),
                "got: {err}"
            );
        }
    }

    // ── OutputFiles::record ──

    #[cfg(unix)]
    #[test]
    fn record_outputs_naming_one_file_returns_error() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("public");
        fs::create_dir_all(output.join("assets/real")).unwrap();
        fs::write(output.join("assets/real/logo.png"), "image").unwrap();
        symlink(output.join("assets/real"), output.join("assets/alias")).unwrap();
        let files = |source: &str, output: &str| PublishedFiles {
            paths: BTreeMap::from([(root.path().join(source), PathBuf::from(output))]),
            aliases: BTreeMap::new(),
        };
        let mut written = OutputFiles {
            root: root.path(),
            output_dir: &output,
            files: BTreeMap::new(),
            canonical: BTreeMap::new(),
        };

        written
            .record(
                &files("assets/real/logo.png", "assets/real/logo.png"),
                Naming::Addressable,
            )
            .unwrap();
        written
            .record(
                &files("assets/real/logo.png", "assets/real/logo.png"),
                Naming::Fixed,
            )
            .unwrap();
        let err = written
            .record(
                &files("static/assets/alias/logo.png", "assets/alias/logo.png"),
                Naming::Fixed,
            )
            .unwrap_err()
            .to_string();

        assert!(
            err.contains(
                "assets/real/logo.png and static/assets/alias/logo.png publish \
                 /assets/real/logo.png and /assets/alias/logo.png"
            ),
            "got: {err}"
        );
    }

    /// Converts a `/`-separated relative path to the platform's separators, as errors display it.
    fn native_path(path: &str) -> String {
        path.replace('/', std::path::MAIN_SEPARATOR_STR)
    }

    fn publish_files(root: &Path, files: &[&str]) -> PublishedAssets {
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "body").unwrap();
        }
        PublishedAssets::publish(root, None, &[], &root.join("public")).unwrap()
    }
}
