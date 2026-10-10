# Assets and Stylesheets

`kiln build` and `kiln serve` publish assets and compile stylesheets automatically. Keep handwritten sources in the site, theme, or page that owns them. Generated files belong only in the build output. Failed builds preserve the previous output.

## Public Files

| Source              | Output           | Use                                                                                   |
| ------------------- | ---------------- | ------------------------------------------------------------------------------------- |
| `assets/`           | `public/assets/` | Shared images, fonts, JavaScript, and other assets                                    |
| `static/`           | `public/`        | Files requiring root paths, such as favicons, manifests, `_headers`, and `_redirects` |
| `content/<bundle>/` | `public/<page>/` | Co-located page assets, retaining their relative paths                                |

Files and directories beginning with `_` are private within `assets/` and content bundles. This keeps stylesheet sources alongside public assets. `static/` publishes underscore-prefixed root files such as `_headers` and `_redirects`.

Site files override theme files. Within each owner, `static/` overlays `assets/` at the output root. Page bundle assets are published next, followed by compiled stylesheets.

Reference shared files through `asset_url()` in templates. Markdown and featured images resolve local URLs against the page output URL. Root-relative Markdown and CSS URLs must include any deployment prefix. Bundle-relative URLs such as `assets/photo.avif` remain unchanged in source. See [Page Bundles](content.md#page-bundles) for frontmatter examples.

## Stylesheet Sources

Every owner uses the same private entry path:

```text
my-site/
├── assets/
│   ├── css/_src/style.css          # Site source
│   └── images/                     # Shared public images
├── content/about/
│   ├── assets/
│   │   ├── css/_src/style.css      # Page source
│   │   └── diagram.svg             # Public page asset
│   └── index.md
├── static/                         # Root-level public files
└── themes/my-theme/
    └── assets/css/_src/style.css   # Theme source
```

The site entry takes precedence over the theme entry. To extend theme styles, import them explicitly from the site entry:

```css
@import '../../../themes/my-theme/assets/css/_src/style.css';

.site-banner {
  font-weight: bold;
}
```

Shared CSS is written to `public/assets/css/site.css`. Page CSS is written to `public/<page>/assets/css/page.css`, including for a page at the site root. Other CSS files are ordinary public assets and are not automatically loaded.

Local `@import` rules are bundled and CSS nesting is supported. Relative `url(...)` references resolve from the source file declaring them, including imported partials, and are rewritten for the published stylesheet location. References must resolve to a published shared asset or an asset belonging to the owning page. Private bundle inputs cannot be exposed through CSS. Published root-relative asset references receive the same fingerprints. External URLs and fragment-only references pass through unchanged.

### Processor Setup

The accepted processors are `"plain"` and `"tailwind"`. An unset value inherits the theme setting, otherwise it uses plain CSS. Set `processor = "plain"` to override a Tailwind theme. Plain CSS needs no external compiler. A theme selects Tailwind in `theme.toml`, and a site can override the same setting in `config.toml`:

```toml
[css]
processor = "tailwind"
```

kiln's Nix package includes everything needed for Tailwind.

For other installations, install Node.js 20+ and the [Tailwind processor](https://github.com/hakula139/kiln-tailwindcss):

```bash
npm install -g @kiln-ssg/tailwindcss
```

This provides `kiln-tailwindcss` on `PATH`. Subsequent builds invoke it automatically, and compiler failures fail the build.

kiln supplies Tailwind scanning for content and site / theme templates. Page entries automatically receive the selected shared entry through `@reference`, so `@apply` and shared theme definitions work without duplicating the shared stylesheet.

### Template Links and Page Scope

Link the shared stylesheet in the base template and page CSS in post / page templates:

```jinja
<link rel="stylesheet" href="{{ asset_url('/assets/css/site.css') | safe }}">
{% if page_css %}<link rel="stylesheet" href="{{ page_css | safe }}">{% endif %}
```

Only the owning page receives `page_css`. Its selectors still share a document with theme components. Use a wrapper class to limit rules to the intended content when needed.

## Fingerprints and Minification

`asset_url(path)` resolves a published site-root-relative path or a path relative to the current page, including inside directives, and includes the configured deployment prefix. Encode filename components in the input URL, such as `%20` for a space or `%25` for a literal percent sign. Queries and fragments are preserved, and external URLs pass through unchanged. Missing local paths fail the build. `page_css` already contains the fingerprinted page stylesheet URL.

Raster images and fonts receive content-hashed URLs under `/_assets/`, such as `/_assets/images/photo.a1b2c3d4e5f6.avif`. This directory is reserved for generated files. Markdown images, featured images, template `asset_url()` calls and compiled CSS references use these URLs automatically. Fingerprints use file bytes and work independently of image decoding and AVIF support. Original published paths remain available. Missing Markdown images retain their authored URLs.

CSS and JavaScript receive sibling content-hashed filenames, such as `/assets/css/site.a1b2c3d4e5f6.css`, preserving relative import locations. Compiled CSS includes fingerprinted image and font references before its own hash is computed, so changing an image also changes the stylesheet URL. Ordinary public CSS / JS imports retain their original URLs. Bundle those entries separately when their dependencies also need fingerprinted URLs.

Hosts may cache `/_assets/*` immutably. SVG files retain their original URLs because references can depend on the document's exact URL. Original asset URLs and CSS / JS copies need revalidation because their relative dependencies can change. Raw HTML, JavaScript strings and file contents are not rewritten.

`kiln build --minify` minifies published CSS / JS before computing fingerprints, then minifies HTML. Files named `*.min.css` or `*.min.js` pass through unchanged. Inputs that cannot be minified log a warning and keep their original bytes.

## Live Reload

`kiln serve` watches existing site `assets/`, `content/`, `i18n/`, `static/`, and `templates/` directories, plus theme configuration and corresponding theme source directories. Imported files within those trees trigger rebuilds.

Restart after creating a top-level source directory that was absent at startup, switching themes, or changing an imported file outside the watched trees.
