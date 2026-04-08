use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use jiff::{Timestamp, tz::TimeZone};
use syntect::parsing::SyntaxSet;

use crate::config::Config;
use crate::content::discovery::discover_content;
use crate::content::frontmatter::FeaturedImage;
use crate::content::page::{Page, PageKind};
use crate::output::{clean_output_dir, copy_file, copy_static, write_output};
use crate::pagination::{PaginationVars, Paginator, page_url as pagination_url};
use crate::render::RenderOptions;
use crate::render::pipeline::render_page;
use crate::section::{Section, collect_sections, load_index_title};
use crate::taxonomy::{TaxonomyKind, TaxonomySet, Term, build_taxonomies};
use crate::template::{
    HomePageVars, LinkedTerm, PageGroup, PageSummary, PostTemplateVars, SectionPageVars,
    TaxonomyIndexVars, TemplateEngine, TermPageVars, TermSummary,
};
use crate::text::slugify;

/// Shared build state, created once per build invocation.
struct BuildContext {
    config: Config,
    time_zone: Option<TimeZone>,
    syntax_set: SyntaxSet,
    template_engine: TemplateEngine,
}

/// Internal listing model for build-time sorting and grouping.
#[derive(Debug, Clone)]
struct ListedPage {
    summary: PageSummary,
    timestamp: Option<Timestamp>,
    year: String,
}

impl ListedPage {
    fn into_summary(self) -> PageSummary {
        self.summary
    }
}

/// Builds the site from the given project root directory.
///
/// When `base_url_override` is provided, it replaces the `base_url` from
/// config. This is used by `kiln serve` to match the actual server port.
///
/// # Errors
///
/// Returns an error if configuration loading, content discovery, rendering,
/// or output writing fails.
pub fn build(root: &Path, base_url_override: Option<&str>) -> Result<()> {
    build_to(root, base_url_override, None)
}

/// Builds the site, optionally writing to a custom output directory.
///
/// Used by the dev server to build into a staging directory so the live
/// output stays intact until the new build succeeds.
pub(crate) fn build_to(
    root: &Path,
    base_url_override: Option<&str>,
    output_dir_override: Option<&Path>,
) -> Result<()> {
    let mut config = Config::load(root).context("failed to load config")?;
    if let Some(base_url) = base_url_override {
        base_url.clone_into(&mut config.base_url);
    }
    let time_zone = config
        .time_zone()
        .context("failed to resolve configured time zone")?;
    let syntax_set = two_face::syntax::extra_newlines();

    let site_templates = root.join("templates");
    let theme_dir = config.theme_dir(root);
    let theme_templates = theme_dir.as_ref().map(|d| d.join("templates"));

    if config.theme.is_none() {
        tracing::warn!("no theme configured; set `theme` in config.toml to use a theme");
    }
    if !site_templates.is_dir() && theme_templates.as_ref().is_none_or(|d| !d.is_dir()) {
        tracing::warn!("no templates found; provide templates/ or configure a theme");
    }

    let template_engine = TemplateEngine::new(Some(&site_templates), theme_templates.as_deref())
        .context("failed to initialize template engine")?;

    let ctx = BuildContext {
        config,
        time_zone,
        syntax_set,
        template_engine,
    };

    let content = discover_content(root)?;
    let output_dir =
        output_dir_override.map_or_else(|| root.join(&ctx.config.output_dir), Path::to_owned);

    clean_output_dir(&output_dir)?;

    // Theme static files first, then site static files (site overrides theme).
    if let Some(ref td) = theme_dir {
        copy_static(&td.join("static"), &output_dir)?;
    }
    copy_static(&root.join("static"), &output_dir)?;

    // Collect sections first so listed_page() can resolve section titles.
    let sections = collect_sections(&content.pages, &content.content_dir);
    let section_titles: HashMap<&str, &str> = sections
        .iter()
        .map(|s| (s.slug.as_str(), s.title.as_str()))
        .collect();

    // All listed pages are used for taxonomy lookups; posts are also reused
    // for the home page.
    let mut listed_posts = Vec::new();
    let mut listed_pages = Vec::new();
    for page in &content.pages {
        let Some(listed_page) = listed_page(
            page,
            &content.content_dir,
            &ctx.config.base_url,
            ctx.time_zone.as_ref(),
            &section_titles,
        ) else {
            continue;
        };
        if page.is_post() {
            listed_posts.push(listed_page.clone());
        }
        listed_pages.push(listed_page);
    }

    for page in &content.pages {
        build_page(
            &ctx,
            page,
            &content.content_dir,
            &output_dir,
            &section_titles,
        )?;
    }
    build_home_pages(&ctx, &listed_posts, &output_dir)?;
    build_posts_index(&ctx, &listed_posts, &content.content_dir, &output_dir)?;
    build_section_pages(
        &ctx,
        &sections,
        &content.pages,
        &content.content_dir,
        &output_dir,
        &section_titles,
    )?;
    build_taxonomy_pages(
        &ctx,
        &listed_pages,
        &content.pages,
        &content.content_dir,
        &output_dir,
    )?;

    println!("Build complete: {} page(s).", content.pages.len());
    Ok(())
}

// ── Single-page rendering ──

/// Builds an internal listed page model for taxonomy / listing generation.
///
/// Returns `None` if the output path cannot be computed (shouldn't happen
/// for pages that passed discovery).
fn listed_page(
    page: &Page,
    content_dir: &Path,
    base_url: &str,
    time_zone: Option<&TimeZone>,
    section_titles: &HashMap<&str, &str>,
) -> Option<ListedPage> {
    let output_path = page.output_path(content_dir).ok()?;
    let url = page_url(base_url, &output_path);
    let timestamp = page.frontmatter.date;
    let section = page_section(page, base_url, section_titles);
    let featured_image = resolve_featured_image(page.frontmatter.featured_image.as_ref(), &url);
    Some(ListedPage {
        summary: PageSummary {
            title: page.frontmatter.title.clone(),
            url,
            date: timestamp.map(|date| format_page_date(date, time_zone)),
            description: page
                .frontmatter
                .description
                .clone()
                .or_else(|| page.summary.clone())
                .unwrap_or_default(),
            featured_image,
            tags: linked_tags(&page.frontmatter.tags, base_url),
            section,
        },
        timestamp,
        year: timestamp
            .map(|date| page_year(date, time_zone))
            .unwrap_or_default(),
    })
}

