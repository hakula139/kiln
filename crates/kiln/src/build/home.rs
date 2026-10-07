use std::path::Path;

use anyhow::{Context, Result};

use super::BuildContext;
use super::listing::{PreparedPage, sort_pinned_first};
use super::paginate::{paginate_config, write_paginated};
use crate::pagination::paginated_url;
use crate::template::vars::{HomePageVars, PageMetadata};

/// Generates paginated home pages listing recent posts and returns the number written.
///
/// Skipped when `home.html` is not present in the template set.
pub(super) fn build_home_pages(
    ctx: &BuildContext,
    listed_posts: &[&PreparedPage],
    output_dir: &Path,
) -> Result<usize> {
    if !ctx.template_engine.has_template("home.html") {
        return Ok(0);
    }

    let per_page = page_size(ctx);

    let mut home_posts = listed_posts.to_vec();
    sort_pinned_first(&mut home_posts);

    write_paginated(
        &home_posts,
        per_page,
        Path::new(""),
        &ctx.deployment_prefix,
        output_dir,
        |pages, pagination| {
            let vars = HomePageVars {
                metadata: PageMetadata {
                    title: &ctx.config.title,
                    description: &ctx.config.description,
                    url: paginated_url(&ctx.config.base_url, pagination.current_page).into(),
                },
                pages: pages.iter().map(|page| &page.summary).collect(),
                pagination,
                config: &ctx.config,
            };
            ctx.template_engine
                .render_home(&vars)
                .context("failed to render home page")
        },
    )
}

pub(super) fn page_size(ctx: &BuildContext) -> usize {
    paginate_config(
        &ctx.config.params,
        &[&["home", "paginate"], &["paginate"]],
        10,
    )
}
