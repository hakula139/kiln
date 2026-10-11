use anyhow::{Context, Result};

use super::BuildContext;
use super::listing::{PreparedPage, format_page_date};
use crate::content::page::PageKind;
use crate::render::PageResources;
use crate::render::pipeline::render_page;
use crate::template::vars::{PageMetadata, PostTemplateVars};

/// Renders a post or standalone page with its template.
pub(super) fn render(ctx: &BuildContext, prepared: &PreparedPage) -> Result<String> {
    let page = prepared.page;
    let rendered = render_page(
        &page.body,
        &ctx.template_engine,
        &ctx.config,
        &ctx.render_options,
        &PageResources {
            source_dir: page.source_path.parent(),
            images: &ctx.image_resolver,
            assets: &ctx.static_assets,
            page_url: &prepared.summary.url,
            deployment_prefix: ctx.config.base_url.prefix(),
        },
        page.frontmatter.heading_numbering,
    )
    .with_context(|| format!("failed to render {}", page.source_path.display()))?;

    let page_css =
        ctx.stylesheets
            .page_url(ctx.config.base_url.prefix(), &ctx.static_assets, page)?;
    let vars = PostTemplateVars {
        metadata: PageMetadata {
            title: &prepared.summary.title,
            description: &prepared.summary.description,
            url: prepared.summary.url.as_str().into(),
        },
        featured_image: prepared.summary.featured_image.as_ref(),
        license: page.frontmatter.license.as_deref(),
        page_css,
        date: prepared.summary.date.as_deref(),
        updated: prepared
            .updated
            .map(|date| format_page_date(date, &ctx.config.timezone)),
        tags: &prepared.summary.tags,
        section: prepared.summary.section.as_ref(),
        assets: rendered.assets,
        content: &rendered.content_html,
        toc: &rendered.toc_html,
        config: &ctx.config,
    };

    match page.kind {
        PageKind::Page if ctx.template_engine.has_template("page.html") => {
            ctx.template_engine.render_page(&vars)
        }
        _ => ctx.template_engine.render_post(&vars),
    }
    .with_context(|| format!("failed to render {}", page.source_path.display()))
}
