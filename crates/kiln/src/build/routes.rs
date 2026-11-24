use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jiff::Timestamp;
use strum::IntoEnumIterator;

use super::bucket::{BucketKind, Listing, Listings};
use super::listing::{PreparedPage, format_page_date, group_by_year, newest};
use super::sitemap::SitemapEntry;
use super::{BuildContext, feed, sitemap};
use crate::output::write_output;
use crate::pagination::{PaginationVars, page_dir, paginate};
use crate::render::pipeline::render_page;
use crate::template::vars::{
    ArchivePageVars, HomePageVars, OverviewPageVars, PageMetadata, PostTemplateVars,
};
use crate::url::{BaseUrl, directory_url, join_site_url, page_url};

enum Route<'a> {
    Content(&'a PreparedPage<'a>),
    Home(ListingPage<'a>),
    Archive(&'a Listing<'a>, ListingPage<'a>),
    /// Lists every section or tag listing of its kind.
    Overview(BucketKind, &'a [Listing<'a>]),
    /// The site-wide feed of all posts.
    SiteFeed(&'a Listing<'a>),
    Feed(&'a Listing<'a>),
    Sitemap,
    Robots,
    NotFound,
    /// Reserves the directory Pagefind writes after rendering.
    SearchIndex,
}

impl Route<'_> {
    /// Names what produced this output in collision errors.
    fn owner(&self) -> String {
        match self {
            Self::Content(page) => page.page.source_path.display().to_string(),
            Self::Home(page) => format!("home page {}", page.number),
            Self::Archive(bucket, page) => format!("{} page {}", bucket.name, page.number),
            Self::Overview(kind, _) => format!("{} overview", kind.plural()),
            Self::SiteFeed(_) => "generated index.xml".into(),
            Self::Feed(bucket) => format!("{} feed", bucket.name),
            Self::Sitemap => "generated sitemap.xml".into(),
            Self::Robots => "generated robots.txt".into(),
            Self::NotFound => "404 template".into(),
            Self::SearchIndex => "search index directory".into(),
        }
    }

    fn is_html(&self) -> bool {
        matches!(
            self,
            Self::Content(_)
                | Self::Home(_)
                | Self::Archive(..)
                | Self::Overview(..)
                | Self::NotFound
        )
    }
}

struct ListingPage<'a> {
    /// Listing directory relative to the output root.
    base: PathBuf,
    number: usize,
    total: usize,
    pages: &'a [&'a PreparedPage<'a>],
    /// Newest modification across the whole listing, shared by every page.
    lastmod: Option<Timestamp>,
}

impl ListingPage<'_> {
    fn path(&self) -> PathBuf {
        page_dir(&self.base, self.number).join("index.html")
    }

    fn pagination(&self, prefix: &str) -> PaginationVars {
        let listing_url = join_site_url(prefix, &directory_url(&self.base));
        PaginationVars::new(&listing_url, self.number, self.total)
    }
}

/// Every generated destination, validated before rendering into the staged output.
#[derive(Default)]
pub(super) struct RoutePlan<'a> {
    routes: BTreeMap<PathBuf, Route<'a>>,
}

