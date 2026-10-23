// Counterpart in sigrok:
//   libsigrok/src/output/vcd.c,
//   Copyright (C) 2010 Uwe Hermann <uwe@hermann-uwe.de>,
//   Copyright (C) 2013 Bert Vermeulen <bert@biot.com>,
//   Copyright (C) 2017-2020 Gerhard Sittig <gerhard.sittig@gmx.net>,
//   with commits by Daniel Elstner, Soeren Apel, Joel Holdsworth, Martin Ling
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! `vcd`: a Value Change Dump, readable by waveform viewers and by the `vcd`
//! input.
//!
//! Logic channels are `wire` signals, analog channels `real` signals.
//! The timescale is the smallest power of ten period that divides the
//! sample period, so sample numbers map to integer timestamps.

use super::{Output, OutputContext, OutputModule, meta_samplerate};
use oxrok_core::util::{fmt_g, samplerate_string};
use oxrok_core::{ChannelKind, Packet, Result};
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub(super) static MODULE: OutputModule = OutputModule {
    id: "vcd",
    name: "VCD",
    description: "Value Change Dump data",
    options: &[],
    extensions: &["vcd"],
    analog: true,
    create: |ctx, _options| Ok(Box::new(Vcd::new(ctx))),
};

struct Signal {
    index: usize,
    kind: ChannelKind,
    name: String,
    ident: String,
    last_bit: Option<bool>,
    last_real: Option<f32>,
    /// Samples received so far for this signal.
    received: u64,
}

struct Vcd {
    signals: Vec<Signal>,
    total_channels: usize,
    samplerate: u64,
    /// Timescale ticks per second.
    period: u64,
    header_done: bool,
    /// Pending value changes by sample number, written once every signal
    /// has passed that sample.
    queue: BTreeMap<u64, String>,
}

/// The VCD identifier for signal `idx`: `!`..`~`, then `aa`..`zz`, then
/// `aaa`..`zzz`.
fn identifier(mut idx: usize) -> String {
    const ONE: usize = (b'~' - b'!' + 1) as usize;
    if idx < ONE {
        return ((b'!' + idx as u8) as char).to_string();
    }
    idx -= ONE;
    let letter = |i: usize| (b'a' + (i % 26) as u8) as char;
    if idx < 26 * 26 {
        return format!("{}{}", letter(idx / 26), letter(idx));
    }
    idx -= 26 * 26;
    format!("{}{}{}", letter(idx / (26 * 26)), letter(idx / 26), letter(idx))
}

/// Ticks per second of the timescale: the smallest power of ten at or
/// above the sample rate that the rate divides, trying up to two more
/// decades.
fn timescale_freq(samplerate: u64) -> u64 {
    let rate = samplerate.max(1);
    let mut ts = 1u64;
    while ts < rate {
        ts *= 10;
    }
    for _ in 0..2 {
        if ts / rate * rate == ts {
            break;
        }
        ts *= 10;
    }
    ts
}

/// The `$timescale` text for `freq` ticks per second: `1 us`, `10 ns`.
fn timescale_text(freq: u64) -> String {
    oxrok_core::util::period_string(1, freq)
}

impl Vcd {
    fn new(ctx: &OutputContext) -> Self {
        let signals = ctx
            .channels
            .iter()
            .filter(|c| c.enabled)
            .enumerate()
            .map(|(i, c)| Signal {
                index: c.index,
                kind: c.kind,
                name: c.name.clone(),
                ident: identifier(i),
                last_bit: None,
                last_real: None,
                received: 0,
            })
            .collect();
        Vcd {
            signals,
            total_channels: ctx.channels.len(),
            samplerate: ctx.samplerate.unwrap_or(0),
            period: 0,
            header_done: false,
            queue: BTreeMap::new(),
        }
    }

    fn header(&mut self, ctx_date: &str) -> String {
        self.period = timescale_freq(self.samplerate);
        let mut s = String::new();
        let _ = writeln!(s, "$date {ctx_date} $end");
        let _ = writeln!(s, "$version oxrok {} $end", oxrok_core::VERSION);
        s.push_str("$comment\n");
        let _ =
            write!(s, "  Acquisition with {}/{} channels", self.signals.len(), self.total_channels);
        if self.samplerate != 0 {
            let _ = write!(s, " at {}", samplerate_string(self.samplerate));
        }
        s.push_str("\n$end\n");
        let _ = writeln!(s, "$timescale {} $end", timescale_text(self.period));
        s.push_str("$scope module oxrok $end\n");
        for sig in &self.signals {
            let (kind, size) = match sig.kind {
                ChannelKind::Logic => ("wire", "1"),
                ChannelKind::Analog => ("real", "64"),
            };
            let _ = writeln!(s, "$var {kind} {size} {} {} $end", sig.ident, sig.name);
        }
        // Every timestamp line starts with a newline, so the header's
        // final newline makes the blank line libsigrok 0.6 prints.
        s.push_str("$upscope $end\n$enddefinitions $end\n");
        s
    }

    fn timestamp(&self, snum: u64) -> String {
        let ts = snum as f64 / self.samplerate.max(1) as f64 * self.period as f64;
        format!("{ts:.0}")
    }

    fn queue_change(&mut self, snum: u64, text: &str) {
        let entry = self.queue.entry(snum).or_default();
        if !entry.is_empty() {
            entry.push(' ');
        }
        entry.push_str(text);
    }

