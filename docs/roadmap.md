# Roadmap

kiln is a small static site generator built for [hakula.xyz](https://hakula.xyz) and already powering it day to day. This page is the high-level product view — what works today, what's being built next, and what is intentionally out of scope.

The project's shape is deliberate:

- Make CJK-heavy writing, technical posts, and custom content components a pleasure to author.
- Finish the publishing workflow before reaching for broader platform scope.
- Keep the architecture understandable — new features fit the current model.

## What Works Today

### Writing

- TOML frontmatter, GitHub Flavored Markdown, and KaTeX math out of the box
- CJK-aware heading IDs and table of contents — Chinese / Japanese / Korean headings stay linkable
- `:::` directive blocks rendered through theme templates: callouts, link cards, music embeds, anything you can template
- Image attributes, emoji and Font Awesome icon shortcodes, and rich code-block presentation helpers

### Publishing

- Pretty URLs, page bundles with co-located assets, and per-page CSS injection
- Home, section, standalone, and paginated taxonomy / term pages
- Pinned posts on the home page via a `weight` frontmatter field — hero pieces stay above the fold without affecting archive, tag, or RSS order
- Time-zone-aware dates rendered in your site's local time
- RSS 2.0 feeds for the whole site, each section, and each taxonomy term
- Sitemap, `robots.txt`, and an optional template-driven 404 page
- Full-text search via [Pagefind](https://pagefind.app), wired in at build time
- Optional HTML / CSS / JS minification with `kiln build --minify` — pure Rust, no Node toolchain required
- Page-scoped asset detection: themes load KaTeX only on pages that actually contain math expressions, no frontmatter flag required

### Internationalization

- Translatable theme strings via layered TOML files: site override → theme language → English fallback, so partial translations degrade gracefully
- `{{ t("key", name=value) }}` template helper with placeholder interpolation
- Navigation menu labels resolve through the same i18n tables as the rest of your strings
- `kiln init-theme` scaffolds starter `en.toml` and `zh-Hans.toml` files for new themes

### Theming

- Layered MiniJinja templates: site files transparently override theme files
- Deep parameter merging for nested theme config tables
- Directive template helpers including `read_file()` and `parse_csv()` for data-driven blocks
- Configurable navigation menu via `[[menu.main]]` with weight sorting and external link support

The default theme [**IgnIt**](https://github.com/hakula139/IgnIt) ships with Tailwind CSS v4 and a polished feature set:

- Glassmorphism panels with cursor-tracking glow and a configurable background image
- Dark / light mode (system preference + manual toggle, flash-free)
- Responsive layout with hover-reveal image cards on the home page
- Pagefind search modal, link card directives, modern favicon set
- Back-to-top button, mobile menu animations, print styles
- Keyboard focus-visible styling and `prefers-reduced-motion` support

### Tooling

- `kiln build` for one-shot builds
- `kiln serve` with file watching and live reload for fast iteration
- `kiln convert` to migrate Hugo sites into kiln, frontmatter and shortcodes included

## What's Next

### Richer Authoring

- Code-block attributes: titles, line highlighting (`highlight="1,3-5"`), collapse / expand
- Bundled scripts for directive templates via a `register_script()` mechanism, retiring the inline `<script>` workaround

### Reader Experience

- Comment integration via [Twikoo](https://twikoo.js.org/) in the IgnIt theme — bring threads back to per-post pages and the global `/comments/` index, with the hook designed so other backends can drop in later

### Runtime Polish

- Mermaid diagram rendering — auto-detection is wired; remaining work is a markup change so themes can drop in mermaid.js
- Directive-registered scripts via a `register_script()` helper, retiring the inline `<script>` workaround inside directive templates
- Stricter `output_dir` validation so a misconfigured path can never reach somewhere unintended
- Small authoring and tooling improvements as they surface from real publishing

## Later

A demo site to show kiln in motion, once the core publishing workflow feels finished. Beyond that, engine work continues to be opportunistic — driven by concrete publishing needs, not speculative parity.

## Not the Goal Right Now

- One-to-one Hugo feature parity
- Full multi-language site generation (separate per-language URL trees)
- Build-system complexity ahead of a complete publishing workflow
- Scope expansion that outpaces real usage
