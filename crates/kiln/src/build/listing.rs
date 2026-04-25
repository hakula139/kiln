use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use jiff::{Timestamp, tz::TimeZone};

use crate::content::frontmatter::FeaturedImage;
use crate::content::page::{Page, PageKind};
use crate::taxonomy::{TaxonomyKind, TaxonomySet};
use crate::template::vars::{LinkedTerm, PageGroup, PageSummary};
use crate::text::slugify;

use super::url::{page_url, resolve_relative_url};

// ── Listing model ──

/// Internal listing model for build-time sorting and grouping.
#[derive(Debug, Clone)]
pub(crate) struct ListedPage {
    pub(crate) summary: PageSummary,
    pub(crate) timestamp: Option<Timestamp>,
    pub(crate) weight: Option<i64>,
    pub(crate) year: String,
}

impl ListedPage {
    #[must_use]
    pub(crate) fn into_summary(self) -> PageSummary {
        self.summary
    }
}

/// Precomputed listing data for all output generators.
///
/// Built in a single pass over discovered pages. The three collections
/// are derived from the same listing pipeline, so ordering and content
/// are guaranteed consistent.
pub(crate) struct ListingArtifacts {
    /// All listable pages, indexed to match `TaxonomySet::term_pages`.
    pub(crate) listed_pages: Vec<ListedPage>,
    /// Posts only, sorted by date descending.
    pub(crate) listed_posts: Vec<ListedPage>,
    /// Posts grouped by section slug, each bucket sorted by date descending.
    pub(crate) section_posts: HashMap<String, Vec<ListedPage>>,
}

// ── Listing construction ──

/// Builds listing artifacts from discovered pages in a single pass.
///
/// Every discovered page produces exactly one `ListedPage`, maintaining
/// index alignment with the input slice (required by `TaxonomySet::term_pages`).
/// Posts are additionally collected into `listed_posts` and `section_posts`.
/// Post lists are pre-sorted by date descending.
pub(crate) fn build_listing_artifacts(
    pages: &[Page],
    content_dir: &Path,
    base_url: &str,
    time_zone: Option<&TimeZone>,
    section_titles: &HashMap<&str, &str>,
) -> Result<ListingArtifacts> {
    let mut listed_pages = Vec::with_capacity(pages.len());
    let mut listed_posts = Vec::new();
    let mut section_posts: HashMap<String, Vec<ListedPage>> = HashMap::new();

    for page in pages {
        let lp = build_listed_page(page, content_dir, base_url, time_zone, section_titles)
            .with_context(|| {
                format!(
                    "failed to build listing entry for {}",
                    page.source_path.display()
                )
            })?;

        if let PageKind::Post { section } = &page.kind {
            if let Some(slug) = section {
                section_posts
                    .entry(slug.clone())
                    .or_default()
                    .push(lp.clone());
            }
            listed_posts.push(lp.clone());
        }
        listed_pages.push(lp);
    }

    sort_by_date_desc(&mut listed_posts);
    for posts in section_posts.values_mut() {
        sort_by_date_desc(posts);
    }

    Ok(ListingArtifacts {
        listed_pages,
        listed_posts,
        section_posts,
    })
}

/// Builds a `ListedPage` from a content page.
fn build_listed_page(
    page: &Page,
    content_dir: &Path,
    base_url: &str,
    time_zone: Option<&TimeZone>,
    section_titles: &HashMap<&str, &str>,
) -> Result<ListedPage> {
    // `output_path` already includes the source and content-dir paths in
    // its error, so no extra `with_context` is needed here.
    let output_path = page.output_path(content_dir)?;
    let url = page_url(base_url, &output_path);
    let timestamp = page.frontmatter.date;
    let weight = page.frontmatter.weight;
    let section = page_section(page, base_url, section_titles);
    let featured_image = resolve_featured_image(page.frontmatter.featured_image.as_ref(), &url);

    Ok(ListedPage {
        summary: PageSummary {
            title: page.frontmatter.title.clone(),
            url,
            date: timestamp.map(|date| format_page_date(date, time_zone)),
            pinned: weight.is_some(),
            description: page
                .frontmatter
                .description
                .clone()
                .or_else(|| page.summary.clone())
                .unwrap_or_default(),
            featured_image,
            tags: linked_tags(&page.frontmatter.tags, base_url),
            section,
        },
        timestamp,
        weight,
        year: timestamp
            .map(|date| page_year(date, time_zone))
            .unwrap_or_default(),
    })
}

