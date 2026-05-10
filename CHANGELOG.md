# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0-rc.2] - 2026-05-10

### Added

- _(release)_ 4-platform release matrix + cancel stale runs (#61)

## [0.3.0-rc.1] - 2026-05-09

### Breaking changes

- Named menu groups + literal-friendly t() (#59)

## [0.2.0] - 2026-05-08

### Breaking changes

- _(render)_ Wrap LQIP-enabled images in `<span class="lqip">` (#41)
- _(template)_ Register_script() load=defer|async|sync kwarg (#46)

### Added

- _(output)_ Pass through `_headers` and `_redirects` at `static/` root (#39)
- _(render)_ Build-time image dimensions + LQIP backdrop pipeline (#40)
- Directive register_script() helper + --base-url CLI override (#43)
- _(nix)_ Package kiln + pagefind as flake outputs (#45)
- _(render)_ Pandoc-style code-block attributes (#48)
- _(template)_ Expose config to directive templates (#51)

## [0.1.0] - 2026-05-01

### Added

- _(kiln)_ Scaffold workspace, CLI, and TOML config
- _(content)_ Add content model (frontmatter, page, discovery) (#1)
- _(render)_ Add markdown rendering + heading IDs + math + ToC (#2)
- _(render)_ Add syntax highlighting, image rendering + tracing (#3)
- _(directive)_ Add directive parser + admonition renderer (#4)
- _(kiln)_ Add render pipeline, template engine + single-page build (#5)
- _(kiln)_ Add multi-page builds with static files and content assets (#6)
- _(directive)_ Add Pandoc `#id` / `.class` propagation + fenced divs (#7)
- _(kiln)_ Add theme system, pre-processors, and code block wrapper (#8)
- _(convert)_ Add Hugo → kiln content converter (#9)
- _(directive)_ Add structured arg parsing, source_dir, and read_file template function (#10)
- _(template)_ Add parse_csv and fix converter brace emission (#11)
- _(serve)_ Add dev server with file watching and live reload (#12)
- _(taxonomy)_ Add taxonomy system with pagination (#13)
- _(highlight)_ Switch to two-face for 200+ languages (#14)
- _(build)_ Add home / section pages, date rendering, and /posts permalinks (#15)
- Menu config, PageSummary enrichment, and theme docs (#16)
- Code block data-lang, plaintext normalization, image decoding (#18)
- _(frontmatter)_ Add math field for KaTeX script loading (#20)
- _(frontmatter)_ Structured featured image with credit metadata (#22)
- _(build)_ Sections index and unified archive / overview listing pipeline (#23)
- _(build)_ RSS feeds, sitemap, robots.txt, and 404 page (#25)
- Pagefind full-text search integration (#26)
- _(build)_ Per-page CSS bundling via co-located style.css (#28)
- _(build)_ --minify flag for Rust-native HTML / CSS / JS minification (#31)
- _(i18n)_ Layered TOML resolver with t() and menu key convention (#32)
- _(config)_ Validate output_dir to prevent overwriting project root (#33)
- _(build)_ Pinned posts via weight frontmatter (#34)
- _(render)_ Page-scoped asset registry with auto-detected features (#35)
- _(render)_ Emit `<pre class="mermaid">` for mermaid fences (#36)
- _(release)_ Publish prebuilt binaries with git-cliff changelog (#37)
- _(cli)_ Support `kiln --version`

### Fixed

- _(serve)_ Use staged build to eliminate 404s during rebuild (#19)
- _(serve)_ Switch live reload from SSE to WebSocket (#21)
- _(render)_ Extract image attrs inside directive bodies (#24)
- _(serve)_ Set Cache-Control: no-cache on dev server responses
- _(directive)_ Tolerate trailing content after attribute braces (#27)
- Resolve Rust 1.95 clippy lints and pin toolchain (#30)

### Dependencies

- _(deps)_ Bump rand from 0.9.2 to 0.9.4 (#29)

[0.3.0-rc.2]: https://github.com/hakula139/kiln/compare/v0.3.0-rc.1..v0.3.0-rc.2
[0.3.0-rc.1]: https://github.com/hakula139/kiln/compare/v0.2.0..v0.3.0-rc.1
[0.2.0]: https://github.com/hakula139/kiln/compare/v0.1.0..v0.2.0
[0.1.0]: https://github.com/hakula139/kiln/releases/tag/v0.1.0
