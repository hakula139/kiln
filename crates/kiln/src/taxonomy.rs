use std::collections::HashMap;

use strum::{EnumIter, IntoEnumIterator};

use crate::content::page::Page;
use crate::text::slugify;

/// The two built-in taxonomy kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum TaxonomyKind {
    Tags,
    Categories,
}

impl TaxonomyKind {
    /// Returns the singular form (e.g., `"tag"`, `"category"`).
    #[must_use]
    pub fn singular(self) -> &'static str {
        match self {
            Self::Tags => "tag",
            Self::Categories => "category",
        }
    }

    /// Returns the plural form (e.g., `"tags"`, `"categories"`).
    #[must_use]
    pub fn plural(self) -> &'static str {
        match self {
            Self::Tags => "tags",
            Self::Categories => "categories",
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
/// Groups pages by their tag / category values, deduplicates terms by slug,
/// and sorts terms by page count descending (then name ascending).
/// Page indices within each term are in the same order as the input (newest first).
#[must_use]
pub fn build_taxonomies(pages: &[Page]) -> TaxonomySet {
    // Collect (kind, slug) → (display_name, Vec<page_index>).
    let mut grouped: HashMap<(TaxonomyKind, String), (String, Vec<usize>)> = HashMap::new();

    for (idx, page) in pages.iter().enumerate() {
        collect_terms(
            &page.frontmatter.tags,
            TaxonomyKind::Tags,
            idx,
            &mut grouped,
        );
        collect_terms(
            &page.frontmatter.categories,
            TaxonomyKind::Categories,
            idx,
            &mut grouped,
        );
    }

    let mut term_pages = HashMap::new();
    let mut kind_terms: HashMap<TaxonomyKind, Vec<Term>> = HashMap::new();

    for ((kind, slug), (name, indices)) in grouped {
        let page_count = indices.len();
        kind_terms.entry(kind).or_default().push(Term {
            name,
            slug: slug.clone(),
            page_count,
        });
        term_pages.insert((kind, slug), indices);
    }

    // Sort terms: page count descending, then name ascending.
    for terms in kind_terms.values_mut() {
        terms.sort_by(|a, b| b.page_count.cmp(&a.page_count).then(a.name.cmp(&b.name)));
    }

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
    use std::path::PathBuf;

    use crate::content::frontmatter::Frontmatter;

    use super::*;

    fn make_page(title: &str, tags: &[&str], categories: &[&str]) -> Page {
        Page {
            frontmatter: Frontmatter {
                title: title.to_owned(),
                tags: tags.iter().map(|s| (*s).to_owned()).collect(),
                categories: categories.iter().map(|s| (*s).to_owned()).collect(),
                ..Frontmatter::default()
            },
            raw_content: String::new(),
            slug: title.to_lowercase().replace(' ', "-"),
            summary: None,
            source_path: PathBuf::from(format!("content/posts/{title}/index.md")),
            assets: Vec::new(),
        }
    }

    // -- build_taxonomies --

    #[test]
    fn build_taxonomies_empty() {
        let set = build_taxonomies(&[]);
        assert_eq!(set.taxonomies.len(), 2);
        assert!(set.taxonomies[0].terms.is_empty());
        assert!(set.taxonomies[1].terms.is_empty());
    }

    #[test]
    fn build_taxonomies_single_tag() {
        let pages = [make_page("Post 1", &["rust"], &[])];
        let set = build_taxonomies(&pages);

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
            make_page("Post 1", &["rust", "web"], &[]),
            make_page("Post 2", &["rust"], &[]),
            make_page("Post 3", &["web"], &[]),
        ];
        let set = build_taxonomies(&pages);

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
            make_page("Post 1", &["Rust"], &[]),
            make_page("Post 2", &["rust"], &[]),
        ];
        let set = build_taxonomies(&pages);

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
    fn build_taxonomies_sorted_by_count_desc() {
        let pages = [
            make_page("Post 1", &["rare"], &[]),
            make_page("Post 2", &["common"], &[]),
            make_page("Post 3", &["common"], &[]),
            make_page("Post 4", &["common"], &[]),
        ];
        let set = build_taxonomies(&pages);

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        assert_eq!(tags.terms[0].name, "common");
        assert_eq!(tags.terms[0].page_count, 3);
        assert_eq!(tags.terms[1].name, "rare");
        assert_eq!(tags.terms[1].page_count, 1);
    }

    #[test]
    fn build_taxonomies_preserves_page_order() {
        let pages = [
            make_page("Newest", &["rust"], &[]),
            make_page("Oldest", &["rust"], &[]),
        ];
        let set = build_taxonomies(&pages);

        let indices = &set.term_pages[&(TaxonomyKind::Tags, "rust".to_owned())];
        assert_eq!(
            indices,
            &[0, 1],
            "should preserve input order (newest first)"
        );
    }

    #[test]
    fn build_taxonomies_categories() {
        let pages = [
            make_page("Post 1", &[], &["tutorial"]),
            make_page("Post 2", &[], &["tutorial", "note"]),
        ];
        let set = build_taxonomies(&pages);

        let cats = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Categories)
            .unwrap();
        assert_eq!(cats.terms.len(), 2);
        assert_eq!(cats.terms[0].name, "tutorial");
        assert_eq!(cats.terms[0].page_count, 2);
    }

    #[test]
    fn build_taxonomies_empty_tags_ignored() {
        let pages = [make_page("Post 1", &["", "  ", "rust"], &[])];
        let set = build_taxonomies(&pages);

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        assert_eq!(tags.terms.len(), 1);
        assert_eq!(tags.terms[0].name, "rust");
    }

    #[test]
    fn build_taxonomies_both_kinds() {
        let pages = [make_page("Post 1", &["rust"], &["tutorial"])];
        let set = build_taxonomies(&pages);

        let tags = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Tags)
            .unwrap();
        let cats = set
            .taxonomies
            .iter()
            .find(|t| t.kind == TaxonomyKind::Categories)
            .unwrap();
        assert_eq!(tags.terms.len(), 1);
        assert_eq!(cats.terms.len(), 1);
    }

    // -- TaxonomyKind --

    #[test]
    fn kind_singular() {
        assert_eq!(TaxonomyKind::Tags.singular(), "tag");
        assert_eq!(TaxonomyKind::Categories.singular(), "category");
    }

    #[test]
    fn kind_plural() {
        assert_eq!(TaxonomyKind::Tags.plural(), "tags");
        assert_eq!(TaxonomyKind::Categories.plural(), "categories");
    }
}
