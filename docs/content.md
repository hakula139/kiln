# Content Structure

kiln discovers content in the `content/` directory. This document describes how to organize pages, posts, and their assets.

## Content Directory

```text
content/
├── about-me/
│   └── index.md                          # Standalone page → /about-me/
├── posts/
│   ├── _index.md                         # Optional: sets title for /posts/ listing
│   ├── note/
│   │   ├── _index.md                     # Optional: sets title for /posts/note/ listing
│   │   └── my-post/
│   │       ├── index.md                  # Post (sectioned) → /posts/note/my-post/
│   │       ├── cover.webp                # Co-located asset
│   │       └── assets/
│   │           └── diagram.svg
│   └── standalone-post.md                # Post (orphan, no bundle) → /posts/standalone-post/
└── comments/
    └── index.md                          # Standalone page → /comments/
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
- The filename starts with `_` (including `_index.md` — these are listing metadata files, not pages)
- The file has no TOML frontmatter (`+++` delimiters)

## Page Bundles

A **page bundle** is a directory containing an `index.md` alongside related files. Bundles are the recommended way to organize pages because they keep content and assets together. Non-bundle `.md` files get pretty URLs but cannot use co-located assets or per-page CSS.

```text
content/posts/note/my-post/
├── index.md           # Page content
├── cover.webp         # Image (co-located asset)
├── style.css          # Per-page CSS (auto-detected)
└── assets/
    ├── diagram.svg    # Nested assets work too
    └── data.csv       # Data files for directives
```

All non-markdown files in the bundle directory (at any depth) are copied to the output alongside the rendered HTML. They become accessible at the same relative path:

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

A page bundle may include a `style.css` file at any depth. kiln auto-detects it and injects a `<link>` tag in the page's `<head>`, after the main stylesheet.

```text
content/posts/avg/impressions/
├── index.md
└── style.css         ← auto-detected
```

or:

```text
content/posts/avg/impressions/
├── index.md
└── assets/
    └── style.css     ← also detected (nested)
```

The CSS is **plain CSS** — not processed by Tailwind or any other tool. To scope styles to the page, use the `:::` directive system to create a wrapper `<div>` with a class:

```markdown
::: { .rating-table }
| Score | Title |
| ----- | ----- |
| 9.5   | Great |
:::
```

Then target that class in `style.css`:

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

Files and directories whose names start with `_` are skipped when `static/` is copied to the output. This lets you keep build-time inputs alongside the shipped bundle without exposing them. Typical use: colocating Tailwind sources with the compiled stylesheet.

```text
static/
├── css/
│   ├── _src/           # not copied to output
│   │   ├── main.css
│   │   └── components/
│   └── style.css       → /css/style.css
└── ...
```

The same convention applies to theme `static/` directories.
