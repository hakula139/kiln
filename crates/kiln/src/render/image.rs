use std::fmt::Write;

use super::image_attrs::ImageAttrs;
use crate::html::escape;

/// Renders a standalone (block-level) image as a `<figure>` element.
///
/// The image gets `loading="lazy" decoding="async"`. If `alt` is non-empty, a `<figcaption>` is
/// included. The `title` attribute is omitted when empty. Optional `attrs`
/// apply `id` CSS classes to `<figure>`, and `width` / `height` to `<img>`.
#[must_use]
pub fn render_block_image(src: &str, alt: &str, title: &str, attrs: Option<&ImageAttrs>) -> String {
    let fig_id = attrs
        .and_then(|a| a.id.as_deref())
        .map(|v| format!(" id=\"{}\"", escape(v)))
        .unwrap_or_default();

    let fig_class = attrs
        .filter(|a| !a.classes.is_empty())
        .map(|a| {
            let classes: Vec<_> = a.classes.iter().map(|c| escape(c)).collect();
            format!(" class=\"{}\"", classes.join(" "))
        })
        .unwrap_or_default();

    let mut html = format!("<figure{fig_id}{fig_class}>\n  ");
    push_img_tag(&mut html, src, alt, title, attrs, false);
    html.push('\n');

    if !alt.is_empty() {
        _ = writeln!(html, "  <figcaption>{}</figcaption>", escape(alt));
    }

    html.push_str("</figure>\n");
    html
}

/// Renders an inline image as a plain `<img>` element with `loading="lazy" decoding="async"`.
///
/// The `title` attribute is omitted when empty. Optional `attrs` apply `id`,
/// CSS classes, `width`, and `height` directly to the `<img>` element.
#[must_use]
pub fn render_inline_image(
    src: &str,
    alt: &str,
    title: &str,
    attrs: Option<&ImageAttrs>,
) -> String {
    let mut html = String::new();
    push_img_tag(&mut html, src, alt, title, attrs, true);
    html
}

