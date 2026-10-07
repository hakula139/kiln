use std::fmt::Write;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, SamplingMode, Throughput};
use scraper::{Html, Selector};
use walkdir::WalkDir;

use kiln::feed::DEFAULT_FEED_LIMIT;

use super::fixtures::{PROSE, Site, extra_params};

/// Includes CLI startup and prior-output cleanup, with fixture validation outside sampling.
pub(super) fn benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("build");
    group.sample_size(10);
    group.sampling_mode(SamplingMode::Flat);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));
    for (pages, minify, extra_params) in [
        (20_usize, false, 0),
        (200, false, 0),
        (200, false, 1_000),
        (200, true, 0),
    ] {
        let site = Site::new(pages, PROSE);
        if extra_params > 0 {
            add_params(&site, extra_params);
        }
        verify_build(&site, pages, minify);
        let mode = if minify {
            "minified"
        } else if extra_params > 0 {
            "large_params"
        } else {
            "default"
        };
        group.throughput(Throughput::Elements(pages.try_into().unwrap()));
        group.bench_with_input(BenchmarkId::new(mode, pages), &site, |bencher, site| {
            bencher.iter(|| run_build(site, minify));
        });
    }
    group.finish();
}

fn add_params(site: &Site, count: usize) {
    let path = site.root().join("config.toml");
    let mut config = fs::read_to_string(&path).unwrap();
    for (name, value) in extra_params(count) {
        _ = writeln!(config, "{name} = {value}");
    }
    fs::write(path, config).unwrap();
}

fn verify_build(site: &Site, pages: usize, minify: bool) {
    let result = build_command(site, minify).output().unwrap();
    assert!(
        result.status.success(),
        "benchmark site build failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = site.root().join("public");
    for index in 0..pages {
        let html = fs::read_to_string(output.join(format!("posts/notes/post-{index}/index.html")))
            .unwrap();
        assert!(html.contains(&format!("<h1>Post {index}</h1>")));
        assert!(html.contains("Benchmark paragraph with"));
    }
    let actual_pages = WalkDir::new(&output)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "html")
        })
        .count();
    let expected_pages = pages + 4 * pages.div_ceil(10) + 4 * (pages / 4).div_ceil(10) + 3;
    assert_eq!(actual_pages, expected_pages);
    verify_listings(&output, pages);
    let feed = fs::read_to_string(output.join("index.xml")).unwrap();
    assert_eq!(
        feed.matches("<item>").count(),
        pages.min(DEFAULT_FEED_LIMIT)
    );
    assert!(feed.contains("<title>Post 0</title>"));
    let css = fs::read_to_string(output.join("style.css")).unwrap();
    let script = fs::read_to_string(output.join("script.js")).unwrap();
    if minify {
        assert_eq!(css, "body{color:#123456}");
        assert_eq!(script, "const value=42;");
    } else {
        assert_eq!(css, "body { color: #123456; }\n");
        assert_eq!(script, "const value = 42;\n");
    }
}

fn verify_listings(output: &Path, pages: usize) {
    let mut expected_posts: Vec<_> = (0..pages)
        .map(|index| {
            (
                format!("Post {index}"),
                format!("https://example.com/posts/notes/post-{index}/"),
            )
        })
        .collect();
    expected_posts.sort_by(|a, b| a.1.cmp(&b.1));
    expected_posts.truncate(10);

    let selector = Selector::parse("a").unwrap();
    for path in [
        "index.html",
        "posts/index.html",
        "posts/notes/index.html",
        "tags/common/index.html",
    ] {
        let html = fs::read_to_string(output.join(path)).unwrap();
        let document = Html::parse_document(&html);
        let posts: Vec<_> = document
            .select(&selector)
            .map(|post| {
                (
                    post.text().collect::<String>(),
                    post.value().attr("href").unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(posts, expected_posts, "{path}");
    }
    let html = fs::read_to_string(output.join("sections/index.html")).unwrap();
    let document = Html::parse_document(&html);
    let sections: Vec<_> = document.select(&selector).collect();
    assert_eq!(sections.len(), 1);
    assert_eq!(
        sections[0].value().attr("href"),
        Some("https://example.com/posts/notes/")
    );
}

fn run_build(site: &Site, minify: bool) {
    let status = build_command(site, minify)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "benchmark site build failed: {status}");
}

fn build_command(site: &Site, minify: bool) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kiln"));
    command.arg("build").arg("--root").arg(site.root());
    if minify {
        command.arg("--minify");
    }
    command
}
