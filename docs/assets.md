# Assets and Stylesheets

`kiln build` and `kiln serve` publish assets and compile stylesheets automatically. Keep handwritten sources in the site, theme, or page that owns them. Generated files belong in the build output.

## Public Files

| Source              | Output           | Use                                                                  |
| ------------------- | ---------------- | -------------------------------------------------------------------- |
| `assets/`           | `public/assets/` | Shared images, fonts, JavaScript, and other assets                   |
| `static/`           | `public/`        | Root files such as favicons, manifests, `_headers`, and `_redirects` |
| `content/<bundle>/` | `public/<page>/` | Co-located page assets, retaining their relative paths               |

Files and directories beginning with `_` are private within `assets/` and content bundles. `static/` also publishes underscore-prefixed files.

Site files override theme files. Within each owner, `static/` overlays `assets/` at the output root. Page bundle assets overlay these shared files, and compiled stylesheets take precedence over copied files at their output paths.

## Referencing Assets

Use `asset_url(path)` in templates, including directives, to resolve published files. Markdown images, featured images and compiled CSS references also use the asset manifest. Keep authored filenames unchanged when their contents change.

| Reference               | Relative URL                                   | Root-relative URL                             |
| ----------------------- | ---------------------------------------------- | --------------------------------------------- |
| Template `asset_url()`  | Relative to the page output URL                | Site path, with the deployment prefix added   |
| Featured image          | Relative to the owning page output URL         | Site path, with the deployment prefix added   |
| Markdown image          | Relative to the page output URL                | Browser root, including any deployment prefix |
| Compiled CSS `url(...)` | Relative to the stylesheet source declaring it | Browser root, including any deployment prefix |

For a site deployed under `/blog`, `asset_url('/images/photo.png')` and Markdown `![Photo](/blog/images/photo.png)` select the same published file. Bundle-relative references such as `assets/photo.png` resolve from the page's output location, including when its slug differs from its source directory. Image dimensions and placeholders use that same published file.

Encode filename components, using `%20` for a space and `%25` for a literal percent sign. Queries and fragments are preserved, and external URLs pass through unchanged.

A missing local `asset_url()` reference fails the build. Missing Markdown images retain their authored URLs. Relative template references require a page context.

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

Local `@import` rules are bundled, and CSS nesting is supported. Relative `url(...)` paths are resolved from the declaring source, including imported partials, and rewritten for the published stylesheet.

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

Link the shared stylesheet in the base template and page CSS in post / page templates:

```jinja
<link rel="stylesheet" href="{{ asset_url('/assets/css/site.css') | safe }}">
{% if page_css %}<link rel="stylesheet" href="{{ page_css | safe }}">{% endif %}
```

`page_css` contains the fingerprinted stylesheet URL, including the deployment prefix. Only the owning page receives it. Its selectors still share a document with theme components, so use a wrapper class to limit their scope when needed.

## Fingerprints and Minification

Raster images (AVIF, BMP, GIF, ICO, JPEG, PNG and WebP) and fonts (OTF, TTF, WOFF and WOFF2) receive content-hashed URLs under the reserved `/_assets/` directory. Fingerprints use file bytes independently of image decoding and AVIF support. Original published paths remain available.

CSS and JavaScript receive sibling content-hashed filenames, preserving relative import locations. Changes to images or fonts referenced by compiled CSS also change its URL. Ordinary public CSS / JS imports retain their original URLs. Bundle those entries separately when their dependencies also need fingerprinted URLs.

Other asset types retain their original URLs, including SVG files whose references can depend on the document's exact URL. Raw HTML, JavaScript strings and file contents are not rewritten.

Set host cache policies according to the URL:

- `/_assets/*` can be cached immutably.
- Original paths and CSS / JS copies need revalidation because their contents or relative dependencies can change.

`kiln build --minify` fingerprints minified CSS / JS and also minifies HTML. Files named `*.min.css` or `*.min.js` pass through unchanged. Inputs that cannot be minified log a warning and keep their original bytes.

## Live Reload

`kiln serve` watches existing site `assets/`, `content/`, `i18n/`, `static/`, and `templates/` directories, plus theme configuration and corresponding theme source directories. Imported files within those trees trigger rebuilds.

Restart after changing `output_dir`, creating a top-level source directory that was absent at startup, switching themes, or changing an imported file outside the watched trees.
