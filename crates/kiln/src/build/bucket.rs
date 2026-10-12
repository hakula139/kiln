use std::collections::{BTreeMap, HashMap};
use std::iter;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail, ensure};
use strum::{AsRefStr, EnumIter};

use super::listing::PreparedPage;
use crate::config::Paginate;
use crate::content::index::load_index_title;
use crate::content::page::{Page, PageKind};
use crate::template::vars::{BucketSummary, LinkedTerm};
use crate::text::{slugify, titlecase};
use crate::url::{BaseUrl, directory_url};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, AsRefStr, EnumIter)]
#[strum(serialize_all = "lowercase")]
pub(super) enum BucketKind {
    Posts,
    Section,
    Tag,
}

impl BucketKind {
    pub(super) fn plural(self) -> &'static str {
        match self {
            Self::Posts => "posts",
            Self::Section => "sections",
            Self::Tag => "tags",
        }
    }

    pub(super) fn has_overview(self) -> bool {
        matches!(self, Self::Section | Self::Tag)
    }

    /// Archive directory of the bucket `slug` relative to the output root, which is also where
    /// its `_index.md` lives under `content/`.
    pub(super) fn path(self, slug: &str) -> PathBuf {
        match self {
            Self::Posts => PathBuf::from("posts"),
            Self::Section => Path::new("posts").join(slug),
            Self::Tag => Path::new("tags").join(slug),
        }
    }

    pub(super) fn page_size(self, paginate: &Paginate) -> NonZeroUsize {
        match self {
            Self::Posts | Self::Section => paginate.section,
            Self::Tag => paginate.tag,
        }
    }
}

/// A post listing with its own archive and feed: all posts, one section, or one tag.
///
/// Members are page indices while buckets are collected, and prepared pages once resolved.
#[derive(Debug)]
pub(super) struct Bucket<P> {
    pub(super) kind: BucketKind,
    pub(super) slug: String,
    pub(super) name: String,
    pub(super) pages: Vec<P>,
}

/// A resolved bucket whose pages are sorted newest first.
pub(super) type Listing<'a> = Bucket<&'a PreparedPage<'a>>;

impl<P> Bucket<P> {
    pub(super) fn path(&self) -> PathBuf {
        self.kind.path(&self.slug)
    }

    pub(super) fn url(&self, base_url: &BaseUrl) -> String {
        base_url.site_url(&directory_url(&self.path()))
    }

    pub(super) fn link(&self, base_url: &BaseUrl) -> LinkedTerm {
        LinkedTerm {
            name: self.name.clone(),
            url: self.url(base_url),
        }
    }
}

impl Bucket<usize> {
    /// Names the bucket from its `_index.md` title, falling back to `default_name`.
    fn load(
        kind: BucketKind,
        slug: String,
        default_name: impl FnOnce() -> String,
        pages: Vec<usize>,
        content_dir: &Path,
    ) -> Result<Self> {
        let name =
            load_index_title(&content_dir.join(kind.path(&slug)))?.unwrap_or_else(default_name);
        Ok(Self {
            kind,
            slug,
            name,
            pages,
        })
    }

    /// Replaces page indices with the prepared pages, newest first.
    fn resolve<'a>(self, pages: &'a [PreparedPage<'a>]) -> Listing<'a> {
        let mut members: Vec<_> = self.pages.into_iter().map(|index| &pages[index]).collect();
        members.sort_by_key(|&page| page.date_order());
        Bucket {
            kind: self.kind,
            slug: self.slug,
            name: self.name,
            pages: members,
        }
    }
}

impl<'a> Listing<'a> {
    pub(super) fn summary(&self, base_url: &BaseUrl) -> BucketSummary<'a> {
        BucketSummary {
            name: self.name.clone(),
            url: self.url(base_url),
            pages: self.pages.iter().map(|page| &page.summary).collect(),
        }
    }
}

/// Every bucket with its member page indices, plus the buckets each page links to.
#[derive(Debug)]
pub(super) struct Taxonomy {
    pub(super) posts: Bucket<usize>,
    /// Sections by slug, then tags by descending page count.
    pub(super) terms: Vec<Bucket<usize>>,
    /// Section and tag bucket indices for each input page.
    pub(super) page_terms: Vec<PageTerms>,
}

/// Every resolved listing, with pages newest first.
#[derive(Debug)]
pub(super) struct Listings<'a> {
    pub(super) posts: Listing<'a>,
    /// Sections by slug, then tags by descending page count.
    pub(super) terms: Vec<Listing<'a>>,
}

/// Indices into [`Taxonomy::terms`] for the section and tags of one page.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct PageTerms {
    pub(super) section: Option<usize>,
    /// In authored order.
    pub(super) tags: Vec<usize>,
}

