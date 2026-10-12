use std::cmp::Reverse;

use anyhow::{Context, Result};
use jiff::{Timestamp, tz::TimeZone};

use super::BuildContext;
use super::bucket::Taxonomy;
use crate::content::frontmatter::FeaturedImage;
use crate::content::page::Page;
use crate::render::PageResources;
use crate::template::vars::{LinkedTerm, PageGroup, PageSummary};
use crate::url::page_url;

// ── Prepared pages ──

/// A content page with the resolved metadata shared by every output generator.
#[derive(Debug)]
pub(super) struct PreparedPage<'a> {
    pub(super) page: &'a Page,
    pub(super) summary: PageSummary,
    pub(super) updated: Option<Timestamp>,
    pub(super) year: String,
}

impl PreparedPage<'_> {
    pub(super) fn published(&self) -> Option<Timestamp> {
        self.page.frontmatter.date
    }

    /// Last modification time, falling back to publication when the page was never updated.
    pub(super) fn modified(&self) -> Option<Timestamp> {
        self.updated.or(self.published())
    }

    /// Sort key placing newer pages first, with ties broken by URL.
    pub(super) fn date_order(&self) -> (Reverse<Option<Timestamp>>, &str) {
        (Reverse(self.published()), &self.summary.url)
    }
}

/// Prepares every page in input order, linking each to its section and tag buckets.
pub(super) fn prepare_pages<'a>(
    ctx: &BuildContext,
    pages: &'a [Page],
    taxonomy: &Taxonomy,
) -> Result<Vec<PreparedPage<'a>>> {
    let links: Vec<_> = taxonomy
        .terms
        .iter()
        .map(|bucket| bucket.link(&ctx.config.base_url))
        .collect();
    pages
        .iter()
        .zip(&taxonomy.page_terms)
        .map(|(page, terms)| {
            let section = terms.section.map(|index| links[index].clone());
            let tags = terms
                .tags
                .iter()
                .map(|&index| links[index].clone())
                .collect();
            prepare_page(ctx, page, section, tags)
                .with_context(|| format!("failed to prepare {}", page.source_path.display()))
        })
        .collect()
}

fn prepare_page<'a>(
    ctx: &BuildContext,
    page: &'a Page,
    section: Option<LinkedTerm>,
    tags: Vec<LinkedTerm>,
) -> Result<PreparedPage<'a>> {
    let url = page_url(&ctx.config.base_url, &page.output_path());
    let published = page.frontmatter.date;
    let updated = page
        .frontmatter
        .updated
        .or_else(|| ctx.git_info.as_ref()?.last_modified(&page.source_path));
    let featured_image = resolve_featured_image(
        page.frontmatter.featured_image.as_ref(),
        &ctx.page_resources(page, &url),
    )?;
    Ok(PreparedPage {
        page,
        summary: PageSummary {
            title: page.frontmatter.title.clone(),
            url,
            date: published.map(|date| format_page_date(date, &ctx.config.timezone)),
            pinned: page.frontmatter.weight.is_some(),
            description: page
                .frontmatter
                .description
                .clone()
                .or_else(|| page.summary.clone())
                .unwrap_or_default(),
            featured_image,
            tags,
            section,
        },
        updated,
        year: published
            .map(|date| page_year(date, &ctx.config.timezone))
            .unwrap_or_default(),
    })
}

// ── Sorting and grouping ──

/// Places pages with a frontmatter `weight` first by ascending weight, then the rest newest first.
pub(super) fn sort_pinned_first(pages: &mut [&PreparedPage]) {
    pages.sort_by_key(|&prepared| {
        let weight = prepared.page.frontmatter.weight;
        (weight.is_none(), weight, prepared.date_order())
    });
}

pub(super) fn group_by_year<'a>(pages: &[&'a PreparedPage]) -> Vec<PageGroup<'a>> {
    pages
        .chunk_by(|left, right| left.year == right.year)
        .map(|group| PageGroup {
            key: group[0].year.clone(),
            pages: group.iter().map(|page| &page.summary).collect(),
        })
        .collect()
}

/// Returns the newest modification time among `pages`.
pub(super) fn newest(pages: &[&PreparedPage]) -> Option<Timestamp> {
    pages.iter().filter_map(|page| page.modified()).max()
}

// ── Page metadata helpers ──

/// Prepares an absolute featured image URL and local image metadata.
fn resolve_featured_image(
    featured_image: Option<&FeaturedImage>,
    resources: &PageResources<'_>,
) -> Result<Option<FeaturedImage>> {
    let Some(fi) = featured_image else {
        return Ok(None);
    };
    let (published, meta) = resources.resolve_site_image(&fi.src)?;
    let resolved_src = url::Url::parse(resources.page_url)?
        .join(&published)
        .with_context(|| format!("invalid featured image URL: {}", fi.src))?
        .into();
    let mut out = FeaturedImage {
        src: resolved_src,
        ..fi.clone()
    };
    if let Some(meta) = meta {
        out.width = Some(meta.width);
        out.height = Some(meta.height);
        out.lqip_uri.clone_from(&meta.lqip_uri);
    }
    Ok(Some(out))
}

/// Formats a page date for templates in the site time zone.
pub(super) fn format_page_date(date: Timestamp, time_zone: &TimeZone) -> String {
    date.display_with_offset(time_zone.to_offset(date))
        .to_string()
}

/// Returns the grouping year for a page date in the configured site time zone.
fn page_year(date: Timestamp, time_zone: &TimeZone) -> String {
    time_zone.to_datetime(date).year().to_string()
}

