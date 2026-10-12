use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput};

use kiln::render::RenderOptions;
use kiln::render::pipeline::render_page;

use super::fixtures::RenderFixture;

/// Measures page rendering with shortcode expansion for ordinary text and unmatched colon prefixes.
pub(super) fn benchmarks(criterion: &mut Criterion) {
    let fixture = RenderFixture::new();
    let engine = fixture.engine();
    let options = RenderOptions {
        emojis: true,
        fontawesome: true,
        ..RenderOptions::default()
    };
    let resources = fixture.resources();
    let mut group = criterion.benchmark_group("shortcodes");
    for (name, prefix) in [
        ("ordinary", "Example ".repeat(100)),
        ("colons", ": ".repeat(5_000)),
    ] {
        let input = format!("{prefix}:smile: :(fas fa-link):");
        let page = render_page(&input, engine, &options, &resources, false).unwrap();
        assert_eq!(
            page.content_html,
            format!(
                "<p>{prefix}\u{1f604} <i class=\"fas fa-link\" aria-hidden=\"true\"></i></p>\n"
            )
        );
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
}
