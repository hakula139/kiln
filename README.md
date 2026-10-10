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

### Via npm

Install Node.js 20 or newer, for example with [fnm](https://github.com/Schniz/fnm), then choose one kiln variant:

```bash
fnm install --latest --use
npm install -g @kiln-ssg/kiln@next
```

Choose `@kiln-ssg/kiln-extended@next` for AVIF loading placeholders. It has the same [dav1d requirements](#prebuilt-binary) as the extended archive. Both variants install `kiln`, so uninstall the current variant before switching.

Prereleases use the `next` npm tag. Supported platforms are Linux x86_64 (glibc), Apple Silicon and Windows x86_64.

Add optional tools to the same installation command when your site needs them:

```bash
npm install -g @kiln-ssg/kiln@next @kiln-ssg/tailwindcss@0.1.2 pagefind@1.5.2
```

- `@kiln-ssg/tailwindcss`: required when the site or theme compiles Tailwind CSS.
- `pagefind`: required when search is enabled.

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

For 0.5.0 releases, choose the default `kiln-<target>` archive or `kiln-extended-<target>` for AVIF loading placeholders. Only extended binaries need dav1d:

| Platform | Extended runtime requirement |
| -------- | ---------------------------- |
| Linux    | System `libdav1d` package    |
| macOS    | `brew install dav1d`         |
| Windows  | Included in the binary       |

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

The flake offers the `hakula` Cachix cache for prebuilt packages. Nix prompts for trust interactively. For noninteractive use, verify the cache key before passing `--accept-flake-config`.

To use this cache from another flake, configure it in that flake or your Nix installation.

To use kiln in another flake, add this input:

```nix
inputs.kiln.url = "github:hakula139/kiln";
# Outputs: packages.${system}.{default,kiln,kiln-tailwindcss,pagefind}
```

`pagefind` ships alongside `kiln` so consumers don't have to pin the search backend separately.

## First Site

After installing kiln, create a site with [IgnIt](https://github.com/hakula139/IgnIt). Use a kiln version meeting the theme's `min_kiln_version`.

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

`build`, `serve` and `init-theme` use the current directory as the site root unless `--root` is supplied. `KILN_BASE_URL` supplies a build URL override when `--base-url` is absent.

See [Hugo migration](docs/content.md#hugo-content-migration) for the converter's supported scope and manual follow-up.

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

### AVIF placeholders

Image publication and dimension detection work in either build. To add AVIF loading placeholders, install dav1d ≥ 1.3 and `pkg-config`:

| Platform        | Build dependencies                                           |
| --------------- | ------------------------------------------------------------ |
| macOS           | Xcode command-line tools and `brew install dav1d pkg-config` |
| Debian / Ubuntu | `sudo apt install build-essential libdav1d-dev pkg-config`   |

Then enable the feature:

```bash
cargo build --release --features avif
```

On macOS and Linux, dav1d is also a runtime dependency.

### Reproducible dev shell (Nix)

The Nix package enables AVIF support by default. Its development shell provides the build dependencies and pre-commit hooks:

```bash
nix develop      # interactive shell
nix flake check  # run pre-commit hooks
```

`direnv` auto-activates the shell via `.envrc`.

## License

Copyright (c) 2026 [Hakula](https://hakula.xyz). Licensed under the [MIT License](LICENSE).
