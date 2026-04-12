# kiln

[![CI](https://github.com/hakula139/kiln/actions/workflows/ci.yml/badge.svg)](https://github.com/hakula139/kiln/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/hakula139/kiln/graph/badge.svg)](https://codecov.io/gh/hakula139/kiln)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/hakula139/kiln)

A custom static site generator (SSG) written in Rust, replacing a [Hugo](https://gohugo.io) + [LoveIt](https://github.com/dillonzq/LoveIt) stack for [hakula.xyz](https://hakula.xyz).

## Overview

kiln is purpose-built for hakula.xyz: strong CJK-friendly authoring, explicit rendering behavior, and a theme system that stays understandable. Instead of chasing broad SSG feature parity, it focuses on a smaller publishing workflow that is easier to reason about and extend.

## Highlights

### Authoring

- TOML frontmatter, GitHub Flavored Markdown, KaTeX math
- CJK-friendly heading IDs and table of contents generation
- `:::` directives with theme-template rendering
- Directive template helpers (`read_file`, `parse_csv`)
- Image attributes, emoji / icon shortcodes, and code-block presentation helpers

### Site Generation

- Pretty URLs, static file copying, co-located content assets
- Home pages, section pages, standalone pages, taxonomy indexes, and paginated term pages
- Configurable site time zones for rendered dates
- RSS feeds, sitemap, custom 404 page
- Full-text search via [Pagefind](https://pagefind.app)

### Theming

- MiniJinja templates with layered site overrides and theme parameter merging
- Ships with [IgnIt](https://github.com/hakula139/IgnIt): Tailwind CSS v4, glassmorphism panels with cursor-tracking glow, dark mode, responsive layout, search modal, back-to-top, mobile menu animations, print styles, keyboard accessibility

### Tooling

- Dev server with live reload (`kiln serve`)
- Hugo-to-kiln content migration (`kiln convert`)

## Documentation

| Document                       | Description                                         |
| ------------------------------ | --------------------------------------------------- |
| [Roadmap](docs/roadmap.md)     | Current shipped capability areas and planned work   |
| [Syntax Guide](docs/syntax.md) | Markdown extensions, frontmatter fields, directives |
| [Theming](docs/themes.md)      | Theme installation, configuration, and creation     |

## Current Focus

Improve the production pipeline with Rust-native asset minification, i18n groundwork, and further ergonomics polish. See the [roadmap](docs/roadmap.md) for details.

## Usage

```bash
kiln build                                                  # Build the site
kiln build --root /path/to/site                             # Build from a specific root
kiln serve                                                  # Dev server with live reload
kiln serve --port 3000 --open                               # Custom port, auto-open browser
kiln init-theme my-theme                                    # Scaffold a new theme
kiln convert --source /path/to/hugo --dest /path/to/kiln    # Convert a Hugo site
```

### Search

kiln integrates with [Pagefind](https://pagefind.app) for full-text search. Install the binary (`cargo install pagefind` or `npm install -g pagefind`), then enable it in `config.toml`:

```toml
[search]
enabled = true
# binary = "/path/to/pagefind"    # optional, if not on $PATH
```

`kiln build` and `kiln serve` both run Pagefind automatically after HTML generation.

## Building from Source

Requires [Rust](https://www.rust-lang.org/tools/install) 1.85+ (edition 2024).

```bash
cargo build --release    # Binary at target/release/kiln
```

## License

Copyright (c) 2026 [Hakula](https://hakula.xyz). Licensed under the [MIT License](LICENSE).
