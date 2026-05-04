use std::path::Path;

use anyhow::{Context, Result};

use crate::template::vars::HomePageVars;

use super::BuildContext;
use super::listing::{ListedPage, collect_page_summaries, sort_pinned_first};
use super::paginate::{paginate_config, write_paginated};

/// Generates paginated home pages listing recent posts.
///
/// Skipped when `home.html` is not present in the template set.
pub(crate) fn build_home_pages(
    ctx: &BuildContext,
    listed_posts: &[ListedPage],
    output_dir: &Path,
) -> Result<()> {
    if !ctx.template_engine.has_template("home.html") {
        return Ok(());
    }

    let per_page = paginate_config(
        &ctx.config.params,
        &[&["home", "paginate"], &["paginate"]],
        10,
    );

    let home_url = format!("{}/", ctx.config.base_url.trim_end_matches('/'));

    let mut home_posts = listed_posts.to_vec();
    sort_pinned_first(&mut home_posts);

    write_paginated(
        &home_posts,
        per_page,
        "",
        output_dir,
        |pages, pagination| {
            let vars = HomePageVars {
                title: &ctx.config.title,
                description: &ctx.config.description,
                url: home_url.clone(),
                pages: collect_page_summaries(pages),
                pagination,
                config: &ctx.config,
            };
            ctx.template_engine
                .render_home(&vars)
                .context("failed to render home page")
        },
    )
}
