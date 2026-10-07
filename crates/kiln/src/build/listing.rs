use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use jiff::{Timestamp, tz::TimeZone};
use strum::{EnumIter, IntoStaticStr};

use super::BuildContext;
use super::git::updated_timestamp;
use crate::content::frontmatter::FeaturedImage;
use crate::content::page::{Page, PageKind};
use crate::render::lqip::ImageResolver;
use crate::section::Section;
use crate::taxonomy::TaxonomySet;
use crate::template::vars::{BucketSummary, LinkedTerm, PageGroup, PageSummary};
use crate::url::{encode_component, join_site_url, page_url, resolve_relative_url};

// ── Prepared pages ──

/// Resolved content metadata shared by every output generator.
#[derive(Debug)]
pub(super) struct PreparedPage {
    pub(super) output_path: PathBuf,
    pub(super) summary: PageSummary,
    pub(super) published: Option<Timestamp>,
    pub(super) updated: Option<Timestamp>,
    pub(super) weight: Option<i64>,
    pub(super) year: String,
}

pub(super) struct ListingArtifacts {
    pub(super) pages: Vec<PreparedPage>,
    post_indices: Vec<usize>,
    section_posts: HashMap<String, Vec<usize>>,
    tag_pages: HashMap<String, Vec<usize>>,
}

impl ListingArtifacts {
    pub(super) fn posts(&self) -> Vec<&PreparedPage> {
        self.select(&self.post_indices)
    }

    fn select(&self, indices: &[usize]) -> Vec<&PreparedPage> {
        indices.iter().map(|&index| &self.pages[index]).collect()
    }
}

pub(super) fn build_listing_artifacts(
    ctx: &BuildContext,
    pages: &[Page],
    content_dir: &Path,
    sections: &[Section],
    taxonomy_set: &TaxonomySet,
) -> Result<ListingArtifacts> {
    let mut prepared = Vec::with_capacity(pages.len());
    let mut post_indices = Vec::new();
    let mut section_posts: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, page) in pages.iter().enumerate() {
        let tags = linked_tags(taxonomy_set, index, &ctx.config.base_url);
        prepared.push(
            prepare_page(ctx, page, content_dir, sections, tags)
                .with_context(|| format!("failed to prepare {}", page.source_path.display()))?,
        );
        if let PageKind::Post { section } = &page.kind {
            post_indices.push(index);
            if let Some(slug) = section {
                section_posts.entry(slug.clone()).or_default().push(index);
            }
        }
    }
    let mut tag_pages = taxonomy_set.tag_pages.clone();
    for indices in std::iter::once(&mut post_indices)
        .chain(section_posts.values_mut())
        .chain(tag_pages.values_mut())
    {
        indices.sort_by(|&left, &right| {
            let left = &prepared[left];
            let right = &prepared[right];
            right
                .published
                .cmp(&left.published)
                .then(left.summary.url.cmp(&right.summary.url))
        });
    }
    Ok(ListingArtifacts {
        pages: prepared,
        post_indices,
        section_posts,
        tag_pages,
    })
}

fn prepare_page(
    ctx: &BuildContext,
    page: &Page,
    content_dir: &Path,
    sections: &[Section],
    tags: Vec<LinkedTerm>,
) -> Result<PreparedPage> {
    let output_path = page.output_path(content_dir)?;
    let url = page_url(&ctx.config.base_url, &output_path);
    let published = page.frontmatter.date;
    let updated = updated_timestamp(
        page.frontmatter.updated,
        &page.source_path,
        ctx.git_info.as_ref(),
    );
    let featured_image = resolve_featured_image(
        page.frontmatter.featured_image.as_ref(),
        &url,
        &ctx.image_resolver,
        page.source_path.parent(),
    );
    Ok(PreparedPage {
        output_path,
        summary: PageSummary {
            title: page.frontmatter.title.clone(),
            url,
            date: published.map(|date| format_page_date(date, ctx.time_zone.as_ref())),
            pinned: page.frontmatter.weight.is_some(),
            description: page
                .frontmatter
                .description
                .clone()
                .or_else(|| page.summary.clone())
                .unwrap_or_default(),
            featured_image,
            tags,
            section: page_section(page, &ctx.config.base_url, sections),
        },
        published,
        updated,
        weight: page.frontmatter.weight,
        year: published
            .map(|date| page_year(date, ctx.time_zone.as_ref()))
            .unwrap_or_default(),
    })
}

