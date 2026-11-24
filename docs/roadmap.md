# Roadmap

kiln powers [hakula.xyz](https://hakula.xyz). This is the developer overview of current constraints, maintenance priorities and planned work. The project follows the site's publishing needs, with CJK technical writing and an understandable engine as priorities.

## Shipped Areas

Detailed feature behavior lives in the reference guides:

- [Syntax Reference](syntax.md) covers Markdown extensions, syntax highlighting, math, Mermaid, images and `:::` directives.
- [Content](content.md) covers content organization, page bundles, frontmatter, listings, feeds and Hugo content migration.
- [Assets and Stylesheets](assets.md) covers asset publication, stylesheet compilation, content hashing, minification and live reload.
- [Themes](themes.md) covers site configuration, template variables, functions, image placeholders and localization.
- [README](../README.md) covers highlights, installation paths and CLI usage.
- [Benchmarks](benchmarks.md) covers rendering, shortcode, image, stylesheet and full-build workloads.

## Current Constraints

### Authoring and Rendering

- The main table of contents includes page-body headings, while headings inside directives keep IDs and numbering outside that outline.
- Custom directives and raw HTML are authored site content, so their output depends on the site's templates and styles.

### Content, Listings and Discovery

- Home, archive, overview and 404 generation require their corresponding templates.
- Non-bundle Markdown files cannot own co-located assets or page CSS.
- Tags are the supported taxonomy, and frontmatter dates represent absolute instants.
- Git-derived updates require the Git executable and full repository history.

### Assets, Stylesheets and Images

- Image processing uses local files and does not generate responsive variants. Missing or undecodable images may have no placeholder.
- References inside JavaScript, SVG and plain public CSS are not rewritten, so those files cannot reach content-hashed outputs, and module scripts that import or are imported keep source names.
- Plain CSS needs no external compiler, while Tailwind requires its processor and Node.js outside the supplied Nix environment.

### Themes and Localization

- [IgnIt](https://github.com/hakulab/IgnIt) owns reader-facing layouts, dark mode, search interaction, comments and other presentation behavior.
- A theme shipping translations must include `en.toml`. Localization selects one language for theme strings and does not create separate per-language content trees.

### Builds, Development and Migration

- Supports Unix platforms (Linux and macOS, including WSL on Windows). Native Windows builds are unsupported.
- Search requires the external Pagefind executable when enabled.
- Builds regenerate the whole site.
- Live reload watches source directories present at startup. New top-level source directories, theme or output-directory changes and imports outside watched trees require a restart.
- `kiln convert` migrates a defined subset of Hugo content and static files between non-overlapping roots, leaving configuration, theme templates and unsupported Hugo features to manual migration. Files without recognized frontmatter are copied unchanged and need TOML frontmatter before kiln publishes them.

## Current Focus

Review and simplify the existing engine before adding features:

- Keep route construction, asset ownership and template metadata consistent across individual pages, listings, feeds and sitemaps.
- Reduce duplicated parsing and rendering logic while preserving authoring behavior, particularly across nested directives, images and footnotes.
- Keep publication failures actionable and preserve prior output. Exercise these boundaries with behavioral tests and real-site builds.
- Keep public APIs, documentation and test suites proportionate to the requirements they serve.

These are maintenance priorities for existing capabilities. New engine work should address a concrete publishing need.

## Planned Work

A demo site showing the supported authoring and publishing workflow remains planned. It should make theme integration and content examples easier to evaluate. Further feature priorities will follow actual site usage.

## Intentional Boundaries

- Full Hugo compatibility is outside the current scope. The converter supports a defined migration subset.
- Separate per-language site generation is outside the current plan. Theme-string localization is implemented.
- Reader interactions belong to themes. The engine provides rendered content, metadata and asset declarations for those interfaces.
