# Themes

kiln uses a theme system that separates site content from presentation. Themes provide templates, static assets, and default parameters. Site-level files always take precedence over theme files when both exist at the same path.

## Installation

Add a theme to your site as a Git submodule:

```bash
cd my-site
git submodule add https://github.com/hakula139/IgnIt.git themes/IgnIt
```

Then set the theme in your site's `config.toml`:

```toml
theme = "IgnIt"
```

The `theme` value corresponds to the directory name under `themes/`.

## Theme Structure

A theme lives in `themes/<name>/` and follows this layout:

```text
themes/IgnIt/
├── static/                   # Static assets (CSS, JS, images)
├── templates/                # MiniJinja templates
│   ├── base.html             # Base layout
│   ├── directives/           # Directive templates (optional)
│   │   └── site.html         # Renders ::: site directives
│   ├── archive.html          # Year-grouped archive page (e.g., /posts/, /tags/rust/)
│   ├── home.html             # Home page with paginated post listing
│   ├── overview.html         # Bucket overview page (e.g., /tags/, /sections/)
│   ├── page.html             # Standalone page (about, etc.)
│   └── post.html             # Post page template
└── theme.toml                # Theme metadata and default parameters
```

### `theme.toml`

Every theme must have a `theme.toml` at its root. It contains metadata and default parameters:

```toml
name = "IgnIt"
min_kiln_version = "0.1.0"

[params]
code_max_lines = 40
emojis = true
fontawesome = true
```

All fields are optional. kiln uses the following:

| Field              | Description                                         |
| ------------------ | --------------------------------------------------- |
| `min_kiln_version` | Minimum kiln version (semver); build fails if unmet |
| `[params]`         | Default parameters that sites can override          |

The theme name is inferred from the directory name (e.g., `themes/IgnIt/` → `"IgnIt"`).

Additional fields (`name`, `description`, `license`, `[author]`, etc.) are ignored by kiln but recommended for discoverability:

```toml
description = "A clean, feature-rich theme for kiln"
license = "MIT"

[author]
name = "Hakula"
link = "https://hakula.xyz"
```

