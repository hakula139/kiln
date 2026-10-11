use indoc::{formatdoc, indoc};

use super::support::TestSite;

// ── build: pages ──

#[test]
fn build_exposes_dates_in_configured_timezone_and_linked_tags_without_git() {
    let site = TestSite::new(indoc! {r#"
        base_url = "https://example.com"
        timezone = "Asia/Shanghai"
    "#});
    site.write(
        "templates/post.html",
        indoc! {r#"
            <time datetime="{{ date }}">{{ date[:10] }}</time>
            {% if updated %}<time datetime="{{ updated }}">{{ updated[:10] }}</time>{% endif %}
            {% for tag in tags %}<a href="{{ tag.url }}">{{ tag.name }}</a>{% endfor %}
        "#},
    )
    .page(
        "posts/note/hello",
        indoc! {r#"
            +++
            title = "Hello"
            date = "2026-03-13T09:36:00Z"
            updated = "2026-03-13T22:36:00Z"
            tags = ["C++", "<script>"]
            +++
            Body
        "#},
    );

    site.build().unwrap();

    assert_eq!(
        site.read("posts/note/hello/index.html"),
        indoc! {r#"
            <time datetime="2026-03-13T17:36:00+08:00">2026-03-13</time>
            <time datetime="2026-03-14T06:36:00+08:00">2026-03-14</time>
            <a href="https://example.com/tags/c%2B%2B/">C++</a><a href="https://example.com/tags/script/">&lt;script&gt;</a>"#}
    );
}

#[test]
fn build_numbers_headings_per_page() {
    let site = TestSite::new(indoc! {r#"
        base_url = "https://example.com"
        title = "Test Site"

        [params]
        heading_numbering = true
    "#});
    let pages = [
        (
            "numbered",
            "heading_numbering = true",
            "",
            "",
            Some(("1", "1.1")),
        ),
        (
            "continued",
            "heading_numbering = true",
            " {numbering-start=2}",
            "",
            Some(("2", "2.1")),
        ),
        (
            "child",
            "heading_numbering = true",
            " {numbering-start=2}",
            " {numbering-start=0}",
            Some(("2", "2.0")),
        ),
        (
            "another",
            "heading_numbering = true",
            "",
            "",
            Some(("1", "1.1")),
        ),
        (
            "disabled",
            "heading_numbering = false",
            " {numbering-start=bad}",
            "",
            None,
        ),
        ("default", "", " {numbering-start=bad}", "", None),
    ];
    for (slug, setting, root_attribute, child_attribute, _) in pages {
        site.page(
            &format!("posts/{slug}"),
            &formatdoc! {r#"
                +++
                title = "Post"
                {setting}
                +++
                ## Section{root_attribute}
                ### Detail{child_attribute}
            "#},
        );
    }

    site.build().unwrap();

    for (slug, _, _, _, numbers) in pages {
        let html = site.read(&format!("posts/{slug}/index.html"));
        if let Some((root, child)) = numbers {
            for (id, number, title, level) in [
                ("section", root, "Section", 2),
                ("detail", child, "Detail", 3),
            ] {
                assert!(html.contains(&format!(
                    r#"<h{level} id="{id}"><span class="heading-number">{number}</span> {title}</h{level}>"#
                )));
                assert!(html.contains(&format!(
                    r##"href="#{id}"><span class="heading-number">{number}</span> {title}</a>"##
                )));
            }
        } else {
            assert!(html.contains(r#"<h2 id="section">Section</h2>"#));
            assert!(!html.contains("heading-number"));
        }
    }
}

#[test]
fn build_localizes_and_escapes_code_copy_labels() {
    let site = TestSite::new(indoc! {r#"
        base_url = "https://example.com"
        language = "zh-Hans"
    "#});
    site.write(
        "i18n/zh-Hans.toml",
        indoc! {r#"
            copy = "复制"
            copy_code = '复制 "代码" <&>'
        "#},
    )
    .page(
        "posts/hello",
        indoc! {r#"
            +++
            title = "Hello"
            +++
            ```rust
            fn main() {}
            ```
        "#},
    );

    site.build().unwrap();

    let html = site.read("posts/hello/index.html");
    let button = r#"<button class="copy-btn" data-pagefind-ignore aria-label="复制 &quot;代码&quot; &lt;&amp;&gt;">复制</button>"#;
    assert!(html.contains(button), "{html}");
}

#[test]
fn build_rejects_invalid_render_options() {
    let site = TestSite::new(indoc! {r#"
        [params]
        emojis = "yes"
    "#});

    let err = site.build().unwrap_err();
    assert_eq!(
        err.to_string(),
        "failed to parse render options from [params]"
    );
    assert!(format!("{err:#}").contains("invalid type: string \"yes\", expected a boolean"));
    assert!(!site.path("public").exists());
}
