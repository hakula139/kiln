use std::hint::black_box;
use std::time::{Duration, Instant};

use kiln::render::emoji::replace_emojis;
use kiln::render::icon::replace_icons;

const SAMPLES: usize = 5;
const ITERATIONS: u32 = 10;

fn main() {
    let prefix = ": ".repeat(5_000);
    let emoji = format!("{prefix}:smile:");
    let icon = format!("{prefix}:(fas fa-link):");
    assert_eq!(replace_emojis(&emoji), format!("{prefix}\u{1f604}"));
    assert_eq!(
        replace_icons(&icon),
        format!(r#"{prefix}<i class="fas fa-link" aria-hidden="true"></i>"#)
    );

    benchmark("emoji", &emoji, replace_emojis);
    benchmark("icon", &icon, replace_icons);
}

fn benchmark(name: &str, input: &str, replace: fn(&str) -> String) {
    let mut samples = [Duration::ZERO; SAMPLES];
    for sample in &mut samples {
        let started = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(replace(black_box(input)));
        }
        *sample = started.elapsed() / ITERATIONS;
    }
    samples.sort_unstable();
    println!(
        "{name}: {} bytes, median {:.3} ms/call ({SAMPLES} samples of {ITERATIONS} calls)",
        input.len(),
        samples[SAMPLES / 2].as_secs_f64() * 1_000.0
    );
}
