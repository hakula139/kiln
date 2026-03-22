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
├── theme.toml                # Theme metadata and default parameters
├── templates/                # MiniJinja templates
│   ├── base.html             # Base layout
│   ├── post.html             # Post page template
│   └── directives/           # Directive templates (optional)
│       └── site.html         # Renders ::: site directives
└── static/                   # Static assets (CSS, JS, images)
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
├── theme.toml          # Empty (all fields are optional)
├── templates/
│   ├── base.html       # Minimal base layout with block inheritance
│   └── post.html       # Post template extending base.html
└── static/             # Empty directory for CSS, JS, images
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

#### Post templates (`post.html`)

| Variable          | Type             | Description                     |
| ----------------- | ---------------- | ------------------------------- |
| `title`           | string           | Post title from frontmatter     |
| `description`     | string           | Post description                |
| `url`             | string           | Canonical URL of the post       |
| `featured_image`  | string or `none` | Featured image path             |
| `date`            | string or `none` | Publication date (ISO 8601)     |
| `content`         | string           | Rendered HTML content           |
| `toc`             | string           | Rendered table of contents HTML |
| `config`          | object           | Site configuration              |
| `config.base_url` | string           | Site base URL                   |
| `config.title`    | string           | Site title                      |

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

#### Template functions

##### `read_file(filename)`

Reads a file relative to the page's `source_dir`. Only available in directive templates (where `source_dir` is set). Useful for directives that reference co-located data files (e.g., CSV for score tables):

```html
{% set csv = read_file(positional_args[0]) %}
```

The return value is auto-escaped by MiniJinja. Use `| safe` if the content should be rendered as raw HTML. Path traversal (`..`) and absolute paths are rejected.
