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
- Static file copying, co-located content assets, and per-page CSS bundling
- Home pages, section pages, standalone pages, taxonomy indexes, and paginated term pages
- Configurable site time zones for rendered dates
- RSS 2.0 feeds (main site + per-section + per-taxonomy-term)
- Sitemap (`sitemap.xml`) and `robots.txt`
- Custom 404 error page (optional, template-driven)
- Full-text search via Pagefind (post-build indexing, `[search]` config)
- Optional HTML / CSS / JS minification via `kiln build --minify` (lightningcss, oxc_minifier, minify-html)

### Internationalization

- Layered i18n resolver (site override → theme active language → theme English)
- `{{ t("key") }}` template function with `{name}` keyword-argument interpolation
- `[[menu.main]].name` fields treated as i18n keys, resolved via `t()` by themes
- `kiln init-theme` scaffolds `i18n/en.toml` and `i18n/zh-Hans.toml` with example keys

### Theming and Extensibility

- [IgnIt](https://github.com/hakula139/IgnIt) default theme (Tailwind CSS v4)
  - Glassmorphism panels with cursor-tracking glow, configurable background image
  - Dark / light mode (system preference + manual toggle, flash-free)
  - Responsive layout, home page image cards with hover reveal
  - Pagefind search modal, link card directive, modern favicon set
  - Back-to-top button, mobile menu animations, print styles
  - Keyboard focus-visible styling, `prefers-reduced-motion` support
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

### Authoring Enhancements

- Code block attributes: `title="..."`, collapse / fold, line highlighting (`highlight="1,3-5"`)
- Bundled scripts: `register_script()` mechanism for directive templates (replaces inline `<script>` workaround)

### Runtime / Ergonomics Polish

- Load optional assets conditionally for features such as KaTeX or Mermaid
- Validate `output_dir` is a safe relative path (prevent `remove_dir_all` on absolute paths)
- Make small authoring and tooling improvements discovered through day-to-day site work

## Later

- Demo / example site material once the core workflow feels stable
- Additional engine work only when it solves concrete problems in the publishing workflow

## Not the Goal Right Now

- Chasing one-to-one Hugo feature parity
- Full multi-language site generation
- Adding build-system complexity before the core publishing workflow feels complete
- Expanding scope faster than real usage justifies
