# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.1] - 2026-10-25

### Fixed

- _(markdown)_ Fix parsing panics and excessive processing time for pathological emphasis and deeply nested lists. ([#203](https://github.com/hakula139/kiln/pull/203))
- _(serve)_ Report macOS file-watcher startup failures and fix native watcher cleanup for deleted paths. ([#216](https://github.com/hakula139/kiln/pull/216))

## [0.5.0] - 2026-10-18

### Breaking changes

- _(assets)_ Publish fingerprinted images, fonts, compiled stylesheets and eligible scripts only under their content-hashed filenames. Resolve template links with `asset_url()` and keep files requiring fixed URLs in `static/`. See the [asset publication rules](docs/assets.md#fingerprints-and-minification). ([#127](https://github.com/hakula139/kiln/pull/127), [#151](https://github.com/hakula139/kiln/pull/151))
- _(config)_ Reject unknown configuration and frontmatter fields, invalid pagination sizes and slugs containing `:`. Remove `[author] email` and `link`, convert Hugo frontmatter with `kiln convert`, and write `featured_image` as a table with `src`. UTC template date values now render with `+00:00`. ([#139](https://github.com/hakula139/kiln/pull/139), [#140](https://github.com/hakula139/kiln/pull/140), [#148](https://github.com/hakula139/kiln/pull/148))
- _(deps)_ Require Rust 1.89 or newer when building from source. HTML and JavaScript minification now use SWC. MiniJinja 3 changes collection formatting, rounding and arithmetic to follow Python semantics, including errors on division by zero. ([#135](https://github.com/hakula139/kiln/pull/135), [#177](https://github.com/hakula139/kiln/pull/177))
- _(directive)_ Themes must supply `templates/directives/callout.html` and `callout_<type>` translations for styled, localized callouts. Directive templates receive `body_html`, `id` and `classes`, and must apply the authored identity themselves. `body_raw` is removed and directive template filenames must be lowercase. Invalid callout types or `open` values fail the build. ([#132](https://github.com/hakula139/kiln/pull/132))
- _(kiln)_ Rust callers must use module paths such as `kiln::build::{build, BuildOptions}`, `kiln::convert::convert`, `kiln::init::init_theme` and `kiln::serve::serve`. `build` now borrows `BuildOptions`, and the port constant is `kiln::config::DEFAULT_PORT`. Previously public parsing, feed, sitemap and output helpers are no longer exposed. ([#115](https://github.com/hakula139/kiln/pull/115), [#130](https://github.com/hakula139/kiln/pull/130), [#148](https://github.com/hakula139/kiln/pull/148), [#159](https://github.com/hakula139/kiln/pull/159), [#162](https://github.com/hakula139/kiln/pull/162))
- _(kiln)_ Rust integrations must adapt to typed configuration values, obtain pages through `kiln::content::discovery::discover_content` after removal of the public page constructors, and use `Page.body` in place of `raw_content`. `render_page` now takes `PageResources` and a separate `heading_numbering` argument, replacing its separate syntax-set, configuration, source-directory and image-resolver arguments. `TemplateEngine::new` requires configuration, translations and an asset manifest. `RenderOptions::code_max_lines` is optional and nonzero, and page asset features are accessed through methods. ([#134](https://github.com/hakula139/kiln/pull/134), [#139](https://github.com/hakula139/kiln/pull/139), [#140](https://github.com/hakula139/kiln/pull/140), [#141](https://github.com/hakula139/kiln/pull/141), [#151](https://github.com/hakula139/kiln/pull/151), [#152](https://github.com/hakula139/kiln/pull/152))
- _(nix)_ Remove the exported `packages.${system}.pagefind` and `packages.${system}.kiln-tailwindcss` flake outputs. Use `packages.${system}.kiln` or `packages.${system}.default`, whose kiln executable bundles Pagefind and the Tailwind compiler on its runtime path. ([#142](https://github.com/hakula139/kiln/pull/142))
- _(render)_ Native AVIF decoding is optional. Use the extended binary or enable the `avif` Cargo feature for AVIF loading placeholders. AVIF publication and dimension detection work in both variants. ([#116](https://github.com/hakula139/kiln/pull/116))
- _(render)_ Themes or sites must define `copy` and `copy_code` translations for code-block button text and accessible labels. These labels and the language label are excluded from Pagefind indexing, while authored code titles remain searchable. ([#156](https://github.com/hakula139/kiln/pull/156))
- _(render)_ Code-block `data-lang` now uses the lowercase fence language on the wrapper. The nested `<code>` element no longer repeats language attributes. Shortcodes remain literal in code, math, raw HTML and link destinations. ([#134](https://github.com/hakula139/kiln/pull/134), [#141](https://github.com/hakula139/kiln/pull/141))
- _(template)_ Update themes to use the shared `title`, singular `kind` and `pagination.base_path` variables. Define the `all_posts`, `all_sections`, `all_tags` and `error_404_title` translations, render standalone pages with `post.html`, and pass source paths to `register_script()`. Pagination gaps are `none`. See the [template variables](docs/themes.md#template-variables). ([#150](https://github.com/hakula139/kiln/pull/150), [#152](https://github.com/hakula139/kiln/pull/152))
- _(template)_ HTML templates escape URLs automatically while preserving `/`. Remove URL `| safe` filters and retain them for generated HTML such as `content`, `toc` and `body_html`. `asset_url()` returns a plain URL. ([#155](https://github.com/hakula139/kiln/pull/155))

### Added

- _(release)_ Install base or AVIF-enabled extended binaries through the `@kiln-ssg/kiln` and `@kiln-ssg/kiln-extended` npm packages, or download archives for Linux x86_64, Apple Silicon and Windows x86_64. ([#118](https://github.com/hakula139/kiln/pull/118), [#121](https://github.com/hakula139/kiln/pull/121))

### Fixed

- _(build)_ Publish builds transactionally and reject output paths that overlap source trees. ([#114](https://github.com/hakula139/kiln/pull/114))
- _(cli)_ Send logs to stderr, preserving stdout for command output. ([#160](https://github.com/hakula139/kiln/pull/160))
- _(config)_ Allow output directories inside linked theme repositories. ([#126](https://github.com/hakula139/kiln/pull/126))
- _(content)_ Apply render options to summaries, ignore summary separators inside code, and follow content symlinks. ([#138](https://github.com/hakula139/kiln/pull/138), [#159](https://github.com/hakula139/kiln/pull/159))
- _(convert)_ Preserve post and section indexes, accept null YAML dates, follow symlinked source bundles, and map Hugo `lastmod` to `updated`. ([#133](https://github.com/hakula139/kiln/pull/133), [#143](https://github.com/hakula139/kiln/pull/143), [#159](https://github.com/hakula139/kiln/pull/159))
- _(kiln)_ Encode URL components consistently with JavaScript's `encodeURIComponent` and isolate site Git metadata reads from ambient `GIT_DIR`. ([#161](https://github.com/hakula139/kiln/pull/161))
- _(npm)_ Include installation guidance in launcher packages and report musl Linux as unsupported. ([#129](https://github.com/hakula139/kiln/pull/129), [#136](https://github.com/hakula139/kiln/pull/136))
- _(render)_ Preserve brace-only and adjacent code-fence attributes, apply code-line limits inside directives, reserve authored heading IDs, and count display math when deciding table column widths. ([#134](https://github.com/hakula139/kiln/pull/134), [#141](https://github.com/hakula139/kiln/pull/141), [#163](https://github.com/hakula139/kiln/pull/163))
- _(search)_ Run npm-installed Pagefind on Windows. ([#119](https://github.com/hakula139/kiln/pull/119))
- _(serve)_ Detect atomic configuration saves and serve complete custom 404 pages with live reload. ([#130](https://github.com/hakula139/kiln/pull/130))

### Changed

- _(build)_ Read content commit timestamps in one Git pass. ([#146](https://github.com/hakula139/kiln/pull/146))
- _(css)_ Compile Tailwind stylesheets concurrently. ([#147](https://github.com/hakula139/kiln/pull/147))

## [0.5.0-alpha.4] - 2026-10-13

### Breaking changes

- _(deps)_ Migrate to minijinja 3 (#177)

### Dependencies

- _(deps)_ Update dependency prettier to v3.9.10 (#167)
- _(deps)_ Update rust dependencies (#169)
- _(deps)_ Update dependency dprint to ^0.61.0 (#172)
- _(deps)_ Update rust crate base64 to 0.23 (#173)
- _(deps)_ Update rust crate sha2 to v0.11.1 (#174)
- _(deps)_ Update actions/download-artifact action to v8 (#175)

## [0.5.0-alpha.3] - 2026-10-12

### Fixed

- _(release)_ Publish npm tarballs by explicit relative path (#165)

## [0.5.0-alpha.2] - 2026-10-12

### Breaking changes

- _(serve)_ Detect atomic config saves and simplify the dev server (#130)
- _(directive)_ Render callouts through theme templates and unify attribute parsing (#132)
- _(render)_ Parse each fragment once and flatten the document model (#134)
- _(render)_ Unify code block highlighting and fence parsing (#141)
- _(content)_ Build pages once with kind and output directory (#139)
- _(config)_ Validate configuration once at load (#140)
- _(nix)_ Bundle Pagefind with the kiln package and simplify CI wiring (#142)
- _(build)_ Fold feed and sitemap into build and validate page sizes (#148)
- _(deps)_ Replace minify-html and Oxc with swc and declare the MSRV (#135)
- _(build)_ Plan and render every output through one route table (#150)
- _(template)_ Give every template one context contract (#152)
- _(template)_ Autoescape HTML without encoding slashes (#155)
- _(kiln)_ Follow convert source symlinks, ignore summary separators in code and make text crate-private (#159)
- _(render)_ Localize the code-block copy button and keep it out of search (#156)
- _(assets)_ Publish assets under content-hashed filenames in place (#151)
- _(kiln)_ Remove crate-root re-exports and finish the core final review (#162)

### Added

- _(release)_ Make npm publication resumable (#157)

### Fixed

- _(npm)_ Include the root README in launcher packages (#129)
- _(npm)_ Report musl Linux as an unsupported platform (#136)
- _(convert)_ Keep post and section indexes that kiln reads (#133)
- _(content)_ Match render options in summaries and follow content symlinks (#138)
- _(convert)_ Accept null YAML dates (#143)
- _(cli)_ Write logs to stderr (#160)
- _(kiln)_ Match encodeURIComponent in URLs, ignore ambient GIT_DIR and finish the final review refactors (#161)
- _(kiln)_ Count display math toward table nowrap widths and finish the final render review (#163)

### Changed

- _(build)_ Read content commit timestamps in one git pass (#146)
- _(css)_ Run Tailwind compilations concurrently (#147)

## [0.5.0-alpha.1] - 2026-10-10

### Breaking changes

- _(kiln)_ Share page metadata and parser contracts (#115)
- _(render)_ Make native AVIF decoding optional (#116)

### Added

- _(release)_ Publish base and optional AVIF binaries (#118)
- _(release)_ Distribute kiln through npm platform packages (#121)
- _(assets)_ Fingerprint raster images and fonts (#127)

### Fixed

- _(build)_ Publish output transactionally and preserve source trees (#114)
- _(search)_ Run npm Pagefind on Windows (#119)
- _(config)_ Allow output within linked theme repositories (#126)

## [0.4.0] - 2026-10-07

### Breaking changes

- _(render)_ Collect scoped endnotes with unique anchors and backlinks (#71)
- _(render)_ Use native code block disclosures (#82)
- _(css)_ Compile shared and page-owned styles during site builds (#95)

### Added

- _(render)_ Mark short table columns as nowrap (#74)
- _(build)_ Expose post tags and last update metadata (#78)
- _(render)_ Add per-page hierarchical heading numbering (#83)
- _(render)_ Support numbering starts on headings (#87)
- _(build)_ Report total build time and generated page counts (#88)
- _(output)_ Materialize symlinked source assets (#98)
- _(bench)_ Add reusable performance workloads (#105)

### Fixed

- _(render)_ Keep rendered directive HTML out of the markdown pass (#77)
- _(directive)_ Render stable callout body structure (#79)
- _(render)_ Escape custom code block classes (#85)
- _(convert)_ Handle admonitions without positional arguments (#89)
- _(taxonomy)_ Deduplicate repeated page tags (#90)
- _(template)_ Surface optional template loading errors (#92)
- _(render)_ Balance highlighted code line spans (#93)
- _(content)_ Propagate discovery read errors (#94)
- _(taxonomy)_ Reject tags with empty slugs (#97)
- _(serve)_ Watch site translation files (#99)
- _(serve)_ Preserve queries in directory redirects (#100)
- _(build)_ Normalize site URL joining (#96)
- _(attrs)_ Detect closing braces outside quoted values (#102)
- _(release)_ Include prerelease changes in stable notes (#111)
- _(bench)_ Run each revision with its own harness (#113)

### Changed

- _(render)_ Speed up LQIP thumbnail generation (#72)
- _(render)_ Avoid repeated shortcode suffix searches (#103)
- _(build)_ Reuse render options across content pages (#106)

### Dependencies

- _(deps)_ Refresh project dependencies (#70)
- _(deps)_ Refresh project dependencies (#107)

## [0.4.0-rc.4] - 2026-10-06

### Breaking changes

- _(css)_ Compile shared and page-owned styles during site builds (#95)

### Added

- _(build)_ Report total build time and generated page counts (#88)
- _(output)_ Materialize symlinked source assets (#98)
- _(bench)_ Add reusable performance workloads (#105)

### Fixed

- _(convert)_ Handle admonitions without positional arguments (#89)
- _(taxonomy)_ Deduplicate repeated page tags (#90)
- _(template)_ Surface optional template loading errors (#92)
- _(render)_ Balance highlighted code line spans (#93)
- _(content)_ Propagate discovery read errors (#94)
- _(taxonomy)_ Reject tags with empty slugs (#97)
- _(serve)_ Watch site translation files (#99)
- _(serve)_ Preserve queries in directory redirects (#100)
- _(build)_ Normalize site URL joining (#96)
- _(attrs)_ Detect closing braces outside quoted values (#102)

### Changed

- _(render)_ Avoid repeated shortcode suffix searches (#103)
- _(build)_ Reuse render options across content pages (#106)

### Dependencies

- _(deps)_ Refresh project dependencies (#107)

## [0.4.0-rc.3] - 2026-10-03

### Breaking changes

- _(render)_ Use native code block disclosures (#82)

### Added

- _(render)_ Add per-page hierarchical heading numbering (#83)
- _(render)_ Support numbering starts on headings (#87)

### Fixed

- _(render)_ Escape custom code block classes (#85)

## [0.4.0-rc.2] - 2026-09-29

### Added

- _(build)_ Expose post tags and last update metadata (#78)

### Fixed

- _(directive)_ Render stable callout body structure (#79)

## [0.4.0-rc.1] - 2026-09-28

### Breaking changes

- _(render)_ Collect scoped endnotes with unique anchors and backlinks (#71)

### Added

- _(render)_ Mark short table columns as nowrap (#74)

### Fixed

- _(render)_ Keep rendered directive HTML out of the markdown pass (#77)

### Changed

- _(render)_ Speed up LQIP thumbnail generation (#72)

### Dependencies

- _(deps)_ Refresh project dependencies (#70)

## [0.3.0] - 2026-09-06

### Breaking changes

- Named menu groups + literal-friendly t() (#59)
- _(release)_ Drop x86_64-apple-darwin target
- _(text)_ Preserve URL-safe punctuation in slugs (#67)

### Added

- _(release)_ 4-platform release matrix + cancel stale runs (#61)
- _(build)_ Fingerprint static asset URLs (#63)

## [0.3.0-rc.3] - 2026-08-19

### Breaking changes

- _(release)_ Drop x86_64-apple-darwin target

### Added

- _(build)_ Fingerprint static asset URLs (#63)

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

[0.5.1]: https://github.com/hakula139/kiln/compare/v0.5.0..v0.5.1
[0.5.0]: https://github.com/hakula139/kiln/compare/v0.4.0..v0.5.0
[0.5.0-alpha.4]: https://github.com/hakula139/kiln/compare/v0.5.0-alpha.3..v0.5.0-alpha.4
[0.5.0-alpha.3]: https://github.com/hakula139/kiln/compare/v0.5.0-alpha.2..v0.5.0-alpha.3
[0.5.0-alpha.2]: https://github.com/hakula139/kiln/compare/v0.5.0-alpha.1..v0.5.0-alpha.2
[0.5.0-alpha.1]: https://github.com/hakula139/kiln/compare/v0.4.0..v0.5.0-alpha.1
[0.4.0]: https://github.com/hakula139/kiln/compare/v0.3.0..v0.4.0
[0.4.0-rc.4]: https://github.com/hakula139/kiln/compare/v0.4.0-rc.3..v0.4.0-rc.4
[0.4.0-rc.3]: https://github.com/hakula139/kiln/compare/v0.4.0-rc.2..v0.4.0-rc.3
[0.4.0-rc.2]: https://github.com/hakula139/kiln/compare/v0.4.0-rc.1..v0.4.0-rc.2
[0.4.0-rc.1]: https://github.com/hakula139/kiln/compare/v0.3.0..v0.4.0-rc.1
[0.3.0]: https://github.com/hakula139/kiln/compare/v0.2.0..v0.3.0
[0.3.0-rc.3]: https://github.com/hakula139/kiln/compare/v0.3.0-rc.2..v0.3.0-rc.3
[0.3.0-rc.2]: https://github.com/hakula139/kiln/compare/v0.3.0-rc.1..v0.3.0-rc.2
[0.3.0-rc.1]: https://github.com/hakula139/kiln/compare/v0.2.0..v0.3.0-rc.1
[0.2.0]: https://github.com/hakula139/kiln/compare/v0.1.0..v0.2.0
[0.1.0]: https://github.com/hakula139/kiln/releases/tag/v0.1.0
