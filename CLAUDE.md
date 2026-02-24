# CLAUDE.md — kiln

## Project Overview

kiln is a custom static site generator (SSG) written in Rust, replacing a Hugo + LoveIt theme stack for [hakula.xyz](https://hakula.xyz).

**Status**:

- [x] Workspace scaffold + CLI
- [x] TOML configuration + content model (frontmatter, pages, discovery)
- [x] Markdown rendering (GFM, syntax highlighting, KaTeX math, images)
- [x] Directive parser + admonition renderer
- [x] Render pipeline (directive processing → markdown → ToC)
- [x] MiniJinja template engine (OG / Twitter Card / SEO meta)
- [x] Single-page build pipeline
- [x] Multi-page builds + static file copying + pretty URLs + co-located assets
- [ ] Hugo → kiln content converter (`kiln convert`)
- [ ] Remaining directive renderers (style, embed, site, score-table)
- [ ] Taxonomy support (tags, categories) with pagination
- [ ] Home page + section pages + special pages
- [ ] Tailwind CSS + dark theme
- [ ] RSS feed + sitemap
- [ ] Full-text search (Pagefind)
- [ ] 404 page + final polish

### CLI

```bash
kiln build [--root <dir>]   # Build the site (default root: cwd)
```

### Project Layout

```text
config.toml      # Site configuration (TOML)
content/         # Markdown content (posts, standalone pages)
static/          # Static files copied to output root (favicons, images)
templates/       # MiniJinja templates (base.html, post.html)
crates/kiln/     # SSG engine — library (lib.rs) + CLI binary (main.rs)
public/          # Build output (configurable via output_dir)
```

### Crate Structure (`crates/kiln/src/`)

- `build` — build orchestration: `BuildContext`, per-page rendering, canonical URL computation, static file + co-located asset copying
- `config` — TOML site configuration loading + defaults
- `content/` — content model
  - `frontmatter` — TOML frontmatter parsing (`+++` delimited), `Frontmatter` struct with jiff timestamps
  - `page` — `Page` struct, slug derivation, summary extraction, output path computation, co-located asset discovery
  - `discovery` — recursive content directory walking with draft / `_`-prefix exclusion
- `directive/` — `:::`-fenced directive parsing + rendering (shared types in `directive.rs`)
  - `parser` — line-based stack parser with nesting + code block awareness
  - `admonition` — HTML renderer for 12 admonition types
- `output` — file output, static file copying, output directory cleaning
- `render/` — markdown rendering pipeline (shared `escape_html` utility in `render.rs`)
  - `highlight` — syntect CSS-class syntax highlighting with line numbers, canonical language labels
  - `image` — block (`<figure>`) and inline (`<img>`) image rendering with lazy loading
  - `markdown` — pulldown-cmark rendering with GFM extensions, CJK-aware heading ID generation, KaTeX math, syntax highlighting, block / inline image detection
  - `pipeline` — full render pipeline: directive processing → markdown rendering → ToC generation
  - `toc` — `TocEntry` struct, nested `<nav>` table of contents HTML generation
- `template` — MiniJinja template engine with `PostTemplateVars` view model

## Coding Conventions

### Error Handling

- Application code: `anyhow::Result` with `.context()` for actionable messages.
- Library error types: `thiserror::Error` derive for errors that callers need to match on.

### Lint Suppression

- Use `#[expect(lint)]` instead of `#[allow(lint)]`. `#[expect]` warns when the suppressed lint is no longer triggered, preventing stale suppressions from accumulating.

### Module Organization

- New-style module paths: `foo.rs` alongside `foo/` directory, not `foo/mod.rs`.
- Keep files focused: one primary type or concern per file.
- Avoid deep `pub use` re-export chains that obscure where items are defined.
- Order helper functions by their caller.

### Enum String Mappings

- Use `strum` derives (`AsRefStr`, `EnumString`, `EnumIter`) for enum ↔ string conversions instead of handwritten matches.
- Keep manual `Display` impls when the display form differs from the serialized form (e.g., titlecase vs. lowercase).

### Dependencies

- Versions centralized in `[workspace.dependencies]` in the root `Cargo.toml`. Member crates reference them with `dep.workspace = true`.
- Only add dependencies to the workspace when a PR first needs them.
- Prefer crates with minimal transitive dependencies.

### Git Conventions

- Commit messages: `type(scope): description`
  - Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `style`, `perf`
  - Scope: crate or module name (e.g., `kiln`, `config`, `render`)
- Feature branches: `feat/<feature-name>`
- Keep commits atomic — one logical change per commit.

### Testing

- Unit tests in the same file as the code they test (`#[cfg(test)]` module).
- Integration tests in `tests/` directory for cross-module behavior.
- Group tests by function under `// -- function_name --` section headers. Within each section, order: happy path → variants → error cases.

## Verification

Run after implementation and before review:

```bash
cargo build
cargo clippy --all-targets -- -D warnings  # zero warnings (pedantic lints)
cargo test
cargo llvm-cov --ignore-filename-regex 'main\.rs'  # check test coverage
```

## Code Review

After verification passes, run a dual review using both a reviewer subagent and a Codex MCP reviewer in parallel. Focus on:

- Correctness and edge cases
- Adherence to project conventions (this file)
- Conciseness — prefer the simplest idiomatic solution
- Existing crates — flag hand-written logic that an established crate already handles
- Test coverage gaps
