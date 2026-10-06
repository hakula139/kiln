use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use crate::content::page::Page;
use crate::output::{copy_file, copy_static};

#[derive(Default)]
struct PublishedFiles {
    paths: BTreeMap<PathBuf, PathBuf>,
    aliases: BTreeMap<PathBuf, PathBuf>,
}

/// Source → output paths recorded while publishing static and owning-page assets.
pub(crate) struct PublishedAssets {
    static_files: PublishedFiles,
    pages: BTreeMap<PathBuf, PublishedFiles>,
}

impl PublishedAssets {
    /// Copies theme, site, and page assets and records their paths relative to `output_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if an asset cannot be copied or its source or output path cannot be resolved.
    pub(crate) fn publish(
        root: &Path,
        theme_dir: Option<&Path>,
        content_dir: &Path,
        pages: &[Page],
        output_dir: &Path,
    ) -> Result<Self> {
        let mut static_files = PublishedFiles::default();
        for source in theme_dir
            .into_iter()
            .map(|dir| dir.join("static"))
            .chain(std::iter::once(root.join("static")))
        {
            static_files.extend(PublishedFiles::new(
                copy_static(&source, output_dir)?,
                output_dir,
            )?);
        }

        let mut page_files = BTreeMap::new();
        for page in pages {
            let files = copy_page_assets(content_dir, page, output_dir)?;
            page_files.insert(
                page.source_path.clone(),
                PublishedFiles::new(files, output_dir)?,
            );
        }
        Ok(Self {
            static_files,
            pages: page_files,
        })
    }

    /// Resolves a copied source path, preferring owning-page assets and explicit aliases.
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
        self.static_files.resolve(&source)
    }
}

fn copy_page_assets(
    content_dir: &Path,
    page: &Page,
    output_dir: &Path,
) -> Result<BTreeMap<PathBuf, PathBuf>> {
    let bundle = page
        .source_path
        .parent()
        .context("page source has no parent")?;
    let output = page.output_path(content_dir)?;
    let destination = output_dir.join(output.parent().context("page output has no parent")?);
    let mut files = BTreeMap::new();
    for asset in &page.assets {
        let relative = asset
            .strip_prefix(bundle)
            .context("asset is outside its bundle")?;
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
    // Canonicalization would erase the names under which symlinked assets are published.
    let mut normalized = PathBuf::new();
    for component in std::path::absolute(source)?.components() {
        if component == Component::ParentDir {
            normalized.pop();
        } else {
            normalized.push(component);
        }
    }
    Ok(normalized)
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::*;

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
        let mut pages = Vec::new();
        for name in ["first", "second"] {
            let source = content.join(name).join("index.md");
            fs::write(&source, "+++\ntitle = 'Example'\n+++\n").unwrap();
            pages.push(Page::from_file(&source).unwrap());
        }

        let assets =
            PublishedAssets::publish(root.path(), Some(&theme), &content, &pages, &output).unwrap();

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
}
