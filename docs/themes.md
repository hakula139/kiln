# Themes

Themes provide templates, stylesheet sources, public assets and default parameters. Site files take precedence over theme files.

## Installation

Add a theme to an initialized site repository and select its directory name in `config.toml`:

```bash
git submodule add https://github.com/hakula139/IgnIt.git themes/IgnIt
```

```toml
theme = "IgnIt"
```

A theme can declare a minimum kiln version and a stylesheet processor. Check its README for installation requirements and supported `[params]` settings. [IgnIt](https://github.com/hakula139/IgnIt) uses Tailwind, whose installation is covered in [Processor Setup](assets.md#processor-setup).

## Site Configuration

kiln reads `config.toml` from the site root. An absent file uses the defaults below.

| Setting            | Default                    | Purpose                                                     |
| ------------------ | -------------------------- | ----------------------------------------------------------- |
| `base_url`         | `"http://localhost:5456"`  | Published site URL, including a deployment path when needed |
| `title`            | `"My Site"`                | Site title                                                  |
| `description`      | `""`                       | Site description                                            |
| `language`         | `"en"`                     | Active theme translation language                           |
| `timezone`         | unset                      | IANA time zone for rendered timestamps, otherwise UTC       |
| `enable_git_info`  | `false`                    | Derive missing page updates from Git history                |
| `output_dir`       | `"public"`                 | Build destination, resolved relative to the site root       |
| `theme`            | unset                      | Theme directory under `themes/`                             |
| `[author]`         | empty fields               | `name`, `email` and `link`, available to templates          |
| `[css]`            | inherited, otherwise plain | [Stylesheet processor](assets.md#processor-setup)           |
| `[search]`         | disabled                   | [Pagefind indexing](../README.md#search)                    |
| `[image]`          | size 16, quality 25        | [Image placeholders](#image-rendering)                      |
| `[params]`         | theme defaults             | Theme settings and [renderer options](syntax.md)            |
| `[[menu.<group>]]` | no entries                 | [Navigation menus](#navigation-menus)                       |

`kiln build --base-url <url>` overrides `base_url`. `KILN_BASE_URL` supplies the override when the flag is absent. An empty override uses the configuration value. The final URL must be absolute with a host and contain no query or fragment. Templates receive a normalized `config.base_url` without a trailing slash. `kiln serve` uses its local server URL.

### Parameter Merging

Site `[params]` values override theme defaults recursively. Missing keys inherit the theme value, nested tables merge, and arrays replace the entire theme array. A type mismatch fails configuration loading.

```toml
# theme.toml
[params]
emojis = true

[params.home]
paginate = 10
```

```toml
# config.toml
[params.home]
paginate = 5
```

The site keeps `emojis = true` and uses five posts per home page. The engine reads `params.home.paginate` for home listings, `params.section.paginate` for post / section archives, and `params.paginate` as their fallback and the tag archive setting. The default is 10. Non-positive values fall through to the next setting or default.

### Navigation Menus

Menu group names are chosen by the theme. Entries are sorted by `weight` ascending within each group.

```toml
[[menu.main]]
name = "menu_posts"
url = "/posts/"
icon = "fas fa-archive"
weight = 1

[[menu.social]]
name = "GitHub"
url = "https://github.com/example"
external = true
```

| Field      | Default  | Contract                                                           |
| ---------- | -------- | ------------------------------------------------------------------ |
| `name`     | required | Translation key or literal label, resolved by the theme with `t()` |
| `url`      | required | Link destination                                                   |
| `icon`     | unset    | Theme-specific icon identifier                                     |
| `weight`   | `0`      | Sort order, with lower values first                                |
| `external` | `false`  | External-link presentation hint for the theme                      |

Themes access a group through `config.menu.<group>` and should document the group names they support.

## Override Model

### Templates

A file in the site's `templates/` overrides the same path in the theme's `templates/`. This applies to page templates, included partials and `directives/<name>.html`.

### Assets and Stylesheets

Use the shared [asset contract](assets.md) for publication precedence, stylesheet entries, processor configuration, template links and live reload. A site stylesheet can import the theme entry to extend its styles.

### Internationalization

The active language is `config.language`. Translation filenames use language tags such as `en`, `zh-Hans` or `ja`.

```text
themes/my-theme/i18n/
├── en.toml        # English fallback
└── zh-Hans.toml   # Active-language strings

i18n/
└── zh-Hans.toml   # Site overrides
```

Each file is a flat TOML table of string values. Nested tables are rejected.

```toml
all_posts = "All Posts"
page_counter = "Page {current} of {total}"
```

For each key, lookup follows site active language → theme active language → theme English fallback. A theme shipping translations must provide the exact filename `en.toml`. Site-only translations are supported when the theme has no translation directory.

Use `{{ t("all_posts") }}` to look up a key and `{{ t("page_counter", current=1, total=3) }}` to substitute named placeholders. `{{` and `}}` in a translation escape literal braces.

#### Missing-Key Behavior

A missing key renders as the key itself. This lets menu labels and similar values contain either a translation key or literal text.

## Creating a Theme

```bash
kiln init-theme my-theme
```

Select `theme = "my-theme"` in the site configuration. The scaffold supplies a base template, a post template, a plain CSS entry and example translations. Add the page types and assets your theme needs.

### Theme Structure

```text
themes/my-theme/
├── assets/
│   └── css/_src/style.css   # Shared stylesheet entry
├── i18n/
│   └── en.toml             # Translation fallback
├── static/                 # Output-root files
├── templates/
│   ├── base.html
│   ├── post.html
│   └── directives/
└── theme.toml
```

Every selected theme must have `theme.toml`, even if empty. Its supported settings are:

```toml
min_kiln_version = "0.4.0-rc.4"

[css]
processor = "plain"

[params]
emojis = true
```

`min_kiln_version` is a minimum semantic version. Other theme metadata, such as a name, license or author, can describe the theme but is not consumed by kiln.

### Template Variables

Post, standalone, home, archive and overview templates share `title`, `description`, canonical `url` and `config`.

MiniJinja auto-escapes strings. Apply `| safe` to generated HTML such as `content`, `toc` and `body_html`. Dates are ISO 8601 timestamps in the configured time zone, or UTC. Templates may use `date[:10]` when only the date is wanted.

#### Post templates (`post.html`)

| Variable               | Contract                                              |
| ---------------------- | ----------------------------------------------------- |
| `title`, `description` | Page title and description                            |
| `url`                  | Canonical page URL                                    |
| `date`, `updated`      | Publication / modification timestamps, or `none`      |
| `featured_image`       | Resolved image metadata, or `none`                    |
| `license`              | Authored page license, or `none`                      |
| `page_css`             | Fingerprinted owning-page stylesheet URL, or `none`   |
| `tags`                 | Linked terms with `name` and `url`                    |
| `section`              | Linked section, or `none`                             |
| `content`, `toc`       | Rendered page HTML and table of contents              |
| `assets`               | Detected features and registered scripts              |
| `config`               | Site configuration, including merged theme parameters |

#### Standalone page templates (`page.html`)

Standalone pages use the post variables. When `page.html` is absent, kiln uses `post.html`.

#### Home page templates (`home.html`)

Receives `title`, `description`, `url`, `config`, the current slice of `pages`, and `pagination`. Only posts appear here. Posts with a frontmatter `weight` are pinned first, ordered by ascending weight. Remaining posts are newest first. No home page is generated when this template is absent.

#### Archive page templates (`archive.html`)

| Variable           | Contract                                             |
| ------------------ | ---------------------------------------------------- |
| `kind`, `singular` | Archive scope, such as `"tags"` and `"tag"`          |
| `name`, `slug`     | Display title and URL slug                           |
| `page_groups`      | Current page's entries grouped by year, newest first |
| `pagination`       | Navigation for this archive                          |
| `config`           | Site configuration                                   |

Archives cover `/posts/`, `/posts/<section>/` and `/tags/<slug>/`. Tagged standalone pages appear in tag archives. Pinning does not change archive or feed order. No archives are generated when this template is absent.

#### Overview page templates (`overview.html`)

Receives the common metadata, `kind`, `singular` and `buckets` for `/sections/` or `/tags/`. Each bucket has `name`, `slug`, `url` and its date-sorted `pages`. Use `bucket.pages | length` for the count. No overviews are generated when this template is absent.

#### Error page templates (`404.html`)

Receives `title` and `config`. The output is `404.html`. Generation is skipped when this template is absent.

#### Shared Listing Types

Each entry in `pages`, `bucket.pages` or `page_groups[].pages` has:

| Field                         | Contract                           |
| ----------------------------- | ---------------------------------- |
| `title`, `description`, `url` | Page metadata and canonical URL    |
| `date`                        | Publication timestamp, or `none`   |
| `pinned`                      | Whether frontmatter sets a weight  |
| `featured_image`              | Resolved image metadata, or `none` |
| `tags`                        | Linked terms with `name` and `url` |
| `section`                     | Linked section, or `none`          |

A page group has `key` (the year, or an empty string for undated entries) and `pages`.

The `pagination` object contains `current_page`, `total_pages`, `base_url`, `prev_url`, `next_url` and `items`. Previous / next URLs are `none` at the respective boundaries. Each item contains `number`, `url` and `is_current`. Gaps are represented by `number = none` and `url = none`.

Controls include the first and last pages plus pages within two of the current page. For a page-jump control, page one is `{base_url}/` and later pages use `{base_url}/page/{n}/`.

#### Featured Images

`featured_image` contains authored `src`, `position` and `credit`, plus build-resolved `width`, `height` and `lqip_uri`. `credit` contains optional `title`, `author` and `url`. Relative image sources resolve against the owning page URL. Local resolvable images receive dimensions, and supported decodable images can receive a placeholder. Gate rendering on optional fields.

#### Page Assets

`assets.features` contains `"math"` and / or `"mermaid"` when required. `assets.scripts` contains registered script declarations with `url`, `load` and `module`. Only post and standalone page templates receive `assets`.

A shared partial can check presence before loading a runtime:

```jinja
{% if assets is defined and "math" in assets.features %}
  <link rel="stylesheet" href="https://cdn.example.com/katex.css">
{% endif %}
```

Themes supply KaTeX rendering, Mermaid initialization and registered script tags. Keep these runtimes conditional on the page's declarations.

#### Directive templates (`directives/<name>.html`)

| Variable                | Contract                                 |
| ----------------------- | ---------------------------------------- |
| `name`                  | Directive name                           |
| `positional_args`       | Positional argument strings              |
| `named_args`            | Named argument string values             |
| `id`, `classes`         | Authored `#id` and `.class` tokens       |
| `body_html`, `body_raw` | Rendered body HTML and original Markdown |
| `source_dir`            | Page source directory, or `none`         |
| `config`                | Site configuration                       |

Arguments remain nested in `named_args`, so `named_args.id` and the outer `id` are separate values. Use `body_html | safe` for rendered content. `body_raw` is available to data-driven components that interpret their own input.

### Template Functions

| Function                                                          | Availability        |
| ----------------------------------------------------------------- | ------------------- |
| `now()`, `parse_csv(text)`, `t(key, **kwargs)`, `asset_url(path)` | All templates       |
| `read_file(filename)`, `register_script(url, ...)`                | Directive templates |

#### `now()`

Returns the current local timestamp as an ISO 8601 string.

#### `read_file(filename)`

Reads a file relative to the directive's page source directory. Absolute paths and `..` components are rejected. Source-directory symlinks may point to external files. The returned text is auto-escaped.

#### `parse_csv(text)`

Returns a list of rows, each a list of field strings. Quoted commas and escaped quotes are supported.

```jinja
{% for row in parse_csv(read_file(positional_args[0])) %}
  <tr>{% for cell in row %}<td>{{ cell }}</td>{% endfor %}</tr>
{% endfor %}
```

#### `t(key, **kwargs)`

Looks up and interpolates a translation. See [Internationalization](#internationalization).

#### `asset_url(path)`

Resolves a published site-root-relative path to its public URL, fingerprinting CSS / JS. See [Fingerprints and Minification](assets.md#fingerprints-and-minification).

#### `register_script(url, load="defer", module=false)`

Registers a script for the current page and returns an empty string:

```jinja
{{ register_script(asset_url('/assets/js/widget.js')) }}
```

Repeated identical declarations produce one script in registration order. Conflicting attributes for the same URL fail the build. `load` accepts `"defer"`, `"async"` or `"sync"`. `module = true` supports `"defer"` or `"async"` and rejects `"sync"`, since module scripts cannot execute synchronously. The theme renders each declaration with the corresponding script attributes.

## Rendered Content

### Code Blocks

Highlighted code uses `<details class="code-block" data-lang="...">`, a `<summary class="code-header">`, and a `.code-body > .highlight` containing line numbers and code. The header contains either `.code-lang` or `.code-title` and a `.copy-btn`. Themes provide styling and copy behavior. Native disclosure controls the open state.

A positive `code_max_lines` adds `data-max-lines` and sets `--max-lines` on `.code-body`. Themes use that value to limit the highlighted area. Zero omits the limit.

Authored IDs and classes belong to the outer details element. Per-line highlighting uses `.line.hl` and `.line-number.hl`. See [Fence Attributes](syntax.md#fence-attributes) for authoring.

### Callouts and Divs

Callouts use `<details class="callout <type>">`, `.callout-title`, and `.callout-body > .callout-body-inner`. Authored IDs and classes belong to the details element. Generic directives use a div with the directive name and authored classes. See [Directives](syntax.md#directives) for syntax.

## Image Rendering

Markdown images receive lazy loading and asynchronous decoding. A paragraph containing only one image becomes a figure with a caption from its alt text. Authored IDs / classes belong to the figure for block images and the img for inline images. Width / height always belong to the img.

Locally resolvable images receive natural dimensions. Supported decoded images also receive a small WebP placeholder. When present, an `.lqip` span wraps the img and exposes the placeholder as the `--lqip-uri` CSS custom property. It sits inside the figure for block images.

```html
<span class="lqip" style="--lqip-uri:url('data:image/webp;base64,...')">
  <img src="photo.webp" alt="Photo" width="800" height="600" loading="lazy" decoding="async">
</span>
```

Themes can paint the placeholder behind the foreground image:

```css
.lqip {
  display: inline-block;
  position: relative;
  isolation: isolate;
}

.lqip::before {
  content: "";
  position: absolute;
  inset: 0;
  z-index: -1;
  background: var(--lqip-uri) center / cover;
  filter: blur(20px);
}
```

Template-rendered featured images need their own wrapper, gated on `featured_image.lqip_uri`. Remote, unresolved or undecodable images may lack a placeholder, so keep the bare img path available.

```toml
[image]
lqip_size = 16
lqip_quality = 25
```

`lqip_size` is the positive maximum placeholder dimension in pixels. `lqip_quality` is WebP quality from 1 to 100. These are also the defaults shown above.
