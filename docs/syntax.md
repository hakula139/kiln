# Syntax Reference

kiln processes Markdown content with several extensions beyond standard CommonMark. This document describes the supported syntax.

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
categories = ["programming"]
slug = "custom-slug"
featured_image = "/images/hero.jpg"
+++
```

All fields are optional. Defaults:

| Field            | Default               |
| ---------------- | --------------------- |
| `title`          | `""`                  |
| `description`    | none                  |
| `date`           | none                  |
| `updated`        | none                  |
| `draft`          | `false`               |
| `tags`           | `[]`                  |
| `categories`     | `[]`                  |
| `slug`           | derived from filename |
| `featured_image` | none                  |

Draft pages (`draft = true`) and pages whose filename starts with `_` are excluded from the build.

## Markdown

kiln uses [pulldown-cmark](https://github.com/raphlinus/pulldown-cmark) for Markdown rendering. Standard CommonMark syntax is fully supported, along with the following extensions.

### GFM Extensions

[GitHub Flavored Markdown](https://github.github.com/gfm/) extensions are enabled:

#### Tables

```markdown
| Left | Center | Right |
| :--- | :----: | ----: |
| a    |   b    |     c |
```

#### Strikethrough

```markdown
~~deleted text~~
```

#### Task lists

```markdown
- [x] Completed
- [ ] Pending
```

#### Autolinks

URLs and email addresses are automatically linked.

### Footnotes

```markdown
Here is a claim[^1] that needs a source.

[^1]: The source for the claim.
```

### Math (KaTeX)

Inline math uses single dollar signs, display math uses double:

```markdown
Inline: $E = mc^2$

Display:

$$
\int_0^\infty e^{-x^2} dx = \frac{\sqrt{\pi}}{2}
$$
```

Math expressions are rendered as KaTeX-compatible markup (`<span class="math math-inline">` / `<span class="math math-display">`). The page template must include the [KaTeX](https://katex.org) CSS and JS for client-side rendering.

### Headings

Headings automatically receive `id` attributes generated from their text, suitable for linking:

```markdown
## Getting Started
<!-- renders as: <h2 id="getting-started">Getting Started</h2> -->
```

The slugification algorithm is CJK-aware: Chinese / Japanese / Korean characters are preserved in IDs rather than being stripped. Duplicate IDs are disambiguated with numeric suffixes (`-1`, `-2`, ...).

Explicit heading IDs override the auto-generated one:

```markdown
## My Section {#custom-id}
<!-- renders as: <h2 id="custom-id">My Section</h2> -->
```

### Images

Standard Markdown image syntax is supported. kiln distinguishes between **block** and **inline** images:

#### Block image

A paragraph containing only a single image:

```markdown
![Alt text as caption](/path/to/image.jpg "Optional title")
```

Renders as a `<figure>` with `<figcaption>` (from the alt text). Images receive `loading="lazy"` automatically.

#### Inline image

An image appearing alongside other text in a paragraph:

```markdown
Here is an icon ![icon](/icon.png) in the middle of text.
```

Renders as a plain `<img>` element.

### Syntax Highlighting

Fenced code blocks with a language tag receive syntax highlighting via [syntect](https://github.com/trishume/syntect):

````markdown
```rust
fn main() {
    println!("Hello, world!");
}
```
````

Features:

- CSS-class-based highlighting (no inline styles; requires a syntect theme stylesheet).
- Line numbers are included automatically.
- Language labels are canonicalized from syntect's syntax definitions (e.g., `rs` maps to `rust`).
- Unrecognized languages fall back to plain text.

### Table of Contents

Headings are collected during rendering and made available as structured `TocEntry` data for template-driven `<nav>` generation. The table of contents is generated from all headings in the document, preserving their hierarchy.

## Directives

Directives use `:::` fenced blocks (similar to [Pandoc fenced divs](https://pandoc.org/MANUAL.html#divs-and-spans)). They provide structured content blocks beyond standard Markdown.

### Basic Syntax

A directive block starts with three or more colons followed by an optional directive name, and ends with a matching (or longer) colon fence:

```markdown
::: callout
This is a note.
:::
```

Pandoc-style attributes (`#id`, `.class`, `key=value`) are specified inside curly braces after the directive name:

