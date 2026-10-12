# Assets and Stylesheets

`kiln build` and `kiln serve` publish assets and compile stylesheets automatically. Keep handwritten sources in the site, theme or page that owns them. Generated files belong in the build output.

## Public Files

| Source              | Output           | Use                                                                             |
| ------------------- | ---------------- | ------------------------------------------------------------------------------- |
| `assets/`           | `public/assets/` | Shared images, fonts, JavaScript and other assets                               |
| `static/`           | `public/`        | Files with fixed URLs, such as favicons, manifests, `_headers` and `_redirects` |
| `content/<bundle>/` | `public/<page>/` | Co-located page assets, retaining their relative paths                          |

Images and fonts from `assets/` and page bundles are published only under [content-hashed names](#fingerprints-and-minification). `static/` files always keep their names.

Files and directories beginning with `_` are private within `assets/` and content bundles. `static/` also publishes underscore-prefixed files.

Site files override theme files. Within each owner, `static/` overlays `assets/` at the output root. Page bundle assets overlay these shared files, and compiled stylesheets take precedence over copied files at their output paths.

## Referencing Assets

Use `asset_url(path)` in templates, including directives, to resolve published files. Markdown images, featured images and compiled CSS references also use the asset manifest. Keep authored filenames unchanged when their contents change.

| Reference               | Relative URL                                   | Root-relative URL                                 |
| ----------------------- | ---------------------------------------------- | ------------------------------------------------- |
| Template `asset_url()`  | Relative to the page output URL                | Site path, with the deployment prefix added       |
| Featured image          | Relative to the owning page output URL         | Site path, with the deployment prefix added       |
| Markdown image          | Relative to the page output URL                | Path starting with the configured `base_url` path |
| Compiled CSS `url(...)` | Relative to the stylesheet source declaring it | Path starting with the configured `base_url` path |
| Compiled CSS `@import`  | Bundled into the stylesheet                    | Path starting with the configured `base_url` path |

For a site whose `base_url` ends in `/blog/`, `asset_url('/images/photo.png')` and Markdown `![Photo](/blog/images/photo.png)` select the same published file. Authored root-relative paths follow the configured `base_url` even when `kiln serve` or `kiln build --base-url` publishes under another path, and kiln rewrites them to that path. Bundle-relative references such as `assets/photo.png` resolve from the page's output location, including when its slug differs from its source directory. Image dimensions and placeholders use that same published file.

Encode filename components, using `%20` for a space and `%25` for a literal percent sign. Queries and fragments are preserved, and external URLs pass through unchanged.

Every site reference that kiln resolves must select a published file, so a missing one fails the build. External URLs, `data:` URLs, fragments and root-relative paths outside the configured `base_url` path pass through unchanged. Relative template references require a page context.

Prepared `featured_image.src` is an absolute URL, ready for image elements and metadata. Protocol-relative featured images inherit the page's scheme. See [Page Bundles](content.md#page-bundles) for frontmatter examples.

## Stylesheet Sources

Each site, theme or page bundle uses `assets/css/_src/style.css` as its private entry:

| Owner         | Compiled output                     |
| ------------- | ----------------------------------- |
| Site or theme | `public/assets/css/site.css`        |
| Page bundle   | `public/<page>/assets/css/page.css` |

Page CSS also works for a page at the site root. Other CSS files are ordinary public assets and are not automatically loaded.

The site entry takes precedence over the theme entry. To extend theme styles, import them explicitly from the site entry:

```css
@import '../../../themes/my-theme/assets/css/_src/style.css';
```

Relative `@import` rules are bundled, while root-relative ones remain `@import` rules that load the published file. CSS nesting is supported. Relative `url(...)` paths are resolved from the declaring source, including imported partials, and rewritten for the published stylesheet.

These references must select a published shared asset or an asset belonging to the owning page. Private bundle inputs cannot be exposed through CSS. External URLs and fragment-only references pass through unchanged.

### Processor Setup

The accepted processors are `"plain"` and `"tailwind"`. An unset value inherits the theme setting, otherwise it uses plain CSS. Set `processor = "plain"` to override a Tailwind theme. A theme selects its processor in `theme.toml`, and a site can override it in `config.toml`:

```toml
[css]
processor = "tailwind"
```

Plain CSS needs no external compiler. Tailwind requires the [optional processor](../README.md#installation), included in kiln's Nix package.

kiln supplies content and site / theme template scanning. Tailwind page entries receive the selected shared entry through `@reference`, so `@apply` and shared theme definitions work without duplicating the shared stylesheet.

### Template Links and Page Scope

Link the shared stylesheet in the base template and page CSS in the post template:

```jinja
<link rel="stylesheet" href="{{ asset_url('/assets/css/site.css') }}">
{% if page_css %}<link rel="stylesheet" href="{{ page_css }}">{% endif %}
```

`page_css` contains the fingerprinted stylesheet URL, including the deployment prefix. Only the owning page receives it. Its selectors still share a document with theme components, so use a wrapper class to limit their scope when needed.

## Fingerprints and Minification

kiln publishes a file under a content-addressed name, `<stem>.<hash>.<ext>` beside its source path, when that name can be derived from the file's final bytes, including every URL the file references. The hashed file is the only copy published, and every reference kiln resolves points at it.

| Files                                                                                                                        | Published name                                                          |
| ---------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Raster images (AVIF, BMP, GIF, ICO, JPEG, PNG and WebP) and fonts (OTF, TTF, WOFF and WOFF2) from `assets/` and page bundles | `<stem>.<hash>.<ext>`, such as `/assets/images/cover.1cabe42ce865.webp` |
| Compiled stylesheets                                                                                                         | `assets/css/site.<hash>.css` and `<page>/assets/css/page.<hash>.css`    |
| Scripts (`.js` and `.mjs`) from `assets/` and page bundles that neither import nor are imported                              | `<stem>.<hash>.<ext>`, such as `/assets/js/search.3f2a9c41d0be.js`      |
| All `static/` files, importing or imported scripts, SVG, other CSS and remaining files                                       | Source name                                                             |

Images and fonts reference no other files, so their fingerprints cover their bytes alone and do not depend on image decoding or AVIF support. Compiled stylesheets are fingerprinted after their `url(...)` references are rewritten and the output is minified, so a changed image or font from `assets/` or a page bundle also changes the stylesheet URL. A `static/` file keeps its source name, so changing it leaves the stylesheet URL unchanged.

Scripts are fingerprinted after minification. Module specifiers resolve by URL, and kiln does not rewrite them, so a script keeps its source name when it has a static `import`, `export ... from` or `import()`, or when another published script imports it. A computed `import()` expression could load any script, so one anywhere in the output keeps every script under its source name. Scripts that kiln cannot parse also keep their source names.

kiln does not rewrite references inside JavaScript, SVG, plain public CSS, raw HTML or other file contents. A fixed URL in such a file cannot reach a content-hashed file, including string URLs such as `new Worker("worker.js")`, `importScripts()` and `fetch()`. Place files that need a stable URL, such as hotlinked images, images listed in JSON or worker scripts, in `static/`, and import CSS that references shared images or fonts from a stylesheet entry.

Templates receive the same URLs. `asset_url()` returns the hashed URL of a content-addressed file and the plain URL of a source-named file.

### Cache Rules

Content-addressed files can be cached immutably, while every other file needs revalidation. Their names have at least two dots in the final path segment, which hosts without regular expressions can still match. kiln rejects source-named files that would match the patterns below, namely an image, font or script whose stem contains a dot, a `site.*.css` file in the root `assets/css/`, and a `page.*.css` file in any `assets/css/` directory:

| Files                     | Patterns                                                                                   |
| ------------------------- | ------------------------------------------------------------------------------------------ |
| Images, fonts and scripts | `/*.:hash.<ext>` for each extension listed above                                           |
| Compiled stylesheets      | `/assets/css/site.:hash.css`, `/assets/css/page.:hash.css`, `/*/assets/css/page.:hash.css` |

A vendor file such as `static/vendor.min.js` therefore needs a stem without dots, such as `vendor-min.js`, or a place under `assets/` where it can take a content-addressed name. Pagefind's generated scripts have no dots in their stems, and its stylesheets live outside `assets/css/`, so these patterns do not match them.

A Cloudflare `_headers` placeholder matches characters within one path segment, and `!` removes a header inherited from the catch-all rule:

```text
/*
  Cache-Control: public, max-age=300, stale-while-revalidate=3600

/*.:hash.webp
  ! Cache-Control
  Cache-Control: public, max-age=31536000, immutable
```

Repeat the immutable block for each pattern.

`kiln build --minify` minifies CSS / JS before scripts and compiled stylesheets are fingerprinted and minifies HTML after rendering. Files named `*.min.css` or `*.min.js` pass through unchanged. Inputs that cannot be minified log a warning and keep their original bytes.

## Live Reload

`kiln serve` watches the site `config.toml` and existing `assets/`, `content/`, `i18n/`, `static/` and `templates/` directories, plus the active theme's `theme.toml` and corresponding source directories. Imported files within those trees trigger rebuilds. The startup log lists the watched paths.

Restart after changing `output_dir`, creating a top-level input that was absent at startup, switching themes or changing an imported file outside the watched trees.
