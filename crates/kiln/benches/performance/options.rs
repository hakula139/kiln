use std::hint::black_box;

use criterion::{BenchmarkId, Criterion};

use kiln::render::RenderOptions;

/// Measures option extraction from prebuilt params tables with unrelated settings.
pub(super) fn benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("render_options");
    for count in [0, 1_000] {
        let mut params = toml::Table::new();
        params.insert("emojis".to_owned(), toml::Value::Boolean(true));
        for index in 0..count {
            params.insert(
                format!("setting_{index}"),
                format!("Example value {index}").into(),
            );
        }
        let expected = RenderOptions::from_params(&params).unwrap();
        assert!(expected.code_max_lines.is_none());
        assert!(expected.emojis);
        assert!(!expected.fontawesome);
        assert!(!expected.heading_numbering);
        assert!(expected.table_nowrap_width.is_none());
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &params,
            |bencher, params| {
                bencher.iter(|| RenderOptions::from_params(black_box(params)).unwrap());
            },
        );
    }
    group.finish();
}
