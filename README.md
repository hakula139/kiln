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
- Mermaid diagrams via `` ```mermaid `` fences
- Syntax highlighting for 200+ languages, image attributes, emoji and Font Awesome icon shortcodes

### Site Generation

- Pretty URLs, static file copying, co-located content assets, per-page CSS bundling
- Home pages, section pages, standalone pages, taxonomy indexes, and paginated term pages
- Pinned posts on the home page via `weight` frontmatter
- Page-scoped asset registry — themes load KaTeX / Mermaid / search only on pages that need them
- Configurable site time zones for rendered dates
- Build-time image pipeline — every `<img>` gets natural `width`/`height` plus a base64 WebP LQIP backdrop, so the browser reserves the exact box and paints a low-frequency placeholder while the source decodes
- RSS feeds, sitemap, custom 404 page
- Full-text search via [Pagefind](https://pagefind.app)

### Internationalization

- Translatable theme strings with layered TOML overrides — themes ship defaults, sites customize freely
- Localized templates and navigation menus, with graceful fallback to English when a translation is missing

### Theming

- MiniJinja templates with layered site overrides and theme parameter merging
- Ships with [IgnIt](https://github.com/hakula139/IgnIt): Tailwind CSS v4, glassmorphism panels with cursor-tracking glow, dark mode, responsive layout, search modal, back-to-top, mobile menu animations, print styles, keyboard accessibility

### Tooling

- Dev server with live reload (`kiln serve`)
- Hugo-to-kiln content migration (`kiln convert`)
- Theme scaffolding (`kiln init-theme`)
- Optional HTML / CSS / JS minification (`kiln build --minify`)

## Documentation

| Document                         | Description                                         |
| -------------------------------- | --------------------------------------------------- |
| [Roadmap](docs/roadmap.md)       | Current shipped capability areas and planned work   |
| [Content Guide](docs/content.md) | Page bundles, co-located assets, per-page CSS       |
| [Syntax Guide](docs/syntax.md)   | Markdown extensions, frontmatter fields, directives |
| [Theming](docs/themes.md)        | Theme installation, configuration, and creation     |

## Current Focus

Comment integration via Twikoo in the [IgnIt](https://github.com/hakula139/IgnIt) theme, plus ongoing engine and theme polish. See the [roadmap](docs/roadmap.md) for details.

## Installation

### Prebuilt binary

Download the latest release for your platform from [Releases](https://github.com/hakula139/kiln/releases/latest):

```bash
# Linux x86_64
curl -fsSL https://github.com/hakula139/kiln/releases/latest/download/kiln-x86_64-unknown-linux-gnu.tar.gz | tar -xz
sudo mv kiln /usr/local/bin/

# macOS aarch64 (Apple Silicon)
curl -fsSL https://github.com/hakula139/kiln/releases/latest/download/kiln-aarch64-apple-darwin.tar.gz | tar -xz
sudo mv kiln /usr/local/bin/

kiln --version
```

### From source

```bash
cargo install --git https://github.com/hakula139/kiln --locked
```

### Via Nix

```bash
nix run github:hakula139/kiln -- build         # one-shot
nix profile install github:hakula139/kiln      # install to user profile
```

Or as a flake input from another project (e.g., a site repo):

```nix
inputs.kiln.url = "github:hakula139/kiln";
# Outputs: packages.${system}.{default,kiln,pagefind}
```

`pagefind` ships alongside `kiln` so consumers don't have to pin the search backend separately.

See [`RELEASING.md`](./RELEASING.md) for how releases are produced.

## Usage

```bash
kiln build                                                  # Build the site
kiln build --root /path/to/site                             # Build from a specific root
kiln build --minify                                         # Build, then minify HTML / CSS / JS
kiln serve                                                  # Dev server with live reload
kiln serve --port 3000 --open                               # Custom port, auto-open browser
kiln init-theme my-theme                                    # Scaffold a new theme
kiln convert --source /path/to/hugo --dest /path/to/kiln    # Convert a Hugo site
```

### Minification

Passing `--minify` to `kiln build` runs a Rust-native pass over the output directory and rewrites each HTML / CSS / JS file in place:

- HTML via [`minify-html`](https://crates.io/crates/minify-html)
- CSS via [`lightningcss`](https://crates.io/crates/lightningcss)
- JS via [`oxc_minifier`](https://crates.io/crates/oxc_minifier)

Files matching `*.min.css` or `*.min.js` are skipped so that pre-minified vendor bundles (e.g., Pagefind's UI JS) pass through untouched. Unusable inputs log a warning and keep the original file, so `--minify` never blocks a build.

### Search

kiln integrates with [Pagefind](https://pagefind.app) for full-text search. Install the binary (`cargo install pagefind` or `npm install -g pagefind`), then enable it in `config.toml`:

```toml
[search]
enabled = true
# binary = "/path/to/pagefind"    # optional, if not on $PATH
```

`kiln build` and `kiln serve` both run Pagefind automatically after HTML generation.

## Building from Source

Requires [Rust](https://www.rust-lang.org/tools/install) 1.85+ (edition 2024) and `libdav1d` (for the `image` crate's AVIF decoder).

```bash
cargo build --release             # Binary at target/release/kiln
```

### Reproducible dev shell (Nix)

For hacking on kiln itself, the shipped `flake.nix` pins the Rust toolchain, `libdav1d`, `pagefind`, `git-cliff`, and pre-commit hooks:

```bash
nix develop                       # interactive shell
nix flake check                   # run pre-commit hooks
```

`direnv` auto-activates the shell via `.envrc`.

## License

Copyright (c) 2026 [Hakula](https://hakula.xyz). Licensed under the [MIT License](LICENSE).
