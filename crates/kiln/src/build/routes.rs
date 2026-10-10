use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jiff::Timestamp;
use strum::IntoEnumIterator;

use super::BuildContext;
use super::listing::{BucketKind, ListingArtifacts, ListingBucket};
use super::paginate::paginated_path;
use crate::content::page::Page;
use crate::pagination::Paginator;
use crate::sitemap::SitemapEntry;
use crate::url::page_url;

struct PlannedOutput {
    owner: String,
    sitemap: Option<SitemapEntry>,
}

/// All generated destinations, validated before rendering into the staged output.
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
        let mut plan = Self {
            outputs: BTreeMap::new(),
        };
        for (page, prepared) in pages.iter().zip(&artifacts.pages) {
            plan.insert(
                prepared.output_path.clone(),
                page.source_path.display().to_string(),
                Some(SitemapEntry {
                    loc: prepared.summary.url.clone(),
                    lastmod: prepared
                        .updated
                        .or(prepared.published)
                        .map(|date| date.to_string()),
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
        plan.validate_assets(output_dir)?;
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
            let per_page = super::home::page_size(ctx);
            self.paginated(
                ctx,
                Path::new(""),
                Paginator::new(&posts, per_page).total_pages(),
                "home",
                newest(&posts),
            )?;
        }
        if ctx.template_engine.has_template("archive.html") {
            for bucket in buckets {
                let per_page = super::archive::page_size(ctx, bucket.kind);
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
        updated: Option<Timestamp>,
    ) -> Result<()> {
        for number in 1..=count {
            let path = paginated_path(base, number);
            self.insert(
                path.clone(),
                format!("{owner} page {number}"),
                Some(SitemapEntry {
                    loc: page_url(&ctx.config.base_url, &path),
                    lastmod: updated.map(|date| date.to_string()),
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

    fn validate_assets(&self, output_dir: &Path) -> Result<()> {
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
            reserved.insert(destination.canonicalize()?, output.owner.clone());
        }

        // Keep every reservation until native filesystem aliases have been checked together.
        for path in self.outputs.keys() {
            fs::remove_file(output_dir.join(path))?;
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

fn newest(pages: &[&super::listing::PreparedPage]) -> Option<Timestamp> {
    pages
        .iter()
        .filter_map(|page| page.updated.or(page.published))
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── RoutePlan::insert ──

    #[test]
    fn insert_conflicting_files_returns_error() {
        let mut plan = RoutePlan {
            outputs: BTreeMap::new(),
        };
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

    // ── RoutePlan::validate_assets ──

    #[test]
    fn validate_assets_removes_reservations_before_rendering() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = RoutePlan {
            outputs: BTreeMap::new(),
        };
        plan.insert("posts/index.html".into(), "archive".into(), None)
            .unwrap();
        plan.validate_assets(directory.path()).unwrap();
        assert!(!directory.path().join("posts/index.html").exists());
    }

    #[test]
    fn validate_assets_conflicting_destinations_returns_error() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("posts"), "asset").unwrap();
        let mut plan = RoutePlan {
            outputs: BTreeMap::new(),
        };
        plan.insert("posts/index.html".into(), "archive".into(), None)
            .unwrap();
        assert!(
            plan.validate_assets(directory.path())
                .unwrap_err()
                .to_string()
                .contains("published asset")
        );
    }

    #[test]
    fn validate_assets_file_directory_collision_returns_error() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = RoutePlan {
            outputs: BTreeMap::new(),
        };
        plan.insert("sitemap.xml/index.html".into(), "content page".into(), None)
            .unwrap();
        plan.insert("sitemap.xml".into(), "generated sitemap".into(), None)
            .unwrap();
        let error = plan.validate_assets(directory.path()).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("content page"));
        assert!(message.contains("generated sitemap"));
    }

    #[test]
    fn validate_assets_reserved_directory_descendant_returns_error() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = RoutePlan {
            outputs: BTreeMap::new(),
        };
        plan.insert("pagefind".into(), "search index".into(), None)
            .unwrap();
        plan.insert(
            "pagefind/article/index.html".into(),
            "content page".into(),
            None,
        )
        .unwrap();

        let message = format!("{:#}", plan.validate_assets(directory.path()).unwrap_err());
        assert!(message.contains("search index"));
        assert!(message.contains("content page"));
        assert!(message.contains("pagefind/article/index.html"));
    }

    #[test]
    fn validate_assets_native_alias_returns_error() {
        let directory = tempfile::tempdir().unwrap();
        let probe = directory.path().join("CaseProbe");
        fs::write(&probe, "probe").unwrap();
        let insensitive = directory.path().join("caseprobe").exists();
        fs::remove_file(&probe).unwrap();
        if !insensitive {
            return;
        }
        let mut plan = RoutePlan {
            outputs: BTreeMap::new(),
        };
        plan.insert("Page/index.html".into(), "first source".into(), None)
            .unwrap();
        plan.insert("page/index.html".into(), "second source".into(), None)
            .unwrap();
        let error = plan.validate_assets(directory.path()).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("first source"));
        assert!(message.contains("second source"));
    }
}
