# Syntax Reference

kiln processes Markdown content with several extensions beyond standard CommonMark. This document describes the syntax authors write. How themes consume the output is covered in [Theming](themes.md).

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
src = "/images/hero.jpg"
position = "top"

[featured_image.credit]
title = "Work Title"
author = "Artist"
url = "https://example.com/artworks/123"
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
| `slug`           | derived from filename |
| `weight`         | none                  |
| `featured_image` | none (table)          |

Draft pages (`draft = true`) and pages whose filename starts with `_` are excluded from the build.

A post with any `weight` set is pinned on the home page, sorted before unpinned posts and ordered by `weight` ascending (lower floats higher, matching Hugo's convention). Archive, tag, and section listings ignore `weight` and stay strictly date-sorted, so a pinned post still appears at its natural date position in those listings.

`date` / `updated` are absolute instants. When kiln exposes a page date to templates, it renders that instant in the site's configured `timezone` from `config.toml` (UTC if `timezone` is unset):

```toml
timezone = "Asia/Shanghai"
```

## Pandoc-Style Attributes

A `{...}` attribute block is the shared syntax kiln uses to attach metadata to images, fenced code blocks, and directives. The same parser handles all three. They differ in which keys they recognize and how bare words are interpreted.

The block accepts four token kinds, in any order:

| Token       | Meaning                                                          |
| ----------- | ---------------------------------------------------------------- |
| `#id`       | HTML `id`. First wins if duplicates appear; later `#id`s ignored |
| `.class`    | CSS class. Multiple `.class` tokens accumulate                   |
| `key=value` | Key-value pair. Value can be quoted (`key="..."`) or bare        |
| `bare_word` | Standalone word. Interpretation depends on the consumer          |

Quoted values support `\"` (escaped quote) and `\\` (escaped backslash). An unclosed `"` consumes the rest of the input. Unknown keys are silently ignored, so consumers can evolve their recognized set without breaking older content.

Bare words are interpreted differently by each consumer:

| Consumer   | Bare-word meaning                                                |
| ---------- | ---------------------------------------------------------------- |
| Code fence | Boolean flag (`collapse`, `expand`)                              |
| Directive  | Positional argument (surfaced to templates as `positional_args`) |
| Image      | Ignored                                                          |

See the corresponding sections for the specific keys each consumer recognizes:

- [Image Attributes](#image-attributes)
- [Fence Attributes](#fence-attributes)
- [Directives](#directives)

## Markdown

kiln uses [pulldown-cmark](https://github.com/raphlinus/pulldown-cmark) for Markdown rendering. Standard CommonMark is fully supported, along with the following extensions.

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

Headings are also collected into a structured table of contents, exposed to post templates as the `toc` variable — see [Post templates](themes.md#post-templates-posthtml).

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

#### Image Attributes

A [Pandoc-style attribute block](#pandoc-style-attributes) can follow the closing `)` to set id, classes, width, and height:

```markdown
![Photo](photo.jpg){#hero .wide width=800 height=600}
```

The block must appear immediately after the closing `)` on the same line. Recognized keys:

| Key      | Target (block) | Target (inline) |
| -------- | -------------- | --------------- |
| `#id`    | `<figure>`     | `<img>`         |
| `.class` | `<figure>`     | `<img>`         |
| `width`  | `<img>`        | `<img>`         |
| `height` | `<img>`        | `<img>`         |

### Syntax Highlighting

Fenced code blocks with a language tag receive syntax highlighting via [syntect](https://github.com/trishume/syntect) + [two-face](https://github.com/CosmicHorrorDev/two-face) (bat's 200+ language syntax definitions):

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
- Language labels are canonicalized from syntax definitions (e.g., `rs` maps to `rust`).
- Unrecognized languages fall back to plain text. Known non-code DSLs (e.g., `mermaid`) are silently treated as plain text.

Code blocks are wrapped in a structured HTML container:

```html
<div class="code-block" data-lang="rust">
  <div class="code-header">
    <span class="code-lang">Rust</span>
    <button class="copy-btn" aria-label="Copy code">...</button>
  </div>
  <div class="code-body">
    <div class="highlight">...</div>
  </div>
</div>
```

The `code-header` displays the human-readable language name. When `code_max_lines` is set in the site's `[params]`, the `code-body` div includes a `data-max-lines` attribute for JS-driven collapse / expand.

#### Fence Attributes

A [Pandoc-style attribute block](#pandoc-style-attributes) can follow the language tag to refine a fenced code block:

````markdown
```rust {#example .compact title="src/main.rs" highlight="1,3-5" collapse}
fn main() {
    println!("Hello, world!");
}
```
````

Recognized keys:

| Key                 | Effect                                                                               |
| ------------------- | ------------------------------------------------------------------------------------ |
| `#id`               | Sets the `id` attribute on the wrapper `<div class="code-block">`                    |
| `.class`            | Appends additional CSS classes to the wrapper                                        |
| `title="..."`       | Renders a `<span class="code-title">` in place of the language pill                  |
| `highlight="1,3-5"` | Comma-separated lines / ranges to mark with `class="line hl"` (and `line-number hl`) |

Bare flags:

| Flag       | Effect                                                                           |
| ---------- | -------------------------------------------------------------------------------- |
| `collapse` | Forces the block into the collapsed state regardless of the site default         |
| `expand`   | Forces the block into the expanded state, suppressing any `code_max_lines` clamp |

Either flag wins over the site-level `code_max_lines` default. The language tag is preserved on `data-lang` for syntax CSS even when a `title` is set.

### Math (KaTeX)

Inline math uses single dollar signs, display math uses double:

```markdown
Inline: $E = mc^2$

Display:

$$
\int_0^\infty e^{-x^2} dx = \frac{\sqrt{\pi}}{2}
$$
```

Math expressions render as KaTeX-compatible markup (`<span class="math math-inline">` / `<span class="math math-display">`). Themes load the [KaTeX](https://katex.org) CSS and JS for client-side rendering by gating on `"math" in assets.features` — see [Theming](themes.md#template-variables) for the page-scoped asset registry.

### Footnotes

```markdown
Here is a claim[^1] that needs a source.

[^1]: The source for the claim.
```

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

## Shortcodes

Shortcodes are inline replacements processed before Markdown rendering. They are skipped inside fenced code blocks and inline code spans.

### Emoji

When `emojis = true` is set in `[params]`, GitHub-style emoji shortcodes are replaced with Unicode characters:

```markdown
Hello :smile: and :wave:

<!-- renders as: Hello 😄 and 👋 -->
```

Unknown shortcodes (e.g., `:not_a_real_emoji:`) are left as-is. See the [GitHub emoji list](https://github.com/ikatyang/emoji-cheat-sheet) for supported shortcodes.

### Font Awesome Icons

When `fontawesome = true` is set in `[params]`, icon shortcodes produce `<i>` elements:

```markdown
:(fas fa-link): Click here :(fab fa-github):

<!-- renders as: <i class="fas fa-link" aria-hidden="true"></i> Click here <i class="fab fa-github" aria-hidden="true"></i> -->
```

The class inside `:(...):` is passed to the `class` attribute of the `<i>` element. The page template must include the [Font Awesome](https://fontawesome.com) CSS for icons to display.

## Directives

Directives use `:::` fenced blocks (similar to [Pandoc fenced divs](https://pandoc.org/MANUAL.html#divs-and-spans)). They provide structured content blocks beyond standard Markdown.

### Basic Syntax

A directive block starts with three or more colons followed by an optional directive name, and ends with a matching (or longer) colon fence:

```markdown
::: callout
This is a note.
:::
```

A [Pandoc-style attribute block](#pandoc-style-attributes) can follow the directive name. Bare words inside `{...}` become positional arguments accessible in templates as `positional_args`:

```markdown
::: callout {#my-id .custom-class type=tip title="Read This"}
Content here.
:::
```

### Parser Behavior

#### Nesting

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

#### Code Blocks Inside Directives

Fenced code blocks inside directives work normally — the parser is aware of code fences and will not interpret `:::` inside a code block as a directive boundary:

````markdown
::: callout
Here is an example:

```python
print("Hello")
```

:::
````

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

The callout type defaults to `note`. Use `type=` to specify a different type. Custom titles and collapse behavior are set via key-value attributes:

```markdown
::: callout {type=warning title="Careful" open=false}
This warning starts collapsed.
:::
```

Recognized keys:

| Key     | Values           | Default | Description                              |
| ------- | ---------------- | ------- | ---------------------------------------- |
| `type`  | see table above  | `note`  | Callout type (determines icon and style) |
| `title` | any string       | none    | Overrides the default title              |
| `open`  | `true` / `false` | `true`  | Controls whether the `<details>` is open |

`#id` and `.class` attributes work as documented under [Pandoc-Style Attributes](#pandoc-style-attributes): `#id` lands on the `<details>` element, `.class` tokens append after `callout <type>`.

`::: callout` without attributes uses the default type (`note`), default title, and is open by default.

#### Body Content

The body of a callout is standard Markdown. It is rendered to HTML before being placed inside the callout wrapper, so all Markdown features (formatting, code blocks, images, etc.) work inside callouts.

### Generic Div Wrappers

A directive renders as a plain `<div>` wrapper in two cases:

**Untyped (no name)** — Pandoc fenced div convention, useful for applying CSS classes to content blocks without semantic meaning:

```markdown
::: {.compact-table}
| A   | B   |
| --- | --- |
| 1   | 2   |
:::
```

```html
<div class="compact-table">
  <table>
    ...
  </table>
</div>
```

**Unknown name** — when no `templates/directives/<name>.html` template exists, the directive name becomes a CSS class on the wrapper:

```markdown
::: custom-type
Body content.
:::
```

```html
<div class="custom-type">
  <p>Body content.</p>
</div>
```

In both cases, `#id` and `.class` from the `{...}` block are applied to the `<div>` as expected.

### Template-Based Directives

Themes can provide custom directive renderers as MiniJinja templates at `templates/directives/<name>.html`. When a directive name matches a template, kiln renders it using the template instead of the generic `<div>` wrapper:

```markdown
::: site
https://example.com
:::
```

If `templates/directives/site.html` exists, kiln renders it with the [directive template variables](themes.md#directive-templates-directivesnamehtml).

#### Directive Arguments

Arguments inside `{...}` (after `#id` and `.class` extraction) are split into **positional** and **named** components, exposed to templates as `positional_args` and `named_args`:

| Input form        | Example            | Result                      |
| ----------------- | ------------------ | --------------------------- |
| `"quoted string"` | `"scores.csv"`     | Positional: `"scores.csv"`  |
| `bare_word`       | `inline`           | Positional: `"inline"`      |
| `key="value"`     | `server="netease"` | Named: `server → "netease"` |
| `key=value`       | `cols=3`           | Named: `cols → "3"`         |

For example, `::: music {#player .wide server="netease" type="song" id="12345"}` parses to: `id="player"`, `classes=["wide"]`, and `named_args={server: "netease", type: "song", id: "12345"}`.

```html
<iframe
  src="https://{{ named_args.server }}.com/embed/{{ named_args.type }}/{{ named_args.id }}"
></iframe>
```

For data-driven directives, the template-side helpers `read_file`, `parse_csv`, and `register_script` are documented under [Template Functions](themes.md#template-functions).
