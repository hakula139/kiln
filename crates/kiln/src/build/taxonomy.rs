use std::path::Path;

use anyhow::{Context, Result};

use crate::output::write_output;
use crate::taxonomy::{TaxonomyKind, TaxonomySet, Term, build_taxonomies};
use crate::template::{TaxonomyIndexVars, TermPageVars, TermSummary};

use super::BuildContext;
use super::listing::{ListedPage, collect_page_summaries, group_by_year, sort_by_date_desc};
use super::paginate::{paginate_config, write_paginated};

/// Generates taxonomy index pages and paginated term pages.
pub(crate) fn build_taxonomy_pages(
    ctx: &BuildContext,
    listed_pages: &[ListedPage],
    pages: &[crate::content::page::Page],
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    if !ctx.template_engine.has_template("taxonomy.html") {
        return Ok(());
    }

    let taxonomy_set = build_taxonomies(pages, Some(content_dir));

    let per_page = paginate_config(&ctx.config.params, &["paginate"]).unwrap_or(10);

    for taxonomy in &taxonomy_set.taxonomies {
        let kind = taxonomy.kind;
        let base_path = format!("/{}", kind.plural());

        let term_pages: Vec<Vec<ListedPage>> = taxonomy
            .terms
            .iter()
            .map(|term| resolve_term_pages(&taxonomy_set, kind, &term.slug, listed_pages))
            .collect();

        let term_summaries: Vec<TermSummary> = taxonomy
            .terms
            .iter()
            .zip(&term_pages)
            .map(|(term, pages)| TermSummary {
                name: term.name.clone(),
                slug: term.slug.clone(),
                url: format!("{base_path}/{}/", term.slug),
                pages: collect_page_summaries(pages.iter().cloned()),
            })
            .collect();

        build_taxonomy_index(
            ctx,
            kind.plural(),
            kind.singular(),
            term_summaries,
            output_dir,
        )?;

        for (term, pages) in taxonomy.terms.iter().zip(&term_pages) {
            build_term_pages(ctx, kind, term, pages, per_page, output_dir)?;
        }
    }

    Ok(())
}

/// Resolves page indices for a term into sorted listed pages.
fn resolve_term_pages(
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

/// Renders a taxonomy-style index page (e.g., `/tags/index.html`, `/sections/index.html`).
///
/// Reused by both taxonomy and sections index generation.
pub(crate) fn build_taxonomy_index(
    ctx: &BuildContext,
    kind: &str,
    singular: &str,
    terms: Vec<TermSummary>,
    output_dir: &Path,
) -> Result<()> {
    let vars = TaxonomyIndexVars {
        kind,
        singular,
        terms,
        config: &ctx.config,
    };

    let html = ctx
        .template_engine
        .render_taxonomy(&vars)
        .with_context(|| format!("failed to render {kind} index"))?;

    let dest = output_dir.join(kind).join("index.html");
    write_output(&dest, &html).with_context(|| format!("failed to write {}", dest.display()))
}

/// Generates paginated pages for a single taxonomy term.
///
/// Pages must be pre-sorted by date descending.
fn build_term_pages(
    ctx: &BuildContext,
    kind: TaxonomyKind,
    term: &Term,
    listed_pages: &[ListedPage],
    per_page: usize,
    output_dir: &Path,
) -> Result<()> {
    let term_base = format!("/{}/{}", kind.plural(), term.slug);

    write_paginated(
        listed_pages,
        per_page,
        &term_base,
        output_dir,
        |pages, pagination| {
            let page_groups = group_by_year(pages);
            let vars = TermPageVars {
                kind: kind.plural(),
                singular: kind.singular(),
                term_name: &term.name,
                term_slug: &term.slug,
                page_groups,
                pagination,
                config: &ctx.config,
            };
            ctx.template_engine
                .render_term(&vars)
                .with_context(|| format!("failed to render {}/{}", kind.plural(), term.slug))
        },
    )
}
