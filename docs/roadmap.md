# Roadmap

kiln is still early, but it is already usable for real publishing. This roadmap is the high-level product view: it should show what works, what is being built next, and what is intentionally out of scope for now.

The project direction is simple:

- Keep the authoring experience strong for CJK-heavy writing, technical posts, and custom content components.
- Finish the publishing workflow before expanding into broader platform scope.
- Keep the architecture understandable. New features should fit the current model instead of forcing large abstractions too early.

## Working Well Today

### Authoring

- TOML frontmatter
- GitHub Flavored Markdown
- KaTeX math
- CJK-aware heading IDs and table of contents generation
- `:::` directives with theme-template rendering
- Image attributes, emoji / icon shortcodes, and code-block presentation helpers

### Site Generation

- Pretty URLs
- Static file copying and co-located content assets
- Home pages, section pages, standalone pages, taxonomy indexes, and paginated term pages
- Configurable site time zones for rendered dates

### Theming and Extensibility

- [IgnIt](https://github.com/hakula139/IgnIt) default theme: Tailwind CSS v4, glassmorphism panels, home page image cards with hover effects, responsive layout, dark mode (system preference + manual toggle, flash-free)
- Layered MiniJinja templates with site-level overrides
- Theme parameter merging
- Directive template helpers such as `read_file()` and `parse_csv()`
- Navigation menu via `[[menu.main]]` config (sorted by weight, external link support)
- Page summaries with tags and featured images for list templates (home, section, term)

### Tooling

- `kiln build`
- `kiln serve` with file watching and live reload
- `kiln convert` for Hugo-to-kiln content conversion

## Current Focus

### Complete the Publishing Surface

- RSS feeds
- Sitemap
- 404 page
- Full-text search via Pagefind

These are the most important remaining gaps for a complete, self-hosted publishing workflow.

### Engine Extensibility

- Config-driven taxonomies (replace hardcoded `TaxonomyKind` enum with `[[taxonomies]]` config)

## Next Phase

### Build / Asset Pipeline

- Add `kiln build --minify`
- Minify CSS and JS in a Rust-native way
- Keep bundling optional and only add it if real theme usage justifies it

The goal is better production output without making the default build pipeline heavy or frontend-tooling-driven.

### Internationalization

- Add theme and site i18n tables
- Expose a template-level `i18n()` lookup
- Support one active language per build

The immediate goal is to remove hardcoded theme strings and make localization practical, not to build a full multi-language site system yet.

### Runtime / Ergonomics Polish

- Load optional assets conditionally for features such as KaTeX or Mermaid
- Add output validation and safety checks where real usage shows gaps
- Make small authoring and tooling improvements discovered through day-to-day site work

## Later

- Demo / example site material once the core workflow feels stable
- Further theme and authoring polish that proves itself during real publishing use
- Additional engine work only when it solves concrete problems in the publishing workflow

## Not the Goal Right Now

- Chasing one-to-one Hugo feature parity
- Full multi-language site generation
- Adding build-system complexity before the core publishing workflow feels complete
- Expanding scope faster than real usage justifies
