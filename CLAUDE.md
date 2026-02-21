# CLAUDE.md — kiln

## Project Overview

kiln is a custom static site generator (SSG) written in Rust, replacing a Hugo + LoveIt theme stack for [hakula.xyz](https://hakula.xyz).

**Status**: workspace scaffold + CLI + TOML config loading. No content processing yet.

### CLI

```bash
kiln build [--root <dir>]   # Build the site (default root: cwd)
```

### Project Layout

```text
config.toml      # Site configuration (TOML)
crates/kiln/     # SSG engine — library (lib.rs) + CLI binary (main.rs)
public/          # Build output (configurable via output_dir)
```

### Crate Structure (`crates/kiln/src/`)

- `config` — TOML site configuration loading + defaults

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

## Verification

Run before committing:

```bash
cargo build
cargo clippy --all-targets  # zero warnings (pedantic lints)
cargo test
cargo llvm-cov              # check test coverage
```
