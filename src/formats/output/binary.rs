// Counterpart in sigrok:
//   libsigrok/src/output/binary.c.
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like it.

//! `binary`: the raw logic samples, nothing else.

use super::{Output, OutputModule};
use crate::core::{Packet, Result};

pub(super) static MODULE: OutputModule = OutputModule {
    id: "binary",
    name: "Binary",
    description: "Raw binary logic data",
    options: &[],
    extensions: &[],
    analog: false,
    create: |_ctx, _options| Ok(Box::new(Binary)),
};

struct Binary;

impl Output for Binary {
    fn receive(&mut self, packet: &Packet, out: &mut Vec<u8>) -> Result<()> {
        if let Packet::Logic(l) = packet {
            out.extend_from_slice(&l.data);
        }
        Ok(())
    }
}
