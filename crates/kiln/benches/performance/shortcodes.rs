use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput};

use kiln::render::emoji::replace_emojis;
use kiln::render::icon::replace_icons;

pub(super) fn benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("shortcodes");
    for (name, prefix) in [
        ("ordinary", "Example ".repeat(100)),
        ("colons", ": ".repeat(5_000)),
    ] {
        let emoji = format!("{prefix}:smile:");
        let icon = format!("{prefix}:(fas fa-link):");
        assert_eq!(replace_emojis(&emoji), format!("{prefix}\u{1f604}"));
        assert_eq!(
            replace_icons(&icon),
            format!(r#"{prefix}<i class="fas fa-link" aria-hidden="true"></i>"#)
        );

        for (kind, input, replace) in [
            ("emoji", &emoji, replace_emojis as fn(&str) -> String),
            ("icon", &icon, replace_icons as fn(&str) -> String),
        ] {
            group.throughput(Throughput::Bytes(input.len().try_into().unwrap()));
            group.bench_with_input(BenchmarkId::new(kind, name), input, |bencher, input| {
                bencher.iter(|| replace(black_box(input)));
            });
        }
    }
    group.finish();
}
