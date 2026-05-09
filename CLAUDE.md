# CLAUDE.md — kiln

## Project Overview

kiln is a custom static site generator (SSG) written in Rust, replacing a Hugo + LoveIt theme stack for [hakula.xyz](https://hakula.xyz).

User-facing feature positioning belongs in `README.md`. The canonical in-repo roadmap / status summary lives in `docs/roadmap.md`. Do not duplicate long feature checklists in this file.

### CLI

```bash
kiln build [--root <dir>] [--minify]                         # Build the site (default root: cwd)
kiln serve [--root <dir>] [--port 5456] [--open]             # Dev server with live reload
kiln init-theme <name> [--root]                              # Scaffold a new theme under themes/<name>/
kiln convert --source <dir> --dest <dir>                     # Convert a Hugo site root into a kiln site root
```

Both `kiln build` and `kiln serve` run Pagefind search indexing automatically when `[search] enabled = true` in `config.toml`.

`kiln convert` expects site roots. It reads `source/content`, writes to `dest/content`, and copies `source/static` to `dest/static` without overwriting existing destination files.

### Project Layout

```text
.
├── config.toml   # Site configuration (TOML)
├── content/      # Markdown content (posts, standalone pages)
├── crates/kiln/  # SSG engine — library (lib.rs) + CLI binary (main.rs)
├── public/       # Build output (configurable via output_dir)
├── static/       # Static files copied to output root (favicons, images)
├── templates/    # MiniJinja templates (site overrides theme)
└── themes/       # Themes (git submodules), each with templates/ + static/
```

### Crate Structure (`crates/kiln/src/`)

```text
.
├── attrs.rs            # Pandoc-style `{#id .class key=value}` attribute parser, shared across renderers
├── build.rs            # BuildContext, build orchestration, per-page rendering, static / asset copying
├── build/              # Listing pipeline and output generator submodules
│   ├── archive.rs      # Paginated year-grouped archive pages (/posts/, /posts/<section>/, /tags/<slug>/)
│   ├── error.rs        # 404 error page generation
│   ├── feed.rs         # RSS feed orchestration (main + per-section + per-term feeds)
│   ├── home.rs         # Paginated home page generation
│   ├── listing.rs      # ListedPage model, single-pass ListingArtifacts construction, sorting / grouping helpers
│   ├── overview.rs     # Bucket overview index pages (/sections/, /tags/)
│   ├── paginate.rs     # Generic write_paginated, paginate_config
│   ├── sitemap.rs      # sitemap.xml + robots.txt generation
│   └── url.rs          # page_url, resolve_relative_url — build-time URL resolution helpers
├── config.rs           # TOML site configuration loading, theme resolution, param merging
├── content.rs          # Module declarations for content/ submodules
├── content/            # Content model submodules
│   ├── discovery.rs    # Recursive content walking with draft / _-prefix / no-frontmatter exclusion
│   ├── frontmatter.rs  # TOML frontmatter parsing (+++), Frontmatter / FeaturedImage / ImageCredit
│   └── page.rs         # Page struct, PageKind, slug derivation, summary, output paths, co-located assets
├── convert.rs          # Hugo → kiln content converter orchestrator
├── convert/            # Hugo → kiln converter submodules
│   ├── frontmatter.rs  # YAML → TOML frontmatter serde round-trip
│   └── shortcode.rs    # Hugo shortcode → kiln directive conversion
├── directive.rs        # Directive shared types (CalloutKind, DirectiveKind, DirectiveContext) + arg parser
├── directive/          # :::-fenced directive parsing + rendering submodules
│   ├── callout.rs      # 12 callout types (<details> with id / class propagation)
│   ├── div.rs          # Fenced divs and unknown directives (<div> with id / class propagation)
│   └── parser.rs       # Line-based stack parser, nesting, single-pass arg + Pandoc attr parsing
├── feed.rs             # RSS 2.0 XML generation (Channel, generate_rss, RFC 2822 date formatting)
├── html.rs             # Shared HTML utilities (escape, indent, writeln_indented)
├── i18n.rs             # Layered i18n resolver (site → theme lang → theme English), t() with placeholder interpolation
├── init.rs             # Theme scaffolding (kiln init-theme)
├── markdown.rs         # Shared raw-markdown text utilities (code fence detection, code span scanning)
├── minify.rs           # Post-build HTML / CSS / JS minification (lightningcss, oxc_minifier, minify-html)
├── output.rs           # File output, static file copying, output directory cleaning
├── pagination.rs       # Paginator for windowed views over slices, page URL computation
├── render.rs           # RenderOptions struct + render submodule declarations
├── render/             # Markdown rendering pipeline submodules
│   ├── assets.rs       # PageAssets registry: scripts + auto-detected Feature flags (Math, Mermaid)
│   ├── code_block.rs   # Fence info-string parsing → CodeBlockSpec (lang, title, highlights, collapse / expand)
│   ├── emoji.rs        # GitHub-style :shortcode: → Unicode emoji replacement
│   ├── highlight.rs    # syntect + two-face CSS-class highlighting with line numbers, header (lang or title)
│   ├── icon.rs         # :(class): → <i> FontAwesome icon shortcode replacement
│   ├── image.rs        # Block (<figure>) and inline (<img>) image rendering, lazy loading, <span class="lqip"> wrapper emission
│   ├── image_attrs.rs  # Pandoc-style {#id .class width=N} extraction for images
│   ├── lqip.rs         # ImageResolver: on-disk dimension reads + base64 WebP placeholder encoding (consumed via the .lqip wrapper)
│   ├── markdown.rs     # pulldown-cmark, GFM, CJK heading IDs, KaTeX, block / inline images
│   ├── mermaid.rs      # `<pre class="mermaid">` emit for `` ```mermaid `` fences (with data-source mirror)
│   ├── pipeline.rs     # Full pipeline: directives → pre-processors → markdown → ToC
│   └── toc.rs          # TocEntry struct, nested <nav> table of contents generation
├── search.rs           # Pagefind search indexing (external binary invocation)
├── section.rs          # Section struct, collect_sections() from page kinds, _index.md title loading
├── serve.rs            # Dev server with file watching, WebSocket live reload, script injection
├── sitemap.rs          # Sitemap XML + robots.txt generation
├── taxonomy.rs         # TaxonomyKind, Taxonomy, Term, TaxonomySet, build_taxonomies()
├── template.rs         # MiniJinja layered template engine, directive / archive / overview / error rendering
├── template/           # Template submodules
│   ├── functions.rs    # MiniJinja template functions (now, read_file, parse_csv, t, register_script)
│   └── vars.rs         # Template variables structs (PostTemplateVars, PageSummary, etc.)
├── test_utils.rs       # Shared test infrastructure (templates, helpers, Page factory)
└── text.rs             # Shared format-agnostic text utilities (slugify, titlecase)
```

## Coding Conventions

### Error Handling

- Application code: `anyhow::Result` with `.context()` for actionable messages.
- Library error types: `thiserror::Error` derive for errors that callers need to match on.
- Avoid `unwrap()` / `expect()` in production code. Reserve them for cases with a clear invariant comment.

### Discarding Results

- Use `_ = expr` (no `let`) to discard a result — typically infallible `write!` / `writeln!` against a `String`.

### Lint Suppression

- Use `#[expect(lint)]` instead of `#[allow(lint)]`. `#[expect]` warns when the suppressed lint is no longer triggered, preventing stale suppressions from accumulating.
- `#[expect]` reason strings must describe the current state, not future plans.
- For complexity / size lints (`clippy::too_many_lines`, `clippy::cognitive_complexity`, etc.), the default response is to **extract a helper**. Reach for `#[expect]` only when the function is irreducibly cohesive. Say so in the reason string.

### Comments

- Comment the **why**, not the **what**. Comments earn their place by explaining intent, trade-offs, invariants, or constraints the code can't convey on its own. Skip comments that restate the code or narrate the change.
- Keep `//` comments to one line per thought. Multi-line only when the rationale genuinely needs it.
- Doc comments (`///`) state the **contract**, not **mechanics**. One-line doc is the default; multi-line only when the contract genuinely warrants it.
- Wrap comments at **100 columns** (matching `rustfmt` max_width).
- Write `//` comments as prose. Promote to `///` if list structure is genuinely useful.

### Section Dividers

- Use `// ── Section Name ──` for section dividers in code (box-drawing character `─`, U+2500).
- In tests, use `// ── function_name ──` as section headers grouping tests by the function they cover.

### Blank Lines

- One blank line between top-level items (functions, structs, enums, impls, constants). Exception: runs of closely-related one-line `const` / `static` declarations sharing a theme may sit together without blanks.
- One blank line before and after section dividers (`// ── Name ──`). This applies inside `#[cfg(test)]` modules too. The first divider takes a blank line after the `use super::*;` block.
- Inside function bodies, use blank lines to separate logical phases (e.g., setup → validation → execution → result).
- Group a single-line computation with its immediate validation guard (early-return `if`) — no blank between them. Multi-line `let` bindings (async chains, builder patterns) keep the blank before their guard.

### Module Organization

- New-style module paths: `foo.rs` alongside `foo/` directory, not `foo/mod.rs`.
- Keep files focused: one primary type or concern per file. Split proactively when files grow large.
- Place functions and types in the module that reflects their conceptual domain. A cross-module trait belongs where the **contract** lives, not the first implementation. Create new modules when needed for clean organization.
- Avoid `pub use` re-exports that obscure where items are defined. If some items are re-exported, re-export all related items so callers never mix paths.
- Order helper functions after their caller (top-down reading order).
- New struct fields / enum variants go at the most semantically appropriate position, not just appended at the bottom.

### Visibility

- Default to the smallest visibility needed: private → `pub(crate)` → `pub`.
- `pub` items form the crate's API surface. Use `pub(crate)` for items shared across modules but not intended for external use.

### Imports

- Group `use` statements in three blocks separated by blank lines: std → external crates → internal modules.
- Within each block, sort alphabetically. For internal imports, `rustfmt` orders by locality: `self` → `super` → `crate`.

### String Literals

- Prefer raw strings (`r"..."`) when the string contains characters that would need escaping. Always use the minimum delimiter level needed (`r"..."` → `r#"..."#` → `r##"..."##`).
- Use `indoc!` / `formatdoc!` for multiline string content so the literal can be indented with surrounding code. Inline at the call site when the string is used once; use a named constant only when it is shared or very large. Avoid `\n` escapes and `\x20` workarounds for multiline content.
- Ellipsis: always `...` (three ASCII dots), never `…` (U+2026). Applies everywhere: prose, comments, doc comments, and strings.

### Enum String Mappings

- Use `strum` derives (`AsRefStr`, `EnumString`, `EnumIter`) for enum ↔ string conversions instead of handwritten matches.
- Keep manual `Display` impls when the display form differs from the serialized form (e.g., titlecase vs. lowercase).

### Dependencies

- Versions centralized in `[workspace.dependencies]` in the root `Cargo.toml`. Member crates reference them with `dep.workspace = true`.
- Only add dependencies to the workspace when a PR first needs them.
- Prefer crates with minimal transitive dependencies.

### Git Conventions

Follows global CLAUDE.md commit / branch / PR conventions, plus:

- **Scope**: the most specific area changed — module (e.g., `config`, `render`, `directive`), doc target (e.g., `CLAUDE`, `roadmap`), or crate name only for cross-module changes.
- **PRs**: assign to `hakula139`. Label `enhancement` for `feat`, `bug` for `fix`. Do not request review from the PR author (GitHub rejects it).

### Testing

- Unit tests in the same file as the code they test (`#[cfg(test)]` module).
- Integration tests in `tests/` directory for cross-module behavior.
- Group tests by function under `// ── function_name ──` section headers. Section order must mirror the production function order in the same file. Within each section, order: happy path → variants → edge / error cases.
- Test name prefixes match the section's function name. Name after the scenario. Error-case suffixes: `_returns_error`, `_returns_none`, `_returns_false`.
- Use `indoc!` for multi-line test inputs.
- Use generic, fictional test data (e.g., `example.com`, `"Post A"`). Avoid real names or branded content.
- Assertions must verify actual behavior. Avoid unfalsifiable patterns (uniform data with `starts_with`, wildcard `..` matches, loose bounds). Each assertion should fail if the code under test has a plausible bug.
- Prefer a concise suite with full coverage over many minimal tests. Merge tests that cover the same path.

### Documentation Maintenance

- Keep `README.md` user-facing. It should describe value, supported features, and usage, not internal progress tracking.
- Keep `docs/roadmap.md` as the canonical in-repo roadmap / status summary. Update it when shipped capability areas or planned priorities change.
- Crate structure diagrams must match the actual filesystem. When adding, removing, or renaming modules, update the tree in this file. Entries are sorted alphabetically; directories sort alongside their parent `.rs` file.
- Markdown prose is **not hard-wrapped** — paragraphs are single long lines and flow with the reader's viewport. Match the surrounding style; do not introduce 80-column line breaks inside paragraphs.
- After substantive changes, sweep docs for stale claims: `README.md` feature lists, `docs/roadmap.md` status sections, and this file's crate tree.

## Nix Development

`flake.nix` pins the Rust toolchain, `libdav1d` (AVIF), `pagefind`, and `git-cliff` for the dev shell. It also exposes `packages.{default,kiln,pagefind}` so site repos can consume kiln as a flake input (`inputs.kiln.url = "github:hakula139/kiln";`) — `kiln` is source-built (dav1d wired in by Nix), `pagefind` is a vendored prebuilt under `packages/pagefind/`.

```bash
nix develop                            # interactive shell (for hacking on kiln)
nix flake check                        # run pre-commit hooks
nix build '.#kiln'                     # build kiln from source
```

`direnv` auto-activates the shell via `.envrc`.

### Pre-commit hooks

Hygiene (`check-added-large-files`, `check-yaml`, `end-of-file-fixer`, `trim-trailing-whitespace`), Nix (`nixfmt`, `statix`, `deadnix`), and `rustfmt`. Clippy stays in CI — the bare hook process can't see `libdav1d`.

### Adding native dependencies

Append to `devShells.default.packages` in `flake.nix`. `*-sys` crates that use `pkg-config` also need their `.pc` file on `PKG_CONFIG_PATH`.

## Verification

Run after implementation and before review:

```bash
cargo fmt --all --check                            # formatting
cargo build
cargo clippy --all-targets -- -D warnings          # zero warnings (pedantic lints)
cargo test
cargo llvm-cov --ignore-filename-regex 'main\.rs'  # check test coverage
```

## Code Review

After verification passes, run a dual review using both a reviewer subagent and a Codex MCP reviewer in parallel. Focus on:

- Correctness and edge cases
- Adherence to project conventions (this file)
- Conciseness — prefer the simplest idiomatic solution
- DRY — flag duplicate logic across modules; look for extraction opportunities
- Cross-file consistency — parallel types should use the same structure, naming, ordering, and derive traits
- Comment hygiene — verbose multi-line docs that should be one-liners, missing WHY comments where non-obvious
- Visibility — `pub(crate)` where `pub(super)` or private suffices
- Idiomatic Rust — iterators, pattern matching, type system, ownership, standard library
- Existing crates — flag hand-written logic that an established crate already handles
- Test coverage gaps
