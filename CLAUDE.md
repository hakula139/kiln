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
├── build.rs            # BuildContext, build orchestration, per-page rendering, static / asset copying
├── build/              # Listing pipeline and output generators (submodules of build.rs)
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
├── content/            # Content model (module declarations in content.rs)
│   ├── discovery.rs    # Recursive content walking with draft / _-prefix / no-frontmatter exclusion
│   ├── frontmatter.rs  # TOML frontmatter parsing (+++), Frontmatter / FeaturedImage / ImageCredit
│   └── page.rs         # Page struct, PageKind, slug derivation, summary, output paths, co-located assets
├── convert.rs          # Hugo → kiln content converter orchestrator
├── convert/            # Hugo → kiln converter submodules (orchestrator in convert.rs)
│   ├── frontmatter.rs  # YAML → TOML frontmatter serde round-trip
│   └── shortcode.rs    # Hugo shortcode → kiln directive conversion
├── directive/          # :::-fenced directive parsing + rendering (shared types in directive.rs)
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
├── render/             # Markdown rendering pipeline (RenderOptions in render.rs)
│   ├── assets.rs       # PageAssets registry: scripts + auto-detected Feature flags (Math, Mermaid)
│   ├── emoji.rs        # GitHub-style :shortcode: → Unicode emoji replacement
│   ├── highlight.rs    # syntect + two-face CSS-class highlighting with line numbers, code-block wrapper
│   ├── icon.rs         # :(class): → <i> FontAwesome icon shortcode replacement
│   ├── image.rs        # Block (<figure>) and inline (<img>) image rendering, lazy loading
│   ├── image_attrs.rs  # Pandoc-style {#id .class width=N} extraction for images
│   ├── markdown.rs     # pulldown-cmark, GFM, CJK heading IDs, KaTeX, block / inline images
│   ├── mermaid.rs      # `<pre class="mermaid">` emit for ` ```mermaid ` fences (with data-source mirror)
│   ├── pipeline.rs     # Full pipeline: directives → pre-processors → markdown → ToC
│   └── toc.rs          # TocEntry struct, nested <nav> table of contents generation
├── search.rs           # Pagefind search indexing (external binary invocation)
├── section.rs          # Section struct, collect_sections() from page kinds, _index.md title loading
├── serve.rs            # Dev server with file watching, WebSocket live reload, script injection
├── sitemap.rs          # Sitemap XML + robots.txt generation
├── taxonomy.rs         # TaxonomyKind, Taxonomy, Term, TaxonomySet, build_taxonomies()
├── template.rs         # MiniJinja layered template engine, directive / archive / overview / error rendering
├── template/           # Template submodules
│   └── vars.rs         # Template variables structs (PostTemplateVars, PageSummary, etc.)
├── test_utils.rs       # Shared test infrastructure (templates, helpers, Page factory)
└── text.rs             # Shared format-agnostic text utilities (slugify, titlecase)
```

## Coding Conventions

### Error Handling

- Application code: `anyhow::Result` with `.context()` for actionable messages.
- Library error types: `thiserror::Error` derive for errors that callers need to match on.
- Avoid `unwrap()` / `expect()` in production code. Reserve them for cases with a clear invariant comment.

### Lint Suppression

- Use `#[expect(lint)]` instead of `#[allow(lint)]`. `#[expect]` warns when the suppressed lint is no longer triggered, preventing stale suppressions from accumulating.
- `#[expect]` reason strings must describe the current state, not future plans.

### Section Dividers

- Use `// ── Section Name ──` for section dividers in code (box-drawing character `─`, U+2500).
- In tests, use `// ── function_name ──` as section headers grouping tests by the function they cover.

### Blank Lines

- One blank line between top-level items (functions, structs, enums, impls, constants).
- One blank line before and after section dividers (`// ── Name ──`).
- Inside function bodies, use blank lines to separate logical phases (e.g., setup → validation → execution → result).
- Group a single-line computation with its immediate validation guard (early-return `if`) — no blank between them. Multi-line `let` bindings (async chains, builder patterns) keep the blank before their guard.

### Module Organization

