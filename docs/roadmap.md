# Roadmap

kiln powers [hakula.xyz](https://hakula.xyz). This is the developer overview of implemented capabilities, current constraints and planned work. The project follows the site's publishing needs, with CJK technical writing and an understandable engine as priorities. The [README](../README.md) presents selected highlights and installation instructions.

## Implemented Capabilities

### Authoring and Rendering

- CommonMark with tables, strikethrough, task lists and TOML frontmatter.
- CJK-aware heading IDs, explicit heading attributes, a table of contents and optional hierarchical heading numbering. Heading and footnote IDs share a page-wide namespace across nested directives.
- Footnotes with reference-order numbering and return links for repeated references. The page body and each directive body have separate definition scopes.
- Syntax highlighting for 200+ languages, line numbers, code titles, selected-line highlighting and native collapse / expand controls. Themes can honor a configured visible-line limit.
- Inline and display math markup for KaTeX, plus Mermaid code fences. The page records which runtimes it needs, and the theme supplies client-side rendering.
- Block image figures with captions, inline images, and authored IDs, classes and dimensions. Images receive lazy loading and asynchronous decoding.
- Nested `:::` directives, built-in callouts, generic div wrappers and custom template renderers with positional / named arguments. Code blocks and code spans retain literal shortcode and directive text.
- Optional emoji and Font Awesome shortcodes. Short table columns can receive a no-wrap class using CJK-aware text widths.

The main table of contents includes page-body headings. Headings inside directives retain IDs and numbering but stay outside that outline. Custom directives and raw HTML are authored site content, so their output depends on the site's templates and styles. See the [Syntax Reference](syntax.md) for authoring rules.

### Content, Listings and Discovery

- Sectioned posts, posts without a section, and standalone pages, with pretty URLs and frontmatter slug overrides.
- Page bundles that keep Markdown, images, data and stylesheets together. Private underscore-prefixed inputs remain unpublished, and nested bundles own their own assets.
- Draft exclusion and listing metadata through `_index.md`. Content files without TOML frontmatter are skipped.
- Paginated home, all-posts, section and tag archives, with year-grouped archive entries and section / tag overview pages. Tag archives include tagged standalone pages.
- Home-page pinning through frontmatter weights. Archives and feeds retain date order.
- Publication and update timestamps rendered in the configured time zone, with optional Git-derived updates when authored metadata omits them.
- Shared page descriptions from frontmatter or text before `<!--more-->`, used in page and listing templates.
- RSS feeds for the site, all posts, each section and each tag. Sitemaps include emitted HTML routes, and builds also produce `robots.txt` and an optional template-based 404 page.
- Optional full-text search indexing through Pagefind, with the search interface supplied by the theme.

Home, archive, overview and 404 generation depend on the corresponding templates. Non-bundle Markdown files cannot own co-located assets or page CSS. Tags are the supported taxonomy, and frontmatter dates represent absolute instants. Git-derived updates require the Git executable and full repository history. See [Content](content.md) and [Template Variables](themes.md#template-variables).

### Assets, Stylesheets and Images

- Shared `assets/`, output-root `static/` files and bundle-local assets, with defined site / theme override precedence.
- Shared and page-owned stylesheet entries, local imports, CSS nesting and source-relative asset URL resolution. Page CSS is linked only from its owning page.
- Built-in plain CSS processing and optional Tailwind compilation through the external processor. Page entries can reuse shared theme definitions without duplicating the shared stylesheet.
- Content-hashed CSS / JS URLs, including compiled stylesheets, with template helpers that account for deployment path prefixes.
- Optional HTML / CSS / JS minification in Rust. Files already named `*.min.css` or `*.min.js` are preserved, and unsupported inputs retain their original bytes with a warning.
- Natural dimensions for resolvable local images and small WebP loading placeholders for supported decodable formats. Featured images expose the same metadata to page and listing templates.
- Optional native AVIF decoding for placeholders. AVIF publication and dimension reads work without that feature, and the Nix package enables it by default.

Image processing does not fetch remote images or generate responsive image variants. Missing or undecodable images may have no placeholder. Fingerprints cover individual files, so ordinary public CSS / JS imports need separate bundling when their dependencies also need content-hashed URLs. Plain CSS needs no external compiler. Tailwind requires its processor and Node.js outside the supplied Nix environment. See [Assets and Stylesheets](assets.md) and [Image Rendering](themes.md#image-rendering).

### Themes, Templates and Localization

- MiniJinja page, listing and directive templates, with site files overriding theme files at the same path.
- Theme defaults merged recursively with site parameters, named navigation menus sorted by weight, and minimum kiln version checks for themes.
- Shared metadata across page and listing contexts, including canonical URLs, linked tags / sections and resolved featured images.
- Template helpers for translations, asset URLs, timestamps and CSV data. Directive templates can read page-local files and register scripts for the current page.
- Page-scoped math / Mermaid feature detection and script registration. Identical script declarations are deduplicated, and conflicting declarations fail the build.
- Flat TOML translation files with site active language → theme active language → theme English fallback, named placeholder interpolation and literal-label fallback for missing keys.
- `kiln init-theme` scaffolding for templates, a stylesheet entry and English / Simplified Chinese translation files.

The engine supplies content and template contracts. [IgnIt](https://github.com/hakula139/IgnIt) owns reader-facing layouts, dark mode, search interaction, comments and other presentation behavior. A theme shipping translations must include `en.toml`. Localization selects one language for theme strings and does not create separate per-language content trees. See [Themes](themes.md).

### Builds and Development

- `kiln build` generates a static output directory, with optional minification and a base-URL override for preview deployments.
- Deployment path prefixes and encoded filename components are handled consistently in generated page, listing, feed, sitemap and asset URLs.
- Output validation protects the project root, source trees and repository metadata from being overwritten. Route planning reports collisions between content, generated pages and public files before publishing.
- Builds prepare output separately and publish it after rendering and optional search indexing succeed, preserving the previous output when a build fails.
- `kiln serve` provides local preview, directory redirects, a custom 404 response and browser live reload after successful rebuilds. Preview builds use the local server URL and skip minification.
- Rust unit / integration tests cover rendering and build behavior. [Benchmarks](benchmarks.md) cover representative rendering, discovery and full-build workloads.
- Cargo and Nix installation paths, with Nix packages for the engine, Pagefind and the Tailwind processor. Release validation generates npm base / extended entry packages and platform binaries. npm publication remains pending the first release and registry setup.

Pagefind is an external executable when indexing is enabled. Live reload watches the source directories that exist at startup. Creating a new top-level source directory, switching themes, changing the output directory or changing imports outside watched trees requires restarting the server. Builds currently regenerate the site as a whole. See [Usage](../README.md#usage) and [Live Reload](assets.md#live-reload).

### Hugo Content Migration

- `kiln convert` migrates content and static files between separate site roots while preserving existing destination files.
- Supported YAML frontmatter becomes TOML. Category indexes become post-section indexes, and tag indexes keep their location.
- Admonition, image and Mermaid shortcodes receive native equivalents. Other standalone shortcodes become directives for matching theme templates.
- Unsupported metadata produces omission warnings. Malformed recognized frontmatter and unsupported content-bearing shortcodes fail conversion.

Configuration, theme templates and unsupported Hugo behavior require manual migration. Files without recognized frontmatter are copied unchanged and need TOML frontmatter before kiln publishes them. Source and destination roots must not overlap. See [Hugo Content Migration](content.md#hugo-content-migration).

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
