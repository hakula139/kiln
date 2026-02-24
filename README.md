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
- [x] Syntax highlighting via [syntect](https://github.com/trishume/syntect) (CSS classes, no JS runtime)
- [x] `:::` fenced directive system with callouts and Pandoc fenced divs
- [x] CJK-aware heading ID generation and table of contents
- [x] [MiniJinja](https://github.com/mitsuhiko/minijinja) templates with Open Graph / Twitter Card / SEO meta
- [x] Single-page build pipeline
- [x] Multi-page builds with static files and content assets
- [ ] Additional directives (styled blocks, embeds, link cards)
- [ ] Hugo content migration tool (`kiln convert`)
- [ ] Taxonomy support (tags, categories) with pagination
- [ ] Home page, section pages, and special pages
- [ ] Dark theme with [Tailwind CSS](https://tailwindcss.com)
- [ ] RSS feed + sitemap
- [ ] Full-text search via [Pagefind](https://pagefind.app)

## Usage

```bash
# Build the site (default root: current directory)
kiln build

# Build from a specific project root
kiln build --root /path/to/site
```

## Site Structure

A kiln site is organized as follows:

```text
config.toml          # Site configuration (TOML)
content/             # Markdown content
  posts/             # Blog posts organized by category
  about-me/          # Standalone pages
templates/           # MiniJinja templates
static/              # Static assets (copied to output as-is)
public/              # Build output (configurable via output_dir)
```

## Syntax Reference

See [docs/syntax.md](docs/syntax.md) for the full list of supported Markdown extensions, frontmatter fields, and directive syntax.

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
