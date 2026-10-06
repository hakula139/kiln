use std::path::Path;

use anyhow::{Context, Result};

use crate::template::vars::ArchivePageVars;

use super::BuildContext;
use super::listing::{BucketKind, ListingBucket, group_by_year};
use super::paginate::{paginate_config, write_paginated};

/// Generates `/posts/`, section, and tag archives and returns the number of pages written.
///
/// Skipped when `archive.html` is not present in the template set.
pub(crate) fn build_archive_pages(
    ctx: &BuildContext,
    buckets: &[ListingBucket],
    output_dir: &Path,
) -> Result<usize> {
    if !ctx.template_engine.has_template("archive.html") {
        return Ok(0);
    }

    let section_per_page = paginate_config(
        &ctx.config.params,
        &[&["section", "paginate"], &["paginate"]],
        10,
    );
    let tag_per_page = paginate_config(&ctx.config.params, &[&["paginate"]], 10);

    let mut page_count = 0;
    for bucket in buckets {
        let per_page = match bucket.kind {
            BucketKind::Tag => tag_per_page,
            BucketKind::Posts | BucketKind::Section => section_per_page,
        };
        page_count += write_archive(ctx, bucket, per_page, output_dir)?;
    }

    Ok(page_count)
}

// ── Helpers ──

fn write_archive(
    ctx: &BuildContext,
    bucket: &ListingBucket,
    per_page: usize,
    output_dir: &Path,
) -> Result<usize> {
    let base_path = bucket.base_path();
    write_paginated(
        &bucket.pages,
        per_page,
        &base_path,
        output_dir,
        |pages, pagination| {
            let page_groups = group_by_year(pages);
            let vars = ArchivePageVars {
                kind: bucket.kind.plural(),
                singular: bucket.kind.singular(),
                name: &bucket.name,
                slug: &bucket.slug,
                page_groups,
                pagination,
                config: &ctx.config,
            };
            ctx.template_engine.render_archive(&vars).with_context(|| {
                format!(
                    "failed to render archive {}/{}",
                    bucket.kind.plural(),
                    bucket.slug,
                )
            })
        },
    )
}
