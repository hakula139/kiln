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
- The filename starts with `_`, which includes the `_index.md` listing metadata files
- The file has no TOML frontmatter (`+++` delimiters)

## Page Bundles

A **page bundle** is a directory containing an `index.md` alongside related files. Bundles are the recommended way to organize pages because they keep content and assets together. Non-bundle `.md` files get pretty URLs but cannot use co-located assets or per-page CSS.

```text
content/posts/note/my-post/
├── index.md           # Page content
├── cover.webp         # Image (co-located asset)
└── assets/
    ├── diagram.svg    # Nested assets work too
    ├── data.csv       # Data files for directives
    └── css/
        ├── _src/style.css        # Private handwritten source
        └── style.generated.css   # Compiled page stylesheet
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

A page bundle's stylesheet is `assets/css/style.generated.css`. kiln exposes its content-hashed URL as [`page_css`](themes.md#post-templates-posthtml). Themes link it from that page's `<head>` after the shared stylesheet. Other CSS files remain ordinary co-located assets and are not automatically loaded.

```text
content/posts/avg/impressions/
├── index.md
└── assets/css/
    ├── _src/style.css        # Private handwritten source
    └── style.generated.css   # Compiled page stylesheet
```

The same source / output layout applies to shared CSS under `static/css/` in the site or theme. Run the theme's CSS compiler before building with kiln. kiln publishes compiled CSS without running Tailwind or another source processor.

Page CSS follows the shared stylesheet's publication rules: `--minify` minifies it before computing its SHA-256 fingerprint. Both the original and fingerprinted files are published beside their sibling assets, preserving relative `url(...)` references. The fingerprint covers the stylesheet's bytes only, so imported CSS must be bundled before publication.

Only the owning page receives `page_css`, so its selectors and keyframes are independent of other page bundles. Scope selectors carefully within the page to avoid unintentionally styling shared navigation or theme components. The `:::` directive can provide a wrapper class when needed:

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

Files and directories whose names start with `_` are skipped when static trees or page bundle assets are copied to the output. This lets you keep build-time inputs alongside the shipped bundle without exposing them. Typical use: colocating Tailwind sources with the compiled stylesheet.

```text
static/
├── css/
│   ├── _src/           # not copied to output
│   │   ├── style.css
│   │   └── components/
│   └── style.generated.css → /css/style.generated.css
└── ...
```

The same convention applies to theme `static/` directories. Top-level static deployment files `_headers` and `_redirects` are published. All underscore-prefixed entries inside page bundles remain private.
