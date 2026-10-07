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

#[path = "performance/workloads.rs"]
mod workloads;

use criterion::{Criterion, criterion_group, criterion_main};

criterion_group! {
    name = benches;
    config = benchmark_configuration();
    targets =
        shortcodes::benchmarks,
        options::benchmarks,
        render::benchmarks,
        images::benchmarks,
        build::benchmarks,
}
criterion_main!(benches);

fn benchmark_configuration() -> Criterion {
    if let Some(path) = std::env::var_os("KILN_BENCH_MANIFEST") {
        std::fs::write(path, "").unwrap();
    }

    let criterion = Criterion::default();
    if std::env::var_os("CRITERION_HOME").is_some() {
        return criterion;
    }

    // Cargo runs benchmark executables from <target>/<profile>/deps/.
    let executable = std::env::current_exe().unwrap();
    let target_dir = executable.ancestors().nth(3).unwrap();
    criterion.output_directory(&target_dir.join("criterion"))
}