// ── Listing buckets ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub(super) enum BucketKind {
    #[strum(serialize = "post")]
    Posts,
    Section,
    Tag,
}

impl BucketKind {
    pub(super) fn plural(self) -> String {
        format!("{}s", self.singular())
    }

    pub(super) fn singular(self) -> &'static str {
        self.into()
    }

    pub(super) fn has_overview(self) -> bool {
        matches!(self, Self::Section | Self::Tag)
    }
}

#[derive(Debug)]
pub(super) struct ListingBucket<'a> {
    pub(super) kind: BucketKind,
    pub(super) name: String,
    pub(super) slug: String,
    pub(super) pages: Vec<&'a PreparedPage>,
}

impl<'a> ListingBucket<'a> {
    pub(super) fn base_path(&self) -> PathBuf {
        match self.kind {
            BucketKind::Posts => PathBuf::from("posts"),
            BucketKind::Section => Path::new("posts").join(&self.slug),
            BucketKind::Tag => Path::new("tags").join(&self.slug),
        }
    }

    pub(super) fn summary(&self, base_url: &str) -> BucketSummary<'a> {
        BucketSummary {
            name: self.name.clone(),
            slug: self.slug.clone(),
            url: page_url(base_url, &self.base_path().join("index.html")),
            pages: self.pages.iter().map(|page| &page.summary).collect(),
        }
    }
}

pub(super) fn build_listing_buckets<'a>(
    artifacts: &'a ListingArtifacts,
    sections: &[Section],
    taxonomy_set: &TaxonomySet,
    posts_title: String,
) -> Vec<ListingBucket<'a>> {
    let mut buckets = vec![ListingBucket {
        kind: BucketKind::Posts,
        name: posts_title,
        slug: "posts".into(),
        pages: artifacts.posts(),
    }];
    buckets.extend(sections.iter().map(|section| ListingBucket {
        kind: BucketKind::Section,
        name: section.title.clone(),
        slug: section.slug.clone(),
        pages: artifacts.select(&artifacts.section_posts[&section.slug]),
    }));
    buckets.extend(taxonomy_set.tags.iter().map(|term| ListingBucket {
        kind: BucketKind::Tag,
        name: term.name.clone(),
        slug: term.slug.clone(),
        pages: artifacts.select(&artifacts.tag_pages[&term.slug]),
    }));
    buckets
}

// ── Sorting and grouping ──

pub(super) fn sort_pinned_first(pages: &mut [&PreparedPage]) {
    pages.sort_by(|left, right| {
        (
            left.weight.is_none(),
            left.weight,
            std::cmp::Reverse(left.published),
            &left.summary.url,
        )
            .cmp(&(
                right.weight.is_none(),
                right.weight,
                std::cmp::Reverse(right.published),
                &right.summary.url,
            ))
    });
}

pub(super) fn group_by_year<'a>(pages: &[&'a PreparedPage]) -> Vec<PageGroup<'a>> {
    let mut groups: Vec<PageGroup<'a>> = Vec::new();
    for page in pages {
        match groups.last_mut() {
            Some(group) if group.key == page.year => group.pages.push(&page.summary),
            _ => groups.push(PageGroup {
                key: page.year.clone(),
                pages: vec![&page.summary],
            }),
        }
    }
    groups
}

// ── Page metadata helpers ──

#[must_use]
pub(super) fn page_section(
    page: &Page,
    base_url: &str,
    sections: &[Section],
) -> Option<LinkedTerm> {
    let PageKind::Post {
        section: Some(ref slug),
    } = page.kind
    else {
        return None;
    };
    let title = sections
        .iter()
        .find(|s| &s.slug == slug)
        .map_or(slug.as_str(), |s| s.title.as_str());
    Some(LinkedTerm {
        name: title.to_owned(),
        url: join_site_url(base_url, &format!("posts/{}/", encode_component(slug))),
    })
}

/// Resolves a `FeaturedImage`'s `src` path against the page's output URL and stamps on
/// dimensions plus an LQIP placeholder when the image is local and decodable.
#[must_use]
pub(super) fn resolve_featured_image(
    featured_image: Option<&FeaturedImage>,
    page_url: &str,
    image_resolver: &ImageResolver,
    base_dir: Option<&Path>,
) -> Option<FeaturedImage> {
    let fi = featured_image?;
    let resolved_src = resolve_relative_url(&fi.src, page_url);
    let mut out = FeaturedImage {
        src: resolved_src,
        ..fi.clone()
    };
    if let Some(meta) = image_resolver.resolve(&fi.src, base_dir) {
        out.width = Some(meta.width);
        out.height = Some(meta.height);
        out.lqip_uri.clone_from(&meta.lqip_uri);
    }
    Some(out)
}