/// Returns the section slug and listed page for posts that belong to a section.
fn section_listed_page<'a>(
    page: &'a Page,
    content_dir: &Path,
    base_url: &str,
    time_zone: Option<&TimeZone>,
    section_titles: &HashMap<&str, &str>,
) -> Option<(&'a str, ListedPage)> {
    let PageKind::Post {
        section: Some(section),
    } = &page.kind
    else {
        return None;
    };
    let listed_page = listed_page(page, content_dir, base_url, time_zone, section_titles)?;
    Some((section.as_str(), listed_page))
}

/// Builds a `LinkedTerm` for the page's section, if any.
fn page_section(
    page: &Page,
    base_url: &str,
    section_titles: &HashMap<&str, &str>,
) -> Option<LinkedTerm> {
    let PageKind::Post {
        section: Some(ref slug),
    } = page.kind
    else {
        return None;
    };
    let title = section_titles
        .get(slug.as_str())
        .copied()
        .unwrap_or(slug.as_str());
    Some(LinkedTerm {
        name: title.to_owned(),
        url: format!("{base_url}/posts/{slug}/"),
    })
}

/// Resolves a `FeaturedImage`'s `src` path against the page's output URL.
///
/// Absolute paths (starting with `/`) and external URLs (containing `://`)
/// are returned as-is. Relative paths are resolved against the page's
/// directory URL so that co-located assets like `assets/cover.webp` become
/// `/posts/section/slug/assets/cover.webp`.
fn resolve_featured_image(
    featured_image: Option<&FeaturedImage>,
    page_url: &str,
) -> Option<FeaturedImage> {
    let fi = featured_image?;
    let resolved_src = resolve_image_src(&fi.src, page_url);
    Some(FeaturedImage {
        src: resolved_src,
        ..fi.clone()
    })
}

fn resolve_image_src(src: &str, page_url: &str) -> String {
    if src.starts_with('/') || src.contains("://") {
        return src.to_owned();
    }
    let path = if let Some(scheme_end) = page_url.find("://") {
        let after_scheme = scheme_end + 3;
        page_url[after_scheme..]
            .find('/')
            .map_or(page_url, |i| &page_url[after_scheme + i..])
    } else {
        page_url
    };
    format!("{path}{src}")
}

/// Converts raw tag strings into `LinkedTerm`s with pre-computed URLs.
fn linked_tags(tags: &[String], base_url: &str) -> Vec<LinkedTerm> {
    tags.iter()
        .map(|tag| LinkedTerm {
            name: tag.clone(),
            url: format!("{base_url}/tags/{}/", slugify(tag)),
        })
        .collect()
}

/// Formats a page date for templates using the configured site time zone,
/// falling back to UTC when no site time zone is set.
fn format_page_date(date: Timestamp, time_zone: Option<&TimeZone>) -> String {
    let Some(time_zone) = time_zone else {
        return date.to_string();
    };
    let zoned = date.to_zoned(time_zone.clone());
    date.display_with_offset(zoned.offset()).to_string()
}

/// Returns the grouping year for a page date in the configured site time zone.
fn page_year(date: Timestamp, time_zone: Option<&TimeZone>) -> String {
    date.to_zoned(time_zone.cloned().unwrap_or(TimeZone::UTC))
        .year()
        .to_string()
}

/// Renders a single page and writes it to the output directory.
fn build_page(
    ctx: &BuildContext,
    page: &Page,
    content_dir: &Path,
    output_dir: &Path,
    section_titles: &HashMap<&str, &str>,
) -> Result<()> {
    let options = RenderOptions::from_params(&ctx.config.params);

    let rendered = render_page(
        &page.raw_content,
        &ctx.syntax_set,
        &ctx.template_engine,
        &options,
        page.source_path.parent(),
    )
    .with_context(|| format!("failed to render {}", page.source_path.display()))?;

    let output_path = page.output_path(content_dir).with_context(|| {
        format!(
            "failed to compute output path for {}",
            page.source_path.display()
        )
    })?;
    let url = page_url(&ctx.config.base_url, &output_path);

    let featured_image = resolve_featured_image(page.frontmatter.featured_image.as_ref(), &url);
    let vars = PostTemplateVars {
        title: &page.frontmatter.title,
        description: page
            .frontmatter
            .description
            .as_deref()
            .or(page.summary.as_deref())
            .unwrap_or(""),
        url: &url,
        featured_image,
        date: page
            .frontmatter
            .date
            .map(|date| format_page_date(date, ctx.time_zone.as_ref())),
        section: page_section(page, &ctx.config.base_url, section_titles),
        math: page.frontmatter.math,
        content: &rendered.content_html,
        toc: &rendered.toc_html,
        config: &ctx.config,
    };

    let html = match page.kind {
        PageKind::Page if ctx.template_engine.has_template("page.html") => {
            ctx.template_engine.render_page(&vars)
        }
        _ => ctx.template_engine.render_post(&vars),
    }
    .with_context(|| format!("failed to render {}", page.source_path.display()))?;

    let dest = output_dir.join(&output_path);
    write_output(&dest, &html).with_context(|| format!("failed to write {}", dest.display()))?;

    // Copy co-located assets (images, etc.) alongside the rendered page.
    if let Some(bundle_dir) = page.source_path.parent() {
        let asset_output_dir = dest.parent().expect("output file should have a parent");
        for asset in &page.assets {
            let relative = asset.strip_prefix(bundle_dir).with_context(|| {
                format!(
                    "asset {} is not under {}",
                    asset.display(),
                    bundle_dir.display()
                )
            })?;
            let asset_dest = asset_output_dir.join(relative);
            copy_file(asset, &asset_dest)
                .with_context(|| format!("failed to copy asset {}", asset.display()))?;
        }
    }

    Ok(())
}

/// Computes the canonical URL for a page from its output path.
///
/// For `index.html` pages (page bundles), returns the directory path with a
/// trailing slash. For other files, returns the file path as-is.
pub(crate) fn page_url(base_url: &str, output_path: &Path) -> String {
    let base = base_url.trim_end_matches('/');
    let rel = output_path.to_string_lossy();

    // index.html → directory URL with trailing slash
    if let Some(dir) = rel.strip_suffix("index.html") {
        format!("{base}/{dir}")
    } else {
        format!("{base}/{rel}")
    }
}

// ── Listing page generation ──

