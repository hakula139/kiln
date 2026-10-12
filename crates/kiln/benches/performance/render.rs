use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput};
use indoc::indoc;
use scraper::{Html, Selector};

use kiln::render::RenderOptions;
use kiln::render::pipeline::render_page;

use super::fixtures::{PROSE, RenderFixture};

/// Measures rendering with preloaded dependencies and separately times fresh syntax loading.
pub(super) fn benchmarks(criterion: &mut Criterion) {
    let fixture = RenderFixture::new();
    let engine = fixture.engine();
    let options = RenderOptions::default();
    let resources = fixture.resources();
    let code = indoc! {r#"
        ```rust
        fn main() {
            println!("Example");
        }
        ```
    "#}
    .repeat(20);
    let mut group = criterion.benchmark_group("render");
    for (name, input) in [("prose", PROSE.repeat(20)), ("highlighted_code", code)] {
        let expected = render_page(&input, engine, &options, &resources, false).unwrap();
        verify_render(name, &expected.content_html);
        group.throughput(Throughput::Bytes(input.len().try_into().unwrap()));
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &input,
            |bencher, input| {
                bencher.iter(|| {
                    render_page(black_box(input), engine, &options, &resources, false).unwrap()
                });
            },
        );
    }
    group.finish();

    criterion.bench_function("syntax/load", |bencher| {
        bencher.iter(two_face::syntax::extra_newlines);
    });
}

fn verify_render(name: &str, html: &str) {
    let document = Html::parse_fragment(html);
    let (selector, expected) = if name == "prose" {
        ("h2", "Benchmark heading")
    } else {
        (
            "[data-lang=rust] code",
            indoc! {r#"
                fn main() {
                    println!("Example");
                }
            "#},
        )
    };
    let selector = Selector::parse(selector).unwrap();
    let text: Vec<String> = document
        .select(&selector)
        .map(|item| item.text().collect())
        .collect();
    assert_eq!(text, vec![expected; 20]);
    if name == "prose" {
        let selector = Selector::parse("a").unwrap();
        let links: Vec<_> = document
            .select(&selector)
            .map(|item| item.value().attr("href"))
            .collect();
        assert_eq!(links, vec![Some("https://example.com"); 20]);
    } else {
        assert!(
            document
                .select(&Selector::parse("code span").unwrap())
                .next()
                .is_some()
        );
    }
}
