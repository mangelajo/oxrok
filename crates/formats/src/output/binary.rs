// Counterpart in sigrok:
//   libsigrok/src/output/binary.c,
//   Copyright (C) 2010 Uwe Hermann <uwe@hermann-uwe.de>,
//   with commits by Bert Vermeulen, Daniel Elstner, Gerhard Sittig,
//   Joel Holdsworth, Soeren Apel
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! `binary`: the raw logic samples, nothing else.

use super::{Output, OutputModule};
use oxrok_core::{Packet, Result};

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