    /// Writes the queued changes every signal has passed.
    fn write_completed(&mut self, out: &mut Vec<u8>) {
        let upto = self.signals.iter().map(|s| s.received).min().unwrap_or(0);
        while let Some((&snum, _)) = self.queue.first_key_value() {
            if snum >= upto {
                break;
            }
            let (snum, values) = self.queue.pop_first().unwrap_or_default();
            self.write_entry(snum, &values, out);
        }
    }

    fn write_entry(&self, snum: u64, values: &str, out: &mut Vec<u8>) {
        out.push(b'\n');
        out.push(b'#');
        out.extend_from_slice(self.timestamp(snum).as_bytes());
        if values.is_empty() {
            out.push(b'\n');
        } else {
            // Two spaces after the timestamp, as libsigrok 0.6 writes.
            out.extend_from_slice(b"  ");
            out.extend_from_slice(values.as_bytes());
        }
    }

    fn ensure_header(&mut self, out: &mut Vec<u8>) {
        if !self.header_done {
            self.header_done = true;
            let date = chrono::Local::now().format("%a %b %e %H:%M:%S %Y").to_string();
            let h = self.header(&date);
            out.extend_from_slice(h.as_bytes());
        }
    }
}

impl Output for Vcd {
    fn receive(&mut self, packet: &Packet, out: &mut Vec<u8>) -> Result<()> {
        match packet {
            Packet::Meta(_) => {
                if let Some(rate) = meta_samplerate(packet)
                    && !self.header_done
                {
                    self.samplerate = rate;
                }
            }
            Packet::Logic(logic) => {
                self.ensure_header(out);
                let first = self
                    .signals
                    .iter()
                    .find(|s| s.kind == ChannelKind::Logic)
                    .map_or(0, |s| s.received);
                for s in 0..logic.len() {
                    let snum = first + s as u64;
                    let mut changes = String::new();
                    for i in 0..self.signals.len() {
                        if self.signals[i].kind != ChannelKind::Logic {
                            continue;
                        }
                        let bit = logic.bit(s, self.signals[i].index);
                        if snum != 0 && self.signals[i].last_bit == Some(bit) {
                            continue;
                        }
                        self.signals[i].last_bit = Some(bit);
                        if !changes.is_empty() {
                            changes.push(' ');
                        }
                        changes.push(if bit { '1' } else { '0' });
                        changes.push_str(&self.signals[i].ident);
                    }
                    if !changes.is_empty() {
                        self.queue_change(snum, &changes);
                    }
                }
                for sig in &mut self.signals {
                    if sig.kind == ChannelKind::Logic {
                        sig.received += logic.len() as u64;
                    }
                }
                self.write_completed(out);
            }
            Packet::Analog(a) => {
                self.ensure_header(out);
                let n = a.channels.len();
                for (c, index) in a.channels.iter().enumerate() {
                    let Some(i) = self
                        .signals
                        .iter()
                        .position(|s| s.index == *index && s.kind == ChannelKind::Analog)
                    else {
                        continue;
                    };
                    let first = self.signals[i].received;
                    for s in 0..a.len() {
                        let value = a.samples[s * n + c];
                        let snum = first + s as u64;
                        if snum != 0 && self.signals[i].last_real == Some(value) {
                            continue;
                        }
                        self.signals[i].last_real = Some(value);
                        let text =
                            format!("r{} {}", fmt_g(f64::from(value), 16), self.signals[i].ident);
                        self.queue_change(snum, &text);
                    }
                    self.signals[i].received += a.len() as u64;
                }
                self.write_completed(out);
            }
            Packet::End => {
                self.ensure_header(out);
                let last = self.signals.iter().map(|s| s.received).max().unwrap_or(0);
                for sig in &mut self.signals {
                    sig.received = last + 1;
                }
                self.queue.entry(last).or_default();
                self.write_completed(out);
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxrok_core::Channel;

    #[test]
    fn identifiers_and_timescales() {
        assert_eq!(identifier(0), "!");
        assert_eq!(identifier(3), "$");
        assert_eq!(identifier(93), "~");
        assert_eq!(identifier(94), "aa");
        assert_eq!(identifier(95), "ab");
        assert_eq!(timescale_freq(200_000), 1_000_000);
        assert_eq!(timescale_freq(1_000_000), 1_000_000);
        // Like libsigrok: two more decades are tried, then it gives up.
        assert_eq!(timescale_freq(24_000_000), 10_000_000_000);
        assert_eq!(timescale_text(1_000_000), "1 us");
        assert_eq!(timescale_text(1_000_000_000), "1 ns");
        assert_eq!(timescale_text(10_000_000_000), "100 ps");
    }

    #[test]
    fn writes_changes_at_sample_timestamps() {
        let chans = [Channel::logic(0), Channel::logic(1)];
        let ctx =
            OutputContext { channels: &chans, samplerate: Some(200_000), ..Default::default() };
        let mut v = Vcd::new(&ctx);
        let mut out = Vec::new();
        v.receive(&Packet::logic(1, vec![0, 1, 1, 2]), &mut out).unwrap();
        v.receive(&Packet::End, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let body: String =
            text.lines().skip_while(|l| !l.starts_with('#')).collect::<Vec<_>>().join("\n");
        assert_eq!(body, "#0  0! 0\"\n#5  1!\n#15  0! 1\"\n#20");
        assert!(text.contains("$timescale 1 us $end\n$scope module oxrok $end\n$var wire 1 ! D0 $end\n$var wire 1 \" D1 $end\n$upscope $end\n$enddefinitions $end\n\n#0"));
    }
}
