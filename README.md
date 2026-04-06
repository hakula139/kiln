# kiln

[![CI](https://github.com/hakula139/kiln/actions/workflows/ci.yml/badge.svg)](https://github.com/hakula139/kiln/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/hakula139/kiln)

A custom static site generator (SSG) written in Rust, replacing a [Hugo](https://gohugo.io) + [LoveIt](https://github.com/dillonzq/LoveIt) stack for [hakula.xyz](https://hakula.xyz).

## Overview

kiln is purpose-built for hakula.xyz: strong CJK-friendly authoring, explicit rendering behavior, and a theme system that stays understandable. Instead of chasing broad SSG feature parity, it focuses on a smaller publishing workflow that is easier to reason about and extend.

## Highlights

- CJK-friendly authoring with TOML frontmatter, GitHub Flavored Markdown, KaTeX math, heading IDs, and table of contents generation.
- Rich content primitives via `:::` directives, directive template helpers (`read_file`, `parse_csv`), image attributes, emoji / icon shortcodes, and code-block presentation helpers.
- Flexible site generation with pretty URLs, static file copying, co-located assets, standalone pages, home pages, section pages, taxonomy indexes, paginated term pages, and configurable site time zones.
- MiniJinja-based theming with layered site overrides and theme parameter merging. Ships with [IgnIt](https://github.com/hakula139/IgnIt), a Tailwind CSS v4 theme featuring glassmorphism panels, dark mode, and responsive layout.
- Local developer tooling with live reload (`kiln serve`) and Hugo-to-kiln migration (`kiln convert`).

## Current Focus

- Complete the publishing surface with RSS, sitemap, a 404 page, and full-text search via [Pagefind](https://pagefind.app).
- Improve the production pipeline with Rust-native asset minification, i18n groundwork, and further ergonomics polish.
- Make the taxonomy system config-driven (replace hardcoded `TaxonomyKind` with `[[taxonomies]]` config).

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

# Convert a Hugo site root into a kiln site root
kiln convert --source /path/to/hugo-site --dest /path/to/kiln-site
```

`kiln convert` expects site roots, not `content/` directories. It reads from `source/content`, writes converted markdown and co-located assets to `dest/content`, and copies `source/static` to `dest/static` without overwriting existing destination files.

## Site Structure

A kiln site is organized as follows:

```text
.
├── config.toml      # Site configuration (TOML)
├── content/         # Markdown content
│   ├── about-me/    # Standalone pages
│   └── posts/       # Blog posts organized by section
├── public/          # Build output (configurable via output_dir)
├── static/          # Static assets (copied to output as-is)
├── templates/       # MiniJinja templates (site overrides theme)
└── themes/          # Themes (git submodules)
```

## Documentation

| Document                       | Description                                         |
| ------------------------------ | --------------------------------------------------- |
| [Roadmap](docs/roadmap.md)     | Current shipped capability areas and planned work   |
| [Syntax Guide](docs/syntax.md) | Markdown extensions, frontmatter fields, directives |
| [Theming](docs/themes.md)      | Theme installation, configuration, and creation     |

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