impl<'a> RoutePlan<'a> {
    /// Plans the outputs of every enabled template, with home pages listing `home` in order.
    pub(super) fn new(
        ctx: &BuildContext,
        pages: &'a [PreparedPage<'a>],
        home: &'a [&'a PreparedPage<'a>],
        listings: &'a Listings<'a>,
        output_dir: &Path,
    ) -> Result<Self> {
        let mut plan = Self::default();
        let templates = &ctx.template_engine;

        for page in pages {
            plan.insert(page.page.output_path(), Route::Content(page))?;
        }
        if templates.has_template("home.html") {
            plan.listing(Path::new(""), home, ctx.config.paginate.home, Route::Home)?;
        }
        if templates.has_template("archive.html") {
            for bucket in listings.iter() {
                let per_page = bucket.kind.page_size(&ctx.config.paginate);
                plan.listing(&bucket.path(), &bucket.pages, per_page, |page| {
                    Route::Archive(bucket, page)
                })?;
            }
        }
        if templates.has_template("overview.html") {
            for kind in BucketKind::iter().filter(|kind| kind.has_overview()) {
                plan.insert(
                    Path::new(kind.plural()).join("index.html"),
                    Route::Overview(kind, &listings.terms),
                )?;
            }
        }

        plan.insert(PathBuf::from("index.xml"), Route::SiteFeed(&listings.posts))?;
        for bucket in listings.iter() {
            plan.insert(bucket.path().join("index.xml"), Route::Feed(bucket))?;
        }
        plan.insert(PathBuf::from("sitemap.xml"), Route::Sitemap)?;
        plan.insert(PathBuf::from("robots.txt"), Route::Robots)?;
        if templates.has_template("404.html") {
            plan.insert(PathBuf::from("404.html"), Route::NotFound)?;
        }
        if ctx.config.search.enabled {
            plan.insert(PathBuf::from("pagefind"), Route::SearchIndex)?;
        }

        plan.reserve_on_disk(output_dir)?;
        Ok(plan)
    }

    fn listing(
        &mut self,
        base: &Path,
        pages: &'a [&'a PreparedPage<'a>],
        per_page: NonZeroUsize,
        route: impl Fn(ListingPage<'a>) -> Route<'a>,
    ) -> Result<()> {
        let chunks = paginate(pages, per_page);
        let total = chunks.len();
        let lastmod = newest(pages);
        for (number, chunk) in (1..).zip(chunks) {
            let page = ListingPage {
                base: base.to_owned(),
                number,
                total,
                pages: chunk,
                lastmod,
            };
            self.insert(page.path(), route(page))?;
        }
        Ok(())
    }

    fn insert(&mut self, path: PathBuf, route: Route<'a>) -> Result<()> {
        if let Some(existing) = self.routes.get(&path) {
            bail!(
                "output route collision at {} between {} and {}",
                path.display(),
                existing.owner(),
                route.owner()
            );
        }
        self.routes.insert(path, route);
        Ok(())
    }

    /// Creates every planned file in `output_dir` to surface collisions with published assets,
    /// file / directory conflicts and filesystem aliases, then removes the reservations.
    fn reserve_on_disk(&self, output_dir: &Path) -> Result<()> {
        let mut reserved: BTreeMap<PathBuf, &Route> = BTreeMap::new();
        for (path, route) in &self.routes {
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
                    .map_or_else(|| "a published asset".into(), |route| route.owner());
                return Err(error).with_context(|| {
                    format!(
                        "output route collision at {} between {} and {existing}",
                        path.display(),
                        route.owner(),
                    )
                });
            }
            let canonical = destination
                .canonicalize()
                .with_context(|| format!("failed to resolve {}", destination.display()))?;
            reserved.insert(canonical, route);
        }

        // Keep every reservation until native filesystem aliases have been checked together.
        for path in self.routes.keys() {
            let destination = output_dir.join(path);
            fs::remove_file(&destination).with_context(|| {
                format!("failed to remove reservation {}", destination.display())
            })?;
        }
        Ok(())
    }

    /// Renders every route into `output_dir` and returns the number of HTML pages written.
    pub(super) fn render(&self, ctx: &BuildContext, output_dir: &Path) -> Result<usize> {
        let mut html_pages = 0;
        for (path, route) in &self.routes {
            let Some(content) = self.render_route(ctx, path, route)? else {
                continue;
            };
            write_output(&output_dir.join(path), &content)?;
            html_pages += usize::from(route.is_html());
        }
        Ok(html_pages)
    }

    fn render_route(
        &self,
        ctx: &BuildContext,
        path: &Path,
        route: &Route,
    ) -> Result<Option<String>> {
        let base_url = &ctx.config.base_url;
        let content = match route {
            Route::Content(prepared) => render_content(ctx, prepared)?,
            Route::Home(page) => render_home(ctx, page, &page_url(base_url, path))?,
            Route::Archive(bucket, page) => {
                render_archive(ctx, bucket, page, &page_url(base_url, path))?
            }
            Route::Overview(kind, terms) => {
                render_overview(ctx, *kind, terms, &page_url(base_url, path))?
            }
            Route::SiteFeed(posts) => {
                feed::render(ctx, &ctx.config.title, Path::new(""), &posts.pages)
            }
            Route::Feed(bucket) => {
                let title = format!("{} - {}", bucket.name, ctx.config.title);
                feed::render(ctx, &title, &bucket.path(), &bucket.pages)
            }
            Route::Sitemap => sitemap::render_sitemap(&self.sitemap_entries(base_url)),
            Route::Robots => sitemap::render_robots(base_url),
            Route::NotFound => render_not_found(ctx, &page_url(base_url, path))?,
            Route::SearchIndex => return Ok(None),
        };
        Ok(Some(content))
    }

    /// Lists the HTML pages search engines should crawl, in path order.
    fn sitemap_entries(&self, base_url: &BaseUrl) -> Vec<SitemapEntry> {
        self.routes
            .iter()
            .filter_map(|(path, route)| {
                let lastmod = match route {
                    Route::Content(page) => page.modified(),
                    Route::Home(page) | Route::Archive(_, page) => page.lastmod,
                    Route::Overview(..) => None,
                    _ => return None,
                };
                Some(SitemapEntry {
                    loc: page_url(base_url, path),
                    lastmod,
                })
            })
            .collect()
    }
}

