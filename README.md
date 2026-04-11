# kiln

[![CI](https://github.com/hakula139/kiln/actions/workflows/ci.yml/badge.svg)](https://github.com/hakula139/kiln/actions/workflows/ci.yml)
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
- Ships with [IgnIt](https://github.com/hakula139/IgnIt): Tailwind CSS v4, glassmorphism panels, dark mode, responsive layout, search modal

### Tooling

- Dev server with live reload (`kiln serve`)
- Hugo-to-kiln content migration (`kiln convert`)

## Current Focus

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

# Dev server with search indexing enabled (slower rebuilds)
kiln serve --search

# Scaffold a new theme
kiln init-theme my-theme

# Convert a Hugo site root into a kiln site root
kiln convert --source /path/to/hugo-site --dest /path/to/kiln-site
```

`kiln convert` expects site roots, not `content/` directories. It reads from `source/content`, writes converted markdown and co-located assets to `dest/content`, and copies `source/static` to `dest/static` without overwriting existing destination files.

### Search

kiln integrates with [Pagefind](https://pagefind.app) for full-text search. Pagefind runs as a post-build step, indexing HTML output and generating client-side search assets.

**Setup:**

1. Install the Pagefind binary (one of):

   ```bash
   cargo install pagefind
   npm install -g pagefind
   ```

2. Enable search in `config.toml`:

   ```toml
   [search]
   enabled = true
   ```

3. If your theme supports Pagefind (e.g., IgnIt), also set the template flag:

   ```toml
   [params]
   search = true
   ```

4. Build the site — `kiln build` will run Pagefind automatically and write search assets to `{output_dir}/pagefind/`.

**How it works:** `kiln build` invokes the `pagefind` binary with `--site <output_dir>` after all HTML is generated. The `pagefind/` directory it creates is served alongside the rest of the site. During development, `kiln serve` skips search indexing for fast rebuilds — use `kiln serve --search` to test search locally.

**Custom binary path:** If `pagefind` is not on your `$PATH`, specify it in config:

```toml
[search]
enabled = true
binary = "/path/to/pagefind"
```

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
