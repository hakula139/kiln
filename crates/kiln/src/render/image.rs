use std::borrow::Cow;
use std::fmt::Write;

use super::image_attrs::ImageAttrs;
use crate::html::escape;

/// Applies identity to the figure and dimensions to its image.
pub(super) fn render_block_image(
    src: &str,
    alt: &str,
    title: &str,
    attrs: Option<&ImageAttrs>,
) -> String {
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

/// Applies all authored attributes to the inline image.
pub(super) fn render_inline_image(
    src: &str,
    alt: &str,
    title: &str,
    attrs: Option<&ImageAttrs>,
) -> String {
    render_img(src, alt, title, attrs, true)
}

fn render_img(
    src: &str,
    alt: &str,
    title: &str,
    attrs: Option<&ImageAttrs>,
    include_identity: bool,
) -> String {
    let mut img = format!(r#"<img src="{}" alt="{}""#, escape(src), escape(alt));

    if !title.is_empty() {
        _ = write!(img, r#" title="{}""#, escape(title));
    }

    if let Some(a) = attrs {
        if include_identity {
            if let Some(id) = &a.id {
                _ = write!(img, r#" id="{}""#, escape(id));
            }
            if !a.classes.is_empty() {
                let classes: Vec<_> = a.classes.iter().map(|c| escape(c)).collect();
                _ = write!(img, r#" class="{}""#, classes.join(" "));
            }
        }
        let (final_w, final_h) = final_dimensions(a);
        if let Some(w) = &final_w {
            _ = write!(img, r#" width="{}""#, escape(w));
        }
        if let Some(h) = &final_h {
            _ = write!(img, r#" height="{}""#, escape(h));
        }
    }

    img.push_str(r#" loading="lazy" decoding="async" />"#);

    match attrs.and_then(|attrs| attrs.lqip_uri.as_deref()) {
        // Base64 uses safe chars (`A-Za-z0-9+/=`), so `"..."` and `url('...')` need no escape.
        Some(uri) => format!(r#"<span class="lqip" style="--lqip-uri:url('{uri}')">{img}</span>"#),
        None => img,
    }
}

/// Picks the `width` / `height` to emit. Manual `{width=...}` / `{height=...}` always win. When
/// only one is set, the other is scaled from the resolver's auto aspect so the rendered box matches
/// the source shape.
fn final_dimensions(attrs: &ImageAttrs) -> (Option<Cow<'_, str>>, Option<Cow<'_, str>>) {
    let width = attrs.width.as_deref().map(Cow::Borrowed);
    let height = attrs.height.as_deref().map(Cow::Borrowed);
    let (Some(auto_width), Some(auto_height)) = (attrs.auto_width, attrs.auto_height) else {
        return (width, height);
    };

    match (width, height) {
        (Some(w), None) => {
            let h = scale(&w, auto_height, auto_width);
            (Some(w), h)
        }
        (None, Some(h)) => (scale(&h, auto_width, auto_height), Some(h)),
        (None, None) => (
            Some(auto_width.to_string().into()),
            Some(auto_height.to_string().into()),
        ),
        authored => authored,
    }
}

/// Scales an authored pixel length by `numerator / denominator`, or returns `None` when the length
/// is not a plain integer.
fn scale(value: &str, numerator: u32, denominator: u32) -> Option<Cow<'static, str>> {
    let value: u32 = value.parse().ok()?;
    let scaled = u64::from(numerator) * u64::from(value) / u64::from(denominator);
    Some(scaled.to_string().into())
}

#[cfg(test)]
mod tests {
    use indoc::{formatdoc, indoc};

    use super::*;

    // ── render_block_image ──

    #[test]
    fn wraps_img_and_caption_in_figure() {
        for (alt, title, expected) in [
            (
                "A photo",
                "",
                indoc! {r#"
                    <figure>
                      <img src="img.png" alt="A photo" loading="lazy" decoding="async" />
                      <figcaption>A photo</figcaption>
                    </figure>
                "#},
            ),
            (
                "alt text",
                "My Title",
                indoc! {r#"
                    <figure>
                      <img src="img.png" alt="alt text" title="My Title" loading="lazy" decoding="async" />
                      <figcaption>alt text</figcaption>
                    </figure>
                "#},
            ),
            (
                "",
                "",
                indoc! {r#"
                    <figure>
                      <img src="img.png" alt="" loading="lazy" decoding="async" />
                    </figure>
                "#},
            ),
        ] {
            assert_eq!(render_block_image("img.png", alt, title, None), expected);
        }
    }

    #[test]
    fn escapes_special_characters() {
        assert_eq!(
            render_block_image(
                "img.png?a=1&b=2",
                r#"a <photo> & "test""#,
                "title's <value>",
                None,
            ),
            indoc! {r#"
                <figure>
                  <img src="img.png?a=1&amp;b=2" alt="a &lt;photo&gt; &amp; &quot;test&quot;" title="title&#39;s &lt;value&gt;" loading="lazy" decoding="async" />
                  <figcaption>a &lt;photo&gt; &amp; &quot;test&quot;</figcaption>
                </figure>
            "#}
        );
    }

    #[test]
    fn puts_identity_on_figure_and_dimensions_on_img() {
        let attrs = ImageAttrs {
            id: Some("photo".into()),
            classes: vec!["hero".into(), "wide".into()],
            width: Some("500".into()),
            height: Some("300".into()),
            ..ImageAttrs::default()
        };
        assert_eq!(
            render_block_image("img.png", "alt", "", Some(&attrs)),
            indoc! {r#"
                <figure id="photo" class="hero wide">
                  <img src="img.png" alt="alt" width="500" height="300" loading="lazy" decoding="async" />
                  <figcaption>alt</figcaption>
                </figure>
            "#}
        );
    }

    #[test]
    fn wraps_img_in_lqip_span_only_with_placeholder() {
        let img = r#"<img src="img.avif" alt="alt" width="100" height="60" loading="lazy" decoding="async" />"#;
        for (lqip_uri, expected_img) in [
            (
                Some("data:image/webp;base64,AAA"),
                format!(
                    r#"<span class="lqip" style="--lqip-uri:url('data:image/webp;base64,AAA')">{img}</span>"#
                ),
            ),
            (None, img.to_owned()),
        ] {
            let attrs = ImageAttrs {
                auto_width: Some(100),
                auto_height: Some(60),
                lqip_uri: lqip_uri.map(str::to_owned),
                ..ImageAttrs::default()
            };
            assert_eq!(
                render_block_image("img.avif", "alt", "", Some(&attrs)),
                formatdoc! {"
                    <figure>
                      {expected_img}
                      <figcaption>alt</figcaption>
                    </figure>
                "},
                "{lqip_uri:?}"
            );
        }
    }

    // ── render_inline_image ──

    #[test]
    fn renders_bare_img_without_figure() {
        for attrs in [None, Some(&ImageAttrs::default())] {
            assert_eq!(
                render_inline_image("img.png", "alt text", "", attrs),
                r#"<img src="img.png" alt="alt text" loading="lazy" decoding="async" />"#,
                "{attrs:?}"
            );
        }
    }

    #[test]
    fn puts_identity_and_dimensions_on_img() {
        let attrs = ImageAttrs {
            id: Some("photo".into()),
            classes: vec!["hero".into(), "wide".into()],
            width: Some("500".into()),
            height: Some("300".into()),
            ..ImageAttrs::default()
        };
        assert_eq!(
            render_inline_image("img.png", "alt", "", Some(&attrs)),
            r#"<img src="img.png" alt="alt" id="photo" class="hero wide" width="500" height="300" loading="lazy" decoding="async" />"#
        );
    }

    #[test]
    fn lqip_span_keeps_identity_attrs_on_img() {
        let attrs = ImageAttrs {
            id: Some("hero".into()),
            classes: vec!["full-bleed".into()],
            auto_width: Some(100),
            auto_height: Some(60),
            lqip_uri: Some("data:image/webp;base64,AAA".into()),
            ..ImageAttrs::default()
        };
        assert_eq!(
            render_inline_image("img.avif", "alt", "", Some(&attrs)),
            r#"<span class="lqip" style="--lqip-uri:url('data:image/webp;base64,AAA')"><img src="img.avif" alt="alt" id="hero" class="full-bleed" width="100" height="60" loading="lazy" decoding="async" /></span>"#
        );
    }

    #[test]
    fn merges_authored_and_auto_dimensions() {
        for (width, height, expected) in [
            (None, None, r#"width="1200" height="800""#),
            (Some("600"), None, r#"width="600" height="400""#),
            (None, Some("400"), r#"width="600" height="400""#),
            (Some("250"), Some("100"), r#"width="250" height="100""#),
            (Some("50%"), None, r#"width="50%""#),
        ] {
            let attrs = ImageAttrs {
                width: width.map(str::to_string),
                height: height.map(str::to_string),
                auto_width: Some(1200),
                auto_height: Some(800),
                ..ImageAttrs::default()
            };
            assert_eq!(
                render_inline_image("img.avif", "alt", "", Some(&attrs)),
                format!(
                    r#"<img src="img.avif" alt="alt" {expected} loading="lazy" decoding="async" />"#
                )
            );
        }
    }
}
