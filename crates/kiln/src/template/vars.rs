use serde::Serialize;

use crate::config::Config;
use crate::content::frontmatter::FeaturedImage;
use crate::pagination::PaginationVars;

/// Template variables for rendering a post page.
///
/// The `date` field is pre-formatted as a string so the template doesn't need
/// date logic. HTML fields (`content`, `toc`) use `| safe` in the template to
/// avoid double-escaping. All other string fields are auto-escaped by `MiniJinja`.
#[derive(Debug, Serialize)]
pub struct PostTemplateVars<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub url: &'a str,
    pub featured_image: Option<FeaturedImage>,
    pub date: Option<String>,
    pub section: Option<LinkedTerm>,
    pub math: bool,
    pub content: &'a str,
    pub toc: &'a str,
    pub config: &'a Config,
}

/// A named item with a URL, used for tags and sections in page summaries.
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
    pub description: String,
    pub featured_image: Option<FeaturedImage>,
    pub tags: Vec<LinkedTerm>,
    pub section: Option<LinkedTerm>,
}

/// A group of pages sharing a common key (e.g., year).
#[derive(Debug, Clone, Serialize)]
pub struct PageGroup {
    pub key: String,
    pub pages: Vec<PageSummary>,
}

/// Template variables for the home page.
#[derive(Debug, Serialize)]
pub struct HomePageVars<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub url: String,
    pub pages: Vec<PageSummary>,
    pub pagination: PaginationVars,
    pub config: &'a Config,
}

/// Template variables for a paginated, year-grouped archive page.
///
/// Used for the posts index (`/posts/`), per-section archives
/// (`/posts/<slug>/`), and per-tag archives (`/tags/<slug>/`).
#[derive(Debug, Serialize)]
pub struct ArchivePageVars<'a> {
    pub kind: &'a str,
    pub singular: &'a str,
    pub name: &'a str,
    pub slug: &'a str,
    pub page_groups: Vec<PageGroup>,
    pub pagination: PaginationVars,
    pub config: &'a Config,
}

/// Template variables for a bucket overview page (e.g., `/tags/`, `/sections/`).
#[derive(Debug, Serialize)]
pub struct OverviewPageVars<'a> {
    pub kind: &'a str,
    pub singular: &'a str,
    pub buckets: Vec<BucketSummary>,
    pub config: &'a Config,
}

/// A bucket entry for overview pages.
///
/// Templates can use `bucket.pages | length` to get the page count.
#[derive(Debug, Clone, Serialize)]
pub struct BucketSummary {
    pub name: String,
    pub slug: String,
    pub url: String,
    /// All pages in this bucket, sorted by date descending.
    pub pages: Vec<PageSummary>,
}

/// Template variables for the 404 error page.
#[derive(Debug, Serialize)]
pub struct ErrorPageVars<'a> {
    pub title: &'a str,
    pub config: &'a Config,
}
