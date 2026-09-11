//! Opt-in startup measurements, including events before the normal logger exists.
use std::{io::Write, sync::{Mutex, OnceLock}, time::Instant};

static START: OnceLock<Instant> = OnceLock::new();
static FILE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

pub fn mark(event: &str) {
    let start = START.get_or_init(Instant::now);
    let file = FILE.get_or_init(|| {
        std::env::var_os("MONAHUB_STARTUP_TRACE")
            .and_then(|path| std::fs::File::create(path).ok()).map(Mutex::new)
    });
    if let Some(file) = file {
        if let Ok(mut file) = file.lock() {
            let _ = writeln!(file, "[STARTUP pid={} +{:.3}ms thread={:?}] {}",
                std::process::id(), start.elapsed().as_secs_f64() * 1000.0,
                std::thread::current().id(), event);
        }
    }
}

pub fn enabled() -> bool { FILE.get().is_some_and(Option::is_some) }