/// Pages carrying one tag slug, keyed by case-folded term with the first-seen spelling as display
/// name. More than one key means distinct terms slugified to the same value.
type SlugGroup = BTreeMap<String, (String, Vec<usize>)>;

impl Taxonomy {
    /// Groups `pages` into buckets named from `_index.md` titles under `content_dir`. Sections
    /// fall back to their titlecased slug and tags to their first-seen spelling.
    ///
    /// Fails if a nonempty tag has no alphanumeric characters, or if tags differing beyond case
    /// share a slug, since one archive URL cannot serve both.
    pub(super) fn collect(
        pages: &[Page],
        content_dir: &Path,
        posts_name: impl FnOnce() -> String,
    ) -> Result<Self> {
        let mut posts = Vec::new();
        let mut sections: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        let mut tags: HashMap<String, SlugGroup> = HashMap::new();
        let mut page_tag_slugs = Vec::with_capacity(pages.len());
        for (index, page) in pages.iter().enumerate() {
            if let PageKind::Post { section } = &page.kind {
                posts.push(index);
                if let Some(slug) = section {
                    sections.entry(slug).or_default().push(index);
                }
            }
            page_tag_slugs.push(collect_tags(&page.frontmatter.tags, index, &mut tags)?);
        }

        let posts = Bucket::load(
            BucketKind::Posts,
            "posts".into(),
            posts_name,
            posts,
            content_dir,
        )?;
        let mut terms = Vec::new();
        for (slug, members) in sections {
            terms.push(Bucket::load(
                BucketKind::Section,
                slug.to_owned(),
                || titlecase(slug),
                members,
                content_dir,
            )?);
        }
        let mut tag_buckets = tags
            .into_iter()
            .map(|(slug, terms)| {
                let (name, members) = sole_term(&slug, terms)?;
                Bucket::load(BucketKind::Tag, slug, || name, members, content_dir)
            })
            .collect::<Result<Vec<_>>>()?;
        tag_buckets.sort_by(|a, b| {
            b.pages
                .len()
                .cmp(&a.pages.len())
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.slug.cmp(&b.slug))
        });
        terms.extend(tag_buckets);

        let indices: HashMap<_, _> = terms
            .iter()
            .enumerate()
            .map(|(index, bucket)| ((bucket.kind, bucket.slug.as_str()), index))
            .collect();
        let page_terms = pages
            .iter()
            .zip(page_tag_slugs)
            .map(|(page, tag_slugs)| PageTerms {
                section: match &page.kind {
                    PageKind::Post {
                        section: Some(slug),
                    } => Some(indices[&(BucketKind::Section, slug.as_str())]),
                    _ => None,
                },
                tags: tag_slugs
                    .iter()
                    .map(|slug| indices[&(BucketKind::Tag, slug.as_str())])
                    .collect(),
            })
            .collect();

        Ok(Self {
            posts,
            terms,
            page_terms,
        })
    }

    /// Replaces page indices in every bucket with the prepared pages.
    pub(super) fn resolve<'a>(self, pages: &'a [PreparedPage<'a>]) -> Listings<'a> {
        Listings {
            posts: self.posts.resolve(pages),
            terms: self
                .terms
                .into_iter()
                .map(|bucket| bucket.resolve(pages))
                .collect(),
        }
    }
}

impl<'a> Listings<'a> {
    /// All posts, then every section and tag.
    pub(super) fn iter(&self) -> impl Iterator<Item = &Listing<'a>> {
        iter::once(&self.posts).chain(&self.terms)
    }
}

/// Collects a page's tags into the grouped map and returns its distinct tag slugs.
fn collect_tags(
    values: &[String],
    page_idx: usize,
    grouped: &mut HashMap<String, SlugGroup>,
) -> Result<Vec<String>> {
    let mut slugs = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let slug = slugify(trimmed);
        ensure!(
            !slug.is_empty(),
            r#"tag "{trimmed}" must contain at least one letter or number"#,
        );

        let (_, indices) = grouped
            .entry(slug.clone())
            .or_default()
            .entry(trimmed.to_lowercase())
            .or_insert_with(|| (trimmed.to_owned(), Vec::new()));
        if indices.last() != Some(&page_idx) {
            indices.push(page_idx);
            slugs.push(slug);
        }
    }

    Ok(slugs)
}

