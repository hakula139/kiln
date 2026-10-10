use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{Result, bail, ensure};

use crate::content::index::load_index_title;
use crate::content::page::Page;
use crate::text::slugify;

/// A unique term within a taxonomy (e.g., the tag "rust").
#[derive(Debug, Clone)]
pub struct Term {
    /// Display name (first occurrence preserved, e.g., "Rust").
    pub name: String,
    /// URL-safe slug (e.g., "rust").
    pub slug: String,
    /// Number of pages with this term.
    pub page_count: usize,
}

/// The full taxonomy collection built from content pages. Currently tracks tags only.
#[derive(Debug)]
pub struct TaxonomySet {
    /// All tags in the site, sorted by page count descending then name ascending.
    pub tags: Vec<Term>,
    /// Maps `tag_slug → page indices` (in input order) into the original page slice.
    pub tag_pages: HashMap<String, Vec<usize>>,
    /// Indices into `tags` for each input page, preserving authored order.
    pub page_tags: Vec<Vec<usize>>,
}

/// Pages carrying one taxonomy slug, keyed by case-folded term with the first-seen spelling as
/// display name. More than one key means distinct terms slugified to the same value.
type SlugGroup = BTreeMap<String, (String, Vec<usize>)>;

/// Builds the taxonomy set from the given page collection.
///
/// Groups pages by tag, deduplicates terms by slug, and sorts by page count descending (then
/// name ascending). Page indices are in input order. When `content_dir` is
/// provided, looks for `tags/<slug>/_index.md` to override the display name.
///
/// # Errors
///
/// Returns an error if a nonempty tag has no alphanumeric characters, or if tags differing beyond
/// case share a slug, since one archive URL cannot serve both.
pub fn build_taxonomies(pages: &[Page], content_dir: Option<&Path>) -> Result<TaxonomySet> {
    let mut grouped: HashMap<String, SlugGroup> = HashMap::new();

    let page_slugs = pages
        .iter()
        .enumerate()
        .map(|(index, page)| collect_terms(&page.frontmatter.tags, index, &mut grouped))
        .collect::<Result<Vec<_>>>()?;

    let mut tag_pages = HashMap::with_capacity(grouped.len());
    let mut tags: Vec<Term> = Vec::with_capacity(grouped.len());

    for (slug, terms) in grouped {
        let (name, indices) = sole_term(&slug, terms)?;
        let display_name = content_dir
            .map(|dir| load_index_title(&dir.join("tags").join(&slug)))
            .transpose()?
            .flatten()
            .unwrap_or(name);
        let page_count = indices.len();
        tags.push(Term {
            name: display_name,
            slug: slug.clone(),
            page_count,
        });
        tag_pages.insert(slug, indices);
    }

    tags.sort_by(|a, b| {
        b.page_count
            .cmp(&a.page_count)
            .then(a.name.cmp(&b.name))
            .then(a.slug.cmp(&b.slug))
    });

    let term_indices: HashMap<_, _> = tags
        .iter()
        .enumerate()
        .map(|(index, term)| (term.slug.as_str(), index))
        .collect();
    let page_tags = page_slugs
        .into_iter()
        .map(|slugs| {
            slugs
                .iter()
                .map(|slug| term_indices[slug.as_str()])
                .collect()
        })
        .collect();

    Ok(TaxonomySet {
        tags,
        tag_pages,
        page_tags,
    })
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

/// Collects terms from a frontmatter field into the grouped map.
fn collect_terms(
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

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::test_utils::test_page;

    fn make_page(title: &str, tags: &[&str]) -> Page {
        let mut page = test_page(title);
        page.frontmatter.tags = tags.iter().map(|s| (*s).to_owned()).collect();
        page
    }

    // ── build_taxonomies ──

    #[test]
    fn build_taxonomies_multiple_tags_shared() {
        let pages = [
            make_page("Post 1", &["rust", "web"]),
            make_page("Post 2", &["", "  ", "rust"]),
            make_page("Post 3", &["web"]),
        ];
        let set = build_taxonomies(&pages, None).unwrap();

        assert_eq!(set.tags.len(), 2);
        assert_eq!(set.tags[0].name, "rust");
        assert_eq!(set.tags[0].slug, "rust");
        assert_eq!(set.tags[0].page_count, 2);
        assert_eq!(set.tags[1].name, "web");
        assert_eq!(set.tags[1].page_count, 2);
    }

    #[test]
    fn build_taxonomies_deduplicates_page_membership() {
        let pages = [
            make_page(
                "Post A",
                &["web", "Rust", "rust", " Rust ", "", "  ", "Web"],
            ),
            make_page("Post B", &["rust"]),
        ];
        let set = build_taxonomies(&pages, None).unwrap();

        assert_eq!(set.tags.len(), 2);
        assert_eq!(set.tags[0].name, "Rust");
        assert_eq!(set.tags[0].slug, "rust");
        assert_eq!(set.tags[0].page_count, 2);
        assert_eq!(set.tag_pages["rust"], [0, 1]);
        assert_eq!(set.tags[1].name, "web");
        assert_eq!(set.tags[1].slug, "web");
        assert_eq!(set.tags[1].page_count, 1);
        assert_eq!(set.tag_pages["web"], [0]);
        assert_eq!(set.page_tags, [vec![1, 0], vec![0]]);
    }

    #[test]
    fn build_taxonomies_preserved_punctuation_stays_distinct() {
        let pages = [
            make_page("Post 1", &["Alpha"]),
            make_page("Post 2", &["Alpha++"]),
        ];
        let set = build_taxonomies(&pages, None).unwrap();

        assert_eq!(set.tags.len(), 2, "`+` must not be folded away");
        assert_eq!(set.tags[0].slug, "alpha");
        assert_eq!(set.tags[1].slug, "alpha++");
    }

    #[test]
    fn build_taxonomies_sorted_by_count_then_name() {
        let pages = [
            make_page("Post 1", &["zebra"]),
            make_page("Post 2", &["common", "alpha"]),
            make_page("Post 3", &["common"]),
        ];
        let set = build_taxonomies(&pages, None).unwrap();

        // Primary: count descending.
        assert_eq!(set.tags[0].name, "common");
        assert_eq!(set.tags[0].page_count, 2);
        // Tiebreak: name ascending ("alpha" < "zebra").
        assert_eq!(set.tags[1].name, "alpha");
        assert_eq!(set.tags[1].page_count, 1);
        assert_eq!(set.tags[2].name, "zebra");
        assert_eq!(set.tags[2].page_count, 1);
    }

    #[test]
    fn build_taxonomies_uses_index_title() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");

        let tag_dir = content_dir.join("tags").join("ml");
        std::fs::create_dir_all(&tag_dir).unwrap();
        std::fs::write(
            tag_dir.join("_index.md"),
            indoc! {r#"
                +++
                title = "Machine Learning"
                +++
            "#},
        )
        .unwrap();

        let pages = [make_page("Post 1", &["ml"])];
        let set = build_taxonomies(&pages, Some(&content_dir)).unwrap();

        assert_eq!(
            set.tags[0].name, "Machine Learning",
            "should use title from _index.md"
        );
        assert_eq!(set.tags[0].slug, "ml", "slug should stay as-is");
    }

    #[test]
    fn build_taxonomies_falls_back_without_index() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let pages = [make_page("Post 1", &["rust"])];
        let set = build_taxonomies(&pages, Some(&content_dir)).unwrap();

        assert_eq!(
            set.tags[0].name, "rust",
            "should fall back to frontmatter value"
        );
    }

    #[test]
    fn build_taxonomies_ignores_empty_index_title() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");

        let tag_dir = content_dir.join("tags").join("rust");
        std::fs::create_dir_all(&tag_dir).unwrap();
        std::fs::write(
            tag_dir.join("_index.md"),
            indoc! {r"
                +++
                +++
            "},
        )
        .unwrap();

        let pages = [make_page("Post 1", &["rust"])];
        let set = build_taxonomies(&pages, Some(&content_dir)).unwrap();

        assert_eq!(
            set.tags[0].name, "rust",
            "should fall back when _index.md has empty title"
        );
    }

    #[test]
    fn build_taxonomies_empty() {
        let set = build_taxonomies(&[], None).unwrap();
        assert!(set.tags.is_empty());
        assert!(set.tag_pages.is_empty());
    }

    #[test]
    fn build_taxonomies_empty_tag_slug_returns_error() {
        for tag in ["!!!", " +++ ", "..."] {
            let pages = [make_page("Post A", &[tag])];
            let error = build_taxonomies(&pages, None).unwrap_err();
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
    fn build_taxonomies_colliding_terms_returns_error() {
        let pages = [
            make_page("Post 1", &["Alpha Beta"]),
            make_page("Post 2", &["Alpha & Beta"]),
            make_page("Post 3", &["Alpha Beta"]),
            make_page("Post 4", &["Alpha & Beta"]),
            make_page("Post 5", &["Alpha & Beta"]),
        ];
        let err = build_taxonomies(&pages, None).unwrap_err();

        assert_eq!(
            err.to_string(),
            r#"tag slug collision on "alpha-beta": "Alpha & Beta" (3 pages) and "Alpha Beta" (2 pages)"#,
            "should name the slug, both terms, and each term's total page count"
        );
    }
}
