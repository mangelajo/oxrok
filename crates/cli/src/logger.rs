// Counterpart in sigrok: libsigrok/src/log.c,
//   Copyright (C) 2011-2012 Uwe Hermann <uwe@hermann-uwe.de>.
// This file is an independent implementation written for oxrok from the
// observed behaviour and the public interfaces.

//! A small stderr logger driven by `-l`.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

/// The level selected with `-l`, as a `LevelFilter` discriminant.
static FILTER: AtomicU8 = AtomicU8::new(2);

/// Warnings and errors logged so far. Counted before the `-l` filter, so
/// `--strict` fails the run at any log level.
static WARNINGS: AtomicUsize = AtomicUsize::new(0);

struct Stderr;

fn filter() -> LevelFilter {
    match FILTER.load(Ordering::Relaxed) {
        0 => LevelFilter::Off,
        1 => LevelFilter::Error,
        2 => LevelFilter::Warn,
        3 => LevelFilter::Info,
        4 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    }
}

impl Log for Stderr {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= filter()
    }

    fn log(&self, record: &Record) {
        if record.level() <= Level::Warn {
            WARNINGS.fetch_add(1, Ordering::Relaxed);
        }
        // Up to -l 4 only oxrok's own messages are shown; -l 5 shows
        // the libraries' too.
        let ours = record.target().starts_with("oxrok");
        if self.enabled(record.metadata()) && (ours || filter() >= LevelFilter::Trace) {
            let level = match record.level() {
                Level::Error => "err",
                Level::Warn => "warn",
                Level::Info => "info",
                Level::Debug => "dbg",
                Level::Trace => "spew",
            };
            eprintln!("oxrok: {level}: {}: {}", record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

static LOGGER: Stderr = Stderr;

/// Installs the logger. `level` follows sigrok: 0 nothing, 1 errors,
/// 2 warnings, 3 info, 4 debug, 5 everything.
pub fn init(level: u8) {
    FILTER.store(level.min(5), Ordering::Relaxed);
    let _ = log::set_logger(&LOGGER);
    // The log macros drop records above the global max level before the
    // logger sees them. Keep warnings flowing so they are counted, and
    // apply the `-l` filter in `log`.
    log::set_max_level(filter().max(LevelFilter::Warn));
}

/// The number of warnings and errors logged so far.
pub fn warnings() -> usize {
    WARNINGS.load(Ordering::Relaxed)
}
