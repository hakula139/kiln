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

A directive block starts with three or more colons followed by a type name, and ends with a matching (or longer) colon fence:

```markdown
::: note
This is a note.
:::
```

### Admonitions

Admonitions are styled callout blocks. kiln supports 12 types:

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

Type names are case-insensitive (`NOTE`, `Note`, and `note` all work).

Each admonition renders as a collapsible `<details>` element:

```html
<details class="admonition note" open>
  <summary class="admonition-title">Note</summary>
  <div class="admonition-body">...</div>
</details>
```

#### Titles and Options

Custom titles and collapse behavior are set via Pandoc-style key-value attributes in curly braces:

```markdown
::: {.note title="Custom Title"}
This note has a custom title.
:::

::: {.warning title="Careful" open=false}
This warning starts collapsed.
:::
```

The `.type` class specifies the admonition type (e.g., `.note`, `.warning`).

Recognized attributes:

| Attribute | Values           | Default | Description                              |
| --------- | ---------------- | ------- | ---------------------------------------- |
| `title`   | any string       | none    | Overrides the default title              |
| `open`    | `true` / `false` | `true`  | Controls whether the `<details>` is open |

The simple form (`::: note`) uses the default title and is open by default. Key-value attributes also work in the simple form without curly braces:

```markdown
::: note title="Read This" open=false
Collapsed note with a custom title.
:::
```

The `.type` dot prefix is only needed inside `{...}`. Bare words after the type name (without `=`) are ignored.

#### Body Content

The body of an admonition is standard Markdown. It is rendered to HTML before being placed inside the admonition wrapper, so all Markdown features (formatting, code blocks, images, etc.) work inside admonitions.

### Nesting

Directives can be nested by using more colons for the outer fence:

```markdown
:::: warning
::: note
This note is inside a warning.
:::
More warning content.
::::
```

The closing fence must have at least as many colons as the opening fence it closes. A `:::` fence cannot close a `::::` block, but a `::::` fence can close a `:::` block.

### Code Blocks Inside Directives

Fenced code blocks inside directives work normally — the parser is aware of code fences and will not interpret `:::` inside a code block as a directive boundary:

````markdown
::: note
Here is an example:

```python
print("Hello")
```
:::
````

### Unknown Directives

Unrecognized directive types are parsed but passed through without special rendering. This allows for future extension:

```markdown
::: custom-type args
Body content
:::
```
