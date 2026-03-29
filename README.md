# kiln

[![CI](https://github.com/hakula139/kiln/actions/workflows/ci.yml/badge.svg)](https://github.com/hakula139/kiln/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/hakula139/kiln)

A custom static site generator (SSG) written in Rust, replacing a [Hugo](https://gohugo.io) + [LoveIt](https://github.com/dillonzq/LoveIt) stack for [hakula.xyz](https://hakula.xyz).

## Overview

kiln is purpose-built to support the specific needs of hakula.xyz — CJK content, KaTeX math, custom directive-based shortcodes, and full control over the rendering pipeline. Rather than fighting a general-purpose SSG's assumptions, kiln implements exactly what's needed with no more complexity than necessary.

## Roadmap

- [x] TOML configuration and frontmatter (`+++` delimited)
- [x] Markdown with GFM extensions (tables, strikethrough, autolinks, footnotes)
- [x] KaTeX math support (`$...$` / `$$...$$`)
- [x] Syntax highlighting via [syntect](https://github.com/trishume/syntect) + [two-face](https://github.com/CosmicHorrorDev/two-face) (200+ languages, CSS classes, no JS runtime)
- [x] `:::` fenced directive system with callouts and Pandoc fenced divs
- [x] CJK-aware heading ID generation
- [x] Table of contents generation
- [x] Open Graph / Twitter Card / SEO meta tags
- [x] Template engine with block inheritance ([MiniJinja](https://github.com/mitsuhiko/minijinja))
- [x] Theme system with site-level overrides
- [x] Emoji shortcodes and [Font Awesome](https://fontawesome.com) icons
- [x] Pandoc-style image attributes (`{#id .class width=N}`)
- [x] Code blocks with language headers and collapsible max-lines
- [x] Static file handling and co-located content assets
- [x] Pretty URLs (`/post/` instead of `/post.html`)
- [x] Hugo content migration tool (`kiln convert`)
- [x] Template functions for data-driven directives (`read_file`, `parse_csv`)
- [x] Directive templates for link cards, music embeds, and score tables
- [x] Dev server with file watching and live reload (`kiln serve`)
- [x] Taxonomy support (tags, categories) with pagination
- [x] Home page, section pages, and standalone page template
- [ ] Dark theme with [Tailwind CSS](https://tailwindcss.com)
- [ ] RSS feed + sitemap
- [ ] Full-text search via [Pagefind](https://pagefind.app)

## Usage

```bash
# Build the site (default root: current directory)
kiln build

# Build from a specific project root
kiln build --root /path/to/site

# Start a dev server with live reload (default port: 5456)
kiln serve

# Dev server with custom port and auto-open browser
kiln serve --port 3000 --open

# Scaffold a new theme
kiln init-theme my-theme
```

## Site Structure

A kiln site is organized as follows:

```text
.
├── config.toml      # Site configuration (TOML)
├── content/         # Markdown content
│   ├── about-me/    # Standalone pages
│   └── posts/       # Blog posts organized by category
├── public/          # Build output (configurable via output_dir)
├── static/          # Static assets (copied to output as-is)
├── templates/       # MiniJinja templates (site overrides theme)
└── themes/          # Themes (git submodules)
```

## Documentation

- [docs/syntax.md](docs/syntax.md) — Markdown extensions, frontmatter fields, and directive syntax
- [docs/themes.md](docs/themes.md) — Theme installation, configuration, and creation

## Building from Source

Requires [Rust](https://www.rust-lang.org/tools/install) 1.85+ (edition 2024).

```bash
cargo build --release
```

The binary will be at `target/release/kiln`.

## Development

```bash
cargo build                    # Build
cargo fmt --all --check        # Check formatting
cargo clippy --all-targets     # Lint (pedantic)
cargo test                     # Run tests
```

CI runs these same checks on every push and pull request via GitHub Actions.

## License

Copyright (c) 2026 [Hakula](https://hakula.xyz). Licensed under the [MIT License](LICENSE).