// ── Sorting and grouping ──

/// Sorts listed pages by date descending (newest first, undated last). The
/// canonical order for archive surfaces, taxonomy term pages, and RSS feeds.
/// Pinning is a home-page-only concept — see `sort_pinned_first`.
pub(crate) fn sort_by_date_desc(pages: &mut [ListedPage]) {
    pages.sort_by_key(|page| std::cmp::Reverse(page.timestamp));
}

/// Sorts listed pages with pinned posts first (by `weight` ascending), then
/// unpinned posts by date descending. Used only for the home page so that
/// hero pieces stay above the fold on the front door without affecting how
/// the same posts appear in archives, tag pages, or RSS feeds. Posts without
/// a `weight` frontmatter field are unpinned; any `weight` value (positive,
/// zero, or negative) marks the post as pinned, with lower values floating
/// higher inside the pinned band.
pub(crate) fn sort_pinned_first(pages: &mut [ListedPage]) {
    pages.sort_by_key(|page| {
        (
            page.weight.is_none(),
            page.weight.unwrap_or(0),
            std::cmp::Reverse(page.timestamp),
        )
    });
}

/// Resolves the listed pages for a taxonomy term, sorted by date descending.
#[must_use]
pub(crate) fn resolve_term_pages(
    taxonomy_set: &TaxonomySet,
    kind: TaxonomyKind,
    slug: &str,
    listed_pages: &[ListedPage],
) -> Vec<ListedPage> {
    let key = (kind, slug.to_owned());
    let mut pages: Vec<ListedPage> = taxonomy_set
        .term_pages
        .get(&key)
        .map(|indices| {
            indices
                .iter()
                .filter_map(|&idx| listed_pages.get(idx))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    sort_by_date_desc(&mut pages);
    pages
}

/// Groups pages into year-based sections.
///
/// Assumes pages are already sorted by date descending. Consecutive pages
/// with the same year are grouped together.
#[must_use]
pub(crate) fn group_by_year(pages: Vec<ListedPage>) -> Vec<PageGroup> {
    let mut groups: Vec<PageGroup> = Vec::new();

    for page in pages {
        let ListedPage { summary, year, .. } = page;

        match groups.last_mut() {
            Some(group) if group.key == year => group.pages.push(summary),
            _ => groups.push(PageGroup {
                key: year,
                pages: vec![summary],
            }),
        }
    }

    groups
}

/// Collects the template-facing page summaries from listed pages.
#[must_use]
pub(crate) fn collect_page_summaries<I>(listed_pages: I) -> Vec<PageSummary>
where
    I: IntoIterator<Item = ListedPage>,
{
    listed_pages
        .into_iter()
        .map(ListedPage::into_summary)
        .collect()
}

// ── Page metadata helpers ──

/// Builds a `LinkedTerm` for the page's section, if any.
#[must_use]
pub(crate) fn page_section(
    page: &Page,
    base_url: &str,
    section_titles: &HashMap<&str, &str>,
) -> Option<LinkedTerm> {
    let PageKind::Post {
        section: Some(ref slug),
    } = page.kind
    else {
        return None;
    };
    let title = section_titles
        .get(slug.as_str())
        .copied()
        .unwrap_or(slug.as_str());
    Some(LinkedTerm {
        name: title.to_owned(),
        url: format!("{base_url}/posts/{slug}/"),
    })
}

/// Resolves a `FeaturedImage`'s `src` path against the page's output URL.
#[must_use]
pub(crate) fn resolve_featured_image(
    featured_image: Option<&FeaturedImage>,
    page_url: &str,
) -> Option<FeaturedImage> {
    let fi = featured_image?;
    let resolved_src = resolve_relative_url(&fi.src, page_url);
    Some(FeaturedImage {
        src: resolved_src,
        ..fi.clone()
    })
}

/// Converts raw tag strings into `LinkedTerm`s with pre-computed URLs.
fn linked_tags(tags: &[String], base_url: &str) -> Vec<LinkedTerm> {
    tags.iter()
        .map(|tag| LinkedTerm {
            name: tag.clone(),
            url: format!("{base_url}/tags/{}/", slugify(tag)),
        })
        .collect()
}

/// Formats a page date for templates using the configured site time zone,
/// falling back to UTC when no site time zone is set.
#[must_use]
pub(crate) fn format_page_date(date: Timestamp, time_zone: Option<&TimeZone>) -> String {
    let Some(time_zone) = time_zone else {
        return date.to_string();
    };
    let zoned = date.to_zoned(time_zone.clone());
    date.display_with_offset(zoned.offset()).to_string()
}

/// Returns the grouping year for a page date in the configured site time zone.
#[must_use]
pub(crate) fn page_year(date: Timestamp, time_zone: Option<&TimeZone>) -> String {
    date.to_zoned(time_zone.cloned().unwrap_or(TimeZone::UTC))
        .year()
        .to_string()
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::*;
    use crate::content::frontmatter::ImageCredit;

    fn make_listed_page(title: &str, date: Option<&str>) -> ListedPage {
        make_listed_page_with(title, date, None)
    }

    fn make_listed_page_with(title: &str, date: Option<&str>, weight: Option<i64>) -> ListedPage {
        let timestamp = date.map(|date| date.parse().unwrap());
        ListedPage {
            summary: PageSummary {
                title: title.into(),
                url: format!("/{title}/"),
                date: timestamp.map(|date: Timestamp| date.to_string()),
                pinned: weight.is_some(),
                description: String::new(),
                featured_image: None,
                tags: Vec::new(),
                section: None,
            },
            timestamp,
            weight,
            year: timestamp
                .map(|date| page_year(date, None))
                .unwrap_or_default(),
        }
    }

    // ── sort_by_date_desc ──

    #[test]
    fn sort_by_date_desc_basic() {
        let mut pages = vec![
            make_listed_page("old", Some("2025-01-01T00:00:00Z")),
            make_listed_page("new", Some("2026-06-15T00:00:00Z")),
            make_listed_page("mid", Some("2026-01-01T00:00:00Z")),
        ];
        sort_by_date_desc(&mut pages);
        assert_eq!(pages[0].summary.title, "new");
        assert_eq!(pages[1].summary.title, "mid");
        assert_eq!(pages[2].summary.title, "old");
    }

    #[test]
    fn sort_by_date_desc_undated_last() {
        let mut pages = vec![
            make_listed_page("undated", None),
            make_listed_page("dated", Some("2026-01-01T00:00:00Z")),
        ];
        sort_by_date_desc(&mut pages);
        assert_eq!(pages[0].summary.title, "dated");
        assert_eq!(pages[1].summary.title, "undated");
    }

    #[test]
    fn sort_by_date_desc_uses_timestamp_not_rendered_string() {
        let mut pages = vec![
            make_listed_page("older", Some("2024-11-03T01:30:00-04:00")),
            make_listed_page("newer", Some("2024-11-03T01:15:00-05:00")),
        ];
        sort_by_date_desc(&mut pages);
        assert_eq!(pages[0].summary.title, "newer");
        assert_eq!(pages[1].summary.title, "older");
    }

    #[test]
    fn sort_by_date_desc_ignores_weight() {
        let mut pages = vec![
            make_listed_page_with("pinned-old", Some("2020-01-01T00:00:00Z"), Some(1)),
            make_listed_page_with("recent", Some("2026-06-01T00:00:00Z"), None),
        ];
        sort_by_date_desc(&mut pages);
        assert_eq!(pages[0].summary.title, "recent");
        assert_eq!(pages[1].summary.title, "pinned-old");
    }

    // ── sort_pinned_first ──

    #[test]
    fn sort_pinned_first_falls_back_to_date_desc_when_no_pins() {
        let mut pages = vec![
            make_listed_page("old", Some("2025-01-01T00:00:00Z")),
            make_listed_page("new", Some("2026-06-15T00:00:00Z")),
        ];
        sort_pinned_first(&mut pages);
        assert_eq!(pages[0].summary.title, "new");
        assert_eq!(pages[1].summary.title, "old");
    }

    #[test]
    fn sort_pinned_first_pinned_come_before_unpinned() {
        let mut pages = vec![
            make_listed_page_with("recent", Some("2026-06-01T00:00:00Z"), None),
            make_listed_page_with("pinned-old", Some("2020-01-01T00:00:00Z"), Some(1)),
        ];
        sort_pinned_first(&mut pages);
        assert_eq!(pages[0].summary.title, "pinned-old");
        assert!(pages[0].summary.pinned);
        assert_eq!(pages[1].summary.title, "recent");
        assert!(!pages[1].summary.pinned);
    }

    #[test]
    fn sort_pinned_first_pinned_ordered_by_weight_ascending() {
        let mut pages = vec![
            make_listed_page_with("third", Some("2026-01-01T00:00:00Z"), Some(3)),
            make_listed_page_with("first", Some("2026-01-01T00:00:00Z"), Some(1)),
            make_listed_page_with("second", Some("2026-01-01T00:00:00Z"), Some(2)),
        ];
        sort_pinned_first(&mut pages);
        assert_eq!(pages[0].summary.title, "first");
        assert_eq!(pages[1].summary.title, "second");
        assert_eq!(pages[2].summary.title, "third");
    }

    #[test]
    fn sort_pinned_first_negative_weight_sorts_above_positive() {
        let mut pages = vec![
            make_listed_page_with("positive", Some("2026-01-01T00:00:00Z"), Some(1)),
            make_listed_page_with("negative", Some("2025-01-01T00:00:00Z"), Some(-5)),
        ];
        sort_pinned_first(&mut pages);
        assert_eq!(pages[0].summary.title, "negative");
        assert_eq!(pages[1].summary.title, "positive");
    }

    #[test]
    fn sort_pinned_first_pinned_ties_break_by_date_desc() {
        let mut pages = vec![
            make_listed_page_with("pin-old", Some("2025-01-01T00:00:00Z"), Some(1)),
            make_listed_page_with("pin-new", Some("2026-01-01T00:00:00Z"), Some(1)),
        ];
        sort_pinned_first(&mut pages);
        assert_eq!(pages[0].summary.title, "pin-new");
        assert_eq!(pages[1].summary.title, "pin-old");
    }

    #[test]
    fn sort_pinned_first_mixed_pinned_and_unpinned_full_order() {
        let mut pages = vec![
            make_listed_page_with("unpin-old", Some("2024-01-01T00:00:00Z"), None),
            make_listed_page_with("pin-2", Some("2020-01-01T00:00:00Z"), Some(2)),
            make_listed_page_with("unpin-new", Some("2026-01-01T00:00:00Z"), None),
            make_listed_page_with("pin-1", Some("2018-01-01T00:00:00Z"), Some(1)),
        ];
        sort_pinned_first(&mut pages);
        let order: Vec<&str> = pages.iter().map(|p| p.summary.title.as_str()).collect();
        assert_eq!(order, ["pin-1", "pin-2", "unpin-new", "unpin-old"]);
    }

    #[test]
    fn sort_pinned_first_zero_weight_is_pinned() {
        let mut pages = vec![
            make_listed_page_with("recent", Some("2026-06-01T00:00:00Z"), None),
            make_listed_page_with("pin-zero", Some("2020-01-01T00:00:00Z"), Some(0)),
        ];
        sort_pinned_first(&mut pages);
        assert_eq!(pages[0].summary.title, "pin-zero");
        assert!(pages[0].summary.pinned);
    }

    // ── group_by_year ──

    #[test]
    fn group_by_year_basic() {
        let pages = vec![
            make_listed_page("a", Some("2026-03-01T00:00:00Z")),
            make_listed_page("b", Some("2026-01-15T00:00:00Z")),
            make_listed_page("c", Some("2025-12-01T00:00:00Z")),
        ];
        let groups = group_by_year(pages);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "2026");
        assert_eq!(groups[0].pages.len(), 2);
        assert_eq!(groups[1].key, "2025");
        assert_eq!(groups[1].pages.len(), 1);
    }

    #[test]
    fn group_by_year_undated_pages() {
        let pages = vec![
            make_listed_page("a", Some("2026-01-01T00:00:00Z")),
            make_listed_page("b", None),
        ];
        let groups = group_by_year(pages);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "2026");
        assert_eq!(groups[1].key, "", "undated pages should have empty key");
    }

    #[test]
    fn group_by_year_non_consecutive_same_year() {
        let pages = vec![
            make_listed_page("a", Some("2026-03-01T00:00:00Z")),
            make_listed_page("b", Some("2025-06-01T00:00:00Z")),
            make_listed_page("c", Some("2026-01-01T00:00:00Z")),
        ];
        let groups = group_by_year(pages);
        assert_eq!(groups.len(), 3, "groups consecutively, not globally");
        assert_eq!(groups[0].key, "2026");
        assert_eq!(groups[0].pages.len(), 1);
        assert_eq!(groups[1].key, "2025");
        assert_eq!(groups[1].pages.len(), 1);
        assert_eq!(groups[2].key, "2026");
        assert_eq!(groups[2].pages.len(), 1);
    }

    #[test]
    fn group_by_year_empty() {
        let groups = group_by_year(Vec::new());
        assert!(groups.is_empty());
    }

    // ── resolve_featured_image ──

    fn make_featured_image(src: &str) -> FeaturedImage {
        FeaturedImage {
            src: src.into(),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_featured_image_absolute_path() {
        let fi = make_featured_image("/images/cover.webp");
        let resolved = resolve_featured_image(Some(&fi), "https://example.com/posts/foo/").unwrap();
        assert_eq!(resolved.src, "/images/cover.webp");
    }

    #[test]
    fn resolve_featured_image_relative_path() {
        let fi = make_featured_image("assets/cover.webp");
        let resolved =
            resolve_featured_image(Some(&fi), "https://example.com/posts/avg/on-looker/").unwrap();
        assert_eq!(resolved.src, "/posts/avg/on-looker/assets/cover.webp");
    }

    #[test]
    fn resolve_featured_image_external_url() {
        let fi = make_featured_image("https://cdn.example.com/img.jpg");
        let resolved = resolve_featured_image(Some(&fi), "https://example.com/posts/foo/").unwrap();
        assert_eq!(resolved.src, "https://cdn.example.com/img.jpg");
    }

    #[test]
    fn resolve_featured_image_preserves_metadata() {
        let fi = FeaturedImage {
            src: "/images/cover.webp".into(),
            position: Some("top".into()),
            credit: Some(ImageCredit {
                title: Some("Work".into()),
                author: Some("Artist".into()),
                url: Some("https://example.com".into()),
            }),
        };
        let resolved = resolve_featured_image(Some(&fi), "https://example.com/posts/foo/").unwrap();
        assert_eq!(resolved.src, "/images/cover.webp");
        assert_eq!(resolved.position.as_deref(), Some("top"));
        let credit = resolved.credit.as_ref().unwrap();
        assert_eq!(credit.title.as_deref(), Some("Work"));
        assert_eq!(credit.author.as_deref(), Some("Artist"));
        assert_eq!(credit.url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn resolve_featured_image_none() {
        assert!(resolve_featured_image(None, "https://example.com/posts/foo/").is_none());
    }

    // ── page_year ──

    #[test]
    fn page_year_uses_configured_timezone() {
        let date: Timestamp = "2025-12-31T16:30:00Z".parse().unwrap();
        let time_zone = jiff::tz::TimeZone::get("Asia/Shanghai").unwrap();
        assert_eq!(page_year(date, Some(&time_zone)), "2026");
        assert_eq!(page_year(date, None), "2025");
    }
}
