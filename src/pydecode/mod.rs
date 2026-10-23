// Counterpart in sigrok:
//   libsigrokdecode (srd.c, decoder.c, instance.c, type_decoder.c: the
//   Python side of the decoder API).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it, written from the documented decoder API and the observed behaviour.

//! Runs unchanged `pd.py` decoders from a libsigrokdecode installation.
//!
//! Each Python decoder becomes an [`crate::decode::Decoder`]. Two
//! interpreters can run them:
//!
//! - feature `python`: the system's CPython, through pyo3;
//! - feature `rustpython`: RustPython, built into the binary, so no
//!   Python installation is needed. About three times slower.
//!
//! With both features, CPython is used unless `OXROK_PYTHON=rustpython`.
//!
//! The decoders are found in `OXROK_DECODERS_DIR`, `SIGROKDECODE_DIR`,
//! a `decoders` directory next to the executable (as the release
//! archives ship them), or the usual installation directories.

#![allow(clippy::doc_markdown)]

use crate::core::{Error, Result};
use crate::decode::DecoderDef;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(feature = "python")]
mod cpython;
#[cfg(feature = "rustpython")]
mod rustpython;

#[cfg(feature = "python")]
pub use cpython::PyItem;
#[cfg(feature = "rustpython")]
pub use rustpython::RpItem;

const CONF_SAMPLERATE: i64 = 10000;

/// The decoders directory: `OXROK_DECODERS_DIR`, `SIGROKDECODE_DIR`,
/// `decoders` next to the executable, or the first of the usual
/// installation paths that exists.
pub fn decoders_dir() -> Option<PathBuf> {
    for var in ["OXROK_DECODERS_DIR", "SIGROKDECODE_DIR"] {
        if let Ok(v) = std::env::var(var) {
            let p = PathBuf::from(v);
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf))
    {
        candidates.push(dir.join("decoders"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".local/share/libsigrokdecode/decoders"));
    }
    for d in [
        "/opt/homebrew/share/libsigrokdecode/decoders",
        "/usr/local/share/libsigrokdecode/decoders",
        "/usr/share/libsigrokdecode/decoders",
    ] {
        candidates.push(PathBuf::from(d));
    }
    candidates.into_iter().find(|p| p.is_dir())
}

/// The Python decoders found on this system, as registry entries.
/// Scanned once; an empty list when no decoders directory exists.
pub fn defs() -> Vec<&'static DecoderDef> {
    static DEFS: OnceLock<Vec<&'static DecoderDef>> = OnceLock::new();
    DEFS.get_or_init(|| match scan() {
        Ok(d) => d,
        Err(e) => {
            log::debug!("python decoders unavailable: {e}");
            Vec::new()
        }
    })
    .clone()
}

fn scan() -> Result<Vec<&'static DecoderDef>> {
    let dir = decoders_dir()
        .ok_or_else(|| Error::not_supported("no libsigrokdecode decoders directory"))?;
    let mut names: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().join("pd.py").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    scan_with(&dir, names)
}

#[cfg(all(feature = "python", feature = "rustpython"))]
fn scan_with(dir: &Path, names: Vec<String>) -> Result<Vec<&'static DecoderDef>> {
    if std::env::var("OXROK_PYTHON").is_ok_and(|v| v == "rustpython") {
        rustpython::scan(dir, names)
    } else {
        cpython::scan(dir, names)
    }
}

#[cfg(all(feature = "python", not(feature = "rustpython")))]
fn scan_with(dir: &Path, names: Vec<String>) -> Result<Vec<&'static DecoderDef>> {
    cpython::scan(dir, names)
}

#[cfg(all(feature = "rustpython", not(feature = "python")))]
fn scan_with(dir: &Path, names: Vec<String>) -> Result<Vec<&'static DecoderDef>> {
    rustpython::scan(dir, names)
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}
