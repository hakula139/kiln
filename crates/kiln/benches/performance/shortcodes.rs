use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput};

use kiln::config::Config;
use kiln::i18n::{I18n, Language};
use kiln::render::lqip::ImageResolver;
use kiln::render::pipeline::render_page;
use kiln::render::{PageResources, RenderOptions};
use kiln::static_assets::StaticAssetManifest;
use kiln::template::TemplateEngine;

use super::fixtures::{PROSE, Site};

/// Measures page rendering with shortcode expansion for ordinary text and unmatched colon prefixes.
pub(super) fn benchmarks(criterion: &mut Criterion) {
    let site = Site::new(0, PROSE, 0);
    let config = Config::load(site.root()).unwrap();
    let i18n = I18n::load(site.root(), None, &Language::default()).unwrap();
    let engine = TemplateEngine::new(Some(&site.root().join("templates")), None, &i18n).unwrap();
    let options = RenderOptions {
        emojis: true,
        fontawesome: true,
        ..RenderOptions::default()
    };
    let resolver = ImageResolver::new(&site.root().join("static"), config.image.clone());
    let manifest = StaticAssetManifest::default();
    let resources = PageResources {
        source_dir: None,
        images: &resolver,
        assets: &manifest,
        page_url: "/",
        deployment_prefix: "/",
    };
    let mut group = criterion.benchmark_group("shortcodes");
    for (name, prefix) in [
        ("ordinary", "Example ".repeat(100)),
        ("colons", ": ".repeat(5_000)),
    ] {
        let input = format!("{prefix}:smile: :(fas fa-link):");
        let page = render_page(&input, &engine, &config, &options, &resources, false).unwrap();
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
                    render_page(
                        black_box(input),
                        &engine,
                        &config,
                        &options,
                        &resources,
                        false,
                    )
                    .unwrap()
                });
            },
        );
    }
    group.finish();
}