/// Generates paginated home pages listing recent posts.
///
/// Skipped when `home.html` is not present in the template set.
fn build_home_pages(
    ctx: &BuildContext,
    listed_posts: &[ListedPage],
    output_dir: &Path,
) -> Result<()> {
    if !ctx.template_engine.has_template("home.html") {
        return Ok(());
    }

    let per_page = paginate_config(&ctx.config.params, &["home", "paginate"])
        .or_else(|| paginate_config(&ctx.config.params, &["paginate"]))
        .unwrap_or(10);

    let home_url = format!("{}/", ctx.config.base_url.trim_end_matches('/'));

    write_paginated(
        listed_posts,
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

/// Generates the root `/posts/` index page listing all posts.
///
/// Uses the `section.html` template. The page title is read from
/// `content/posts/_index.md` if present, falling back to "Posts".
///
/// Skipped when `section.html` is not present in the template set.
fn build_posts_index(
    ctx: &BuildContext,
    listed_posts: &[ListedPage],
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    if !ctx.template_engine.has_template("section.html") {
        return Ok(());
    }

    let per_page = paginate_config(&ctx.config.params, &["section", "paginate"])
        .or_else(|| paginate_config(&ctx.config.params, &["paginate"]))
        .unwrap_or(10);

    let posts_dir = content_dir.join("posts");
    let title = load_index_title(&posts_dir).unwrap_or_else(|| "Posts".to_owned());

    let mut posts = listed_posts.to_vec();
    sort_by_date_desc(&mut posts);

    write_paginated(
        &posts,
        per_page,
        "/posts",
        output_dir,
        |pages, pagination| {
            let page_groups = group_by_year(pages);
            let vars = SectionPageVars {
                section_title: &title,
                section_slug: "posts",
                page_groups,
                pagination,
                config: &ctx.config,
            };
            ctx.template_engine
                .render_section(&vars)
                .context("failed to render posts index")
        },
    )
}

/// Generates paginated section listing pages.
///
/// Skipped when `section.html` is not present in the template set.
fn build_section_pages(
    ctx: &BuildContext,
    sections: &[Section],
    pages: &[Page],
    content_dir: &Path,
    output_dir: &Path,
    section_titles: &HashMap<&str, &str>,
) -> Result<()> {
    if !ctx.template_engine.has_template("section.html") {
        return Ok(());
    }

    let per_page = paginate_config(&ctx.config.params, &["section", "paginate"])
        .or_else(|| paginate_config(&ctx.config.params, &["paginate"]))
        .unwrap_or(10);

    // Build section → listed pages map.
    let mut section_posts: HashMap<&str, Vec<ListedPage>> = HashMap::new();
    for page in pages {
        let Some((section, listed_page)) = section_listed_page(
            page,
            content_dir,
            &ctx.config.base_url,
            ctx.time_zone.as_ref(),
            section_titles,
        ) else {
            continue;
        };
        section_posts.entry(section).or_default().push(listed_page);
    }

    for section in sections {
        let mut posts = section_posts
            .remove(section.slug.as_str())
            .unwrap_or_default();
        sort_by_date_desc(&mut posts);

        let section_base = format!("/posts/{}", section.slug);
        write_paginated(
            &posts,
            per_page,
            &section_base,
            output_dir,
            |pages, pagination| {
                let page_groups = group_by_year(pages);
                let vars = SectionPageVars {
                    section_title: &section.title,
                    section_slug: &section.slug,
                    page_groups,
                    pagination,
                    config: &ctx.config,
                };
                ctx.template_engine
                    .render_section(&vars)
                    .with_context(|| format!("failed to render section {}", section.slug))
            },
        )?;
    }

    Ok(())
}

/// Generates taxonomy index pages and paginated term pages.
fn build_taxonomy_pages(
    ctx: &BuildContext,
    listed_pages: &[ListedPage],
    pages: &[Page],
    content_dir: &Path,
    output_dir: &Path,
) -> Result<()> {
    let taxonomy_set = build_taxonomies(pages, Some(content_dir));

    let per_page = paginate_config(&ctx.config.params, &["paginate"]).unwrap_or(10);

    for taxonomy in &taxonomy_set.taxonomies {
        let kind = taxonomy.kind;
        let base_path = format!("/{}", kind.plural());

        // Resolve and sort pages for each term once (reused by index and term pages).
        let term_pages: Vec<Vec<ListedPage>> = taxonomy
            .terms
            .iter()
            .map(|term| resolve_term_pages(&taxonomy_set, kind, &term.slug, listed_pages))
            .collect();

        build_taxonomy_index(
            ctx,
            kind,
            &base_path,
            &taxonomy.terms,
            &term_pages,
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

/// Renders a taxonomy index page (e.g., `/tags/index.html`).
fn build_taxonomy_index(
    ctx: &BuildContext,
    kind: TaxonomyKind,
    base_path: &str,
    terms: &[Term],
    term_pages: &[Vec<ListedPage>],
    output_dir: &Path,
) -> Result<()> {
    let term_summaries: Vec<TermSummary> = terms
        .iter()
        .zip(term_pages)
        .map(|(term, pages)| TermSummary {
            name: term.name.clone(),
            slug: term.slug.clone(),
            url: format!("{base_path}/{}/", term.slug),
            pages: collect_page_summaries(pages.iter().cloned()),
        })
        .collect();

    let vars = TaxonomyIndexVars {
        kind: kind.plural(),
        singular: kind.singular(),
        terms: term_summaries,
        config: &ctx.config,
    };

    let html = ctx
        .template_engine
        .render_taxonomy(&vars)
        .with_context(|| format!("failed to render {} index", kind.plural()))?;

    let dest = output_dir.join(kind.plural()).join("index.html");
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

// ── Shared pagination helpers ──

/// Paginates items and writes rendered pages to the output directory.
///
/// For each page of the paginator, collects the items, creates pagination
/// vars, calls the render closure to produce HTML, and writes the result.
/// Always generates at least one page (even when empty).
fn write_paginated<T, F>(
    items: &[T],
    per_page: usize,
    base_path: &str,
    output_dir: &Path,
    mut render: F,
) -> Result<()>
where
    T: Clone,
    F: FnMut(Vec<T>, PaginationVars) -> Result<String>,
{
    let paginator = Paginator::new(items, per_page);

    for page_num in 1..=paginator.total_pages().max(1) {
        let page_items = paginator.page_items(page_num).to_vec();
        let pagination = PaginationVars::new(base_path, page_num, paginator.total_pages());

        let html = render(page_items, pagination)?;

        let rel_path = pagination_url(base_path, page_num);
        let dest = output_dir
            .join(rel_path.trim_start_matches('/'))
            .join("index.html");
        write_output(&dest, &html)
            .with_context(|| format!("failed to write {}", dest.display()))?;
    }

    Ok(())
}

/// Collects the template-facing page summaries from listed pages.
fn collect_page_summaries<I>(listed_pages: I) -> Vec<PageSummary>
where
    I: IntoIterator<Item = ListedPage>,
{
    listed_pages
        .into_iter()
        .map(ListedPage::into_summary)
        .collect()
}

/// Sorts listed pages by date descending (newest first, undated last).
fn sort_by_date_desc(pages: &mut [ListedPage]) {
    pages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
}

/// Reads a pagination count from a nested TOML params path.
///
/// `path` specifies the keys to traverse (e.g., `["home", "paginate"]` reads
/// `params.home.paginate`).
fn paginate_config(params: &toml::value::Table, path: &[&str]) -> Option<usize> {
    let (&first, rest) = path.split_first()?;
    let mut current: &toml::Value = params.get(first)?;
    for key in rest {
        current = current.get(key)?;
    }
    current.as_integer().and_then(|n| usize::try_from(n).ok())
}

/// Groups pages into year-based sections.
///
/// Assumes pages are already sorted by date descending. Consecutive pages
/// with the same year in the configured site time zone are grouped together.
fn group_by_year(pages: Vec<ListedPage>) -> Vec<PageGroup> {
    let mut groups: Vec<PageGroup> = Vec::new();

    for page in pages {
        let year = page.year.clone();

        if groups.last().is_none_or(|g| g.key != year) {
            groups.push(PageGroup {
                key: year,
                pages: Vec::new(),
            });
        }
        groups
            .last_mut()
            .expect("just pushed")
            .pages
            .push(page.into_summary());
    }

    groups
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::indoc;

    use super::*;

    use crate::test_utils::{PermissionGuard, copy_templates, template_dir, write_test_file};

    /// Writes a content page at `content/<rel_path>/index.md`.
    fn write_page(root: &Path, rel_path: &str, content: &str) {
        write_test_file(root, &format!("content/{rel_path}/index.md"), content);
    }

    /// Copies all test templates except those listed in `exclude`.
    fn copy_templates_except(dest: &Path, exclude: &[&str]) {
        let src = template_dir();
        fs::create_dir_all(dest).unwrap();
        for entry in fs::read_dir(&src).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if !name.to_str().is_some_and(|n| exclude.contains(&n)) {
                fs::copy(entry.path(), dest.join(&name)).unwrap();
            }
        }
    }

    // ── build ──

    #[test]
    fn build_no_content() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        assert!(output_dir.exists(), "output directory should exist");
        assert!(
            output_dir.join("tags").join("index.html").exists(),
            "should generate empty tags index"
        );
    }

    #[test]
    fn build_end_to_end() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
                title = "Test Site"
            "#},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello World"
                description = "A test post"
                date = "2026-02-24T12:34:56Z"
                +++

                ## First

                This is a test **post**.

                ## Second

                More content.
            "#},
        );

        build(root.path(), None).unwrap();

        let output = root
            .path()
            .join("public")
            .join("posts")
            .join("hello")
            .join("index.html");
        assert!(output.exists(), "output file should exist");

        let html = fs::read_to_string(&output).unwrap();

        // <head>
        assert!(
            html.contains("<title>Hello World - Test Site</title>"),
            "should have title, html:\n{html}"
        );
        assert!(
            html.contains(r#"<meta name="description" content="A test post">"#),
            "should have meta description, html:\n{html}"
        );
        assert!(
            html.contains(r#"<link rel="canonical" href="https://example.com/posts/hello/">"#),
            "should have canonical URL, html:\n{html}"
        );

        // <body>
        assert!(
            html.contains("<h1>Hello World</h1>"),
            "should have title heading, html:\n{html}"
        );
        assert!(
            html.contains("2026-02-24T12:34:56Z"),
            "should have date, html:\n{html}"
        );
        assert!(
            html.contains(r##"<a href="#first">First</a>"##),
            "should have ToC with links to headings, html:\n{html}"
        );
        assert!(
            html.contains("<p>This is a test <strong>post</strong>.</p>"),
            "should have rendered content, html:\n{html}"
        );
    }

    #[test]
    fn build_base_url_override() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
            "#},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );

        build(root.path(), Some("http://localhost:5456")).unwrap();

        let html = fs::read_to_string(
            root.path()
                .join("public")
                .join("posts")
                .join("hello")
                .join("index.html"),
        )
        .unwrap();
        assert!(
            html.contains("http://localhost:5456/posts/hello/"),
            "canonical URL should use overridden base_url, html:\n{html}"
        );
        assert!(
            !html.contains("https://example.com"),
            "should NOT use config base_url when overridden, html:\n{html}"
        );
    }

    #[test]
    fn build_copies_static_files() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        let static_dir = root.path().join("static");
        fs::create_dir_all(static_dir.join("images")).unwrap();
        fs::write(static_dir.join("favicon.ico"), "icon").unwrap();
        fs::write(static_dir.join("images").join("logo.png"), "logo").unwrap();

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        assert_eq!(
            fs::read_to_string(output_dir.join("favicon.ico")).unwrap(),
            "icon"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("images").join("logo.png")).unwrap(),
            "logo"
        );
    }

    #[test]
    fn build_copies_colocated_assets() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );
        let bundle = root.path().join("content").join("posts").join("hello");
        fs::create_dir_all(bundle.join("assets")).unwrap();
        fs::write(bundle.join("cover.webp"), "cover-data").unwrap();
        fs::write(bundle.join("assets").join("diagram.svg"), "svg-data").unwrap();

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public").join("posts").join("hello");
        assert_eq!(
            fs::read_to_string(output_dir.join("cover.webp")).unwrap(),
            "cover-data"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("assets").join("diagram.svg")).unwrap(),
            "svg-data"
        );
    }

    #[test]
    fn build_cleans_stale_output() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        let output_dir = root.path().join("public");
        fs::create_dir_all(output_dir.join("old")).unwrap();
        fs::write(output_dir.join("old").join("stale.html"), "stale").unwrap();

        build(root.path(), None).unwrap();

        assert!(
            !output_dir.join("old").exists(),
            "stale output should be removed"
        );
    }

    // ── build: theme ──

    /// Sets up a minimal theme for build tests.
    fn setup_theme(root: &Path, theme_name: &str) {
        let theme_dir = root.join("themes").join(theme_name);
        let tmpl_dir = theme_dir.join("templates");
        fs::create_dir_all(&tmpl_dir).unwrap();
        copy_templates(&tmpl_dir);
        fs::write(theme_dir.join("theme.toml"), "").unwrap();
    }

    #[test]
    fn build_with_theme() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                base_url = "https://example.com"
                title = "Test"
                theme = "my-theme"
            "#},
        )
        .unwrap();
        setup_theme(root.path(), "my-theme");

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let output = root
            .path()
            .join("public")
            .join("posts")
            .join("hello")
            .join("index.html");
        assert!(output.exists(), "output file should exist");
        let html = fs::read_to_string(&output).unwrap();
        assert!(
            html.contains("<h1>Hello</h1>"),
            "should render with theme templates, html:\n{html}"
        );
    }

    #[test]
    fn build_theme_static_files_with_site_override() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), r#"theme = "my-theme""#).unwrap();
        setup_theme(root.path(), "my-theme");

        let theme_static = root.path().join("themes/my-theme/static");
        fs::create_dir_all(&theme_static).unwrap();
        fs::write(theme_static.join("theme.css"), "theme-default").unwrap();
        fs::write(theme_static.join("shared.css"), "from-theme").unwrap();

        let site_static = root.path().join("static");
        fs::create_dir_all(&site_static).unwrap();
        fs::write(site_static.join("shared.css"), "from-site").unwrap();

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        assert_eq!(
            fs::read_to_string(output_dir.join("theme.css")).unwrap(),
            "theme-default",
            "theme-only static file should be copied"
        );
        assert_eq!(
            fs::read_to_string(output_dir.join("shared.css")).unwrap(),
            "from-site",
            "site static file should override theme"
        );
    }

    // ── build: page template ──

    #[test]
    fn build_uses_page_template_for_standalone() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "about-me",
            indoc! {r#"
                +++
                title = "About Me"
                +++
                Hello world.
            "#},
        );

        build(root.path(), None).unwrap();

        let output = root
            .path()
            .join("public")
            .join("about-me")
            .join("index.html");
        assert!(output.exists(), "should generate about-me page");
        let html = fs::read_to_string(&output).unwrap();
        assert!(
            html.contains(r#"<article class="page">"#),
            "should use page.html template, html:\n{html}"
        );
    }

    #[test]
    fn build_renders_dates_in_configured_timezone() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r#"
                timezone = "Asia/Shanghai"
            "#},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello"
                date = "2026-03-13T09:36:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let html = fs::read_to_string(
            root.path()
                .join("public")
                .join("posts")
                .join("note")
                .join("hello")
                .join("index.html"),
        )
        .unwrap();
        assert!(
            html.contains("2026-03-13T17:36:00+08:00"),
            "should render the configured time zone offset, html:\n{html}"
        );
        assert!(
            !html.contains("2026-03-13T09:36:00Z"),
            "should not leave the date in UTC, html:\n{html}"
        );
    }

    // ── build: home page ──

    #[test]
    fn build_generates_home_page() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let home = root.path().join("public").join("index.html");
        assert!(home.exists(), "should generate home page /index.html");
        let html = fs::read_to_string(&home).unwrap();
        assert!(
            html.contains("Hello"),
            "home page should list posts, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="http://localhost:5456/posts/note/hello/">Hello</a>"#),
            "home page should link to the post under /posts/, html:\n{html}"
        );
    }

    #[test]
    fn build_empty_home_page() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        build(root.path(), None).unwrap();

        let home = root.path().join("public").join("index.html");
        assert!(
            home.exists(),
            "should generate home page even with zero posts"
        );
    }

    #[test]
    fn build_orphan_posts_on_home_not_in_sections() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/sectioned",
            indoc! {r#"
                +++
                title = "Sectioned Post"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );
        write_page(
            root.path(),
            "posts/orphan",
            indoc! {r#"
                +++
                title = "Orphan Post"
                date = "2026-01-02T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let home_html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
        assert!(
            home_html.contains("Sectioned Post"),
            "sectioned post should also appear on home page, html:\n{home_html}"
        );
        assert!(
            home_html.contains("Orphan Post"),
            "orphan post should appear on home page, html:\n{home_html}"
        );

        let note_html = fs::read_to_string(
            root.path()
                .join("public")
                .join("posts")
                .join("note")
                .join("index.html"),
        )
        .unwrap();
        assert!(
            note_html.contains("Sectioned Post"),
            "sectioned post should appear in section page, html:\n{note_html}"
        );
        assert!(
            !note_html.contains("Orphan Post"),
            "orphan post should NOT appear in section page, html:\n{note_html}"
        );
    }

    #[test]
    fn build_skips_home_without_template() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates_except(&root.path().join("templates"), &["home.html"]);

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let home = root.path().join("public").join("index.html");
        assert!(
            !home.exists(),
            "should NOT generate home page without home.html template"
        );
    }

    #[test]
    fn build_home_pagination() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r"
                [params.home]
                paginate = 2
            "},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        for i in 1..=3 {
            write_page(
                root.path(),
                &format!("posts/note/post-{i}"),
                &format!(
                    indoc! {r#"
                        +++
                        title = "Post {i}"
                        date = "2026-01-0{i}T00:00:00Z"
                        +++
                        Body
                    "#},
                    i = i,
                ),
            );
        }

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        let page1 = output_dir.join("index.html");
        assert!(page1.exists(), "should generate home page 1");
        let html1 = fs::read_to_string(&page1).unwrap();
        assert!(
            html1.contains("Page 1 / 2"),
            "should show pagination, html:\n{html1}"
        );

        let page2 = output_dir.join("page").join("2").join("index.html");
        assert!(page2.exists(), "should generate home page 2");
    }

    #[test]
    fn build_standalone_excluded_from_home() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello Post"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );
        write_page(
            root.path(),
            "about-me",
            indoc! {r#"
                +++
                title = "About Me"
                +++
                Bio
            "#},
        );

        build(root.path(), None).unwrap();

        let html = fs::read_to_string(root.path().join("public").join("index.html")).unwrap();
        assert!(
            html.contains("Hello Post"),
            "home page should list posts, html:\n{html}"
        );
        assert!(
            !html.contains("About Me"),
            "home page should NOT list standalone pages, html:\n{html}"
        );
    }

    // ── build: posts index ──

    #[test]
    fn build_generates_posts_index() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/note/post-a",
            indoc! {r#"
                +++
                title = "Post A"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );
        write_page(
            root.path(),
            "posts/essay/post-b",
            indoc! {r#"
                +++
                title = "Post B"
                date = "2026-01-02T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let posts_index = root.path().join("public").join("posts").join("index.html");
        assert!(posts_index.exists(), "should generate /posts/index.html");
        let html = fs::read_to_string(&posts_index).unwrap();
        assert!(
            html.contains("Post A") && html.contains("Post B"),
            "posts index should list all posts across sections, html:\n{html}"
        );
    }

    #[test]
    fn build_posts_index_uses_index_title() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        let posts_dir = root.path().join("content").join("posts");
        fs::create_dir_all(&posts_dir).unwrap();
        fs::write(
            posts_dir.join("_index.md"),
            indoc! {r#"
                +++
                title = "文章"
                +++
            "#},
        )
        .unwrap();

        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello"
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let html = fs::read_to_string(root.path().join("public").join("posts").join("index.html"))
            .unwrap();
        assert!(
            html.contains("文章"),
            "should use _index.md title for posts index, html:\n{html}"
        );
    }

    #[test]
    fn build_posts_index_generated_even_when_empty() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        // Only standalone pages, no posts.
        write_page(
            root.path(),
            "about-me",
            indoc! {r#"
                +++
                title = "About Me"
                +++
                Bio
            "#},
        );

        build(root.path(), None).unwrap();

        let posts_index = root.path().join("public").join("posts").join("index.html");
        assert!(
            posts_index.exists(),
            "should generate /posts/index.html even with no posts"
        );
    }

    // ── build: section pages ──

    #[test]
    fn build_generates_section_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        for (section, name) in [("note", "post-a"), ("note", "post-b"), ("essay", "hello")] {
            write_page(
                root.path(),
                &format!("posts/{section}/{name}"),
                &format!(
                    indoc! {r#"
                        +++
                        title = "{name}"
                        date = "2026-01-01T00:00:00Z"
                        +++
                        Body
                    "#},
                    name = name,
                ),
            );
        }

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        let note_index = output_dir.join("posts").join("note").join("index.html");
        assert!(
            note_index.exists(),
            "should generate /posts/note/index.html"
        );
        let html = fs::read_to_string(&note_index).unwrap();
        assert!(
            html.contains("Note"),
            "should have section title, html:\n{html}"
        );
        assert!(
            html.contains("post-a") && html.contains("post-b"),
            "should list section posts, html:\n{html}"
        );
        assert!(
            html.contains(r#"<a href="http://localhost:5456/posts/note/post-a/">post-a</a>"#),
            "section page should link to posts under /posts/, html:\n{html}"
        );

        let essay_index = output_dir.join("posts").join("essay").join("index.html");
        assert!(
            essay_index.exists(),
            "should generate /posts/essay/index.html"
        );
    }

    #[test]
    fn build_skips_sections_without_template() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates_except(&root.path().join("templates"), &["section.html"]);

        write_page(
            root.path(),
            "posts/note/my-post",
            indoc! {r#"
                +++
                title = "My Post"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let section_index = root
            .path()
            .join("public")
            .join("posts")
            .join("note")
            .join("index.html");
        assert!(
            !section_index.exists(),
            "should NOT generate section pages without section.html template"
        );
    }

    #[test]
    fn build_section_uses_index_title() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        let section_dir = root.path().join("content").join("posts").join("note");
        fs::create_dir_all(&section_dir).unwrap();
        fs::write(
            section_dir.join("_index.md"),
            indoc! {r#"
                +++
                title = "笔记"
                +++
            "#},
        )
        .unwrap();

        write_page(
            root.path(),
            "posts/note/my-post",
            indoc! {r#"
                +++
                title = "My Post"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let html = fs::read_to_string(
            root.path()
                .join("public")
                .join("posts")
                .join("note")
                .join("index.html"),
        )
        .unwrap();
        assert!(
            html.contains("笔记"),
            "should use _index.md title, html:\n{html}"
        );
    }

    #[test]
    fn build_section_pagination() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r"
                [params.section]
                paginate = 2
            "},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        for i in 1..=3 {
            write_page(
                root.path(),
                &format!("posts/note/post-{i}"),
                &format!(
                    indoc! {r#"
                        +++
                        title = "Post {i}"
                        date = "2026-01-0{i}T00:00:00Z"
                        +++
                        Body
                    "#},
                    i = i,
                ),
            );
        }

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        let page1 = output_dir.join("posts").join("note").join("index.html");
        assert!(page1.exists(), "should generate section page 1");
        let html1 = fs::read_to_string(&page1).unwrap();
        assert!(
            html1.contains("Page 1 / 2"),
            "should show pagination, html:\n{html1}"
        );

        let page2 = output_dir
            .join("posts")
            .join("note")
            .join("page")
            .join("2")
            .join("index.html");
        assert!(page2.exists(), "should generate section page 2");
    }

    // ── build: taxonomies ──

    #[test]
    fn build_generates_taxonomy_index_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                tags = ["rust", "web"]
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        let tags_index = output_dir.join("tags").join("index.html");
        assert!(tags_index.exists(), "should generate /tags/index.html");
        let html = fs::read_to_string(&tags_index).unwrap();
        assert!(
            html.contains("rust") && html.contains("web"),
            "tags index should list terms, html:\n{html}"
        );
    }

    #[test]
    fn build_generates_term_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        for (name, tag) in [("post-1", "rust"), ("post-2", "rust"), ("post-3", "web")] {
            write_page(
                root.path(),
                &format!("posts/{name}"),
                &format!(
                    indoc! {r#"
                        +++
                        title = "{name}"
                        tags = ["{tag}"]
                        +++
                        Body
                    "#},
                    name = name,
                    tag = tag,
                ),
            );
        }

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        let rust_page = output_dir.join("tags").join("rust").join("index.html");
        assert!(rust_page.exists(), "should generate /tags/rust/index.html");
        let html = fs::read_to_string(&rust_page).unwrap();
        assert!(
            html.contains("post-1") && html.contains("post-2"),
            "term page should list posts, html:\n{html}"
        );
        assert!(
            !html.contains("post-3"),
            "term page should not include unrelated posts, html:\n{html}"
        );
    }

    #[test]
    fn build_generates_paginated_term_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("config.toml"),
            indoc! {r"
                [params]
                paginate = 2
            "},
        )
        .unwrap();
        copy_templates(&root.path().join("templates"));

        for i in 1..=3 {
            write_page(
                root.path(),
                &format!("posts/post-{i}"),
                &format!(
                    indoc! {r#"
                        +++
                        title = "Post {i}"
                        tags = ["rust"]
                        date = "2026-01-0{i}T00:00:00Z"
                        +++
                        Body
                    "#},
                    i = i,
                ),
            );
        }

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");

        // Page 1.
        let page1 = output_dir.join("tags").join("rust").join("index.html");
        assert!(page1.exists(), "should generate page 1");
        let html1 = fs::read_to_string(&page1).unwrap();
        assert!(
            html1.contains("Page 1 / 2"),
            "should show pagination, html:\n{html1}"
        );
        assert!(
            html1.contains("Next"),
            "page 1 should have next link, html:\n{html1}"
        );

        // Page 2.
        let page2 = output_dir
            .join("tags")
            .join("rust")
            .join("page")
            .join("2")
            .join("index.html");
        assert!(page2.exists(), "should generate page 2");
        let html2 = fs::read_to_string(&page2).unwrap();
        assert!(
            html2.contains("Page 2 / 2"),
            "should show page 2, html:\n{html2}"
        );
        assert!(
            html2.contains("Prev"),
            "page 2 should have prev link, html:\n{html2}"
        );
    }

    #[test]
    fn build_no_taxonomy_pages_without_tags() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let output_dir = root.path().join("public");
        let tags_index = output_dir.join("tags").join("index.html");
        assert!(
            tags_index.exists(),
            "should generate /tags/index.html even with no tags"
        );
    }

    #[test]
    fn build_taxonomy_correct_with_standalone_pages() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        copy_templates(&root.path().join("templates"));

        write_page(
            root.path(),
            "about-me",
            indoc! {r#"
                +++
                title = "About Me"
                +++
                Bio
            "#},
        );
        write_page(
            root.path(),
            "posts/note/hello",
            indoc! {r#"
                +++
                title = "Hello Post"
                tags = ["rust"]
                date = "2026-01-01T00:00:00Z"
                +++
                Body
            "#},
        );

        build(root.path(), None).unwrap();

        let term_page = root
            .path()
            .join("public")
            .join("tags")
            .join("rust")
            .join("index.html");
        assert!(term_page.exists(), "should generate /tags/rust/index.html");
        let html = fs::read_to_string(&term_page).unwrap();
        assert!(
            html.contains("Hello Post"),
            "term page should list the tagged post, html:\n{html}"
        );
        assert!(
            !html.contains("About Me"),
            "term page should NOT list standalone pages, html:\n{html}"
        );
    }

    // ── build: errors ──

    /// Creates a minimal site with one page for error-path tests.
    fn setup_site_with_page(root: &Path) {
        fs::write(root.join("config.toml"), "").unwrap();
        copy_templates(&root.join("templates"));
        write_page(
            root,
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        );
    }

    #[test]
    fn build_invalid_config_returns_error() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "{{invalid toml").unwrap();

        let err = format!("{:#}", build(root.path(), None).unwrap_err());
        assert!(
            err.contains("failed to load config"),
            "should report config failure, got: {err}"
        );
    }

    #[test]
    fn build_invalid_timezone_returns_error() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), r#"timezone = "Mars/Base""#).unwrap();

        let err = build(root.path(), None).unwrap_err();
        let chain: Vec<String> = err.chain().map(ToString::to_string).collect();
        assert!(
            chain
                .iter()
                .any(|message| message.contains("invalid timezone `Mars/Base` in config.toml")),
            "should report invalid timezone, got: {chain:?}"
        );
    }

    #[test]
    fn build_missing_templates_returns_error() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to initialize template engine"),
            "should report template engine failure, got: {err}"
        );
    }

    #[test]
    fn build_broken_post_template_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        fs::write(
            root.path().join("templates").join("post.html"),
            "{% invalid %}",
        )
        .unwrap();

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to render"),
            "should report render failure, got: {err}"
        );
    }

    #[test]
    fn build_write_permission_denied_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        build(root.path(), None).unwrap();
        let output_dir = root.path().join("public");
        let _guard = PermissionGuard::restrict(&output_dir, 0o555);

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to write") || err.contains("failed to clean"),
            "should report write or clean failure, got: {err}"
        );
    }

    #[test]
    fn build_asset_copy_permission_denied_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        let page_dir = root.path().join("content").join("posts").join("hello");
        fs::write(page_dir.join("image.png"), "img-data").unwrap();

        build(root.path(), None).unwrap();

        let page_output = root.path().join("public").join("posts").join("hello");
        let _guard = PermissionGuard::restrict(&page_output, 0o555);

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to copy asset") || err.contains("failed to clean"),
            "should report asset copy or clean failure, got: {err}"
        );
    }

    #[test]
    fn build_broken_directive_template_returns_error() {
        let root = tempfile::tempdir().unwrap();
        setup_site_with_page(root.path());

        let directives = root.path().join("templates").join("directives");
        fs::create_dir_all(&directives).unwrap();
        fs::write(
            directives.join("broken.html"),
            "{% for k, v in name | items %}{{ k }}{% endfor %}",
        )
        .unwrap();

        // Overwrite the page to include a broken directive.
        write_page(
            root.path(),
            "posts/hello",
            indoc! {r#"
                +++
                title = "Hello"
                +++
                ::: broken
                Body
                :::
            "#},
        );

        let err = build(root.path(), None).unwrap_err().to_string();
        assert!(
            err.contains("failed to render"),
            "should report render failure, got: {err}"
        );
    }

    // ── page_url ──

    #[test]
    fn page_url_index_html() {
        assert_eq!(
            page_url("https://example.com", Path::new("foo/bar/index.html")),
            "https://example.com/foo/bar/"
        );
    }

    #[test]
    fn page_url_root_index() {
        assert_eq!(
            page_url("https://example.com", Path::new("index.html")),
            "https://example.com/"
        );
    }

    #[test]
    fn page_url_non_index() {
        assert_eq!(
            page_url("https://example.com", Path::new("standalone.html")),
            "https://example.com/standalone.html"
        );
    }

    #[test]
    fn page_url_trailing_slash_base() {
        assert_eq!(
            page_url("https://example.com/", Path::new("foo/index.html")),
            "https://example.com/foo/"
        );
    }

    // ── resolve_featured_image ──

    fn make_featured_image(src: &str) -> FeaturedImage {
        FeaturedImage {
            src: src.into(),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_featured_image_absolute_path() {
        let fi = make_featured_image("/images/cover.webp");
        let resolved = resolve_featured_image(Some(&fi), "https://example.com/posts/foo/").unwrap();
        assert_eq!(resolved.src, "/images/cover.webp");
    }

    #[test]
    fn resolve_featured_image_relative_path() {
        let fi = make_featured_image("assets/cover.webp");
        let resolved =
            resolve_featured_image(Some(&fi), "https://example.com/posts/avg/on-looker/").unwrap();
        assert_eq!(resolved.src, "/posts/avg/on-looker/assets/cover.webp");
    }

    #[test]
    fn resolve_featured_image_external_url() {
        let fi = make_featured_image("https://cdn.example.com/img.jpg");
        let resolved = resolve_featured_image(Some(&fi), "https://example.com/posts/foo/").unwrap();
        assert_eq!(resolved.src, "https://cdn.example.com/img.jpg");
    }

    #[test]
    fn resolve_featured_image_preserves_metadata() {
        let fi = FeaturedImage {
            src: "/images/cover.webp".into(),
            position: Some("top".into()),
            credit: Some(crate::content::frontmatter::ImageCredit {
                title: Some("Work".into()),
                author: Some("Artist".into()),
                url: Some("https://example.com".into()),
            }),
        };
        let resolved = resolve_featured_image(Some(&fi), "https://example.com/posts/foo/").unwrap();
        assert_eq!(resolved.src, "/images/cover.webp");
        assert_eq!(resolved.position.as_deref(), Some("top"));
        let credit = resolved.credit.as_ref().unwrap();
        assert_eq!(credit.title.as_deref(), Some("Work"));
        assert_eq!(credit.author.as_deref(), Some("Artist"));
        assert_eq!(credit.url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn resolve_featured_image_none() {
        assert!(resolve_featured_image(None, "https://example.com/posts/foo/").is_none());
    }

    // ── Shared listing helper ──

    fn make_listed_page(title: &str, date: Option<&str>) -> ListedPage {
        let timestamp = date.map(|date| date.parse().unwrap());
        ListedPage {
            summary: PageSummary {
                title: title.into(),
                url: format!("/{title}/"),
                date: timestamp.map(|date: Timestamp| date.to_string()),
                description: String::new(),
                featured_image: None,
                tags: Vec::new(),
                section: None,
            },
            timestamp,
            year: timestamp
                .map(|date| page_year(date, None))
                .unwrap_or_default(),
        }
    }

    // ── sort_by_date_desc ──

    #[test]
    fn sort_by_date_desc_basic() {
        let mut pages = vec![
            make_listed_page("old", Some("2025-01-01T00:00:00Z")),
            make_listed_page("new", Some("2026-06-15T00:00:00Z")),
            make_listed_page("mid", Some("2026-01-01T00:00:00Z")),
        ];
        sort_by_date_desc(&mut pages);
        assert_eq!(pages[0].summary.title, "new");
        assert_eq!(pages[1].summary.title, "mid");
        assert_eq!(pages[2].summary.title, "old");
    }

    #[test]
    fn sort_by_date_desc_undated_last() {
        let mut pages = vec![
            make_listed_page("undated", None),
            make_listed_page("dated", Some("2026-01-01T00:00:00Z")),
        ];
        sort_by_date_desc(&mut pages);
        assert_eq!(pages[0].summary.title, "dated");
        assert_eq!(pages[1].summary.title, "undated");
    }

    #[test]
    fn sort_by_date_desc_uses_timestamp_not_rendered_string() {
        let mut pages = vec![
            make_listed_page("older", Some("2024-11-03T01:30:00-04:00")),
            make_listed_page("newer", Some("2024-11-03T01:15:00-05:00")),
        ];
        sort_by_date_desc(&mut pages);
        assert_eq!(pages[0].summary.title, "newer");
        assert_eq!(pages[1].summary.title, "older");
    }

    // ── page_year ──

    #[test]
    fn page_year_uses_configured_timezone() {
        let date: Timestamp = "2025-12-31T16:30:00Z".parse().unwrap();
        let time_zone = TimeZone::get("Asia/Shanghai").unwrap();
        assert_eq!(page_year(date, Some(&time_zone)), "2026");
        assert_eq!(page_year(date, None), "2025");
    }

    // ── paginate_config ──

    #[test]
    fn paginate_config_nested() {
        let params: toml::value::Table = toml::from_str(indoc! {r"
                [home]
                paginate = 8
            "})
        .unwrap();
        assert_eq!(paginate_config(&params, &["home", "paginate"]), Some(8));
    }

    #[test]
    fn paginate_config_flat() {
        let params: toml::value::Table = toml::from_str("paginate = 16").unwrap();
        assert_eq!(paginate_config(&params, &["paginate"]), Some(16));
    }

    #[test]
    fn paginate_config_missing_returns_none() {
        let params: toml::value::Table = toml::from_str("").unwrap();
        assert_eq!(paginate_config(&params, &["paginate"]), None);
    }

    #[test]
    fn paginate_config_negative_returns_none() {
        let params: toml::value::Table = toml::from_str("paginate = -1").unwrap();
        assert_eq!(paginate_config(&params, &["paginate"]), None);
    }

    #[test]
    fn paginate_config_empty_path_returns_none() {
        let params: toml::value::Table = toml::from_str("paginate = 10").unwrap();
        assert_eq!(paginate_config(&params, &[]), None);
    }

    // ── group_by_year ──

    #[test]
    fn group_by_year_basic() {
        let pages = vec![
            make_listed_page("a", Some("2026-03-01T00:00:00Z")),
            make_listed_page("b", Some("2026-01-15T00:00:00Z")),
            make_listed_page("c", Some("2025-12-01T00:00:00Z")),
        ];
        let groups = group_by_year(pages);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "2026");
        assert_eq!(groups[0].pages.len(), 2);
        assert_eq!(groups[1].key, "2025");
        assert_eq!(groups[1].pages.len(), 1);
    }

    #[test]
    fn group_by_year_undated_pages() {
        let pages = vec![
            make_listed_page("a", Some("2026-01-01T00:00:00Z")),
            make_listed_page("b", None),
        ];
        let groups = group_by_year(pages);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "2026");
        assert_eq!(groups[1].key, "", "undated pages should have empty key");
    }

    #[test]
    fn group_by_year_non_consecutive_same_year() {
        let pages = vec![
            make_listed_page("a", Some("2026-03-01T00:00:00Z")),
            make_listed_page("b", Some("2025-06-01T00:00:00Z")),
            make_listed_page("c", Some("2026-01-01T00:00:00Z")),
        ];
        let groups = group_by_year(pages);
        assert_eq!(groups.len(), 3, "groups consecutively, not globally");
        assert_eq!(groups[0].key, "2026");
        assert_eq!(groups[0].pages.len(), 1);
        assert_eq!(groups[1].key, "2025");
        assert_eq!(groups[1].pages.len(), 1);
        assert_eq!(groups[2].key, "2026");
        assert_eq!(groups[2].pages.len(), 1);
    }

    #[test]
    fn group_by_year_empty() {
        let groups = group_by_year(Vec::new());
        assert!(groups.is_empty());
    }
}
