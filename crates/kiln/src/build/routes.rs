use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jiff::Timestamp;
use strum::IntoEnumIterator;

use super::BuildContext;
use super::listing::{BucketKind, ListingArtifacts, ListingBucket, newest};
use super::paginate::paginated_path;
use super::sitemap::SitemapEntry;
use crate::content::page::Page;
use crate::pagination::Paginator;
use crate::static_assets::MEDIA_DIRECTORY;
use crate::url::page_url;

struct PlannedOutput {
    owner: String,
    sitemap: Option<SitemapEntry>,
}

/// All generated destinations, validated before rendering into the staged output.
#[derive(Default)]
pub(super) struct RoutePlan {
    outputs: BTreeMap<PathBuf, PlannedOutput>,
}

impl RoutePlan {
    pub(super) fn new(
        ctx: &BuildContext,
        pages: &[Page],
        artifacts: &ListingArtifacts,
        buckets: &[ListingBucket<'_>],
        output_dir: &Path,
    ) -> Result<Self> {
        let mut plan = Self::default();
        for (page, prepared) in pages.iter().zip(&artifacts.pages) {
            plan.insert(
                prepared.output_path.clone(),
                page.source_path.display().to_string(),
                Some(SitemapEntry {
                    loc: prepared.summary.url.clone(),
                    lastmod: prepared.modified(),
                }),
            )?;
        }
        plan.listings(ctx, artifacts, buckets)?;
        for path in ["index.xml", "sitemap.xml", "robots.txt"] {
            plan.insert(PathBuf::from(path), format!("generated {path}"), None)?;
        }
        for bucket in buckets {
            plan.insert(
                bucket.base_path().join("index.xml"),
                format!("{} feed", bucket.name),
                None,
            )?;
        }
        if ctx.template_engine.has_template("404.html") {
            plan.insert(PathBuf::from("404.html"), "404 template".into(), None)?;
        }
        if ctx.config.search.enabled {
            plan.insert(
                PathBuf::from("pagefind"),
                "search index directory".into(),
                None,
            )?;
        }
        plan.reserve_on_disk(output_dir)?;
        Ok(plan)
    }

    fn listings(
        &mut self,
        ctx: &BuildContext,
        artifacts: &ListingArtifacts,
        buckets: &[ListingBucket<'_>],
    ) -> Result<()> {
        if ctx.template_engine.has_template("home.html") {
            let posts = artifacts.posts();
            self.paginated(
                ctx,
                Path::new(""),
                Paginator::new(&posts, ctx.config.paginate.home.get()).total_pages(),
                "home",
                newest(&posts),
            )?;
        }
        if ctx.template_engine.has_template("archive.html") {
            for bucket in buckets {
                let per_page = bucket.kind.page_size(&ctx.config.paginate).get();
                self.paginated(
                    ctx,
                    &bucket.base_path(),
                    Paginator::new(&bucket.pages, per_page).total_pages(),
                    &bucket.name,
                    newest(&bucket.pages),
                )?;
            }
        }
        if ctx.template_engine.has_template("overview.html") {
            for kind in BucketKind::iter().filter(|kind| kind.has_overview()) {
                let path = Path::new(&kind.plural()).join("index.html");
                self.insert(
                    path.clone(),
                    format!("{} overview", kind.plural()),
                    Some(SitemapEntry {
                        loc: page_url(&ctx.config.base_url, &path),
                        lastmod: None,
                    }),
                )?;
            }
        }
        Ok(())
    }

    fn paginated(
        &mut self,
        ctx: &BuildContext,
        base: &Path,
        count: usize,
        owner: &str,
        lastmod: Option<Timestamp>,
    ) -> Result<()> {
        for number in 1..=count {
            let path = paginated_path(base, number);
            self.insert(
                path.clone(),
                format!("{owner} page {number}"),
                Some(SitemapEntry {
                    loc: page_url(&ctx.config.base_url, &path),
                    lastmod,
                }),
            )?;
        }
        Ok(())
    }

    fn insert(
        &mut self,
        path: PathBuf,
        owner: String,
        sitemap: Option<SitemapEntry>,
    ) -> Result<()> {
        if path.components().next().is_some_and(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(MEDIA_DIRECTORY)
        }) {
            bail!(
                "generated route {owner} uses reserved asset directory: {}",
                path.display()
            );
        }
        if let Some(existing) = self.outputs.get(&path) {
            bail!(
                "output route collision at {} between {} and {}",
                path.display(),
                existing.owner,
                owner
            );
        }
        self.outputs.insert(path, PlannedOutput { owner, sitemap });
        Ok(())
    }

    /// Creates every planned file in `output_dir` to surface collisions with published assets,
    /// file / directory conflicts and filesystem aliases, then removes the reservations.
    fn reserve_on_disk(&self, output_dir: &Path) -> Result<()> {
        let mut reserved = BTreeMap::new();
        for (path, output) in &self.outputs {
            let destination = output_dir.join(path);
            let parent = destination
                .parent()
                .context("planned output has no parent")?;
            let result = fs::create_dir_all(parent).and_then(|()| {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&destination)
                    .map(drop)
            });
            if let Err(error) = result {
                let existing = destination
                    .ancestors()
                    .filter_map(|ancestor| ancestor.canonicalize().ok())
                    .find_map(|canonical| reserved.get(&canonical))
                    .map_or("a published asset", String::as_str);
                return Err(error).with_context(|| {
                    format!(
                        "output route collision at {} between {} and {existing}",
                        path.display(),
                        output.owner,
                    )
                });
            }
            let canonical = destination
                .canonicalize()
                .with_context(|| format!("failed to resolve {}", destination.display()))?;
            reserved.insert(canonical, output.owner.clone());
        }

        // Keep every reservation until native filesystem aliases have been checked together.
        for path in self.outputs.keys() {
            let destination = output_dir.join(path);
            fs::remove_file(&destination).with_context(|| {
                format!("failed to remove reservation {}", destination.display())
            })?;
        }
        Ok(())
    }