#[cfg(test)]
pub(super) fn prepared_page(
    title: &str,
    date: Option<&str>,
    weight: Option<i64>,
) -> PreparedPage<'static> {
    let published = date.map(|date| date.parse().unwrap());
    let mut page = crate::test_utils::test_page(title);
    page.frontmatter.date = published;
    page.frontmatter.weight = weight;
    PreparedPage {
        page: Box::leak(Box::new(page)),
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
        updated: None,
        year: published
            .map(|date| page_year(date, &TimeZone::UTC))
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::LazyLock;

    use super::*;
    use crate::config::ImageConfig;
    use crate::content::frontmatter::ImageCredit;
    use crate::render::lqip::ImageResolver;
    use crate::static_assets::StaticAssetManifest;
    use crate::test_utils::fingerprinted_manifest;

    static EMPTY_RESOLVER: LazyLock<ImageResolver> =
        LazyLock::new(|| ImageResolver::new(Path::new(""), ImageConfig::default()));

    // ── sort_pinned_first ──

    #[test]
    fn sort_pinned_first_orders_weights_timestamps_and_tied_urls() {
        let pages = [
            prepared_page("undated", None, None),
            prepared_page("old", Some("2024-01-01T00:00:00Z"), None),
            prepared_page("negative", None, Some(-1)),
            prepared_page("z", Some("2024-11-03T01:30:00-04:00"), Some(0)),
            prepared_page("a", Some("2024-11-03T01:30:00-04:00"), Some(0)),
            prepared_page("new", Some("2024-11-03T01:15:00-05:00"), Some(0)),
            prepared_page("positive", Some("2025-01-01T00:00:00Z"), Some(1)),
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
            prepared_page("a", Some("2025-01-01T00:00:00Z"), None),
            prepared_page("b", Some("2025-06-01T00:00:00Z"), None),
            prepared_page("c", None, None),
            prepared_page("d", Some("2025-02-01T00:00:00Z"), None),
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

    // ── resolve_featured_image ──

    #[test]
    fn resolve_featured_image_resolves_site_and_page_urls() {
        let outputs: BTreeMap<_, _> = [
            "images/cover.webp",
            "blog/cover.webp",
            "posts/foo/assets/cover.webp",
            "posts/cover.webp",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let published = path.replace(".webp", &format!(".{index:012}.webp"));
            (PathBuf::from(path), PathBuf::from(published))
        })
        .collect();
        let assets = StaticAssetManifest::new(&outputs).with_prefixes("/blog/", "/blog/");
        for (source, expected) in [
            (
                "/images/cover.webp",
                "https://example.com/blog/images/cover.000000000000.webp",
            ),
            (
                "/blog/cover.webp",
                "https://example.com/blog/blog/cover.000000000001.webp",
            ),
            (
                "assets/cover.webp?q=1#view",
                "https://example.com/blog/posts/foo/assets/cover.000000000002.webp?q=1#view",
            ),
            (
                "../cover.webp",
                "https://example.com/blog/posts/cover.000000000003.webp",
            ),
            (
                "https://cdn.example.com/img.jpg",
                "https://cdn.example.com/img.jpg",
            ),
            (
                "//cdn.example.com/img.jpg",
                "https://cdn.example.com/img.jpg",
            ),
            (
                "data:image/png;base64,example",
                "data:image/png;base64,example",
            ),
        ] {
            let fi = make_featured_image(source);
            let resolved = resolve_featured_image(
                Some(&fi),
                &PageResources {
                    source_dir: None,
                    images: &EMPTY_RESOLVER,
                    assets: &assets,
                    page_url: "https://example.com/blog/posts/foo/",
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(resolved.src, expected);
        }
    }

    #[test]
    fn resolve_featured_image_preserves_metadata() {
        let fi = FeaturedImage {
            src: "https://cdn.example.com/cover.webp".into(),
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
            &PageResources {
                source_dir: None,
                images: &EMPTY_RESOLVER,
                assets: &StaticAssetManifest::default(),
                page_url: "https://example.com/posts/foo/",
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolved.src, "https://cdn.example.com/cover.webp");
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
            &PageResources {
                source_dir: Some(&bundle),
                images: &img_resolver,
                assets: &fingerprinted_manifest(dir.path(), |_| true),
                page_url: "https://example.com/posts/foo/",
            },
        )
        .unwrap()
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
                &PageResources {
                    source_dir: None,
                    images: &EMPTY_RESOLVER,
                    assets: &StaticAssetManifest::default(),
                    page_url: "https://example.com/posts/foo/",
                }
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn resolve_featured_image_missing_site_image_returns_error() {
        let err = resolve_featured_image(
            Some(&make_featured_image("assets/missing.webp")),
            &PageResources {
                source_dir: None,
                images: &EMPTY_RESOLVER,
                assets: &StaticAssetManifest::default(),
                page_url: "https://example.com/posts/foo/",
            },
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("static asset not found: assets/missing.webp"),
            "{err}"
        );
    }

    fn make_featured_image(src: &str) -> FeaturedImage {
        FeaturedImage {
            src: src.into(),
            ..Default::default()
        }
    }

    // ── page_year ──

    #[test]
    fn page_year_uses_configured_timezone() {
        let date: Timestamp = "2025-12-31T16:30:00Z".parse().unwrap();
        let time_zone = TimeZone::get("Asia/Shanghai").unwrap();
        assert_eq!(page_year(date, &time_zone), "2026");
        assert_eq!(page_year(date, &TimeZone::UTC), "2025");
    }
}
