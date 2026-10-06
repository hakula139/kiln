#[path = "performance/build.rs"]
mod build;
#[path = "performance/fixtures.rs"]
mod fixtures;
#[path = "performance/images.rs"]
mod images;
#[path = "performance/options.rs"]
mod options;
#[path = "performance/render.rs"]
mod render;
#[path = "performance/shortcodes.rs"]
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
