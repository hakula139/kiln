use std::path::Path;

use anyhow::{Context, Result};

use super::BuildContext;
use super::bucket::Listing;
use super::listing::group_by_year;
use super::paginate::{paginated_path, write_paginated};
use crate::template::vars::{ArchivePageVars, PageMetadata};
use crate::url::page_url;

/// Generates `/posts/`, section, and tag archives and returns the number of pages written.
///
/// Skipped when `archive.html` is not present in the template set.
pub(super) fn build_archive_pages(
    ctx: &BuildContext,
    buckets: &[Listing<'_>],
    output_dir: &Path,
) -> Result<usize> {
    if !ctx.template_engine.has_template("archive.html") {
        return Ok(0);
    }

    let mut page_count = 0;
    for bucket in buckets {
        page_count += write_archive(ctx, bucket, output_dir)?;
    }

    Ok(page_count)
}

// ── Helpers ──

fn write_archive(ctx: &BuildContext, bucket: &Listing<'_>, output_dir: &Path) -> Result<usize> {
    let base_path = bucket.path();
    write_paginated(
        &bucket.pages,
        bucket.kind.page_size(&ctx.config.paginate).get(),
        &base_path,
        ctx.config.base_url.prefix(),
        output_dir,
        |pages, pagination| {
            let page_groups = group_by_year(pages);
            let vars = ArchivePageVars {
                metadata: PageMetadata {
                    title: &bucket.name,
                    description: &ctx.config.description,
                    url: page_url(
                        &ctx.config.base_url,
                        &paginated_path(&base_path, pagination.current_page),
                    )
                    .into(),
                },
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
