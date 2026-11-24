use serde::Serialize;

use crate::content::frontmatter::FeaturedImage;
use crate::pagination::PaginationVars;

/// Common metadata for every page template.
#[derive(Debug, Serialize)]
pub(crate) struct PageMetadata<'a> {
    pub(crate) title: &'a str,
    pub(crate) description: &'a str,
    pub(crate) url: &'a str,
}

/// Template variables for rendering a post page.
///
/// `date` and `updated` are pre-formatted. HTML fields (`content`, `toc`) use `| safe` to avoid
/// double-escaping. All other string fields are auto-escaped.
#[derive(Debug, Serialize)]
pub(crate) struct PostTemplateVars<'a> {
    #[serde(flatten)]
    pub(crate) metadata: PageMetadata<'a>,
    pub(crate) featured_image: Option<&'a FeaturedImage>,
    pub(crate) license: Option<&'a str>,
    pub(crate) page_css: Option<String>,
    pub(crate) date: Option<&'a str>,
    pub(crate) updated: Option<String>,
    pub(crate) tags: &'a [LinkedTerm],
    pub(crate) section: Option<&'a LinkedTerm>,
    pub(crate) content: &'a str,
    pub(crate) toc: &'a str,
}

/// A named item with a URL, used for tags and sections in post and listing templates.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LinkedTerm {
    pub(crate) name: String,
    pub(crate) url: String,
}

/// Lightweight page summary for list / taxonomy templates.
#[derive(Debug, Serialize)]
pub(crate) struct PageSummary {
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) date: Option<String>,
    /// True when frontmatter sets `weight`. Home listings place pinned posts first.
    pub(crate) pinned: bool,
    pub(crate) description: String,
    pub(crate) featured_image: Option<FeaturedImage>,
    pub(crate) tags: Vec<LinkedTerm>,
    pub(crate) section: Option<LinkedTerm>,
}

/// A group of pages sharing a common key.
#[derive(Debug, Serialize)]
pub(crate) struct PageGroup<'a> {
    pub(crate) key: String,
    pub(crate) pages: Vec<&'a PageSummary>,
}

/// Template variables for the home page.
#[derive(Debug, Serialize)]
pub(crate) struct HomePageVars<'a> {
    #[serde(flatten)]
    pub(crate) metadata: PageMetadata<'a>,
    pub(crate) pages: Vec<&'a PageSummary>,
    pub(crate) pagination: PaginationVars,
}

/// Template variables for a paginated, year-grouped archive page (`/posts/`, per-section, per-tag).
#[derive(Debug, Serialize)]
pub(crate) struct ArchivePageVars<'a> {
    #[serde(flatten)]
    pub(crate) metadata: PageMetadata<'a>,
    /// `"posts"`, `"section"` or `"tag"`.
    pub(crate) kind: &'a str,
    pub(crate) page_groups: Vec<PageGroup<'a>>,
    pub(crate) pagination: PaginationVars,
}

/// Template variables for a bucket overview page (`/sections/`, `/tags/`).
#[derive(Debug, Serialize)]
pub(crate) struct OverviewPageVars<'a> {
    #[serde(flatten)]
    pub(crate) metadata: PageMetadata<'a>,
    /// `"section"` or `"tag"`.
    pub(crate) kind: &'a str,
    pub(crate) buckets: Vec<BucketSummary<'a>>,
}

/// A bucket entry for overview pages.
///
/// Templates can use `bucket.pages | length` to get the page count.
#[derive(Debug, Serialize)]
pub(crate) struct BucketSummary<'a> {
    pub(crate) name: String,
    pub(crate) url: String,
    /// All pages in this bucket, sorted by date descending.
    pub(crate) pages: Vec<&'a PageSummary>,
}
