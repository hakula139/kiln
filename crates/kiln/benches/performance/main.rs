#![expect(clippy::unwrap_used, reason = "benchmark failures invalidate the run")]

mod build;
mod fixtures;
#[path = "../../tests/support/git.rs"]
mod git;
mod images;
mod options;
mod render;
mod shortcodes;
mod stylesheets;

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