```markdown
::: callout {#my-id .custom-class type=tip title="Read This"}
Content here.
:::
```

### Callouts

Callouts are styled content blocks. The `callout` directive supports 12 types:

| Type       | Default Title |
| ---------- | ------------- |
| `abstract` | Abstract      |
| `bug`      | Bug           |
| `danger`   | Danger        |
| `example`  | Example       |
| `failure`  | Failure       |
| `info`     | Info          |
| `note`     | Note          |
| `question` | Question      |
| `quote`    | Quote         |
| `success`  | Success       |
| `tip`      | Tip           |
| `warning`  | Warning       |

Each callout renders as a collapsible `<details>` element:

```html
<details class="callout note" open>
  <summary class="callout-title">Note</summary>
  <div class="callout-body">...</div>
</details>
```

#### Type and Options

The callout type defaults to `note`. Use `type=` to specify a different type. Custom titles and collapse behavior are set via Pandoc-style key-value attributes:

```markdown
::: callout {type=warning title="Careful" open=false}
This warning starts collapsed.
:::
```

Recognized attributes:

| Attribute | Values           | Default | Description                              |
| --------- | ---------------- | ------- | ---------------------------------------- |
| `type`    | see table above  | `note`  | Callout type (determines icon and style) |
| `title`   | any string       | none    | Overrides the default title              |
| `open`    | `true` / `false` | `true`  | Controls whether the `<details>` is open |

`::: callout` without attributes uses the default type (`note`), default title, and is open by default. Attributes always require `{…}` braces.

#### Pandoc Attributes

Callouts support `#id` and `.class` attributes inside `{...}`:

```markdown
::: callout {#important .highlight type=tip}
This tip has a custom id and extra CSS class.
:::
```

- `#id` sets the HTML `id` attribute on the `<details>` element; the first `#id` wins if duplicates are specified.
- `.class` appends extra CSS classes after `callout <type>`; multiple `.class` tokens are allowed.

#### Body Content

The body of a callout is standard Markdown. It is rendered to HTML before being placed inside the callout wrapper, so all Markdown features (formatting, code blocks, images, etc.) work inside callouts.

### Fenced Divs

Directives using only Pandoc attributes (no directive name) render as `<div>` wrappers:

```markdown
::: {.compact-table}
| A   | B   |
| --- | --- |
| 1   | 2   |
:::
```

Renders as:

```html
<div class="compact-table">
  <table>...</table>
</div>
```

This follows the [Pandoc fenced div](https://pandoc.org/MANUAL.html#divs-and-spans) convention and is useful for applying CSS classes to content blocks without semantic meaning.

Both `#id` and `.class` attributes are supported:

```markdown
::: {#results .wide .striped}
Content here.
:::
```

### Nesting

Directives can be nested by using more colons for the outer fence:

```markdown
:::: callout {type=warning}
::: callout {type=tip}
This tip is inside a warning.
:::
More warning content.
::::
```

The closing fence must have at least as many colons as the opening fence it closes. A `:::` fence cannot close a `::::` block, but a `::::` fence can close a `:::` block.

### Code Blocks Inside Directives

Fenced code blocks inside directives work normally — the parser is aware of code fences and will not interpret `:::` inside a code block as a directive boundary:

````markdown
::: callout
Here is an example:

```python
print("Hello")
```
:::
````

### Unknown Directives

Unrecognized directive names are rendered as `<div>` elements with the name as a CSS class:

```markdown
::: custom-type
Body content.
:::
```

Renders as:

```html
<div class="custom-type">
  <p>Body content.</p>
</div>
```
