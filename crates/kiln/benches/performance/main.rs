#![expect(clippy::unwrap_used, reason = "benchmark setup panics on failure")]

mod build;
mod fixtures;
mod images;
mod options;
mod render;
mod shortcodes;
mod stylesheets;
#[path = "../../tests/support/fixtures.rs"]
#[expect(dead_code, reason = "benchmarks use only the Git helpers")]
mod test_fixtures;

use criterion::{criterion_group, criterion_main};

criterion_group!(
    benches,
    shortcodes::benchmarks,
    options::benchmarks,
    render::benchmarks,
    images::benchmarks,
    build::benchmarks,
    stylesheets::benchmarks,
);
criterion_main!(benches);