See [Parameter Merging](#parameter-merging) for how `[params]` interacts with site config.

## Override Model

kiln merges site and theme files using a consistent rule: **site files take precedence**.

### Templates

When rendering, kiln looks for each template in this order:

1. **Site** `templates/` directory (highest priority)
2. **Theme** `templates/` directory (fallback)

To override a theme template, place a file with the same name in your site's `templates/` directory:

```text
my-site/
├── templates/
│   └── post.html             # ← overrides theme's post.html
└── themes/IgnIt/
    └── templates/
        ├── base.html         # used (no site override)
        └── post.html         # overridden by site's version
```

This works for all templates, including directive templates under `templates/directives/`.

### Static Files

Static files follow the same precedence:

1. **Site** `static/` directory (highest priority)
2. **Theme** `static/` directory (fallback)

When both the site and theme provide a file at the same path, the site's version is used:

```text
my-site/
├── static/
│   └── shared.css            # ← wins over theme's shared.css
└── themes/IgnIt/
    └── static/
        ├── theme.css         # copied (no site override)
        └── shared.css        # overridden by site's version
```

### Parameter Merging

The `[params]` table is merged recursively. Site values take precedence over theme defaults:

```toml
# theme.toml
[params]
code_max_lines = 40
emojis = true
fontawesome = true

[params.social]
github = ""
twitter = ""
```

```toml
# config.toml (site)
theme = "IgnIt"

[params]
fontawesome = false           # overrides theme default

[params.social]
github = "hakula139"          # overrides theme default
                              # twitter inherits "" from theme
```

Merge rules:

- **Scalars**: site value wins.
- **Arrays**: site value wins (arrays are replaced entirely, not concatenated).
- **Tables**: merged recursively — site keys override matching theme keys, theme-only keys are preserved.
- **Missing keys**: theme defaults fill in any keys not present in the site config.

## Creating a Theme

The quickest way to create a new theme is with the built-in scaffolding command:

```bash
kiln init-theme my-theme
```

This creates the following structure under `themes/my-theme/`:

```text
themes/my-theme/
├── static/             # Empty directory for CSS, JS, images
├── templates/
│   ├── base.html       # Minimal base layout with block inheritance
│   └── post.html       # Post template extending base.html
└── theme.toml          # Empty (all fields are optional)
```

Set `theme = "my-theme"` in your site's `config.toml` to use it. From there, customize the templates and add static assets as needed.

### Manual Setup

To create a theme manually instead:

1. Create the theme directory structure:

   ```bash
   mkdir -p themes/my-theme/templates themes/my-theme/static
   ```

2. Add an empty `theme.toml` (all fields are optional):

   ```bash
   touch themes/my-theme/theme.toml
   ```

3. Create a `templates/base.html` with the base layout. Use [MiniJinja](https://github.com/mitsuhiko/minijinja) template syntax with block inheritance:

   ```html
   <!DOCTYPE html>
   <html lang="{{ config.language }}">
     <head>
       <meta charset="utf-8">
       {% block title %}<title>{{ config.title }}</title>{% endblock %}
       {% block head %}{% endblock %}
     </head>
     <body>
       {% block body %}{% endblock %}
     </body>
   </html>
   ```

4. Create a `templates/post.html` that extends the base:

   ```html
   {% extends "base.html" %}

   {% block title %}<title>{{ title }} - {{ config.title }}</title>{% endblock %}

   {% block body %}
   <article>
     <h1>{{ title }}</h1>
     <div class="content">{{ content | safe }}</div>
   </article>
   {% endblock %}
   ```

5. Set `theme = "my-theme"` in your site's `config.toml`.

### Template Variables

Templates receive the following variables during rendering:

Whenever a template variable includes a page `date`, kiln renders it as an ISO 8601 string in the site's configured `timezone` from `config.toml`. When `timezone` is unset, kiln uses UTC.

#### Post templates (`post.html`)

| Variable          | Type             | Description                             |
| ----------------- | ---------------- | --------------------------------------- |
| `title`           | string           | Post title from frontmatter             |
| `description`     | string           | Post description                        |
| `url`             | string           | Canonical URL of the post               |
| `featured_image`  | object or `none` | Featured image (see below)              |
| `page_css`        | string or `none` | URL to co-located `style.css` (if any)  |
| `date`            | string or `none` | Publication date (ISO 8601)             |
| `section`         | object or `none` | Section the post belongs to (see below) |
| `content`         | string           | Rendered HTML content                   |
| `toc`             | string           | Rendered table of contents HTML         |
| `config`          | object           | Site configuration                      |
| `config.base_url` | string           | Site base URL                           |
| `config.title`    | string           | Site title                              |

#### Standalone page templates (`page.html`)

Uses the same variables as `post.html` (see above). The `page.html` template is used for standalone pages (e.g., "About Me") that live outside the `posts/` directory. If `page.html` is not present, standalone pages fall back to `post.html`.

#### Home page templates (`home.html`)

| Variable      | Type          | Description                                                            |
| ------------- | ------------- | ---------------------------------------------------------------------- |
| `title`       | string        | Site title (from `config.title`)                                       |
| `description` | string        | Site description (from `config.description`)                           |
| `url`         | string        | Canonical home page URL                                                |
| `pages`       | list of pages | Posts for the current page (see page fields in overview section below) |
| `pagination`  | object        | Pagination metadata (same structure as archive pages below)            |
| `config`      | object        | Site configuration                                                     |

Only posts (`PageKind::Post`) appear on the home page; standalone pages are excluded. The number of posts per page is configurable via `params.home.paginate` or `params.paginate` (default: 10). If `home.html` is not present, no home page is generated.

#### Archive page templates (`archive.html`)

| Variable      | Type           | Description                                                    |
| ------------- | -------------- | -------------------------------------------------------------- |
| `kind`        | string         | Archive scope plural (e.g., `"posts"`, `"sections"`, `"tags"`) |
| `singular`    | string         | Archive scope singular (e.g., `"post"`, `"section"`, `"tag"`)  |
| `name`        | string         | Display name (e.g., `"Posts"`, `"Note"`, `"Rust"`)             |
| `slug`        | string         | URL-safe slug (e.g., `"posts"`, `"note"`, `"rust"`)            |
| `page_groups` | list of groups | Posts grouped by year, newest first                            |
| `pagination`  | object         | Pagination metadata (see below)                                |
| `config`      | object         | Site configuration                                             |

Archive pages are generated for:

- **Posts index** (`/posts/`): `kind="posts"`, `singular="post"`. Title from `content/posts/_index.md` or `"All Posts"`.
- **Section archives** (`/posts/<slug>/`): `kind="sections"`, `singular="section"`. Title from `content/posts/<section>/_index.md` or titlecased slug.
- **Tag archives** (`/tags/<slug>/`): `kind="tags"`, `singular="tag"`. Title from frontmatter or `content/tags/<slug>/_index.md`.

Posts per page: `params.section.paginate` or `params.paginate` (default: 10) for posts / sections; `params.paginate` (default: 10) for tags. If `archive.html` is not present, no archive pages are generated.

#### Overview page templates (`overview.html`)

| Variable   | Type            | Description                                                                    |
| ---------- | --------------- | ------------------------------------------------------------------------------ |
| `kind`     | string          | Overview scope plural (e.g., `"sections"`, `"tags"`)                           |
| `singular` | string          | Overview scope singular (e.g., `"section"`, `"tag"`)                           |
| `buckets`  | list of buckets | All buckets in this scope, sorted by page count descending then name ascending |
| `config`   | object          | Site configuration                                                             |

Each bucket in `buckets` has:

| Field   | Type          | Description                                         |
| ------- | ------------- | --------------------------------------------------- |
| `name`  | string        | Display name (e.g., `"Rust"`)                       |
| `slug`  | string        | URL-safe slug (e.g., `"rust"`)                      |
| `url`   | string        | URL to the archive page (e.g., `"/tags/rust/"`)     |
| `pages` | list of pages | All pages in this bucket, sorted by date descending |

Use `bucket.pages | length` to get the page count.

Each page in `pages` has:

| Field            | Type             | Description                          |
| ---------------- | ---------------- | ------------------------------------ |
| `title`          | string           | Post title                           |
| `url`            | string           | Canonical URL                        |
| `date`           | string or `none` | Publication date                     |
| `description`    | string           | Post description                     |
| `featured_image` | object or `none` | Featured image (see below)           |
| `tags`           | list of objects  | Tags with `name` and `url` fields    |
| `section`        | object or `none` | Section with `name` and `url` fields |

`featured_image` (when present) has:

| Field      | Type             | Description                                 |
| ---------- | ---------------- | ------------------------------------------- |
| `src`      | string           | Resolved image path / URL                   |
| `position` | string or `none` | CSS `object-position` value (e.g., `"top"`) |
| `credit`   | object or `none` | Attribution metadata (see below)            |

`credit` (when present) has:

| Field    | Type             | Description                                          |
| -------- | ---------------- | ---------------------------------------------------- |
| `title`  | string or `none` | Title of the original work                           |
| `author` | string or `none` | Author / artist name                                 |
| `url`    | string or `none` | Link to the original work (e.g., Pixiv artwork page) |

`section` and each tag entry have:

| Field  | Type   | Description                                             |
| ------ | ------ | ------------------------------------------------------- |
| `name` | string | Display name (e.g., `"Rust"`, `"笔记"`)                 |
| `url`  | string | Canonical URL (e.g., `"/tags/rust/"`, `"/posts/note/"`) |

Each group in `page_groups` has:

| Field   | Type          | Description                             |
| ------- | ------------- | --------------------------------------- |
| `key`   | string        | Group key (year, e.g., `"2026"`)        |
| `pages` | list of pages | Pages in this group (same fields above) |

The `pagination` object has:

| Field          | Type             | Description                                  |
| -------------- | ---------------- | -------------------------------------------- |
| `current_page` | number           | Current page number (1-indexed)              |
| `total_pages`  | number           | Total number of pages                        |
| `base_url`     | string           | Base URL for page links (e.g., `/tags/rust`) |
| `prev_url`     | string or `none` | URL to the previous page (if exists)         |
| `next_url`     | string or `none` | URL to the next page (if exists)             |
| `items`        | list of items    | Numbered page entries for display            |

Each item in `items` has:

| Field        | Type             | Description                                 |
| ------------ | ---------------- | ------------------------------------------- |
| `number`     | number or `none` | Page number, or `none` for ellipsis markers |
| `url`        | string or `none` | Page URL, or `none` for ellipsis markers    |
| `is_current` | boolean          | Whether this is the active page             |

Items include the first page, last page, and pages within ±2 of the current page. Gaps between shown ranges are represented by a single ellipsis marker (`number: none`).

To build a "jump to page" control, use `base_url`: page 1 is `{base_url}/`, page N is `{base_url}/page/{n}/`.

The number of items per page is configurable via `paginate` in `[params]` (default: 10).

#### Directive templates (`directives/<name>.html`)

| Variable          | Type                | Description                               |
| ----------------- | ------------------- | ----------------------------------------- |
| `name`            | string              | Directive name                            |
| `positional_args` | list of strings     | Parsed positional arguments               |
| `named_args`      | map (string→string) | Parsed named arguments (`key=value`)      |
| `id`              | string or `none`    | Pandoc `#id` attribute                    |
| `classes`         | list of strings     | Pandoc `.class` attributes                |
| `body_html`       | string              | Rendered HTML body of the directive block |
| `body_raw`        | string              | Raw markdown source of the directive body |
| `source_dir`      | string or `none`    | Page source directory (for `read_file`)   |

### Template Functions

The following functions are available in all templates.

#### `now()`

Returns the current local timestamp as an ISO 8601 string (e.g., `"2026-03-29T23:00:00+08:00[Asia/Shanghai]"`):

```html
<footer>&copy; {{ now()[0:4] }} My Site</footer>
```

#### `read_file(filename)`

Reads a file relative to the page's `source_dir`. Only available in directive templates (where `source_dir` is set). Useful for directives that reference co-located data files (e.g., CSV for score tables):

```html
{% set csv = read_file(positional_args[0]) %}
```

The return value is auto-escaped by MiniJinja. Use `| safe` if the content should be rendered as raw HTML. Path traversal (`..`) and absolute paths are rejected.

#### `parse_csv(text)`

Parses CSV text (RFC 4180) into a list of rows, where each row is a list of field strings. Handles quoted fields with embedded commas and escaped quotes. Useful with `read_file` for data-driven directive templates:

```html
{% set rows = parse_csv(read_file("scores.csv")) %}
{% set headers = rows[0] %}
{% for row in rows[1:] %}
  <tr>{% for cell in row %}<td>{{ cell }}</td>{% endfor %}</tr>
{% endfor %}
```

#### `t(key, **kwargs)`

Resolves a translatable string for the active language. See [Internationalization](#internationalization) for the full model.

```html
<a href="#top">{{ t("back_to_top") }}</a>
<p>{{ t("page_counter", current=page, total=pages) }}</p>
```

When `kwargs` are supplied, Python-style `{name}` placeholders in the string are replaced with the corresponding values. Missing keys emit a warning and render as the key literal (or `«missing:<key>»` under `KILN_DEV`) so the build does not crash.

## Internationalization

kiln supports translatable strings via a layered i18n system. Themes ship defaults per language and sites can override any string.

### File Layout

```text
themes/my-theme/i18n/
├── en.toml              # Ultimate fallback (required if any other file exists)
└── zh-Hans.toml         # Additional language (BCP 47 tag)

my-site/i18n/
└── zh-Hans.toml         # Site-level overrides for the active language
```

The active language comes from `language` in `config.toml` (default `"en"`). Language tags follow [BCP 47](https://www.rfc-editor.org/info/bcp47) (e.g., `en`, `zh-Hans`, `ja`).

### Resolution Order

For each key, kiln merges three tables in descending precedence:

1. `<site>/i18n/<language>.toml` — site-level override
2. `<theme>/i18n/<language>.toml` — theme strings for the active language
3. `<theme>/i18n/en.toml` — theme English fallback

If the theme has no `i18n/` directory at all, site-only i18n is also supported. If a theme ships any `i18n/*.toml` file other than `en.toml`, the loader requires `en.toml` as the ultimate fallback.

### File Format

i18n files are flat TOML tables of string values:

```toml
all_posts = "All Posts"
back_to_top = "Back to Top"
page_counter = "Page {current} of {total}"
```

Nested tables are rejected.

### Template Usage

- `{{ t("key") }}` — look up a string for the active language.
- `{{ t("key", name=value) }}` — interpolate keyword arguments into Python-style `{name}` placeholders. `{{` / `}}` escape to literal braces.

Dates render as plain ISO `YYYY-MM-DD` regardless of the active language. When a template receives a full timestamp, slice it with `{{ page.date[:10] }}`.

### Missing-Key Behavior

A missing key emits a warning the first time it is requested and renders as the key literal, so a broken translation is visible in the output without crashing the build. Set the `KILN_DEV` environment variable (to any non-empty value) while developing to render misses as `«missing:<key>»` instead — useful for spotting untranslated strings in preview builds.
