use std::borrow::Cow;

use serde::Serialize;

use crate::config::Config;
use crate::content::frontmatter::FeaturedImage;
use crate::pagination::PaginationVars;
use crate::render::assets::PageAssets;

/// Common metadata for content and listing templates.
#[derive(Debug, Serialize)]
pub struct PageMetadata<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub url: Cow<'a, str>,
}

/// Template variables for rendering a post page.
///
/// `date` and `updated` are pre-formatted. HTML fields (`content`, `toc`) use `| safe` to avoid
/// double-escaping. All other string fields are auto-escaped by `MiniJinja`.
#[derive(Debug, Serialize)]
pub struct PostTemplateVars<'a> {
    #[serde(flatten)]
    pub metadata: PageMetadata<'a>,
    pub featured_image: Option<&'a FeaturedImage>,
    pub license: Option<&'a str>,
    pub page_css: Option<String>,
    pub date: Option<&'a str>,
    pub updated: Option<String>,
    pub tags: &'a [LinkedTerm],
    pub section: Option<&'a LinkedTerm>,
    /// Auto-detected runtime dependencies (math, mermaid, registered scripts). Themes iterate
    /// `assets.features` and `assets.scripts` to load the right CSS / JS.
    pub assets: PageAssets,
    pub content: &'a str,
    pub toc: &'a str,
    pub config: &'a Config,
}

/// A named item with a URL, used for tags and sections in post and listing templates.
#[derive(Debug, Clone, Serialize)]
pub struct LinkedTerm {
    pub name: String,
    pub url: String,
}

/// Lightweight page summary for list / taxonomy templates.
#[derive(Debug, Clone, Serialize)]
pub struct PageSummary {
    pub title: String,
    pub url: String,
    pub date: Option<String>,
    /// True when frontmatter sets `weight`. Home listings place pinned posts first.
    pub pinned: bool,
    pub description: String,
    pub featured_image: Option<FeaturedImage>,
    pub tags: Vec<LinkedTerm>,
    pub section: Option<LinkedTerm>,
}

/// A group of pages sharing a common key (e.g., year).
#[derive(Debug, Clone, Serialize)]
pub struct PageGroup<'a> {
    pub key: String,
    pub pages: Vec<&'a PageSummary>,
}

/// Template variables for the home page.
#[derive(Debug, Serialize)]
pub struct HomePageVars<'a> {
    #[serde(flatten)]
    pub metadata: PageMetadata<'a>,
    pub pages: Vec<&'a PageSummary>,
    pub pagination: PaginationVars,
    pub config: &'a Config,
}

/// Template variables for a paginated, year-grouped archive page (`/posts/`, per-section, per-tag).
#[derive(Debug, Serialize)]
pub struct ArchivePageVars<'a> {
    #[serde(flatten)]
    pub metadata: PageMetadata<'a>,
    pub kind: &'a str,
    pub singular: &'a str,
    pub name: &'a str,
    pub slug: &'a str,
    pub page_groups: Vec<PageGroup<'a>>,
    pub pagination: PaginationVars,
    pub config: &'a Config,
}

/// Template variables for a bucket overview page (e.g., `/tags/`, `/sections/`).
#[derive(Debug, Serialize)]
pub struct OverviewPageVars<'a> {
    #[serde(flatten)]
    pub metadata: PageMetadata<'a>,
    pub kind: &'a str,
    pub singular: &'a str,
    pub buckets: Vec<BucketSummary<'a>>,
    pub config: &'a Config,
}

/// A bucket entry for overview pages.
///
/// Templates can use `bucket.pages | length` to get the page count.
#[derive(Debug, Clone, Serialize)]
pub struct BucketSummary<'a> {
    pub name: String,
    pub slug: String,
    pub url: String,
    /// All pages in this bucket, sorted by date descending.
    pub pages: Vec<&'a PageSummary>,
}

/// Template variables for the 404 error page.
#[derive(Debug, Serialize)]
pub struct ErrorPageVars<'a> {
    pub title: &'a str,
    pub config: &'a Config,
}
