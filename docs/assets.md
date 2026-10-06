# Assets and Stylesheets

`kiln build` and `kiln serve` publish assets and compile stylesheets automatically. Keep handwritten sources in the site, theme, or page that owns them. Generated files belong only in the build output.

## Public Files

| Source              | Output           | Use                                                                                   |
| ------------------- | ---------------- | ------------------------------------------------------------------------------------- |
| `assets/`           | `public/assets/` | Shared images, fonts, JavaScript, and other assets                                    |
| `static/`           | `public/`        | Files requiring root paths, such as favicons, manifests, `_headers`, and `_redirects` |
| `content/<bundle>/` | `public/<page>/` | Co-located page assets, retaining their relative paths                                |

Files and directories beginning with `_` are private within `assets/` and content bundles. This keeps stylesheet sources alongside public assets. `static/` publishes every file verbatim, including underscore-prefixed root files such as `_headers` and `_redirects`.

Site files override theme files. Within each owner, `static/` overlays `assets/` at the output root. Page bundle assets are published next, followed by compiled stylesheets.

Reference shared files with root-relative URLs, such as `/assets/images/logo.svg`. Use bundle-relative URLs in Markdown for page assets, such as `assets/diagram.svg`. See [Page Bundles](content.md#page-bundles) for frontmatter examples.

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

Local `@import` rules are bundled and CSS nesting is supported. Relative `url(...)` references resolve from the source file declaring them, including imported partials, and are rewritten for the published stylesheet location. References must resolve to a published shared asset or an asset belonging to the owning page. Private bundle inputs cannot be exposed through CSS. External and root-relative URLs pass through unchanged.

### Processor Setup

Plain CSS needs no external compiler. A theme selects Tailwind in `theme.toml`, and a site can override the same setting in `config.toml`:

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

`asset_url(path)` resolves a published root-relative path. CSS and JavaScript receive a content-hashed filename, such as `/assets/css/site.a1b2c3d4e5f6.css`. Other files keep their original URLs. Missing paths, queries, fragments, and traversal components fail the build. `page_css` already contains the fingerprinted page stylesheet URL.

Original CSS / JS files remain available for relative imports and direct references. Each digest covers one file. Stylesheet source imports are bundled, while ordinary public CSS and JavaScript retain their import URLs. Bundle those entries separately when imported dependencies also need fingerprinted URLs.

`kiln build --minify` minifies published CSS / JS before computing fingerprints, then minifies generated HTML. Files named `*.min.css` or `*.min.js` pass through unchanged. Inputs that cannot be minified log a warning and keep their original bytes.

## Live Reload

`kiln serve` watches existing site `assets/`, `content/`, `i18n/`, `static/`, and `templates/` directories, plus theme configuration and corresponding theme source directories. Imported files within those trees trigger rebuilds.

Restart after creating a top-level source directory that was absent at startup, switching themes, or changing an imported file outside the watched trees.
