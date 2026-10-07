use std::hint::black_box;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use criterion::{BatchSize, Criterion};
use image::{Rgb, RgbImage};

use kiln::render::lqip::{ImageConfig, ImageResolver};

/// Measures fresh and primed image caches, excluding image creation and resolver construction.
pub(super) fn benchmarks(criterion: &mut Criterion) {
    let directory = tempfile::tempdir().unwrap();
    let image = RgbImage::from_fn(1024, 768, |x, y| {
        Rgb([
            u8::try_from(x % 256).unwrap(),
            u8::try_from(y % 256).unwrap(),
            u8::try_from((x + y) % 256).unwrap(),
        ])
    });
    image.save(directory.path().join("example.png")).unwrap();
    let resolver = ImageResolver::new(directory.path(), ImageConfig::default());
    let expected = resolver.resolve("/example.png", None).unwrap();
    assert_eq!((expected.width, expected.height), (1024, 768));
    let placeholder = expected
        .lqip_uri
        .as_ref()
        .unwrap()
        .strip_prefix("data:image/webp;base64,")
        .unwrap();
    let placeholder = image::load_from_memory(&STANDARD.decode(placeholder).unwrap()).unwrap();
    assert_eq!((placeholder.width(), placeholder.height()), (16, 12));
    assert_eq!(resolver.resolve("/example.png", None).unwrap(), expected);

    let mut group = criterion.benchmark_group("images");
    group.bench_function("cold", |bencher| {
        bencher.iter_batched(
            || ImageResolver::new(directory.path(), ImageConfig::default()),
            |resolver| resolver.resolve(black_box("/example.png"), None).unwrap(),
            BatchSize::SmallInput,
        );
    });
    group.bench_function("cached", |bencher| {
        bencher.iter(|| resolver.resolve(black_box("/example.png"), None).unwrap());
    });
    group.finish();
}
