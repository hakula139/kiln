use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput};
use indoc::indoc;
use scraper::{Html, Selector};

use kiln::config::Config;
use kiln::i18n::I18n;
use kiln::render::lqip::ImageResolver;
use kiln::render::pipeline::render_page;
use kiln::render::{PageResources, RenderOptions};
use kiln::static_assets::StaticAssetManifest;
use kiln::template::TemplateEngine;

use super::fixtures::{PROSE, Site};

/// Measures rendering with preloaded dependencies and separately times fresh syntax loading.
pub(super) fn benchmarks(criterion: &mut Criterion) {
    let site = Site::new(0, PROSE);
    let config = Config::load(site.root()).unwrap();
    let i18n = I18n::load(site.root(), None, "en").unwrap();
    let engine = TemplateEngine::new(Some(&site.root().join("templates")), None, &i18n).unwrap();
    let options = RenderOptions::default();
    let resolver = ImageResolver::new(&site.root().join("static"), config.image.clone());
    let manifest = StaticAssetManifest::default();
    let resources = PageResources {
        source_dir: None,
        images: &resolver,
        assets: &manifest,
        page_url: "/",
        deployment_prefix: "",
    };
    let syntax_set = two_face::syntax::extra_newlines();
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
        let expected =
            render_page(&input, &syntax_set, &engine, &config, &options, &resources).unwrap();
        verify_render(name, &expected.content_html);
        group.throughput(Throughput::Bytes(input.len().try_into().unwrap()));
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &input,
            |bencher, input| {
                bencher.iter(|| {
                    render_page(
                        black_box(input),
                        &syntax_set,
                        &engine,
                        &config,
                        &options,
                        &resources,
                    )
                    .unwrap()
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
            "code[data-lang=rust]",
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