/// Renders a post or standalone page with its template.
fn render_content(ctx: &BuildContext, prepared: &PreparedPage) -> Result<String> {
    let page = prepared.page;
    let rendered = render_page(
        &page.body,
        &ctx.template_engine,
        &ctx.render_options,
        &ctx.page_resources(page, &prepared.summary.url),
        page.frontmatter.heading_numbering,
    )
    .with_context(|| format!("failed to render {}", page.source_path.display()))?;

    let page_css = ctx.stylesheets.page_css_url(&ctx.static_assets, page)?;
    let vars = PostTemplateVars {
        metadata: PageMetadata {
            title: &prepared.summary.title,
            description: &prepared.summary.description,
            url: &prepared.summary.url,
        },
        featured_image: prepared.summary.featured_image.as_ref(),
        license: page.frontmatter.license.as_deref(),
        page_css,
        date: prepared.summary.date.as_deref(),
        updated: prepared
            .updated
            .map(|date| format_page_date(date, &ctx.config.timezone)),
        tags: &prepared.summary.tags,
        section: prepared.summary.section.as_ref(),
        content: &rendered.content_html,
        toc: &rendered.toc_html,
    };

    ctx.template_engine
        .render_page_template("post.html", &vars, vars.metadata.url, &rendered.assets)
        .with_context(|| format!("failed to render {}", page.source_path.display()))
}

/// Renders one page of the home listing.
fn render_home(ctx: &BuildContext, page: &ListingPage<'_>, url: &str) -> Result<String> {
    let vars = HomePageVars {
        metadata: PageMetadata {
            title: &ctx.config.title,
            description: &ctx.config.description,
            url,
        },
        pages: page.pages.iter().map(|page| &page.summary).collect(),
        pagination: page.pagination(ctx.config.base_url.prefix()),
    };
    ctx.template_engine
        .render_generated("home.html", &vars, url)
        .context("failed to render home page")
}

/// Renders one page of the archive of `bucket`.
fn render_archive(
    ctx: &BuildContext,
    bucket: &Listing<'_>,
    page: &ListingPage<'_>,
    url: &str,
) -> Result<String> {
    let vars = ArchivePageVars {
        metadata: PageMetadata {
            title: &bucket.name,
            description: &ctx.config.description,
            url,
        },
        kind: bucket.kind.as_ref(),
        page_groups: group_by_year(page.pages),
        pagination: page.pagination(ctx.config.base_url.prefix()),
    };
    ctx.template_engine
        .render_generated("archive.html", &vars, url)
        .with_context(|| {
            format!(
                "failed to render archive {}/{}",
                bucket.kind.plural(),
                bucket.slug,
            )
        })
}

