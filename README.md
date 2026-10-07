# kiln

[![CI](https://github.com/hakula139/kiln/actions/workflows/ci.yml/badge.svg)](https://github.com/hakula139/kiln/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/hakula139/kiln/graph/badge.svg)](https://codecov.io/gh/hakula139/kiln)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![WakaTime coding time for kiln](https://wakatime.com/badge/user/f4a35a1f-0e29-4093-a647-e66aad164737/project/f00a4cb5-df90-47ef-82e5-ebc9432a2b05.svg)

A static site generator written in Rust for [hakula.xyz](https://hakula.xyz), with CJK-friendly Markdown authoring and customizable themes.

## Highlights

- CJK-friendly Markdown for technical writing, with math, diagrams and syntax highlighting
- Custom content components through nested `:::` directives and theme templates
- Page bundles that keep writing, images and page-specific styles together
- Full-text search via [Pagefind](https://pagefind.app), with archives and feeds for finding and following posts
- [IgnIt](https://github.com/hakula139/IgnIt) theme with responsive layouts and dark mode, customizable through site overrides
- Local preview with live reload, plus optimized stylesheets and image loading placeholders for publication

## Documentation

| Document                                 | Description                                             |
| ---------------------------------------- | ------------------------------------------------------- |
| [Content](docs/content.md)               | Content, frontmatter and Hugo migration                 |
| [Syntax Reference](docs/syntax.md)       | Markdown extensions and directives                      |
| [Assets and Stylesheets](docs/assets.md) | Publication, stylesheets, fingerprints and live reload  |
| [Themes](docs/themes.md)                 | Site configuration, themes and templates                |
| [Benchmarks](docs/benchmarks.md)         | Performance measurements and baseline comparisons       |
| [Roadmap](docs/roadmap.md)               | Developer capability status, constraints and priorities |

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

The v0.4.0 Linux and macOS binaries require dav1d at runtime.

### From source

```bash
cargo install --git https://github.com/hakula139/kiln --locked
```

To generate loading placeholders for AVIF images, install the [native dependencies](#building-from-source) and enable the feature:

```bash
cargo install --git https://github.com/hakula139/kiln --locked --features avif
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
# Outputs: packages.${system}.{default,kiln,kiln-tailwindcss,pagefind}
```

`pagefind` ships alongside `kiln` so consumers don't have to pin the search backend separately.

## First Site

After installing kiln, create a site with [IgnIt](https://github.com/hakula139/IgnIt). Use a kiln version meeting the theme's `min_kiln_version`. Nix includes its Tailwind processor. For other installations, follow [Processor Setup](docs/assets.md#processor-setup).

```bash
git init my-site
cd my-site
git submodule add https://github.com/hakula139/IgnIt.git themes/IgnIt
mkdir -p content/posts/hello

cat > config.toml <<'CONFIG'
base_url = "https://example.com"
title = "My Site"
theme = "IgnIt"
CONFIG

cat > content/posts/hello/index.md <<'POST'
+++
title = "Hello"
+++

Welcome to my site.
POST

kiln serve --open
```

Run `kiln build --minify` to generate `public/` for deployment. See [Site Configuration](docs/themes.md#site-configuration) for engine settings and [Content](docs/content.md) for organizing pages.

## Usage

```bash
kiln build --root /path/to/site --minify
kiln build --base-url https://preview.example.com
kiln serve --port 3000 --open
kiln init-theme my-theme
kiln convert --source /path/to/hugo --dest /path/to/kiln
```

`build`, `serve` and `init-theme` use the current directory as the site root unless `--root` is supplied. `KILN_BASE_URL` supplies a build URL override when `--base-url` is absent. [Hugo migration](docs/content.md#hugo-content-migration) covers the converter's supported scope and manual follow-up.

### Search

Search is disabled by default. To enable it, install [Pagefind](https://pagefind.app) (`cargo install pagefind` or `npm install -g pagefind`) and set `[search] enabled = true` in `config.toml`:

```toml
[search]
enabled = true
# binary = "/path/to/pagefind"  # optional, if not on $PATH
```

`kiln build` and `kiln serve` run Pagefind after HTML generation when search is enabled.

## Building from Source

Requires stable [Rust](https://www.rust-lang.org/tools/install) and a C toolchain.

```bash
cargo build --release  # Binary at target/release/kiln
```

AVIF loading placeholders are optional. Enable them with `cargo build --release --features avif`. This requires dav1d ≥ 1.3 and `pkg-config`. On macOS, install the Xcode command-line tools and `brew install dav1d pkg-config`. On Debian / Ubuntu, install `build-essential libdav1d-dev pkg-config`. Builds with AVIF support use the installed dav1d library at runtime. Image publication and dimension detection work with either build.

### Reproducible dev shell (Nix)

The Nix package enables AVIF support by default. Its development shell provides the build dependencies and pre-commit hooks:

```bash
nix develop      # interactive shell
nix flake check  # run pre-commit hooks
```

`direnv` auto-activates the shell via `.envrc`.

## License

Copyright (c) 2026 [Hakula](https://hakula.xyz). Licensed under the [MIT License](LICENSE).
