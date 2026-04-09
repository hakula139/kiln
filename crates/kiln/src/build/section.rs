use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::section::{Section, load_index_title};
use crate::template::{SectionPageVars, TermSummary};

use super::BuildContext;
use super::listing::{ListedPage, collect_page_summaries, group_by_year};
use super::paginate::{paginate_config, write_paginated};
use super::taxonomy::build_taxonomy_index;

// ── Posts index ──

/// Generates the root `/posts/` index page listing all posts.
///
/// Uses the `section.html` template. The page title is read from
/// `content/posts/_index.md` if present, falling back to "Posts".
///
/// Posts must be pre-sorted by date descending. Skipped when `section.html`
/// is not present in the template set.
pub(crate) fn build_posts_index(
    ctx: &BuildContext,
    listed_posts: &[ListedPage],
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    if !ctx.template_engine.has_template("section.html") {
        return Ok(());
    }

    let per_page = section_per_page(&ctx.config.params);
    let posts_dir = content_dir.join("posts");
    let title = load_index_title(&posts_dir).unwrap_or_else(|| "Posts".to_owned());

    write_section_listing(
        ctx,
        &title,
        "posts",
        "/posts",
        listed_posts,
        per_page,
        output_dir,
    )
}

// ── Section pages ──

/// Generates the sections index page and paginated per-section listing pages.
///
/// The sections index (`/sections/`) reuses the taxonomy template to show all
/// sections with their recent posts. Individual section pages (`/posts/<slug>/`)
/// use `section.html`. Either page type is independently skipped when its
/// template is missing.
///
/// Section post buckets must be pre-sorted by date descending.
pub(crate) fn build_section_pages(
    ctx: &BuildContext,
    sections: &[Section],
    section_posts: &HashMap<String, Vec<ListedPage>>,
    output_dir: &Path,
) -> Result<()> {
    let has_section = ctx.template_engine.has_template("section.html");
    let has_taxonomy = ctx.template_engine.has_template("taxonomy.html");
    if !has_section && !has_taxonomy {
        return Ok(());
    }

    if has_taxonomy {
        build_sections_index(ctx, sections, section_posts, output_dir)?;
    }

    if has_section {
        let per_page = section_per_page(&ctx.config.params);

        for section in sections {
            let posts = section_posts
                .get(section.slug.as_str())
                .map(Vec::as_slice)
                .unwrap_or_default();
            let section_base = format!("/posts/{}", section.slug);
            write_section_listing(
                ctx,
                &section.title,
                &section.slug,
                &section_base,
                posts,
                per_page,
                output_dir,
            )?;
        }
    }

    Ok(())
}

/// Renders the `/sections/` index page listing all post sections.
fn build_sections_index(
    ctx: &BuildContext,
    sections: &[Section],
    section_posts: &HashMap<String, Vec<ListedPage>>,
    output_dir: &Path,
) -> Result<()> {
    let term_summaries: Vec<TermSummary> = sections
        .iter()
        .map(|section| {
            let pages = section_posts
                .get(section.slug.as_str())
                .map(|posts| collect_page_summaries(posts.iter().cloned()))
                .unwrap_or_default();
            TermSummary {
                name: section.title.clone(),
                slug: section.slug.clone(),
                url: format!("/posts/{}/", section.slug),
                pages,
            }
        })
        .collect();

    build_taxonomy_index(ctx, "sections", "section", term_summaries, output_dir)
}

// ── Shared helpers ──

/// Shared rendering path for `section.html` paginated listings.
///
/// Used for the root `/posts/` index and individual section pages
/// (e.g., `/posts/note/`).
fn write_section_listing(
    ctx: &BuildContext,
    title: &str,
    slug: &str,
    base_path: &str,
    posts: &[ListedPage],
    per_page: usize,
    output_dir: &Path,
) -> Result<()> {
    write_paginated(
        posts,
        per_page,
        base_path,
        output_dir,
        |pages, pagination| {
            let page_groups = group_by_year(pages);
            let vars = SectionPageVars {
                section_title: title,
                section_slug: slug,
                page_groups,
                pagination,
                config: &ctx.config,
            };
            ctx.template_engine
                .render_section(&vars)
                .with_context(|| format!("failed to render section {slug}"))
        },
    )
}

/// Reads the section pagination config with fallback to global.
fn section_per_page(params: &toml::value::Table) -> usize {
    paginate_config(params, &["section", "paginate"])
        .or_else(|| paginate_config(params, &["paginate"]))
        .unwrap_or(10)
}
