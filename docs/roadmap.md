# Roadmap

kiln is a small static site generator built for [hakula.xyz](https://hakula.xyz) and already powering it day to day. This page is the high-level product view — what works today, what's being built next, and what is intentionally out of scope.

The project's shape is deliberate:

- Make CJK-heavy writing, technical posts, and custom content components a pleasure to author.
- Finish the publishing workflow before reaching for broader platform scope.
- Keep the architecture understandable — new features fit the current model.

## What Works Today

### Writing

- TOML frontmatter, GitHub Flavored Markdown, and KaTeX math out of the box.
- CJK-aware heading IDs and table of contents — Chinese / Japanese / Korean headings stay linkable.
- `:::` directive blocks rendered through theme templates: callouts, link cards, music embeds, anything you can template.
- Image attributes, emoji and Font Awesome icon shortcodes, and rich code-block presentation helpers.
- Pandoc-style code-block attributes — `` ```rust {title="src/main.rs" highlight="1,3-5" collapse} `` for titles, line highlighting, and forced collapse / expand.
- Mermaid diagrams via `` ```mermaid `` fences — themes load mermaid.js only on pages that contain a diagram.

### Publishing

- Pretty URLs, page bundles with co-located assets, and per-page CSS injection.
- Build-time image pipeline: every local image gets natural `width` / `height` plus a base64 WebP placeholder for instant first paint.
- Home, section, standalone, and paginated taxonomy / term pages.
- Pinned posts on the home page via a `weight` frontmatter field — hero pieces stay above the fold without affecting archive, tag, or RSS order.
- Time-zone-aware dates rendered in your site's local time.
- RSS 2.0 feeds for the whole site, each section, and each taxonomy term.
- Sitemap, `robots.txt`, and an optional template-driven 404 page.
- Full-text search via [Pagefind](https://pagefind.app), wired in at build time.
- Content-hashed CSS / JS URLs resolved from the merged static asset tree.
- Optional HTML / CSS / JS minification with `kiln build --minify` — pure Rust, no Node toolchain required.
- Page-scoped asset registry: themes load KaTeX, Mermaid, search, and other scripts only on pages that need them, no frontmatter flag required.
- `output_dir` validation prevents writing outside the project root.

### Reader Experience

- Comments via [Twikoo](https://twikoo.js.org/) in the IgnIt theme — per-post threads behind a provider switch, so other backends can drop in.

### Internationalization

- Translatable theme strings via layered TOML files: site override → theme language → English fallback, so partial translations degrade gracefully.
- `{{ t("key", name=value) }}` template helper with placeholder interpolation.
- Navigation menu labels resolve through the same i18n tables as the rest of your strings.
- `kiln init-theme` scaffolds starter `en.toml` and `zh-Hans.toml` files for new themes.

### Theming

- Layered MiniJinja templates: site files transparently override theme files.
- Named `[[menu.<group>]]` blocks with per-group `weight` sorting — themes pick which groups to render.
- Deep parameter merging for nested theme config tables.

The default theme [**IgnIt**](https://github.com/hakula139/IgnIt) ships with Tailwind CSS v4 and a polished feature set:

- Glassmorphism panels with a configurable background image and optional cursor-tracking glow.
- Dark / light mode (system preference + manual toggle, flash-free).
- Responsive layout with hover-reveal image cards on the home page.
- Pagefind search modal, link card directives, modern favicon set.
- Back-to-top button, mobile menu animations, print styles.
- Keyboard focus-visible styling and `prefers-reduced-motion` support.

### Tooling

- `kiln build` for one-shot builds.
- `kiln serve` with file watching and live reload for fast iteration.
- `kiln convert` to migrate Hugo sites into kiln, frontmatter and shortcodes included.

## Current Focus

- Small authoring and tooling improvements as they surface from real publishing.

## Later

A demo site to show kiln in motion, once the core publishing workflow feels finished. Beyond that, engine work continues to be opportunistic.

## Not the Goal Right Now

- One-to-one Hugo feature parity.
- Full multi-language site generation (separate per-language URL trees).
- Build-system complexity ahead of a complete publishing workflow.
- Scope expansion that outpaces real usage.
