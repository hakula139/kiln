# Content Structure

kiln discovers content in the `content/` directory. This document describes how to organize pages, posts, and their assets.

## Content Directory

```text
content/
├── about-me/
│   └── index.md                  # Standalone page → /about-me/
├── posts/
│   ├── _index.md                 # Optional: sets title for /posts/ listing
│   ├── note/
│   │   ├── _index.md             # Optional: sets title for /posts/note/ listing
│   │   └── my-post/
│   │       ├── index.md          # Post (sectioned) → /posts/note/my-post/
│   │       ├── cover.webp        # Co-located asset
│   │       └── assets/
│   │           └── diagram.svg
│   └── standalone-post.md        # Post (orphan, no bundle) → /posts/standalone-post/
└── comments/
    └── index.md                  # Standalone page → /comments/
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
├── _assets/css/style.css     # Private page stylesheet source
├── index.md                  # Page content
├── cover.webp                # Image (co-located asset)
└── assets/
    ├── diagram.svg           # Nested assets work too
    └── data.csv              # Data files for directives
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

A page bundle's stylesheet source is `_assets/css/style.css`. `kiln build` and `kiln serve` compile it automatically and expose its content-hashed URL as [`page_css`](themes.md#post-templates-posthtml). Themes link it from that page's `<head>` after the shared stylesheet. Other CSS files remain ordinary co-located assets and are not automatically loaded.

```text
content/posts/avg/impressions/
├── _assets/css/style.css        # Handwritten source, never published
├── assets/                      # Images and other public page assets
└── index.md                     # Page content
```

Compiled CSS is written only to the build output at `<page>/assets/css/style.css`, alongside a fingerprinted copy. `--minify` minifies it before computing its SHA-256 fingerprint. Local `@import` rules are bundled, and relative `url(...)` references are resolved from their original source files and rewritten for the published stylesheet location. Referenced assets must belong to the page bundle or the site / theme `static/` tree. Underscore-prefixed bundle assets remain private and cannot be referenced by published CSS.

Plain CSS supports imports and nesting without an external compiler. Themes can select Tailwind through [`[css]`](themes.md#stylesheets), and page styles then receive the shared entry's Tailwind definitions through `@reference`, supporting utilities such as `@apply` without duplicating shared styles.

Only the owning page receives `page_css`, so other pages do not load its selectors and keyframes. Scope selectors carefully within the page to avoid unintentionally styling shared navigation or theme components. The `:::` directive can provide a wrapper class when needed:

<!-- dprint-ignore -->
```markdown
::: {.rating-table}
| Score | Title |
| ----- | ----- |
| 9.5   | Great |
:::
```

Then target that class in `_assets/css/style.css`:

```css
.rating-table td:first-child {
  font-weight: bold;
  color: red;
}
```

## Static Files

Files in the site's `static/` directory are copied to the output root. Use this for files shared across all pages:

```text
static/
├── favicon.ico       → /favicon.ico
├── images/
│   └── logo.png      → /images/logo.png
└── manifest.webmanifest
```

Static files differ from co-located assets: they are global (not tied to a page) and are referenced with absolute paths (e.g., `/images/logo.png`).

### Private build inputs (`_` prefix)

Files and directories whose names start with `_` are excluded from content discovery and page bundle asset publication. This keeps build inputs such as `_assets/css/style.css` with their owning page without exposing them. `_index.md` remains available as listing metadata.

Site and theme `_assets/` directories are build inputs outside their static trees. Every file in `static/` is explicitly published, including underscore-prefixed names such as `_headers` and `_redirects`. Keep private inputs outside `static/`.
