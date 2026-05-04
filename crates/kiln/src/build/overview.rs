use std::path::Path;

use anyhow::{Context, Result};

use crate::output::write_output;
use crate::section::Section;
use crate::taxonomy::TaxonomySet;
use crate::template::vars::{BucketSummary, OverviewPageVars};

use super::BuildContext;
use super::listing::{ListingArtifacts, collect_page_summaries};

/// Generates overview index pages: `/sections/` and `/tags/`.
///
/// Skipped when `overview.html` is not present in the template set.
pub(crate) fn build_overview_pages(
    ctx: &BuildContext,
    artifacts: &ListingArtifacts,
    sections: &[Section],
    taxonomy_set: &TaxonomySet,
    output_dir: &Path,
) -> Result<()> {
    if !ctx.template_engine.has_template("overview.html") {
        return Ok(());
    }

    let section_buckets: Vec<BucketSummary> = sections
        .iter()
        .map(|section| {
            let pages = artifacts
                .section_posts
                .get(section.slug.as_str())
                .map(|posts| collect_page_summaries(posts.iter().cloned()))
                .unwrap_or_default();
            BucketSummary {
                name: section.title.clone(),
                slug: section.slug.clone(),
                url: format!("/posts/{}/", section.slug),
                pages,
            }
        })
        .collect();
    write_overview(ctx, "sections", "section", section_buckets, output_dir)?;

    let tag_buckets: Vec<BucketSummary> = taxonomy_set
        .tags
        .iter()
        .map(|term| {
            let pages = taxonomy_set
                .tag_pages
                .get(&term.slug)
                .map(|indices| {
                    collect_page_summaries(
                        indices
                            .iter()
                            .filter_map(|&idx| artifacts.listed_pages.get(idx))
                            .cloned(),
                    )
                })
                .unwrap_or_default();
            BucketSummary {
                name: term.name.clone(),
                slug: term.slug.clone(),
                url: format!("/tags/{}/", term.slug),
                pages,
            }
        })
        .collect();
    write_overview(ctx, "tags", "tag", tag_buckets, output_dir)?;

    Ok(())
}

fn write_overview(
    ctx: &BuildContext,
    kind: &str,
    singular: &str,
    buckets: Vec<BucketSummary>,
    output_dir: &Path,
) -> Result<()> {
    let vars = OverviewPageVars {
        kind,
        singular,
        buckets,
        config: &ctx.config,
    };

    let html = ctx
        .template_engine
        .render_overview(&vars)
        .with_context(|| format!("failed to render {kind} overview"))?;

    let dest = output_dir.join(kind).join("index.html");
    write_output(&dest, &html).with_context(|| format!("failed to write {}", dest.display()))
}
