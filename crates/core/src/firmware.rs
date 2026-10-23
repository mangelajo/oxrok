// Counterpart in sigrok:
//   libsigrok/src/resource.c,
//   Copyright (C) 2015 Daniel Elstner <daniel.kitta@gmail.com>,
//   with commits by Uwe Hermann, Soeren Apel, Gerhard Sittig
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! Finding firmware and bitstream files.
//!
//! Files are looked up in `OXROK_FIRMWARE_DIR` and `SIGROK_FIRMWARE_DIR`
//! (colon separated lists), then the user's and the system's data
//! directories, oxrok's first and sigrok's second, so an existing sigrok
//! installation keeps working.

use crate::error::{Error, Result};
use std::path::PathBuf;

/// The directories searched, in order.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for var in ["OXROK_FIRMWARE_DIR", "SIGROK_FIRMWARE_DIR"] {
        if let Ok(v) = std::env::var(var) {
            dirs.extend(std::env::split_paths(&v));
        }
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/share/oxrok-firmware"));
        dirs.push(home.join(".local/share/sigrok-firmware"));
    }
    for d in [
        "/usr/local/share/oxrok-firmware",
        "/usr/local/share/sigrok-firmware",
        "/opt/homebrew/share/sigrok-firmware",
        "/usr/share/oxrok-firmware",
        "/usr/share/sigrok-firmware",
    ] {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

/// The first directory holding `name`.
pub fn find_file(name: &str) -> Option<PathBuf> {
    search_dirs().into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// Reads `name` from the first directory that has it, or fails with
/// [`Error::Firmware`] naming the file and `hint`.
pub fn read(name: &str, hint: &str) -> Result<Vec<u8>> {
    match find_file(name) {
        Some(path) => {
            log::debug!("Loading {} from {}.", name, path.display());
            Ok(std::fs::read(path)?)
        }
        None => Err(Error::Firmware(format!("{name} ({hint})"))),
    }
}
