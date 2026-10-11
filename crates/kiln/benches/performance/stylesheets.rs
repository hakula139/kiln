use std::fs;
use std::time::Duration;

use criterion::{Criterion, SamplingMode};
use indoc::formatdoc;

use super::build::{build_command, run_build};
use super::fixtures::{PROSE, Site};

/// Number of page bundles that each own a Tailwind stylesheet beside the shared one.
const PAGE_STYLES: usize = 2;

/// Builds a site whose shared and page stylesheets each start one Tailwind compiler.
pub(super) fn benchmarks(criterion: &mut Criterion) {
    let site = Site::new(0, PROSE, 0);
    let root = site.root();
    let config = fs::read_to_string(root.join("config.toml")).unwrap();
    fs::write(
        root.join("config.toml"),
        formatdoc! {r#"
            {config}
            [css]
            processor = "tailwind"
        "#},
    )
    .unwrap();
    fs::create_dir_all(root.join("assets/css/_src")).unwrap();
    fs::write(
        root.join("assets/css/_src/style.css"),
        r#"@import "tailwindcss";"#,
    )
    .unwrap();
    for index in 0..PAGE_STYLES {
        let bundle = root.join(format!("content/posts/notes/styled-{index}"));
        fs::create_dir_all(bundle.join("assets/css/_src")).unwrap();
        fs::write(
            bundle.join("index.md"),
            formatdoc! {r#"
                +++
                title = "Styled {index}"
                +++
                ::: {{.italic}}
                {PROSE}
                :::
            "#},
        )
        .unwrap();
        fs::write(
            bundle.join("assets/css/_src/style.css"),
            format!(".styled-{index} {{ @apply font-bold underline; }}"),
        )
        .unwrap();
    }
    verify_build(&site);

    let mut group = criterion.benchmark_group("stylesheets");
    group.sample_size(10);
    group.sampling_mode(SamplingMode::Flat);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
    group.bench_function("tailwind", |bencher| {
        bencher.iter(|| run_build(&site, false));
    });
    group.finish();
}

fn verify_build(site: &Site) {
    let result = build_command(site, false).output().unwrap();
    assert!(
        result.status.success(),
        "Tailwind benchmark site build failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = site.root().join("public");
    let shared = fs::read_to_string(output.join("assets/css/site.css")).unwrap();
    assert!(shared.contains(".italic"), "{shared}");
    for index in 0..PAGE_STYLES {
        let page = fs::read_to_string(
            output.join(format!("posts/notes/styled-{index}/assets/css/page.css")),
        )
        .unwrap();
        assert!(
            page.contains(&format!(".styled-{index}")) && page.contains("text-decoration-line"),
            "{page}"
        );
    }
}
