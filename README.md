# kiln

[![CI](https://github.com/hakula139/kiln/actions/workflows/ci.yml/badge.svg)](https://github.com/hakula139/kiln/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/hakula139/kiln/graph/badge.svg)](https://codecov.io/gh/hakula139/kiln)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![WakaTime coding time for kiln](https://wakatime.com/badge/user/f4a35a1f-0e29-4093-a647-e66aad164737/project/f00a4cb5-df90-47ef-82e5-ebc9432a2b05.svg)

A static site generator written in Rust for [hakula.xyz](https://hakula.xyz), with CJK-friendly Markdown authoring and customizable themes.

## Highlights

### Authoring

- GitHub Flavored Markdown with TOML frontmatter and CJK-friendly headings
- KaTeX math, Mermaid diagrams, and syntax highlighting for 200+ languages
- Custom `:::` directives rendered through theme templates

### Publishing

- Page bundles, section and tag archives, and pagination
- Full-text search via [Pagefind](https://pagefind.app), RSS feeds, and sitemaps
- Automatic image dimensions and blurred loading placeholders
- Asset fingerprinting and optional HTML / CSS / JS minification

### Theming & Development

- MiniJinja templates with site overrides and translatable theme strings
- [IgnIt](https://github.com/hakula139/IgnIt) theme with responsive layouts and dark mode
- Live-reloading dev server, theme scaffolding, and Hugo content migration

## Documentation

| Document                         | Description                                         |
| -------------------------------- | --------------------------------------------------- |
| [Roadmap](docs/roadmap.md)       | Current shipped capability areas and planned work   |
| [Content Guide](docs/content.md) | Page bundles, co-located assets, per-page CSS       |
| [Syntax Guide](docs/syntax.md)   | Markdown extensions, frontmatter fields, directives |
| [Theming](docs/themes.md)        | Themes, templates, navigation menus, and i18n       |
| [Benchmarks](docs/benchmarks.md) | Performance measurements and baseline comparisons   |

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

# Windows x86_64 (Git Bash / MSYS / WSL)
curl -fsSLO https://github.com/hakula139/kiln/releases/latest/download/kiln-x86_64-pc-windows-msvc.zip
unzip kiln-x86_64-pc-windows-msvc.zip

kiln --version
```

### From source

```bash
cargo install --git https://github.com/hakula139/kiln --locked
```

### Via Nix

```bash
nix run github:hakula139/kiln -- build     # one-shot
nix profile install github:hakula139/kiln  # install to user profile
```

The flake offers the `hakula` Cachix cache for prebuilt kiln packages. Nix prompts for trust interactively. For noninteractive use, pass `--accept-flake-config` after verifying the cache key, or configure the cache in Nix. Projects that import kiln as an input need to configure the cache in their own top-level flake or Nix installation.

To use kiln in another flake, add this input:

```nix
inputs.kiln.url = "github:hakula139/kiln";
# Outputs: packages.${system}.{default,kiln,pagefind}
```

`pagefind` ships alongside `kiln` so consumers don't have to pin the search backend separately.

See [`RELEASING.md`](./RELEASING.md) for how releases are produced.

## Usage

```bash
kiln build                                                # Build the site
kiln build --root /path/to/site                           # Build from a specific root
kiln build --minify                                       # Build, then minify HTML / CSS / JS
kiln serve                                                # Dev server with live reload
kiln serve --port 3000 --open                             # Custom port, auto-open browser
kiln init-theme my-theme                                  # Scaffold a new theme
kiln convert --source /path/to/hugo --dest /path/to/kiln  # Convert a Hugo site
```

### Static Asset URLs

Use `asset_url()` in templates when referencing a file from the merged theme and site `static/` trees:

```jinja
<link rel="stylesheet" href="{{ asset_url('/css/style.generated.css') | safe }}">
<script src="{{ asset_url('/js/app.js') | safe }}"></script>
```

kiln copies CSS and JS to names containing the first 12 hexadecimal characters of their SHA-256 digest, such as `/css/style.generated.a1b2c3d4e5f6.css`. Other static files keep their original URLs. A missing path fails the build.

The original CSS / JS files remain in the output because relative imports and existing hard-coded references may still depend on them. Templates using `asset_url()` receive the fingerprinted URL.

The digest covers one file. Bundle self-contained entry assets before passing them to kiln because relative CSS imports and JavaScript module imports continue to use their original URLs.

### Minification

Passing `--minify` to `kiln build` minifies shared CSS / JS and canonical page stylesheets before their digests are computed, then processes generated HTML and other page-bundle assets:

- HTML via [`minify-html`](https://crates.io/crates/minify-html)
- CSS via [`lightningcss`](https://crates.io/crates/lightningcss)
- JS via [`oxc_minifier`](https://crates.io/crates/oxc_minifier)

Files matching `*.min.css` or `*.min.js` are skipped so pre-minified vendor bundles such as Pagefind's UI JS pass through untouched. Unusable inputs log a warning and keep the original file, so `--minify` never blocks a build.

### Search

kiln integrates with [Pagefind](https://pagefind.app) for full-text search. Install the binary (`cargo install pagefind` or `npm install -g pagefind`), then enable it in `config.toml`:

```toml
[search]
enabled = true
# binary = "/path/to/pagefind"  # optional, if not on $PATH
```

`kiln build` and `kiln serve` both run Pagefind automatically after HTML generation.

## Building from Source

Requires [Rust](https://www.rust-lang.org/tools/install) 1.85+ (edition 2024) and `libdav1d` (for the `image` crate's AVIF decoder).

```bash
cargo build --release  # Binary at target/release/kiln
```

### Reproducible dev shell (Nix)

For hacking on kiln itself, the shipped `flake.nix` pins the Rust toolchain, `libdav1d`, `pagefind`, `git-cliff`, and pre-commit hooks:

```bash
nix develop      # interactive shell
nix flake check  # run pre-commit hooks
```

`direnv` auto-activates the shell via `.envrc`.

## License

Copyright (c) 2026 [Hakula](https://hakula.xyz). Licensed under the [MIT License](LICENSE).