    pub(super) fn sitemap_entries(&self) -> Vec<&SitemapEntry> {
        self.outputs
            .values()
            .filter_map(|output| output.sitemap.as_ref())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── RoutePlan::insert ──

    #[test]
    fn insert_conflicting_files_returns_error() {
        let mut plan = RoutePlan::default();
        plan.insert("posts/index.html".into(), "first source".into(), None)
            .unwrap();
        let error = plan
            .insert("posts/index.html".into(), "second source".into(), None)
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("first source"));
        assert!(message.contains("second source"));
        assert!(message.contains("collision"));
    }

    #[test]
    fn insert_reserved_asset_directory_returns_error() {
        let mut plan = RoutePlan::default();
        for path in [
            "_assets/index.html",
            "_ASSETS/index.html",
            "_Assets/index.html",
        ] {
            let error = plan.insert(path.into(), "page".into(), None).unwrap_err();
            assert!(error.to_string().contains("reserved asset directory"));
        }
    }

    // ── RoutePlan::reserve_on_disk ──

    #[test]
    fn reserve_on_disk_removes_reservations_before_rendering() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = RoutePlan::default();
        plan.insert("posts/index.html".into(), "archive".into(), None)
            .unwrap();
        plan.reserve_on_disk(directory.path()).unwrap();
        assert!(!directory.path().join("posts/index.html").exists());
    }

    #[test]
    fn reserve_on_disk_conflicting_destinations_returns_error() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("posts"), "asset").unwrap();
        let mut plan = RoutePlan::default();
        plan.insert("posts/index.html".into(), "archive".into(), None)
            .unwrap();
        assert!(
            plan.reserve_on_disk(directory.path())
                .unwrap_err()
                .to_string()
                .contains("published asset")
        );
    }

    #[test]
    fn reserve_on_disk_file_directory_collision_returns_error() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = RoutePlan::default();
        plan.insert("sitemap.xml/index.html".into(), "content page".into(), None)
            .unwrap();
        plan.insert("sitemap.xml".into(), "generated sitemap".into(), None)
            .unwrap();
        let error = plan.reserve_on_disk(directory.path()).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("content page"));
        assert!(message.contains("generated sitemap"));
    }

    #[test]
    fn reserve_on_disk_reserved_directory_descendant_returns_error() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = RoutePlan::default();
        plan.insert("pagefind".into(), "search index".into(), None)
            .unwrap();
        plan.insert(
            "pagefind/article/index.html".into(),
            "content page".into(),
            None,
        )
        .unwrap();

        let message = format!("{:#}", plan.reserve_on_disk(directory.path()).unwrap_err());
        assert!(message.contains("search index"));
        assert!(message.contains("content page"));
        assert!(message.contains("pagefind/article/index.html"));
    }

    #[test]
    fn reserve_on_disk_native_alias_returns_error() {
        let directory = tempfile::tempdir().unwrap();
        let probe = directory.path().join("CaseProbe");
        fs::write(&probe, "probe").unwrap();
        let insensitive = directory.path().join("caseprobe").exists();
        fs::remove_file(&probe).unwrap();
        if !insensitive {
            return;
        }
        let mut plan = RoutePlan::default();
        plan.insert("Page/index.html".into(), "first source".into(), None)
            .unwrap();
        plan.insert("page/index.html".into(), "second source".into(), None)
            .unwrap();
        let error = plan.reserve_on_disk(directory.path()).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("first source"));
        assert!(message.contains("second source"));
    }
}
