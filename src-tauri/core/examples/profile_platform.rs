//! Read-only local timings. Does not launch the desktop app or change its data.
use spice_route_core::platform;
use std::time::{Duration, Instant};

fn measure(mut action: impl FnMut(), name: &str, repeats: usize) {
    let mut samples = Vec::new();
    for _ in 0..repeats {
        let start = Instant::now();
        action();
        samples.push(start.elapsed());
    }
    let total: Duration = samples.iter().sum();
    samples.sort();
    println!(
        "{name}: {repeats} samples; min {:.2} ms, median {:.2} ms, max {:.2} ms, mean {:.2} ms",
        samples[0].as_secs_f64() * 1000.0,
        samples[samples.len() / 2].as_secs_f64() * 1000.0,
        samples[samples.len() - 1].as_secs_f64() * 1000.0,
        total.as_secs_f64() * 1000.0 / repeats as f64,
    );
}

fn main() {
    let executable = platform::find_codex_executable();
    println!("Installed Codex runtime found: {}", executable.is_some());
    measure(
        || {
            std::hint::black_box(platform::find_codex_executable());
        },
        "Runtime lookup",
        5,
    );
    measure(
        || {
            std::hint::black_box(platform::codex_version(executable.as_deref()));
        },
        "Runtime --version",
        5,
    );
    measure(
        || {
            std::hint::black_box(platform::codex_running());
        },
        "Writer inventory",
        20,
    );
}
