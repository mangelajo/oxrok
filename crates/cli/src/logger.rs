//! A small stderr logger driven by `-l`.

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Stderr;

impl Log for Stderr {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
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
    let filter = match level {
        0 => LevelFilter::Off,
        1 => LevelFilter::Error,
        2 => LevelFilter::Warn,
        3 => LevelFilter::Info,
        4 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(filter);
}