/// Renders the overview of every bucket of `kind`.
fn render_overview(
    ctx: &BuildContext,
    kind: BucketKind,
    buckets: &[Listing<'_>],
    url: &str,
) -> Result<String> {
    let title = ctx.i18n.t(&format!("all_{}", kind.plural()));
    let vars = OverviewPageVars {
        metadata: PageMetadata {
            title: &title,
            description: &ctx.config.description,
            url,
        },
        kind: kind.as_ref(),
        buckets: buckets
            .iter()
            .filter(|bucket| bucket.kind == kind)
            .map(|bucket| bucket.summary(&ctx.config.base_url))
            .collect(),
    };
    ctx.template_engine
        .render_generated("overview.html", &vars, url)
        .with_context(|| format!("failed to render {} overview", kind.plural()))
}

/// Renders the 404 page.
fn render_not_found(ctx: &BuildContext, url: &str) -> Result<String> {
    let title = ctx.i18n.t("error_404_title");
    let vars = PageMetadata {
        title: &title,
        description: &ctx.config.description,
        url,
    };
    ctx.template_engine
        .render_generated("404.html", &vars, url)
        .context("failed to render 404 page")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::bucket::Bucket;

    // ── RoutePlan::insert ──

    #[test]
    fn rejects_conflicting_files() {
        let posts = Bucket {
            kind: BucketKind::Posts,
            slug: "posts".into(),
            name: "All Posts".into(),
            pages: Vec::new(),
        };
        let mut plan = RoutePlan::default();
        plan.insert("index.xml".into(), Route::SiteFeed(&posts))
            .unwrap();
        let error = plan
            .insert("index.xml".into(), Route::Feed(&posts))
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "output route collision at index.xml between generated index.xml and All Posts feed"
        );
    }

    // ── RoutePlan::reserve_on_disk ──

    #[test]
    fn removes_reservations_before_rendering() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = RoutePlan::default();
        plan.insert("posts/index.html".into(), Route::NotFound)
            .unwrap();
        plan.reserve_on_disk(directory.path()).unwrap();
        assert!(!directory.path().join("posts/index.html").exists());
    }

    #[test]
    fn rejects_conflicting_destinations() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("posts"), "asset").unwrap();
        let mut plan = RoutePlan::default();
        plan.insert("posts/index.html".into(), Route::NotFound)
            .unwrap();
        assert!(
            plan.reserve_on_disk(directory.path())
                .unwrap_err()
                .to_string()
                .contains("published asset")
        );
    }

    #[test]
    fn rejects_file_directory_collision() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = RoutePlan::default();
        plan.insert("sitemap.xml/index.html".into(), Route::NotFound)
            .unwrap();
        plan.insert("sitemap.xml".into(), Route::Sitemap).unwrap();
        let message = format!("{:#}", plan.reserve_on_disk(directory.path()).unwrap_err());
        assert!(message.contains("404 template"));
        assert!(message.contains("generated sitemap.xml"));
    }

    #[test]
    fn rejects_reserved_directory_descendant() {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = RoutePlan::default();
        plan.insert("pagefind".into(), Route::SearchIndex).unwrap();
        plan.insert("pagefind/article/index.html".into(), Route::NotFound)
            .unwrap();

        let message = format!("{:#}", plan.reserve_on_disk(directory.path()).unwrap_err());
        assert!(message.contains("search index directory"));
        assert!(message.contains("404 template"));
        assert!(message.contains("pagefind/article/index.html"));
    }

    #[test]
    fn rejects_native_alias() {
        let directory = tempfile::tempdir().unwrap();
        let probe = directory.path().join("CaseProbe");
        fs::write(&probe, "probe").unwrap();
        let insensitive = directory.path().join("caseprobe").exists();
        fs::remove_file(&probe).unwrap();
        if !insensitive {
            return;
        }
        let mut plan = RoutePlan::default();
        plan.insert("Page/index.html".into(), Route::Sitemap)
            .unwrap();
        plan.insert("page/index.html".into(), Route::Robots)
            .unwrap();
        let message = format!("{:#}", plan.reserve_on_disk(directory.path()).unwrap_err());
        assert!(message.contains("generated sitemap.xml"));
        assert!(message.contains("generated robots.txt"));
    }
}