- New-style module paths: `foo.rs` alongside `foo/` directory, not `foo/mod.rs`.
- Keep files focused: one primary type or concern per file. When a file or function grows large, split it into smaller units proactively rather than letting it accumulate.
- Place functions and types in the module that reflects their conceptual domain — import paths should not mislead about what the item does. Create new modules when needed for clean organization.
- Avoid `pub use` re-exports that obscure where items are defined. Prefer consistent import paths — if some items are re-exported, re-export all related items so callers never mix paths.
- Order helper functions after their caller (top-down reading order).
- When adding new fields to structs or variants to enums, place them at the most semantically appropriate position among existing members, not simply appended at the bottom.

### Visibility

- Default to the smallest visibility needed: private → `pub(crate)` → `pub`.
- `pub` items form the crate's API surface. Use `pub(crate)` for items shared across modules but not intended for external use.

### Imports

- Group `use` statements in three blocks separated by blank lines: std → external crates → internal modules.
- Within each block, sort alphabetically. For internal imports, `rustfmt` orders by locality: `self` → `super` → `crate`.

### String Literals

- Prefer raw strings (`r"..."`) when the string contains characters that would need escaping (e.g., `"`, `\`). Always use the minimum delimiter level needed (`r"..."` → `r#"..."#` → `r##"..."##`).
- Use `indoc!` / `formatdoc!` for multiline string content so the literal can be indented with surrounding code. Inline at the call site when the string is used once; use a named constant only when it is shared or very large. Avoid `\n` escapes and `\x20` workarounds for multiline content.

### Enum String Mappings

- Use `strum` derives (`AsRefStr`, `EnumString`, `EnumIter`) for enum ↔ string conversions instead of handwritten matches.
- Keep manual `Display` impls when the display form differs from the serialized form (e.g., titlecase vs. lowercase).

### Dependencies

- Versions centralized in `[workspace.dependencies]` in the root `Cargo.toml`. Member crates reference them with `dep.workspace = true`.
- Only add dependencies to the workspace when a PR first needs them.
- Prefer crates with minimal transitive dependencies.

### Git Conventions

#### Commits

- Messages: `type(scope): description`
  - Types: `feat`, `fix`, `refactor`, `docs`, `test`, `ci`, `chore`, `style`, `perf`
  - Scope: the most specific area changed — module (e.g., `config`, `render`, `directive`), doc target (e.g., `CLAUDE`, `roadmap`), or crate name only for cross-module changes.
- Keep commits atomic — one logical change per commit.

#### Branches

- Feature branches: `feat/<feature-name>`

#### Pull Requests

- Assign to `hakula139`. Label `enhancement` for `feat`, `bug` for `fix`.
- Do not request review from the PR author (GitHub rejects it).

### Testing

- Unit tests in the same file as the code they test (`#[cfg(test)]` module).
- Integration tests in `tests/` directory for cross-module behavior.
- Group tests by function under `// ── function_name ──` section headers. Section order must mirror the production function order in the same file. Within each section, order: happy path → variants → edge / error cases.
- Test name prefixes should match the section's function name (or a clear shortening). Name tests after the scenario they cover. Error-case test names use a return-type suffix: `_returns_error` (`Result`), `_returns_none` (`Option`), `_returns_false` (`bool`).
- Use `indoc!` for multi-line test inputs whenever possible.
- Use generic, fictional test data (e.g., `example.com`, `"Hello"`, `"Post A"`). Avoid real names, URLs, or branded content.
- Write assertions that verify actual behavior, not just surface properties. Avoid uniform test data that makes `starts_with` / `ends_with` unfalsifiable, wildcard struct matches (`..`) that discard field values, and loose bounds that accept nearly any output. Each assertion should fail if the code under test has a plausible bug.
- Prefer a concise test suite with full coverage over many minimal tests. Drop tests that are subsumed by more thorough ones. Merge tests that cover the same code path when the combined test remains readable.

### Documentation Maintenance

- Keep `README.md` user-facing. It should describe value, supported features, and usage, not internal progress tracking.
- Keep `docs/roadmap.md` as the canonical in-repo roadmap / status summary. Update it when shipped capability areas or planned priorities change.
- Crate structure diagrams must match the actual filesystem. When adding, removing, or renaming modules, update the tree in this file. Entries are sorted alphabetically; directories sort alongside their parent `.rs` file.

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
- Cross-file consistency — parallel types and similar patterns should use the same structure, naming, ordering, and derive traits
- Idiomatic Rust — proper use of iterators, pattern matching, type system, ownership, and standard library
- Existing crates — flag hand-written logic that an established crate already handles
- Test coverage gaps