/// Unwraps the single term behind a tag slug, reporting a collision when several terms share it.
fn sole_term(slug: &str, terms: SlugGroup) -> Result<(String, Vec<usize>)> {
    let mut terms = terms.into_values();
    let (name, indices) = terms
        .next()
        .expect("a slug group is only created together with its first term");

    if let Some((other, other_indices)) = terms.next() {
        bail!(
            "tag slug collision on \"{slug}\": \"{name}\" ({} pages) and \"{other}\" ({} pages)",
            indices.len(),
            other_indices.len(),
        );
    }

    Ok((name, indices))
}

#[cfg(test)]
mod tests {
    use indoc::{formatdoc, indoc};

    use super::*;
    use crate::build::listing::prepared_page;
    use crate::test_utils::{test_page, write_test_file};

    // ── Bucket::url ──

    #[test]
    fn encodes_slug_under_base_path() {
        let base_url = "https://example.com/blog".parse().unwrap();
        for (kind, slug, expected) in [
            (
                BucketKind::Posts,
                "posts",
                "https://example.com/blog/posts/",
            ),
            (
                BucketKind::Section,
                "c++",
                "https://example.com/blog/posts/c%2B%2B/",
            ),
            (
                BucketKind::Tag,
                "机器 学习",
                "https://example.com/blog/tags/%E6%9C%BA%E5%99%A8%20%E5%AD%A6%E4%B9%A0/",
            ),
        ] {
            let bucket = Bucket::<usize> {
                kind,
                slug: slug.into(),
                name: String::new(),
                pages: Vec::new(),
            };
            assert_eq!(bucket.url(&base_url), expected, "{slug}");
        }
    }

    // ── Taxonomy::collect ──

    #[test]
    fn groups_posts_sections_and_tags() {
        let pages = [
            post("Post 1", Some("note"), &["rust", "web"]),
            post("Post 2", Some("essay"), &["", "  ", "rust"]),
            test_page("About"),
            post("Orphan", None, &["web", "rust"]),
        ];
        let taxonomy = collect(&pages, |_| {});

        assert_eq!(
            summarize(&taxonomy),
            [
                (BucketKind::Posts, "posts", "All Posts", vec![0, 1, 3]),
                (BucketKind::Section, "essay", "Essay", vec![1]),
                (BucketKind::Section, "note", "Note", vec![0]),
                (BucketKind::Tag, "rust", "rust", vec![0, 1, 3]),
                (BucketKind::Tag, "web", "web", vec![0, 3]),
            ]
        );
        assert_eq!(
            taxonomy.page_terms,
            [
                PageTerms {
                    section: Some(1),
                    tags: vec![2, 3],
                },
                PageTerms {
                    section: Some(0),
                    tags: vec![2],
                },
                PageTerms::default(),
                PageTerms {
                    section: None,
                    tags: vec![3, 2],
                },
            ]
        );
    }

    #[test]
    fn names_buckets_from_index_titles() {
        let pages = [post("Post 1", Some("hello-world"), &["ml", "rust"])];
        let taxonomy = collect(&pages, |content| {
            for (dir, title) in [
                ("posts", "Archive"),
                ("posts/hello-world", "笔记"),
                ("tags/ml", "Machine Learning"),
            ] {
                write_test_file(
                    content,
                    &format!("{dir}/_index.md"),
                    &formatdoc! {"
                        +++
                        title = {title:?}
                        +++
                    "},
                );
            }
        });

        assert_eq!(
            summarize(&taxonomy)
                .into_iter()
                .map(|(_, slug, name, _)| (slug, name))
                .collect::<Vec<_>>(),
            [
                ("posts", "Archive"),
                ("hello-world", "笔记"),
                ("ml", "Machine Learning"),
                ("rust", "rust"),
            ]
        );
    }

    #[test]
    fn deduplicates_tag_membership_and_keeps_first_spelling() {
        let pages = [
            tagged(
                "Post A",
                &["web", "Rust", "rust", " Rust ", "", "  ", "Web"],
            ),
            tagged("Post B", &["rust"]),
        ];
        let taxonomy = collect(&pages, |_| {});

        assert_eq!(
            summarize(&taxonomy)[1..],
            [
                (BucketKind::Tag, "rust", "Rust", vec![0, 1]),
                (BucketKind::Tag, "web", "web", vec![0]),
            ]
        );
        assert_eq!(taxonomy.page_terms[0].tags, [1, 0]);
    }

    #[test]
    fn sorts_tags_by_count_name_then_slug_and_keeps_punctuation_distinct() {
        let shared = ["epsilon", "delta", "gamma", "beta", "eta"];
        let pages = [
            tagged("Post 1", &["zebra", "alpha++"]),
            tagged("Post 2", &["common", "alpha"]),
            tagged("Post 3", &["common"]),
            tagged("Post 4", &shared),
        ];
        let taxonomy = collect(&pages, |content| {
            for slug in shared {
                write_test_file(
                    content,
                    &format!("tags/{slug}/_index.md"),
                    indoc! {r#"
                        +++
                        title = "shared"
                        +++
                    "#},
                );
            }
        });

        assert_eq!(
            summarize(&taxonomy)[1..]
                .iter()
                .map(|(_, slug, name, _)| (*slug, *name))
                .collect::<Vec<_>>(),
            [
                ("common", "common"),
                ("alpha", "alpha"),
                ("alpha++", "alpha++"),
                ("beta", "shared"),
                ("delta", "shared"),
                ("epsilon", "shared"),
                ("eta", "shared"),
                ("gamma", "shared"),
                ("zebra", "zebra"),
            ]
        );
    }

    #[test]
    fn rejects_invalid_index() {
        for (pages, dir) in [
            (vec![post("Post 1", None, &[])], "posts"),
            (vec![post("Post 1", Some("note"), &[])], "posts/note"),
            (vec![tagged("Post 1", &["rust"])], "tags/rust"),
        ] {
            let error = try_collect(&pages, |content| {
                write_test_file(content, &format!("{dir}/_index.md"), "+++");
            })
            .unwrap_err();
            assert!(
                format!("{error:#}").contains("invalid frontmatter"),
                "{dir}"
            );
        }
    }

    #[test]
    fn rejects_empty_tag_slug() {
        for tag in ["!!!", " +++ ", "..."] {
            let pages = [tagged("Post A", &[tag])];
            let error = try_collect(&pages, |_| {}).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    r#"tag "{}" must contain at least one letter or number"#,
                    tag.trim()
                ),
            );
        }
    }

