# Content Structure

kiln discovers content in the `content/` directory. This document describes how to organize pages, posts, and their assets.

## Content Directory

```text
content/
├── about-me/
│   └── index.md             # Standalone page → /about-me/
├── posts/
│   ├── _index.md            # Optional: sets title for /posts/ listing
│   ├── note/
│   │   ├── _index.md        # Optional: sets title for /posts/note/ listing
│   │   └── my-post/
│   │       ├── index.md     # Post (sectioned) → /posts/note/my-post/
│   │       ├── cover.webp   # Co-located asset
│   │       └── assets/
│   │           └── diagram.svg
│   └── standalone-post.md   # Post (orphan, no bundle) → /posts/standalone-post/
└── comments/
    └── index.md             # Standalone page → /comments/
```

### Page Kinds

kiln classifies pages based on their location under `content/`:

| Location                                  | Kind             | Listed? | Section page? |
| ----------------------------------------- | ---------------- | ------- | ------------- |
| `content/posts/<section>/<slug>/index.md` | Post (sectioned) | Yes     | Yes           |
| `content/posts/<section>/<slug>.md`       | Post (sectioned) | Yes     | Yes           |
| `content/posts/<slug>/index.md`           | Post (orphan)    | Yes     | No            |
| `content/posts/<slug>.md`                 | Post (orphan)    | Yes     | No            |
| `content/<slug>/index.md`                 | Standalone page  | No      | No            |

Posts appear on the home page, posts index, tag archives, and (if sectioned) section pages. Standalone pages are rendered but excluded from all listings.

### Sections

A section is a subdirectory directly under `content/posts/`. Posts inside `content/posts/note/` belong to the `note` section. Each section gets its own archive page at `/posts/<section>/`.

To set a custom title for a section listing, add a `_index.md` with frontmatter:

```toml
+++
title = "笔记"
+++
```

Without `_index.md`, the section title is derived from the directory name (titlecased).

### Drafts and Exclusion

Pages are excluded from the build when:

- `draft = true` in frontmatter
- A file or enclosing directory name starts with `_`, which includes the `_index.md` listing metadata files
- The file has no TOML frontmatter (`+++` delimiters)

## Page Bundles

A **page bundle** is a directory containing an `index.md` alongside related files. Bundles are the recommended way to organize pages because they keep content and assets together. Non-bundle `.md` files get pretty URLs but cannot use co-located assets or per-page CSS.

```text
content/posts/note/my-post/
├── assets/
│   ├── css/_src/style.css   # Private page stylesheet source
│   ├── data.csv            # Data files for directives
│   └── diagram.svg         # Nested assets work too
├── cover.webp              # Image (co-located asset)
└── index.md                # Page content
```

Non-markdown files in the bundle directory (at any depth), excluding underscore-prefixed files and directories, are copied to the output alongside the rendered HTML. They become accessible at the same relative path:

| Source                                          | Output URL                               |
| ----------------------------------------------- | ---------------------------------------- |
| `content/posts/note/my-post/cover.webp`         | `/posts/note/my-post/cover.webp`         |
| `content/posts/note/my-post/assets/diagram.svg` | `/posts/note/my-post/assets/diagram.svg` |

### Referencing Co-Located Assets

Use relative paths in markdown to reference assets in the same bundle:

```markdown
![Diagram](assets/diagram.svg)
```

For featured images in frontmatter, relative paths are resolved against the page URL:

```toml
+++
title = "My Post"

[featured_image]
src = "cover.webp"
+++
```

This resolves to `/posts/note/my-post/cover.webp` in templates and listing pages. Absolute paths (starting with `/`) and external URLs are used as-is.

### Per-Page CSS

Put the page stylesheet at `assets/css/_src/style.css` within its bundle. See [Assets and Stylesheets](assets.md#stylesheet-sources) for compilation, processor setup, asset URLs, and template loading.

The `:::` directive can provide a wrapper class for page-specific selectors:

<!-- dprint-ignore -->
```markdown
::: {.rating-table}
| Score | Title |
| ----- | ----- |
| 9.5   | Great |
:::
```

Then target that class in `assets/css/_src/style.css`:

```css
.rating-table td:first-child {
  font-weight: bold;
  color: red;
}
```

## Shared Assets

Use `assets/` for shared public files and `static/` for files requiring output-root paths. See [Public Files](assets.md#public-files) for publication rules and override precedence.
