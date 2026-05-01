# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-01

### Added

- Initial public release.
- Markdown authoring with TOML frontmatter, GitHub Flavored Markdown, and KaTeX math.
- CJK-friendly heading IDs and table of contents generation.
- `:::` directives with theme-template rendering, plus directive helpers (`read_file`, `parse_csv`).
- Image attributes, GitHub-style emoji shortcodes, FontAwesome icon shortcodes, and code-block presentation helpers.
- Site generation: pretty URLs, static file copying, co-located content assets, home / section / standalone / taxonomy / paginated term pages, configurable site time zones, RSS feeds, sitemap, and a custom 404 page.
- Full-text search via [Pagefind](https://pagefind.app).
- Internationalization with layered TOML translation overrides and graceful fallback to English.
- Layered MiniJinja template engine with site overrides and theme parameter merging.
- Ships with the [IgnIt](https://github.com/hakula139/IgnIt) theme (Tailwind CSS v4, glassmorphism, dark mode, search modal, mobile menu, print styles, keyboard accessibility).
- Dev server with live reload (`kiln serve`) and a Hugo-to-kiln content migration tool (`kiln convert`).
- Optional post-build minification (`kiln build --minify`) for HTML, CSS, and JS.

[Unreleased]: https://github.com/hakula139/kiln/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/hakula139/kiln/releases/tag/v0.1.0
