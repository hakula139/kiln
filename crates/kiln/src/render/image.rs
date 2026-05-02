use std::fmt::Write;

use super::image_attrs::ImageAttrs;
use crate::html::escape;

/// Renders a standalone (block-level) image as a `<figure>` element.
///
/// The image gets `loading="lazy" decoding="async"`. If `alt` is non-empty, a `<figcaption>` is
/// included. The `title` attribute is omitted when empty. Optional `attrs`
/// apply `id` CSS classes to `<figure>`, and `width` / `height` to `<img>`.
/// When `attrs.lqip_uri` is set, the `<img>` is wrapped in a `<span class="lqip">`
/// inside the figure so themes can paint a blurred placeholder behind it.
#[must_use]
pub fn render_block_image(src: &str, alt: &str, title: &str, attrs: Option<&ImageAttrs>) -> String {
    let fig_id = attrs
        .and_then(|a| a.id.as_deref())
        .map(|v| format!(r#" id="{}""#, escape(v)))
        .unwrap_or_default();

    let fig_class = attrs
        .filter(|a| !a.classes.is_empty())
        .map(|a| {
            let classes: Vec<_> = a.classes.iter().map(|c| escape(c)).collect();
            format!(r#" class="{}""#, classes.join(" "))
        })
        .unwrap_or_default();

    let img_html = render_img(src, alt, title, attrs, false);

    let mut html = format!("<figure{fig_id}{fig_class}>\n  {img_html}\n");
    if !alt.is_empty() {
        _ = writeln!(html, "  <figcaption>{}</figcaption>", escape(alt));
    }
    html.push_str("</figure>\n");
    html
}

/// Renders an inline image as a plain `<img>` element with `loading="lazy" decoding="async"`.
///
/// The `title` attribute is omitted when empty. Optional `attrs` apply `id`,
/// CSS classes, `width`, and `height` directly to the `<img>` element. When
/// `attrs.lqip_uri` is set, the `<img>` is wrapped in `<span class="lqip">`.
#[must_use]
pub fn render_inline_image(
    src: &str,
    alt: &str,
    title: &str,
    attrs: Option<&ImageAttrs>,
) -> String {
    render_img(src, alt, title, attrs, true)
}

/// Builds the `<img>` tag, then wraps it in `<span class="lqip">` when an LQIP
/// URI is available. The wrapper exposes the placeholder via the `--lqip-uri`
/// custom property; themes consume it from a `::before` backdrop so the blur
/// can layer behind the image without affecting the bitmap itself.
fn render_img(
    src: &str,
    alt: &str,
    title: &str,
    attrs: Option<&ImageAttrs>,
    include_identity: bool,
) -> String {
    let mut img = String::new();
    push_img_tag(&mut img, src, alt, title, attrs, include_identity);

    // An empty URI is treated as no URI: a `url('')` placeholder is broken
    // and would still trigger the wrapper's CSS path on the theme side.
    match attrs
        .and_then(|a| a.lqip_uri.as_deref())
        .filter(|s| !s.is_empty())
    {
        // Base64 contains only `[A-Za-z0-9+/=]`, so no escaping needed for
        // the surrounding `"` or the CSS `url('...')` delimiters.
        Some(uri) => format!(r#"<span class="lqip" style="--lqip-uri:url('{uri}')">{img}</span>"#),
        None => img,
    }
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
        let (final_w, final_h) = final_dimensions(a);
        if let Some(w) = &final_w {
            _ = write!(html, r#" width="{}""#, escape(w));
        }
        if let Some(h) = &final_h {
            _ = write!(html, r#" height="{}""#, escape(h));
        }
    }

    html.push_str(r#" loading="lazy" decoding="async" />"#);
}

/// Picks the `width` / `height` to emit. Manual `{width=...}` / `{height=...}`
/// always win; when only one is set, the other is scaled from the resolver's
/// auto aspect so the rendered box matches the source shape.
fn final_dimensions(attrs: &ImageAttrs) -> (Option<String>, Option<String>) {
    match (
        attrs.width.as_deref(),
        attrs.height.as_deref(),
        attrs.auto_width,
        attrs.auto_height,
    ) {
        (Some(w), Some(h), _, _) => (Some(w.into()), Some(h.into())),
        (Some(w), None, Some(aw), Some(ah)) => {
            let scaled = w
                .parse::<u32>()
                .ok()
                .map(|wp| (u64::from(ah) * u64::from(wp) / u64::from(aw)).to_string());
            (Some(w.into()), scaled)
        }
        (None, Some(h), Some(aw), Some(ah)) => {
            let scaled = h
                .parse::<u32>()
                .ok()
                .map(|hp| (u64::from(aw) * u64::from(hp) / u64::from(ah)).to_string());
            (scaled, Some(h.into()))
        }
        (None, None, Some(aw), Some(ah)) => (Some(aw.to_string()), Some(ah.to_string())),
        (m_w, m_h, _, _) => (m_w.map(Into::into), m_h.map(Into::into)),
    }
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

    // ── auto dimensions + LQIP ──

    #[test]
    fn inline_image_emits_auto_dimensions() {
        let attrs = ImageAttrs {
            auto_width: Some(1200),
            auto_height: Some(800),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.avif", "alt", "", Some(&attrs));
        assert!(html.contains(r#"width="1200""#), "html:\n{html}");
        assert!(html.contains(r#"height="800""#), "html:\n{html}");
    }

    #[test]
    fn inline_image_with_lqip_wraps_in_span() {
        let attrs = ImageAttrs {
            auto_width: Some(100),
            auto_height: Some(60),
            lqip_uri: Some("data:image/webp;base64,AAA".into()),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.avif", "alt", "", Some(&attrs));
        assert!(
            html.starts_with(
                r#"<span class="lqip" style="--lqip-uri:url('data:image/webp;base64,AAA')">"#
            ),
            "wrapper opens with the lqip span, html:\n{html}"
        );
        assert!(html.ends_with("</span>"), "wrapper closes, html:\n{html}");
        assert!(
            html.contains("<img "),
            "img is inside the wrapper, html:\n{html}"
        );
        assert!(
            !html.contains("background:url"),
            "no inline background style on the img, html:\n{html}",
        );
    }

    #[test]
    fn inline_image_with_lqip_keeps_identity_attrs_on_img() {
        // Identity attrs must stay on the `<img>` so theme selectors like
        // `img#hero` or `img.full-bleed` keep matching after the wrapper lands.
        let attrs = ImageAttrs {
            id: Some("hero".into()),
            classes: vec!["full-bleed".into()],
            auto_width: Some(100),
            auto_height: Some(60),
            lqip_uri: Some("data:image/webp;base64,AAA".into()),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.avif", "alt", "", Some(&attrs));
        let span_open_end = html.find('>').expect("wrapper has an opening tag");
        let span_open = &html[..=span_open_end];
        assert!(
            !span_open.contains(r#"id="hero""#),
            "id should not land on the <span>, span:\n{span_open}",
        );
        assert!(
            !span_open.contains(r#"class="full-bleed""#),
            "user class should not land on the <span>, span:\n{span_open}",
        );
        assert!(
            html.contains(r#"<img src="img.avif" alt="alt" id="hero" class="full-bleed""#),
            "id and class should land on the <img>, html:\n{html}",
        );
    }

    #[test]
    fn inline_image_without_lqip_emits_bare_img() {
        let attrs = ImageAttrs {
            auto_width: Some(100),
            auto_height: Some(60),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.avif", "alt", "", Some(&attrs));
        assert!(
            html.starts_with("<img "),
            "no wrapper without lqip, html:\n{html}"
        );
        assert!(!html.contains(r#"class="lqip""#), "html:\n{html}");
    }

    #[test]
    fn inline_image_with_empty_lqip_uri_emits_bare_img() {
        let attrs = ImageAttrs {
            auto_width: Some(100),
            auto_height: Some(60),
            lqip_uri: Some(String::new()),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.avif", "alt", "", Some(&attrs));
        assert!(
            html.starts_with("<img "),
            "empty uri should not produce a wrapper, html:\n{html}"
        );
        assert!(!html.contains(r#"class="lqip""#), "html:\n{html}");
        assert!(!html.contains("url('"), "html:\n{html}");
    }

    #[test]
    fn block_image_with_lqip_wraps_inside_figure() {
        let attrs = ImageAttrs {
            auto_width: Some(100),
            auto_height: Some(60),
            lqip_uri: Some("data:image/webp;base64,AAA".into()),
            ..ImageAttrs::default()
        };
        let html = render_block_image("img.avif", "alt", "", Some(&attrs));
        assert!(html.contains("<figure>"), "html:\n{html}");
        assert!(
            html.contains(
                r#"<span class="lqip" style="--lqip-uri:url('data:image/webp;base64,AAA')"><img "#
            ),
            "wrapper sits between figure and img, html:\n{html}",
        );
        assert!(html.contains("</span>"), "wrapper closed, html:\n{html}");
        assert!(
            html.contains("<figcaption>alt</figcaption>"),
            "html:\n{html}"
        );
    }

    #[test]
    fn block_image_without_lqip_emits_bare_img_inside_figure() {
        let attrs = ImageAttrs {
            auto_width: Some(100),
            auto_height: Some(60),
            ..ImageAttrs::default()
        };
        let html = render_block_image("img.avif", "alt", "", Some(&attrs));
        assert!(html.contains("<figure>"), "html:\n{html}");
        assert!(
            !html.contains(r#"class="lqip""#),
            "no wrapper without lqip, html:\n{html}",
        );
        assert!(html.contains("<img "), "img is rendered, html:\n{html}");
    }

    #[test]
    fn inline_image_manual_width_scales_auto_height() {
        // {width=600} on a 1200×800 source → 600×400 box.
        let attrs = ImageAttrs {
            width: Some("600".into()),
            auto_width: Some(1200),
            auto_height: Some(800),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.avif", "alt", "", Some(&attrs));
        assert!(html.contains(r#"width="600""#), "html:\n{html}");
        assert!(html.contains(r#"height="400""#), "html:\n{html}");
    }

    #[test]
    fn inline_image_manual_height_scales_auto_width() {
        // {height=400} on a 1200×800 source → 600×400 box.
        let attrs = ImageAttrs {
            height: Some("400".into()),
            auto_width: Some(1200),
            auto_height: Some(800),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.avif", "alt", "", Some(&attrs));
        assert!(html.contains(r#"width="600""#), "html:\n{html}");
        assert!(html.contains(r#"height="400""#), "html:\n{html}");
    }

    #[test]
    fn inline_image_manual_dimensions_win_over_auto() {
        let attrs = ImageAttrs {
            width: Some("250".into()),
            height: Some("100".into()),
            auto_width: Some(1200),
            auto_height: Some(800),
            ..ImageAttrs::default()
        };
        let html = render_inline_image("img.avif", "alt", "", Some(&attrs));
        assert!(html.contains(r#"width="250""#), "html:\n{html}");
        assert!(html.contains(r#"height="100""#), "html:\n{html}");
        assert!(!html.contains(r#"width="1200""#), "html:\n{html}");
    }

    #[test]
    fn inline_image_no_dimensions_emits_nothing() {
        let attrs = ImageAttrs::default();
        let html = render_inline_image("img.png", "alt", "", Some(&attrs));
        assert!(!html.contains("width="), "html:\n{html}");
        assert!(!html.contains("height="), "html:\n{html}");
        assert!(!html.contains("style="), "html:\n{html}");
    }
}
