mod build;
mod fixtures;
mod images;
mod options;
mod render;
mod shortcodes;

use criterion::{criterion_group, criterion_main};

criterion_group!(
    benches,
    shortcodes::benchmarks,
    options::benchmarks,
    render::benchmarks,
    images::benchmarks,
    build::benchmarks,
);
criterion_main!(benches);
