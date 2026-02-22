use std::fmt::Write;

use super::escape_html;

/// Renders a standalone (block-level) image as a `<figure>` element.
///
/// The image gets `loading="lazy"`. If `alt` is non-empty, a `<figcaption>` is
/// included. The `title` attribute is omitted when empty.
#[must_use]
pub fn render_block_image(src: &str, alt: &str, title: &str) -> String {
    let mut html = String::from("<figure>\n");
    push_img_tag(&mut html, src, alt, title);
    html.push('\n');

    if !alt.is_empty() {
        let _ = writeln!(html, "<figcaption>{}</figcaption>", escape_html(alt));
    }

    html.push_str("</figure>\n");
    html
}

/// Renders an inline image as a plain `<img>` element with `loading="lazy"`.
///
/// The `title` attribute is omitted when empty.
#[must_use]
pub fn render_inline_image(src: &str, alt: &str, title: &str) -> String {
    let mut html = String::new();
    push_img_tag(&mut html, src, alt, title);
    html
}

fn push_img_tag(html: &mut String, src: &str, alt: &str, title: &str) {
    let _ = write!(
        html,
        r#"<img src="{}" alt="{}""#,
        escape_html(src),
        escape_html(alt)
    );

    if !title.is_empty() {
        let _ = write!(html, r#" title="{}""#, escape_html(title));
    }

    html.push_str(r#" loading="lazy" />"#);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_image_produces_figure() {
        let html = render_block_image("img.png", "A photo", "");
        assert!(
            html.contains("<figure>"),
            "should become a figure, html:\n{html}"
        );
        assert!(
            html.contains(r#"src="img.png""#),
            "should have src attribute, html:\n{html}"
        );
        assert!(
            html.contains(r#"alt="A photo""#),
            "should have alt attribute, html:\n{html}"
        );
        assert!(
            html.contains(r#"loading="lazy""#),
            "should have lazy loading, html:\n{html}"
        );
        assert!(
            html.contains("<figcaption>A photo</figcaption>"),
            "should have figcaption with alt text, html:\n{html}"
        );
    }

    #[test]
    fn block_image_empty_alt_no_figcaption() {
        let html = render_block_image("img.png", "", "");
        assert!(
            html.contains("<figure>"),
            "should become a figure, html:\n{html}"
        );
        assert!(
            html.contains(r#"src="img.png""#),
            "should have src attribute, html:\n{html}"
        );
        assert!(
            html.contains(r#"alt="""#),
            "should have empty alt, html:\n{html}"
        );
        assert!(
            !html.contains("<figcaption>"),
            "should omit figcaption when alt is empty, html:\n{html}"
        );
    }

    #[test]
    fn block_image_with_title() {
        let html = render_block_image("img.png", "alt text", "My Title");
        assert!(
            html.contains("<figure>"),
            "should become a figure, html:\n{html}"
        );
        assert!(
            html.contains(r#"src="img.png""#),
            "should have src attribute, html:\n{html}"
        );
        assert!(
            html.contains(r#"alt="alt text""#),
            "should have alt attribute, html:\n{html}"
        );
        assert!(
            html.contains(r#"title="My Title""#),
            "should have title attribute, html:\n{html}"
        );
        assert!(
            html.contains("<figcaption>alt text</figcaption>"),
            "figcaption should use alt not title, html:\n{html}"
        );
    }

    #[test]
    fn block_image_escapes_special_characters() {
        let html = render_block_image(
            "img.png?a=1&b=2",
            r#"a <photo> & "test""#,
            "title's <value>",
        );
        assert!(
            html.contains(r#"src="img.png?a=1&amp;b=2""#),
            "src should be escaped, html:\n{html}"
        );
        assert!(
            html.contains(r#"alt="a &lt;photo&gt; &amp; &quot;test&quot;""#),
            "alt should be escaped, html:\n{html}"
        );
        assert!(
            html.contains(r#"title="title&#39;s &lt;value&gt;""#),
            "title should be escaped, html:\n{html}"
        );
        assert!(
            html.contains("<figcaption>a &lt;photo&gt; &amp; &quot;test&quot;</figcaption>"),
            "figcaption should be escaped, html:\n{html}"
        );
    }

    #[test]
    fn inline_image_no_figure() {
        let html = render_inline_image("img.png", "alt text", "");
        assert!(
            !html.contains("<figure>"),
            "should not become a figure, html:\n{html}"
        );
        assert!(
            html.starts_with("<img "),
            "should have img tag, html:\n{html}"
        );
        assert!(
            html.contains(r#"src="img.png""#),
            "should have src attribute, html:\n{html}"
        );
        assert!(
            html.contains(r#"alt="alt text""#),
            "should have alt attribute, html:\n{html}"
        );
        assert!(
            html.contains(r#"loading="lazy""#),
            "should have lazy loading, html:\n{html}"
        );
    }
}