fn linked_tags(taxonomy: &TaxonomySet, page_index: usize, base_url: &str) -> Vec<LinkedTerm> {
    taxonomy.page_tags[page_index]
        .iter()
        .map(|&index| {
            let term = &taxonomy.tags[index];
            LinkedTerm {
                name: term.name.clone(),
                url: join_site_url(base_url, &format!("tags/{}/", encode_component(&term.slug))),
            }
        })
        .collect()
}

/// Formats a page date for templates using the configured site time zone (falls back to UTC).
#[must_use]
pub(super) fn format_page_date(date: Timestamp, time_zone: Option<&TimeZone>) -> String {
    let Some(time_zone) = time_zone else {
        return date.to_string();
    };
    let zoned = date.to_zoned(time_zone.clone());
    date.display_with_offset(zoned.offset()).to_string()
}

/// Returns the grouping year for a page date in the configured site time zone.
#[must_use]
pub(super) fn page_year(date: Timestamp, time_zone: Option<&TimeZone>) -> String {
    date.to_zoned(time_zone.cloned().unwrap_or(TimeZone::UTC))
        .year()
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::content::frontmatter::ImageCredit;
    use crate::render::lqip::ImageConfig;

    static EMPTY_RESOLVER: LazyLock<ImageResolver> =
        LazyLock::new(|| ImageResolver::new(Path::new(""), ImageConfig::default()));

    fn prepared(title: &str, date: Option<&str>, weight: Option<i64>) -> PreparedPage {
        let published = date.map(|date| date.parse().unwrap());
        PreparedPage {
            output_path: PathBuf::from(title).join("index.html"),
            summary: PageSummary {
                title: title.into(),
                url: format!("/{title}/"),
                date: date.map(str::to_owned),
                pinned: weight.is_some(),
                description: String::new(),
                featured_image: None,
                tags: Vec::new(),
                section: None,
            },
            published,
            updated: None,
            weight,
            year: published
                .map(|date| page_year(date, None))
                .unwrap_or_default(),
        }
    }

    // ── sort_pinned_first ──

    #[test]
    fn sort_pinned_first_orders_weights_timestamps_and_tied_urls() {
        let pages = [
            prepared("undated", None, None),
            prepared("old", Some("2024-01-01T00:00:00Z"), None),
            prepared("negative", None, Some(-1)),
            prepared("z", Some("2024-11-03T01:30:00-04:00"), Some(0)),
            prepared("a", Some("2024-11-03T01:30:00-04:00"), Some(0)),
            prepared("new", Some("2024-11-03T01:15:00-05:00"), Some(0)),
            prepared("positive", Some("2025-01-01T00:00:00Z"), Some(1)),
        ];
        let mut sorted: Vec<_> = pages.iter().collect();
        sort_pinned_first(&mut sorted);
        assert_eq!(
            sorted
                .iter()
                .map(|page| page.summary.title.as_str())
                .collect::<Vec<_>>(),
            ["negative", "new", "a", "z", "positive", "old", "undated"]
        );
    }

    // ── group_by_year ──

    #[test]
    fn group_by_year_preserves_members_and_separates_consecutive_groups() {
        let pages = [
            prepared("a", Some("2025-01-01T00:00:00Z"), None),
            prepared("b", Some("2025-06-01T00:00:00Z"), None),
            prepared("c", None, None),
            prepared("d", Some("2025-02-01T00:00:00Z"), None),
        ];
        let refs: Vec<_> = pages.iter().collect();
        let grouped = group_by_year(&refs);
        assert_eq!(
            grouped
                .iter()
                .map(|group| (
                    group.key.as_str(),
                    group
                        .pages
                        .iter()
                        .map(|page| page.title.as_str())
                        .collect::<Vec<_>>()
                ))
                .collect::<Vec<_>>(),
            [
                ("2025", vec!["a", "b"]),
                ("", vec!["c"]),
                ("2025", vec!["d"])
            ]
        );
        assert!(std::ptr::eq(
            grouped[0].pages[0],
            &raw const pages[0].summary
        ));
        assert!(group_by_year(&[]).is_empty());
    }

    // ── page_section ──

    #[test]
    fn page_section_base_url_trailing_slashes() {
        let mut page = crate::test_utils::test_page("Post A");
        page.kind = PageKind::Post {
            section: Some("notes".into()),
        };
        let sections = [Section {
            slug: "notes".into(),
            title: "Notes".into(),
        }];

        for base_url in ["https://example.com/blog", "https://example.com/blog/"] {
            let section = page_section(&page, base_url, &sections).unwrap();
            assert_eq!(section.name, "Notes");
            assert_eq!(section.url, "https://example.com/blog/posts/notes/");
        }
    }

    // ── resolve_featured_image ──

    #[test]
    fn resolve_featured_image_absolute_path() {
        let fi = make_featured_image("/images/cover.webp");
        let resolved = resolve_featured_image(
            Some(&fi),
            "https://example.com/posts/foo/",
            &EMPTY_RESOLVER,
            None,
        )
        .unwrap();
        assert_eq!(resolved.src, "/images/cover.webp");
    }

    #[test]
    fn resolve_featured_image_relative_path() {
        let fi = make_featured_image("assets/cover.webp");
        let resolved = resolve_featured_image(
            Some(&fi),
            "https://example.com/posts/section/page/",
            &EMPTY_RESOLVER,
            None,
        )
        .unwrap();
        assert_eq!(resolved.src, "/posts/section/page/assets/cover.webp");
    }

    #[test]
    fn resolve_featured_image_external_url() {
        let fi = make_featured_image("https://cdn.example.com/img.jpg");
        let resolved = resolve_featured_image(
            Some(&fi),
            "https://example.com/posts/foo/",
            &EMPTY_RESOLVER,
            None,
        )
        .unwrap();
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
            ..FeaturedImage::default()
        };
        let resolved = resolve_featured_image(
            Some(&fi),
            "https://example.com/posts/foo/",
            &EMPTY_RESOLVER,
            None,
        )
        .unwrap();
        assert_eq!(resolved.src, "/images/cover.webp");
        assert_eq!(resolved.position.as_deref(), Some("top"));
        let credit = resolved.credit.as_ref().unwrap();
        assert_eq!(credit.title.as_deref(), Some("Work"));
        assert_eq!(credit.author.as_deref(), Some("Artist"));
        assert_eq!(credit.url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn resolve_featured_image_stamps_dimensions_and_lqip() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("posts/foo");
        fs::create_dir_all(&bundle).unwrap();

        let img = image::RgbaImage::from_pixel(4, 2, image::Rgba([200, 100, 50, 255]));
        img.save_with_format(bundle.join("cover.png"), image::ImageFormat::Png)
            .unwrap();

        let img_resolver = ImageResolver::new(dir.path(), ImageConfig::default());
        let fi = make_featured_image("cover.png");
        let stamped = resolve_featured_image(
            Some(&fi),
            "https://example.com/posts/foo/",
            &img_resolver,
            Some(&bundle),
        )
        .unwrap();

        assert_eq!(stamped.width, Some(4));
        assert_eq!(stamped.height, Some(2));
        assert!(
            stamped
                .lqip_uri
                .as_deref()
                .is_some_and(|u| u.starts_with("data:image/webp;base64,"))
        );
    }

    #[test]
    fn resolve_featured_image_absent_returns_none() {
        assert!(
            resolve_featured_image(
                None,
                "https://example.com/posts/foo/",
                &EMPTY_RESOLVER,
                None
            )
            .is_none()
        );
    }

    fn make_featured_image(src: &str) -> FeaturedImage {
        FeaturedImage {
            src: src.into(),
            ..Default::default()
        }
    }

    // ── linked_tags ──

    #[test]
    fn linked_tags_base_url_trailing_slashes() {
        let tags = ["Rust".into(), "Web Tools".into()];
        for base_url in ["https://example.com/blog", "https://example.com/blog/"] {
            let mut page = crate::test_utils::test_page("Example");
            page.frontmatter.tags = tags.to_vec();
            let taxonomy = crate::taxonomy::build_taxonomies(&[page], None).unwrap();
            let linked = linked_tags(&taxonomy, 0, base_url);
            assert_eq!(linked[0].name, "Rust");
            assert_eq!(linked[0].url, "https://example.com/blog/tags/rust/");
            assert_eq!(linked[1].name, "Web Tools");
            assert_eq!(linked[1].url, "https://example.com/blog/tags/web-tools/");
        }
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
