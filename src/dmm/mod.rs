// Counterpart in sigrok:
//   libsigrok/src/dmm/*.c (one module per chipset, named below),
//   libsigrok/src/libsigrok-internal.h (the packet sizes and flags),
//   Copyright (C) 2012-2014 Uwe Hermann <uwe@hermann-uwe.de> and the
//   authors named in each module
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! Multimeter packet parsers.
//!
//! A handheld multimeter with a serial cable sends its display as a
//! fixed size packet, a few times per second. Each chipset has its own
//! layout; the parsers here turn one packet into one or more
//! [`Reading`]s. A [`Protocol`] bundles a parser with its packet size,
//! so the serial multimeter driver can find packets in a byte stream
//! and feed them through without knowing the chipset.
//!
//! All parsers are pure functions of the packet bytes.

#![forbid(unsafe_code)]
#![allow(
    clippy::struct_excessive_bools,
    clippy::too_many_lines,
    clippy::doc_markdown,
    clippy::match_same_arms
)]

pub mod ascii;
pub mod brymen;
pub mod eev121gw;
pub mod lcd;
pub mod mastech;
pub mod meterman;
pub mod ranged;
pub mod segments;

use crate::core::{Mq, MqFlags, Unit};

/// One measurement from a packet.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// The display it came from, 0 for the main one.
    pub channel: usize,
    /// The value in `unit`; infinite when over range, NaN when the
    /// display showed no number.
    pub value: f32,
    /// What was measured.
    pub mq: Mq,
    /// Its unit.
    pub unit: Unit,
    /// How it was measured.
    pub flags: MqFlags,
    /// Significant digits after the decimal point.
    pub digits: i32,
}

impl Reading {
    /// A reading on display 0 with no flags.
    pub fn new(value: f32, mq: Mq, unit: Unit, digits: i32) -> Reading {
        Reading { channel: 0, value, mq, unit, flags: MqFlags::empty(), digits }
    }
}

/// What a framing function says about the bytes at hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validity {
    /// Not enough bytes yet to decide.
    Need,
    /// No packet starts here.
    Invalid,
    /// A packet of this many bytes starts here.
    Valid(usize),
}

/// A chipset protocol: how to recognise and parse its packets.
#[derive(Clone, Copy)]
pub struct Protocol {
    /// Bytes per packet, or the shortest packet when `frame` is set.
    pub packet_size: usize,
    /// Frames a variable length packet at the start of a buffer; when
    /// unset, packets are `packet_size` bytes and `valid` decides.
    pub frame: Option<fn(&[u8]) -> Validity>,
    /// Displays per packet, each a channel.
    pub channels: usize,
    /// Channel names, or empty for `P1`, `P2`, ...
    pub channel_names: &'static [&'static str],
    /// True when the bytes form a packet of this protocol.
    pub valid: fn(&[u8]) -> bool,
    /// The readings in a valid packet; empty when it holds none.
    pub parse: fn(&[u8]) -> Vec<Reading>,
    /// Bytes to send to ask the meter for a packet, if it needs that.
    pub request: Option<&'static [u8]>,
}

impl Protocol {
    /// Frames the packet at the start of `buf`: fixed size with the
    /// validity check, or the protocol's own framing.
    pub fn frame_at(&self, buf: &[u8]) -> Validity {
        if let Some(f) = self.frame {
            return f(buf);
        }
        if buf.len() < self.packet_size {
            Validity::Need
        } else if (self.valid)(&buf[..self.packet_size]) {
            Validity::Valid(self.packet_size)
        } else {
            Validity::Invalid
        }
    }

    /// Finds the first packet in `buf`: its start and length, or `None`
    /// when more bytes are needed. Leading bytes that start no packet
    /// are skipped.
    pub fn find_packet(&self, buf: &[u8]) -> Option<(usize, usize)> {
        let mut pos = 0;
        while pos < buf.len() {
            match self.frame_at(&buf[pos..]) {
                Validity::Need => return None,
                Validity::Invalid => pos += 1,
                Validity::Valid(n) => return Some((pos, n)),
            }
        }
        None
    }
}

/// Scales `value` by `10^exponent` and returns the digits that go with
/// it (`-exponent`).
fn scaled(value: f32, exponent: i32) -> (f32, i32) {
    (value * 10f32.powi(exponent), -exponent)
}

/// A unit prefix as an exponent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Prefix {
    pico: bool,
    nano: bool,
    micro: bool,
    milli: bool,
    kilo: bool,
    mega: bool,
}

impl Prefix {
    fn exponent(self) -> i32 {
        let mut e = 0;
        if self.pico {
            e -= 12;
        }
        if self.nano {
            e -= 9;
        }
        if self.micro {
            e -= 6;
        }
        if self.milli {
            e -= 3;
        }
        if self.kilo {
            e += 3;
        }
        if self.mega {
            e += 6;
        }
        e
    }

    /// True when at most one prefix is set.
    fn is_single(self) -> bool {
        [self.pico, self.nano, self.micro, self.milli, self.kilo, self.mega]
            .iter()
            .filter(|b| **b)
            .count()
            <= 1
    }
}

/// Counts the true values, for "exactly one mode" checks.
fn count(flags: &[bool]) -> usize {
    flags.iter().filter(|b| **b).count()
}

#[cfg(test)]
pub(crate) fn hexbytes(text: &str) -> Vec<u8> {
    text.split_whitespace().map(|h| u8::from_str_radix(h, 16).unwrap()).collect()
}
