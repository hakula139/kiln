use std::collections::HashMap;
use std::path::Path;

use strum::{EnumIter, IntoEnumIterator};

use crate::content::frontmatter;
use crate::content::page::Page;
use crate::text::slugify;

/// Built-in taxonomy kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum TaxonomyKind {
    Tags,
}

impl TaxonomyKind {
    /// Returns the singular form (e.g., `"tag"`).
    #[must_use]
    pub fn singular(self) -> &'static str {
        match self {
            Self::Tags => "tag",
        }
    }

    /// Returns the plural form (e.g., `"tags"`).
    #[must_use]
    pub fn plural(self) -> &'static str {
        match self {
            Self::Tags => "tags",
        }
    }
}

/// A single taxonomy (e.g., all tags or all categories).
#[derive(Debug)]
pub struct Taxonomy {
    pub kind: TaxonomyKind,
    /// Terms sorted by page count descending, then name ascending.
    pub terms: Vec<Term>,
}

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

/// The full taxonomy collection built from content pages.
#[derive(Debug)]
pub struct TaxonomySet {
    pub taxonomies: Vec<Taxonomy>,
    /// Maps `(kind, term_slug)` → sorted page indices into the original page slice.
    pub term_pages: HashMap<(TaxonomyKind, String), Vec<usize>>,
}

/// Builds taxonomies from the given page collection.
///
/// Groups pages by their tag values, deduplicates terms by slug, and sorts
/// terms by page count descending (then name ascending). Page indices within
/// each term are in the same order as the input (newest first).
///
/// When `content_dir` is provided, looks for `<kind>/<slug>/_index.md` files
/// with a `title` field to override the display name.
#[must_use]
pub fn build_taxonomies(pages: &[Page], content_dir: Option<&Path>) -> TaxonomySet {
    // Collect (kind, slug) → (display_name, Vec<page_index>).
    let mut grouped: HashMap<(TaxonomyKind, String), (String, Vec<usize>)> = HashMap::new();

    for (idx, page) in pages.iter().enumerate() {
        collect_terms(
            &page.frontmatter.tags,
            TaxonomyKind::Tags,
            idx,
            &mut grouped,
        );
    }

    let mut term_pages = HashMap::new();
    let mut kind_terms: HashMap<TaxonomyKind, Vec<Term>> = HashMap::new();

    for ((kind, slug), (name, indices)) in grouped {
        let display_name = content_dir
            .and_then(|dir| load_term_title(dir, kind, &slug))
            .unwrap_or(name);
        let page_count = indices.len();
        kind_terms.entry(kind).or_default().push(Term {
            name: display_name,
            slug: slug.clone(),
            page_count,
        });
        term_pages.insert((kind, slug), indices);
    }

    // Sort terms: page count descending, then name ascending.
    for terms in kind_terms.values_mut() {
        terms.sort_by(|a, b| b.page_count.cmp(&a.page_count).then(a.name.cmp(&b.name)));
    }

    // Always emit one Taxonomy per kind so index pages are generated even when empty.
    let taxonomies = TaxonomyKind::iter()
        .map(|kind| Taxonomy {
            kind,
            terms: kind_terms.remove(&kind).unwrap_or_default(),
        })
        .collect();

    TaxonomySet {
        taxonomies,
        term_pages,
    }
}

/// Loads the display title from a term's `_index.md` file.
///
/// Looks for `<content_dir>/<kind_plural>/<slug>/_index.md` with TOML
/// frontmatter containing a non-empty `title` field. Returns `None` if the
/// file doesn't exist or has no title.
fn load_term_title(content_dir: &Path, kind: TaxonomyKind, slug: &str) -> Option<String> {
    let path = content_dir.join(kind.plural()).join(slug).join("_index.md");
    let content = std::fs::read_to_string(&path).ok()?;
    let (fm, _) = frontmatter::parse(&content).ok()?;
    if fm.title.is_empty() {
        None
    } else {
        Some(fm.title)
    }
}

