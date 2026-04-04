# Plan: Config-Driven Taxonomies

## Problem

`TaxonomyKind` is a hardcoded enum with only `Tags`. Adding a new taxonomy (categories, series, etc.) requires modifying engine source code.

## Proposed Design

Config-driven taxonomies via `config.toml`:

```toml
[[taxonomies]]
name = "tags"

# [[taxonomies]]
# name = "categories"
```

### Changes Required

1. **config.rs**: Add `taxonomies: Vec<TaxonomyConfig>` with default `[{name: "tags"}]`
2. **taxonomy.rs**: Replace `TaxonomyKind` enum with `String`-based key; `build_taxonomies()` takes config and dynamically collects from frontmatter
3. **frontmatter.rs**: Support arbitrary taxonomy fields (catch-all `HashMap` or named-field mapping)
4. **build.rs / template.rs**: Pass taxonomy metadata through to templates

### Open Questions

- Frontmatter approach: `#[serde(flatten)]` with `HashMap<String, Vec<String>>` vs. keeping known fields and mapping by name
- Default behavior when `[[taxonomies]]` is absent (backward compat: default to tags)

## Status

Tracked for a future PR. Identified during Phase 3 of the theme redesign work.
