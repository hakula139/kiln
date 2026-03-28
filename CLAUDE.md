# CLAUDE.md — kiln

## Project Overview

kiln is a custom static site generator (SSG) written in Rust, replacing a Hugo + LoveIt theme stack for [hakula.xyz](https://hakula.xyz).

**Status**:

- [x] Workspace scaffold + CLI
- [x] TOML configuration + content model (frontmatter, pages, discovery)
- [x] Markdown rendering (GFM, syntax highlighting, KaTeX math, images)
- [x] Directive parser + callout renderer + fenced divs
- [x] Render pipeline (directive processing → markdown → ToC)
- [x] MiniJinja template engine (OG / Twitter Card / SEO meta)
- [x] Single-page build pipeline
- [x] Multi-page builds + static file copying + pretty URLs + co-located assets
- [x] Theme system (layered templates, static files, param merging)
- [x] Pre-processors (image attrs, icon shortcodes, emoji shortcodes)
- [x] Code block wrapper with header, language label, and max-lines
- [x] Hugo → kiln content converter (`kiln convert`)
- [x] Directive template functions (`read_file`, `parse_csv`) + structured arg parsing
- [x] Directive renderers (site, music, score-table — template-based in theme / site)
- [x] Dev server with file watching, SSE live reload, and safe rebuild (`kiln serve`)
- [ ] Taxonomy support (tags, categories) with pagination
- [ ] Home page + section pages + special pages
- [ ] Tailwind CSS + dark theme
- [ ] RSS feed + sitemap
- [ ] Full-text search (Pagefind)
- [ ] 404 page + final polish

### CLI

```bash
kiln build [--root <dir>]                         # Build the site (default root: cwd)
kiln serve [--root <dir>] [--port 5456] [--open]  # Dev server with live reload
kiln init-theme <name> [--root]                   # Scaffold a new theme under themes/<name>/
kiln convert --source <dir> --dest <dir>          # Convert Hugo content to kiln format
```

### Project Layout

```text
.
├── config.toml   # Site configuration (TOML)
├── content/      # Markdown content (posts, standalone pages)
├── static/       # Static files copied to output root (favicons, images)
├── templates/    # MiniJinja templates (site overrides theme)
├── themes/       # Themes (git submodules), each with templates/ + static/
├── crates/kiln/  # SSG engine — library (lib.rs) + CLI binary (main.rs)
└── public/       # Build output (configurable via output_dir)
```

### Crate Structure (`crates/kiln/src/`)

```text
.
├── build.rs            # BuildContext, per-page rendering, taxonomy page generation, static / asset copying
├── config.rs           # TOML site configuration loading, theme resolution, param merging
├── init.rs             # Theme scaffolding (kiln init-theme)
├── convert.rs          # Hugo → kiln content converter orchestrator
├── convert/
│   ├── frontmatter.rs  # YAML → TOML frontmatter serde round-trip
│   └── shortcode.rs    # Hugo shortcode → kiln directive conversion
├── content/
│   ├── frontmatter.rs  # TOML frontmatter parsing (+++), Frontmatter with jiff timestamps
│   ├── page.rs         # Page struct, slug derivation, summary, output paths, co-located assets
│   └── discovery.rs    # Recursive content walking with draft / _-prefix / no-frontmatter exclusion
├── html.rs             # Shared HTML utilities (escape, indent, writeln_indented)
├── markdown.rs         # Shared raw-markdown text utilities (code fence detection, code span scanning)
├── text.rs             # Shared format-agnostic text utilities (slugify)
├── directive/          # :::-fenced directive parsing + rendering (shared types in directive.rs)
│   ├── parser.rs       # Line-based stack parser, nesting, single-pass arg + Pandoc attr parsing
│   ├── callout.rs      # 12 callout types (<details> with id / class propagation)
│   └── div.rs          # Fenced divs and unknown directives (<div> with id / class propagation)
├── output.rs           # File output, static file copying, output directory cleaning
├── pagination.rs       # Paginator for windowed views over slices, page URL computation
├── serve.rs            # Dev server with file watching, SSE live reload, script injection
├── taxonomy.rs         # TaxonomyKind, Taxonomy, Term, TaxonomySet, build_taxonomies()
├── render/             # Markdown rendering pipeline (RenderOptions in render.rs)
│   ├── emoji.rs        # GitHub-style :shortcode: → Unicode emoji replacement
│   ├── highlight.rs    # syntect CSS-class highlighting with line numbers, code-block wrapper
│   ├── icon.rs         # :(class): → <i> FontAwesome icon shortcode replacement
│   ├── image.rs        # Block (<figure>) and inline (<img>) image rendering, lazy loading
│   ├── image_attrs.rs  # Pandoc-style {#id .class width=N} extraction for images
│   ├── markdown.rs     # pulldown-cmark, GFM, CJK heading IDs, KaTeX, block / inline images
│   ├── pipeline.rs     # Full pipeline: directives → pre-processors → markdown → ToC
│   └── toc.rs          # TocEntry struct, nested <nav> table of contents generation
└── template.rs         # MiniJinja layered template engine, directive / taxonomy / term rendering
```

## Coding Conventions

### Error Handling

- Application code: `anyhow::Result` with `.context()` for actionable messages.
- Library error types: `thiserror::Error` derive for errors that callers need to match on.

### Lint Suppression

- Use `#[expect(lint)]` instead of `#[allow(lint)]`. `#[expect]` warns when the suppressed lint is no longer triggered, preventing stale suppressions from accumulating.

### Module Organization

- New-style module paths: `foo.rs` alongside `foo/` directory, not `foo/mod.rs`.
- Keep files focused: one primary type or concern per file.
- Place functions and types in the module that reflects their conceptual domain — import paths should not mislead about what the item does. Create new modules when needed for clean organization.
- Avoid deep `pub use` re-export chains that obscure where items are defined.
- Order helper functions by their caller.

### String Literals

- Prefer raw strings (`r#"..."#`) when the string contains characters that would need escaping (e.g., `"`, `\`). Avoid unnecessary backslash escapes.

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
- Test name prefixes should match the section's function name (or a clear shortening).
- Error-case test names use a return-type suffix: `_returns_error` (`Result`), `_returns_none` (`Option`), `_returns_false` (`bool`).
- Use `indoc!` for multi-line test inputs whenever possible.

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
