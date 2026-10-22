//! Native protocol decoders. Each one reproduces the annotation
//! classes, rows and texts of the libsigrokdecode decoder of the same
//! id, so `-A` selections and existing scripts keep working, and is
//! written from the protocol itself.

#![forbid(unsafe_code)]

pub mod uart;

use oxrok_decode::DecoderDef;

/// Every decoder in this crate.
pub static ALL: [&DecoderDef; 1] = [&uart::DEF];
