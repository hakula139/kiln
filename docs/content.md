# Content

Posts, standalone pages and their assets live under `content/`.

## Content Directory

```text
content/
├── about-me/
│   └── index.md             # Standalone page → /about-me/
├── comments/
│   └── index.md             # Standalone page → /comments/
└── posts/
    ├── _index.md            # Optional: sets title for /posts/ listing
    ├── note/
    │   ├── _index.md        # Optional: sets title for /posts/note/ listing
    │   └── my-post/
    │       ├── assets/
    │       │   └── diagram.svg
    │       ├── cover.webp   # Co-located asset
    │       └── index.md     # Post (sectioned) → /posts/note/my-post/
    └── standalone-post.md   # Post (orphan, no bundle) → /posts/standalone-post/
```

### Page Kinds

kiln classifies pages based on their location under `content/`:

| Location                                  | Kind             | Home / posts? | Section page? |
| ----------------------------------------- | ---------------- | ------------- | ------------- |
| `content/posts/<section>/<slug>/index.md` | Post (sectioned) | Yes           | Yes           |
| `content/posts/<section>/<slug>.md`       | Post (sectioned) | Yes           | Yes           |
| `content/posts/<slug>/index.md`           | Post (orphan)    | Yes           | No            |
| `content/posts/<slug>.md`                 | Post (orphan)    | Yes           | No            |
| `content/<slug>/index.md`                 | Standalone page  | No            | No            |
| `content/<slug>.md`                       | Standalone page  | No            | No            |

Posts appear on the home page, posts index and their section archives. Tag archives include every tagged page, including standalone pages.

### Sections

A section is a subdirectory directly under `content/posts/`. Posts inside `content/posts/note/` belong to the `note` section. Each section has an archive at `/posts/<section>/`.

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

## Frontmatter

Each content file begins with a TOML frontmatter block delimited by `+++`:

```toml
+++
title = "My Post"
description = "A brief summary."
date = 2026-01-15T12:00:00Z
updated = 2026-02-01T08:30:00Z
draft = false
tags = ["rust", "web"]
slug = "custom-slug"

[featured_image]
src = "/assets/images/hero.jpg"
position = "top"

[featured_image.credit]
title = "Work Title"
author = "Artist"
url = "https://example.com/artworks/123"
+++
```

All fields are optional. Defaults:

| Field               | Default                      |
| ------------------- | ---------------------------- |
| `title`             | `""`                         |
| `description`       | none                         |
| `slug`              | filename or bundle directory |
| `date`              | none                         |
| `updated`           | none                         |
| `featured_image`    | none (table)                 |
| `tags`              | `[]`                         |
| `license`           | none                         |
| `draft`             | `false`                      |
| `weight`            | none                         |
| `heading_numbering` | `false`                      |

When `description` is absent, text before `<!--more-->` supplies a plain-text description for page and listing templates. Without either, the description is empty.

`slug` overrides the final component of the page route and must be one nonempty directory name. It applies to HTML, bundle assets and canonical links. Conflicting content, generated pages or public files fail the build with a route collision diagnostic.

`date` and `updated` are absolute instants. Post templates receive them as ISO 8601 strings in the time zone set in the site's `config.toml`, or UTC when `timezone` is unset:

```toml
timezone = "Asia/Shanghai"
```

An explicit `updated` value takes priority. To derive it from the latest commit to the content file when frontmatter omits it, enable Git information in the site's `config.toml`:

```toml
enable_git_info = true
```

The Git fallback requires the `git` executable on `PATH` and full repository history, including in CI. When they are unavailable or the checkout is shallow, `updated` has no Git-derived value.

Each tag has an archive at `/tags/<slug>/`, using the same [slug rules as headings](syntax.md#headings). Spellings that differ only in case (`Rust` and `rust`) merge there, using the first spelling as the default display name. A tag's `content/tags/<slug>/_index.md` can override that title. When two tags differ beyond case yet slugify alike (`Rock & Roll` and `Rock Roll`), one URL cannot serve both, so the build fails with a `tag slug collision` error naming the slug, both tags, and their page counts.

On the home page, posts with a `weight` precede unweighted posts and use ascending weight. Archives and feeds remain date-sorted.

## Page Bundles

A **page bundle** is a directory containing an `index.md` alongside related files. Bundles are the recommended way to organize pages because they keep content and assets together. Non-bundle `.md` files get pretty URLs but cannot use co-located assets or per-page CSS.

```text
content/posts/note/my-post/
├── assets/
│   ├── css/_src/style.css   # Private page stylesheet source
│   ├── data.csv             # Data files for directives
│   └── diagram.svg          # Nested assets work too
├── cover.webp               # Image (co-located asset)
└── index.md                 # Page content
```

Public non-Markdown files owned by a bundle are copied beside its rendered HTML. Underscore-prefixed files and directories are private. Nested bundles own their own files, including when excluded from the build. Excluded bundles do not publish their assets. Published assets retain their relative paths:

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

## Hugo Content Migration

Configure a kiln destination with a theme, then migrate from a separate Hugo site root:

```bash
kiln convert --source /path/to/hugo --dest /path/to/kiln
```

The converter writes `content/`, copies `static/`, translates supported YAML frontmatter to TOML and converts admonition, image and Mermaid shortcodes. Other standalone shortcodes become directives requiring matching theme templates. Existing destination files are preserved. Source and destination roots must not overlap. Category indexes become section indexes under `posts/`, tag indexes retain their location, and other Hugo section indexes are skipped.

Configuration, theme templates and unsupported Hugo features require manual migration. Unsupported metadata produces omission warnings. Malformed recognized frontmatter and unsupported content-bearing shortcodes fail conversion. Review the resulting content and run `kiln build` before publishing. Files without recognized frontmatter are copied unchanged and need TOML frontmatter to become published pages.