    #[test]
    fn rejects_colliding_tags() {
        let pages = [
            tagged("Post 1", &["Alpha Beta"]),
            tagged("Post 2", &["Alpha & Beta"]),
            tagged("Post 3", &["Alpha Beta"]),
            tagged("Post 4", &["Alpha & Beta"]),
            tagged("Post 5", &["Alpha & Beta"]),
        ];
        assert_eq!(
            try_collect(&pages, |_| {}).unwrap_err().to_string(),
            r#"tag slug collision on "alpha-beta": "Alpha & Beta" (3 pages) and "Alpha Beta" (2 pages)"#,
        );
    }

    // ── Taxonomy::resolve ──

    #[test]
    fn orders_pages_newest_first_with_url_tiebreak() {
        let pages = [
            prepared_page("B", Some("2026-01-01T00:00:00Z"), None),
            prepared_page("D", None, None),
            prepared_page("C", Some("2026-03-01T00:00:00Z"), None),
            prepared_page("A", Some("2026-01-01T00:00:00Z"), None),
        ];
        let bucket = |kind, slug: &str| Bucket {
            kind,
            slug: slug.into(),
            name: String::new(),
            pages: vec![0, 1, 2, 3],
        };
        let listings = Taxonomy {
            posts: bucket(BucketKind::Posts, "posts"),
            terms: vec![bucket(BucketKind::Tag, "rust")],
            page_terms: Vec::new(),
        }
        .resolve(&pages);

        for listing in listings.iter() {
            assert_eq!(
                listing
                    .pages
                    .iter()
                    .map(|page| page.summary.title.as_str())
                    .collect::<Vec<_>>(),
                ["C", "A", "B", "D"],
                "{}",
                listing.slug
            );
        }
        assert_eq!(listings.iter().count(), 2);
    }

    fn collect(pages: &[Page], setup: impl FnOnce(&Path)) -> Taxonomy {
        try_collect(pages, setup).unwrap()
    }

    fn try_collect(pages: &[Page], setup: impl FnOnce(&Path)) -> Result<Taxonomy> {
        let dir = tempfile::tempdir().unwrap();
        setup(dir.path());
        Taxonomy::collect(pages, dir.path(), || "All Posts".into())
    }

    fn summarize(taxonomy: &Taxonomy) -> Vec<(BucketKind, &str, &str, Vec<usize>)> {
        iter::once(&taxonomy.posts)
            .chain(&taxonomy.terms)
            .map(|bucket| {
                (
                    bucket.kind,
                    bucket.slug.as_str(),
                    bucket.name.as_str(),
                    bucket.pages.clone(),
                )
            })
            .collect()
    }

    fn post(title: &str, section: Option<&str>, tags: &[&str]) -> Page {
        let mut page = tagged(title, tags);
        page.kind = PageKind::Post {
            section: section.map(String::from),
        };
        page
    }

    fn tagged(title: &str, tags: &[&str]) -> Page {
        let mut page = test_page(title);
        page.frontmatter.tags = tags.iter().map(|tag| (*tag).to_owned()).collect();
        page
    }
}