fn push_img_tag(
    html: &mut String,
    src: &str,
    alt: &str,
    title: &str,
    attrs: Option<&ImageAttrs>,
    include_identity: bool,
) {
    _ = write!(html, r#"<img src="{}" alt="{}""#, escape(src), escape(alt));

    if !title.is_empty() {
        _ = write!(html, r#" title="{}""#, escape(title));
    }

    if let Some(a) = attrs {
        if include_identity {
            if let Some(id) = &a.id {
                _ = write!(html, r#" id="{}""#, escape(id));
            }
            if !a.classes.is_empty() {
                let classes: Vec<_> = a.classes.iter().map(|c| escape(c)).collect();
                _ = write!(html, r#" class="{}""#, classes.join(" "));
            }
        }
        if let Some(w) = &a.width {
            _ = write!(html, r#" width="{}""#, escape(w));
        }
        if let Some(h) = &a.height {
            _ = write!(html, r#" height="{}""#, escape(h));
        }
    }

    html.push_str(r#" loading="lazy" decoding="async" />"#);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── render_block_image ──

    #[test]
    fn block_image_produces_figure() {
        let html = render_block_image("img.png", "A photo", "", None);
        assert!(html.contains("<figure>"), "html:\n{html}");
        assert!(html.contains(r#"src="img.png""#), "html:\n{html}");
        assert!(html.contains(r#"alt="A photo""#), "html:\n{html}");
        assert!(html.contains(r#"loading="lazy""#), "html:\n{html}");
        assert!(html.contains(r#"decoding="async""#), "html:\n{html}");
        assert!(
            html.contains("<figcaption>A photo</figcaption>"),
            "html:\n{html}"
        );
    }

    #[test]
    fn block_image_empty_alt_no_figcaption() {
        let html = render_block_image("img.png", "", "", None);
        assert!(html.contains("<figure>"), "html:\n{html}");
        assert!(!html.contains("<figcaption>"), "html:\n{html}");
    }

    #[test]
    fn block_image_with_title() {
        let html = render_block_image("img.png", "alt text", "My Title", None);
        assert!(html.contains(r#"title="My Title""#), "html:\n{html}");
        assert!(
            html.contains("<figcaption>alt text</figcaption>"),
            "html:\n{html}"
        );
    }

    #[test]
    fn block_image_escapes_special_characters() {
        let html = render_block_image(
            "img.png?a=1&b=2",
            r#"a <photo> & "test""#,
            "title's <value>",
            None,
        );
        assert!(
            html.contains(r#"src="img.png?a=1&amp;b=2""#),
            "html:\n{html}"
        );
        assert!(
            html.contains(r#"alt="a &lt;photo&gt; &amp; &quot;test&quot;""#),
            "html:\n{html}"
        );
        assert!(
            html.contains(r#"title="title&#39;s &lt;value&gt;""#),
            "html:\n{html}"
        );
    }

    #[test]
    fn block_image_with_id() {
        let attrs = ImageAttrs {
            id: Some("fig-1".into()),
            ..ImageAttrs::default()
        };
        let html = render_block_image("img.png", "alt", "", Some(&attrs));
        assert!(html.contains(r#"<figure id="fig-1">"#), "html:\n{html}");
    }

    #[test]
    fn block_image_with_class() {
        let attrs = ImageAttrs {
            classes: vec!["hero".into()],
            ..ImageAttrs::default()
        };
        let html = render_block_image("img.png", "alt", "", Some(&attrs));
        assert!(html.contains(r#"<figure class="hero">"#), "html:\n{html}");
    }

    #[test]
    fn block_image_with_width() {
        let attrs = ImageAttrs {
            width: Some("500".into()),
            ..ImageAttrs::default()
        };
        let html = render_block_image("img.png", "alt", "", Some(&attrs));
        assert!(html.contains(r#"width="500""#), "html:\n{html}");
    }

    #[test]
    fn block_image_with_height() {
        let attrs = ImageAttrs {
            height: Some("300".into()),
            ..ImageAttrs::default()
        };
        let html = render_block_image("img.png", "alt", "", Some(&attrs));
        assert!(html.contains(r#"height="300""#), "html:\n{html}");
    }

    // ── render_inline_image ──

    #[test]
    fn inline_image_no_figure() {
        let html = render_inline_image("img.png", "alt text", "", None);
        assert!(!html.contains("<figure>"), "html:\n{html}");
        assert!(html.starts_with("<img "), "html:\n{html}");
        assert!(html.contains(r#"src="img.png""#), "html:\n{html}");
        assert!(html.contains(r#"alt="alt text""#), "html:\n{html}");
        assert!(html.contains(r#"loading="lazy""#), "html:\n{html}");
        assert!(html.contains(r#"decoding="async""#), "html:\n{html}");
    }

    #[test]
    fn inline_image_with_id() {
        let attrs = ImageAttrs {
            id: Some("pic-1".into()),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.png", "alt", "", Some(&attrs));
        assert!(html.contains(r#"id="pic-1""#), "html:\n{html}");
    }

    #[test]
    fn inline_image_with_class() {
        let attrs = ImageAttrs {
            classes: vec!["centered".into()],
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.png", "alt", "", Some(&attrs));
        assert!(html.contains(r#"class="centered""#), "html:\n{html}");
    }

    #[test]
    fn inline_image_with_width() {
        let attrs = ImageAttrs {
            width: Some("500".into()),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.png", "alt", "", Some(&attrs));
        assert!(html.contains(r#"width="500""#), "html:\n{html}");
    }

    #[test]
    fn inline_image_with_height() {
        let attrs = ImageAttrs {
            height: Some("300".into()),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.png", "alt", "", Some(&attrs));
        assert!(html.contains(r#"height="300""#), "html:\n{html}");
    }
}