/// Collects terms from a frontmatter field into the grouped map.
fn collect_terms(
    values: &[String],
    kind: TaxonomyKind,
    page_idx: usize,
    grouped: &mut HashMap<(TaxonomyKind, String), (String, Vec<usize>)>,
) {
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let slug = slugify(trimmed);
        grouped
            .entry((kind, slug))
            .and_modify(|(_, indices)| indices.push(page_idx))
            .or_insert_with(|| (trimmed.to_owned(), vec![page_idx]));
    }
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

    // -- build_taxonomies --

    #[test]
    fn build_taxonomies_empty() {
        let set = build_taxonomies(&[], None);
        // Always produces one Taxonomy per kind, even with no pages.
        assert_eq!(set.taxonomies.len(), 1);
        assert_eq!(set.taxonomies[0].kind, TaxonomyKind::Tags);
        assert!(set.taxonomies[0].terms.is_empty());
    }

    #[test]
    fn build_taxonomies_single_tag() {
        let pages = [make_page("Post 1", &["rust"])];
        let set = build_taxonomies(&pages, None);

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        assert_eq!(tags.terms.len(), 1);
        assert_eq!(tags.terms[0].name, "rust");
        assert_eq!(tags.terms[0].slug, "rust");
        assert_eq!(tags.terms[0].page_count, 1);
    }

    #[test]
    fn build_taxonomies_multiple_tags_shared() {
        let pages = [
            make_page("Post 1", &["rust", "web"]),
            make_page("Post 2", &["rust"]),
            make_page("Post 3", &["web"]),
        ];
        let set = build_taxonomies(&pages, None);

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        assert_eq!(tags.terms.len(), 2);
        // Both have 2 pages, so sorted alphabetically.
        assert_eq!(tags.terms[0].name, "rust");
        assert_eq!(tags.terms[0].page_count, 2);
        assert_eq!(tags.terms[1].name, "web");
        assert_eq!(tags.terms[1].page_count, 2);
    }

    #[test]
    fn build_taxonomies_case_insensitive_slugs() {
        let pages = [
            make_page("Post 1", &["Rust"]),
            make_page("Post 2", &["rust"]),
        ];
        let set = build_taxonomies(&pages, None);

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        assert_eq!(tags.terms.len(), 1, "should deduplicate by slug");
        assert_eq!(
            tags.terms[0].name, "Rust",
            "should preserve first-seen display name"
        );
        assert_eq!(tags.terms[0].page_count, 2);
    }

    #[test]
    fn build_taxonomies_sorted_by_count_then_name() {
        let pages = [
            make_page("Post 1", &["zebra"]),
            make_page("Post 2", &["common", "alpha"]),
            make_page("Post 3", &["common"]),
        ];
        let set = build_taxonomies(&pages, None);

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        // Primary: count descending.
        assert_eq!(tags.terms[0].name, "common");
        assert_eq!(tags.terms[0].page_count, 2);
        // Tiebreak: name ascending ("alpha" < "zebra").
        assert_eq!(tags.terms[1].name, "alpha");
        assert_eq!(tags.terms[1].page_count, 1);
        assert_eq!(tags.terms[2].name, "zebra");
        assert_eq!(tags.terms[2].page_count, 1);
    }

    #[test]
    fn build_taxonomies_preserves_page_order() {
        let pages = [
            make_page("Newest", &["rust"]),
            make_page("Oldest", &["rust"]),
        ];
        let set = build_taxonomies(&pages, None);

        let indices = &set.term_pages[&(TaxonomyKind::Tags, "rust".to_owned())];
        assert_eq!(
            indices,
            &[0, 1],
            "should preserve input order (newest first)"
        );
    }

    #[test]
    fn build_taxonomies_empty_tags_ignored() {
        let pages = [make_page("Post 1", &["", "  ", "rust"])];
        let set = build_taxonomies(&pages, None);

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        assert_eq!(tags.terms.len(), 1);
        assert_eq!(tags.terms[0].name, "rust");
    }

    // -- load_term_title --

    #[test]
    fn build_taxonomies_uses_index_title() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");

        // Create _index.md with display name override.
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
        let set = build_taxonomies(&pages, Some(&content_dir));

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        assert_eq!(
            tags.terms[0].name, "Machine Learning",
            "should use title from _index.md"
        );
        assert_eq!(tags.terms[0].slug, "ml", "slug should stay as-is");
    }

    #[test]
    fn build_taxonomies_falls_back_without_index() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");
        // No _index.md files — display name comes from frontmatter.
        std::fs::create_dir_all(&content_dir).unwrap();

        let pages = [make_page("Post 1", &["rust"])];
        let set = build_taxonomies(&pages, Some(&content_dir));

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        assert_eq!(
            tags.terms[0].name, "rust",
            "should fall back to frontmatter value"
        );
    }

    #[test]
    fn build_taxonomies_ignores_empty_index_title() {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");

        // _index.md with empty title — should fall back.
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
        let set = build_taxonomies(&pages, Some(&content_dir));

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        assert_eq!(
            tags.terms[0].name, "rust",
            "should fall back when _index.md has empty title"
        );
    }

    // -- TaxonomyKind --

    #[test]
    fn kind_names() {
        assert_eq!(TaxonomyKind::Tags.singular(), "tag");
        assert_eq!(TaxonomyKind::Tags.plural(), "tags");
    }
}
