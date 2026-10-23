// Counterpart in sigrok:
//   libsigrok/src/input/binary.c,
//   Copyright (C) 2013 Bert Vermeulen <bert@biot.com>,
//   with commits by Uwe Hermann, Daniel Elstner, Gerhard Sittig, Soeren Apel,
//   Joel Holdsworth, Lars-Peter Clausen, Martin Ling, Stefan Brüns
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! `binary`: raw logic samples, `ceil(numchannels / 8)` bytes each.

use super::{Input, InputModule, InputOption, InputOptionKind};
use oxrok_core::{Channel, Error, Key, Packet, Result};

const CHUNK: usize = 4 * 1024 * 1024;

pub(super) static MODULE: InputModule = InputModule {
    id: "binary",
    name: "Binary",
    description: "Raw binary logic data",
    extensions: &[],
    options: &[
        InputOption {
            id: "numchannels",
            name: "Number of logic channels",
            description: "The number of (logic) channels in the data",
            kind: InputOptionKind::I32,
            default: "8",
        },
        InputOption {
            id: "samplerate",
            name: "Sample rate (Hz)",
            description: "The sample rate of the (logic) data in Hz",
            kind: InputOptionKind::U64,
            default: "0",
        },
    ],
    create: |options| {
        let n = super::option_u64(options, "numchannels", 8)?;
        if n == 0 || n > 512 {
            return Err(Error::arg("Invalid value for numchannels: must be 1 to 512."));
        }
        let samplerate = super::option_u64(options, "samplerate", 0)?;
        let channels = (0..n as usize)
            .map(|i| Channel::new(i, oxrok_core::ChannelKind::Logic, i.to_string()))
            .collect();
        Ok(Box::new(Binary {
            channels,
            samplerate,
            unit_size: (n as usize).div_ceil(8),
            started: false,
            pending: Vec::new(),
        }))
    },
    detect: |_head, _name| 0,
};

struct Binary {
    channels: Vec<Channel>,
    samplerate: u64,
    unit_size: usize,
    started: bool,
    pending: Vec<u8>,
}

impl Binary {
    fn start(&mut self, out: &mut Vec<Packet>) {
        if !self.started {
            self.started = true;
            out.push(Packet::header());
            if self.samplerate != 0 {
                out.push(Packet::meta(Key::Samplerate, self.samplerate));
            }
        }
    }

    fn flush(&mut self, out: &mut Vec<Packet>) {
        let whole = self.pending.len() / self.unit_size * self.unit_size;
        let data: Vec<u8> = self.pending.drain(..whole).collect();
        for chunk in data.chunks(CHUNK / self.unit_size * self.unit_size) {
            out.push(Packet::logic(self.unit_size, chunk.to_vec()));
        }
    }
}

impl Input for Binary {
    fn feed(&mut self, data: &[u8], out: &mut Vec<Packet>) -> Result<()> {
        self.start(out);
        self.pending.extend_from_slice(data);
        self.flush(out);
        Ok(())
    }

    fn end(&mut self, out: &mut Vec<Packet>) -> Result<()> {
        self.start(out);
        self.flush(out);
        out.push(Packet::End);
        Ok(())
    }

    fn channels(&self) -> Option<&[Channel]> {
        Some(&self.channels)
    }

    fn enable_channel(&mut self, pos: usize, enabled: bool) {
        if let Some(c) = self.channels.get_mut(pos) {
            c.enabled = enabled;
        }
    }
}
