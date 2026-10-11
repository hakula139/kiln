use std::hint::black_box;

use criterion::{BenchmarkId, Criterion};

use kiln::render::RenderOptions;

use super::fixtures::extra_params;

/// Measures option extraction from prebuilt params tables with unrelated settings.
pub(super) fn benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("render_options");
    for count in [0, 1_000] {
        let mut params = toml::Table::new();
        params.insert("emojis".to_owned(), toml::Value::Boolean(true));
        params.extend(extra_params(count));
        assert!(RenderOptions::from_params(&params).unwrap().emojis);
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
