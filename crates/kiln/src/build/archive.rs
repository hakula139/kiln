use std::path::Path;

use anyhow::{Context, Result};

use crate::section::{Section, load_index_title};
use crate::taxonomy::TaxonomySet;
use crate::template::vars::ArchivePageVars;

use super::BuildContext;
use super::listing::{ListedPage, ListingArtifacts, group_by_year, resolve_term_pages};
use super::paginate::{paginate_config, write_paginated};

/// Generates all archive pages: `/posts/`, `/posts/<section>/`, and `/tags/<slug>/`.
///
/// Skipped when `archive.html` is not present in the template set.
pub(crate) fn build_archive_pages(
    ctx: &BuildContext,
    artifacts: &ListingArtifacts,
    sections: &[Section],
    taxonomy_set: &TaxonomySet,
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    if !ctx.template_engine.has_template("archive.html") {
        return Ok(());
    }

    let section_per_page = paginate_config(&ctx.config.params, &["section", "paginate"])
        .or_else(|| paginate_config(&ctx.config.params, &["paginate"]))
        .unwrap_or(10);

    let posts_title = load_index_title(&content_dir.join("posts"))
        .unwrap_or_else(|| ctx.i18n.t("all_posts").into_owned());
    write_archive(
        ctx,
        &ArchiveSpec::new("posts", "post", &posts_title, "posts", "/posts"),
        &artifacts.listed_posts,
        section_per_page,
        output_dir,
    )?;

    for section in sections {
        let posts = artifacts
            .section_posts
            .get(section.slug.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        let base_path = format!("/posts/{}", section.slug);
        write_archive(
            ctx,
            &ArchiveSpec::new(
                "sections",
                "section",
                &section.title,
                &section.slug,
                &base_path,
            ),
            posts,
            section_per_page,
            output_dir,
        )?;
    }

    let tag_per_page = paginate_config(&ctx.config.params, &["paginate"]).unwrap_or(10);
    for taxonomy in &taxonomy_set.taxonomies {
        let kind = taxonomy.kind;
        for term in &taxonomy.terms {
            let pages = resolve_term_pages(taxonomy_set, kind, &term.slug, &artifacts.listed_pages);
            let base_path = format!("/{}/{}", kind.plural(), term.slug);
            write_archive(
                ctx,
                &ArchiveSpec::new(
                    kind.plural(),
                    kind.singular(),
                    &term.name,
                    &term.slug,
                    &base_path,
                ),
                &pages,
                tag_per_page,
                output_dir,
            )?;
        }
    }

    Ok(())
}

// ── Helpers ──

struct ArchiveSpec<'a> {
    kind: &'a str,
    singular: &'a str,
    name: &'a str,
    slug: &'a str,
    base_path: &'a str,
}

impl<'a> ArchiveSpec<'a> {
    fn new(
        kind: &'a str,
        singular: &'a str,
        name: &'a str,
        slug: &'a str,
        base_path: &'a str,
    ) -> Self {
        Self {
            kind,
            singular,
            name,
            slug,
            base_path,
        }
    }
}

fn write_archive(
    ctx: &BuildContext,
    spec: &ArchiveSpec<'_>,
    listed_pages: &[ListedPage],
    per_page: usize,
    output_dir: &Path,
) -> Result<()> {
    write_paginated(
        listed_pages,
        per_page,
        spec.base_path,
        output_dir,
        |pages, pagination| {
            let page_groups = group_by_year(pages);
            let vars = ArchivePageVars {
                kind: spec.kind,
                singular: spec.singular,
                name: spec.name,
                slug: spec.slug,
                page_groups,
                pagination,
                config: &ctx.config,
            };
            ctx.template_engine
                .render_archive(&vars)
                .with_context(|| format!("failed to render archive {}/{}", spec.kind, spec.slug))
        },
    )
}
