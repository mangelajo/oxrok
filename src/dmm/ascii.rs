// Counterpart in sigrok:
//   libsigrok/src/dmm/metex14.c,
//   libsigrok/src/dmm/vc96.c,
//   libsigrok/src/dmm/asycii.c,
//   libsigrok/src/dmm/m2110.c,
//   Copyright (C) 2012-2013 Uwe Hermann <uwe@hermann-uwe.de>,
//   Copyright (C) 2014 Matthias Heidbrink <m-sigrok@heidbrink.biz>,
//   Copyright (C) 2018 Gerhard Sittig <gerhard.sittig@gmx.net>,
//   Copyright (C) 2020 Rainer Schmid <rschmid@csl.ch>,
//   with commits by Bert Vermeulen, Daniel Elstner, Martin Ling,
//   Aurelien Jacobs, Frank Stettner, Soeren Apel
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! Text protocols: the Metex 14 byte format and the Voltcraft VC-96
//! and ASYC-II variants of it, and the one line BBC Goertz M2110.

use crate::core::{Mq, MqFlags, Unit};
use crate::dmm::{Prefix, Reading, count};

/// Metex 14 byte packets.
pub const METEX14_SIZE: usize = 14;
/// The byte that asks a Metex meter for a packet.
pub const METEX14_REQUEST: &[u8] = b"D";
/// VC-96: 13 bytes.
pub const VC96_SIZE: usize = 13;
/// ASYC-II: 16 bytes.
pub const ASYCII_SIZE: usize = 16;
/// M2110: 9 bytes.
pub const M2110_SIZE: usize = 9;

/// The characters of `bytes` without spaces.
fn squeeze(bytes: &[u8]) -> String {
    bytes.iter().filter(|b| **b != b' ').map(|b| char::from(*b)).collect()
}

fn is_over_limit(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    ["OL", ".OL", "O.L", "OL."].iter().any(|o| s.eq_ignore_ascii_case(o))
}

/// Applies a unit prefix to a number that already has its decimal
/// point: the value scales by the prefix, the digits count both.
fn prefixed(value: f32, exponent: i32, prefix: Prefix) -> (f32, i32) {
    let pe = prefix.exponent();
    (value * 10f32.powi(pe), -(exponent + pe))
}

/// A decimal number and its exponent from the digits after the point.
fn number(s: &str) -> Option<(f32, i32)> {
    let v: f32 = s.parse().ok()?;
    let exponent = s.find('.').map_or(0, |dot| -((s.len() - dot - 1) as i32));
    Some((v, exponent))
}

#[derive(Clone, Copy, Debug, Default)]
struct Metex14Flags {
    prefix: Prefix,
    ac: bool,
    dc: bool,
    ampere: bool,
    volt: bool,
    ohm: bool,
    farad: bool,
    hertz: bool,
    celsius: bool,
    fahrenheit: bool,
    decibel: bool,
    decibel_mw: bool,
    watt: bool,
    unitless: bool,
    resistance: bool,
    capacity: bool,
    temperature: bool,
    diode: bool,
    frequency: bool,
    gain: bool,
    power: bool,
    power_factor: bool,
    hfe: bool,
    logic: bool,
    min: bool,
    max: bool,
    avg: bool,
}

fn metex14_flags(buf: &[u8], logic: bool) -> Metex14Flags {
    let mode = &buf[0..2];
    let mut f = Metex14Flags { logic, ..Default::default() };
    f.ac = mode == b"AC";
    f.dc = mode == b"DC";
    let unit = squeeze(&buf[9..13]);
    let p = &mut f.prefix;
    match unit.to_ascii_lowercase().as_str() {
        "a" => f.ampere = true,
        "ma" => (p.milli, f.ampere) = (true, true),
        "ua" => (p.micro, f.ampere) = (true, true),
        "v" => f.volt = true,
        "mv" => (p.milli, f.volt) = (true, true),
        "ohm" => f.ohm = true,
        "kohm" => (p.kilo, f.ohm) = (true, true),
        "mohm" => (p.mega, f.ohm) = (true, true),
        "pf" => (p.pico, f.farad) = (true, true),
        "nf" => (p.nano, f.farad) = (true, true),
        "uf" => (p.micro, f.farad) = (true, true),
        "khz" => (p.kilo, f.hertz) = (true, true),
        "c" => f.celsius = true,
        "f" => f.fahrenheit = true,
        "db" => f.decibel = true,
        "dbm" => f.decibel_mw = true,
        "w" => f.watt = true,
        "" => f.unitless = true,
        _ => {}
    }
    let blank = mode == b"  ";
    f.resistance = mode == b"OH" || (blank && f.ohm);
    f.capacity = mode == b"CA" || (blank && f.farad);
    f.temperature = mode == b"TE" || f.celsius || f.fahrenheit;
    f.diode = mode == b"DI" || (blank && f.volt && p.milli);
    f.frequency = mode == b"FR" || (blank && f.hertz);
    f.gain = mode == b"DB" && f.decibel;
    f.power = (mode == b"dB" && f.decibel_mw) || (mode == b"WT" && f.watt);
    f.power_factor = mode == b"CO" && f.unitless;
    f.hfe = mode == b"HF"
        || (blank
            && !f.ampere
            && !f.volt
            && !f.resistance
            && !f.capacity
            && !f.frequency
            && !f.temperature
            && !f.power
            && !f.power_factor
            && !f.gain
            && !f.logic
            && !f.diode);
    f.min = mode == b"MN";
    f.max = mode == b"MX";
    f.avg = mode == b"AG";
    f
}

impl Metex14Flags {
    fn valid(&self) -> bool {
        self.prefix.is_single()
            && count(&[
                self.ac,
                self.dc,
                self.resistance,
                self.capacity,
                self.temperature,
                self.diode,
                self.frequency,
            ]) <= 1
            && !(self.ac && self.dc)
    }

    fn apply(&self, value: f32, exponent: i32) -> Reading {
        let (v, digits) = prefixed(value, exponent, self.prefix);
        let mut r = Reading::new(v, Mq::Gain, Unit::Unitless, digits);
        if self.volt {
            (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
        }
        if self.ampere {
            (r.mq, r.unit) = (Mq::Current, Unit::Ampere);
        }
        if self.ohm {
            (r.mq, r.unit) = (Mq::Resistance, Unit::Ohm);
        }
        if self.hertz {
            (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
        }
        if self.farad {
            (r.mq, r.unit) = (Mq::Capacitance, Unit::Farad);
        }
        if self.temperature {
            r.mq = Mq::Temperature;
            r.unit = if self.celsius {
                Unit::Celsius
            } else if self.fahrenheit {
                Unit::Fahrenheit
            } else {
                Unit::Unitless
            };
        }
        if self.diode {
            (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
        }
        if self.power {
            r.mq = Mq::Power;
            r.unit = if self.decibel_mw {
                Unit::DecibelMw
            } else if self.watt {
                Unit::Watt
            } else {
                Unit::Unitless
            };
        }
        if self.power_factor {
            (r.mq, r.unit) = (Mq::PowerFactor, Unit::Unitless);
        }
        if self.gain {
            (r.mq, r.unit) = (Mq::Gain, Unit::DecibelVolt);
        }
        if self.hfe || self.logic {
            (r.mq, r.unit) = (Mq::Gain, Unit::Unitless);
        }
        if self.ac {
            r.flags |= MqFlags::AC;
        }
        if self.dc {
            r.flags |= MqFlags::DC;
        }
        if self.diode {
            r.flags |= MqFlags::DIODE | MqFlags::DC;
        }
        if self.min {
            r.flags |= MqFlags::MIN;
        }
        if self.max {
            r.flags |= MqFlags::MAX;
        }
        if self.avg {
            r.flags |= MqFlags::AVG;
        }
        r
    }
}

/// True for a Metex 14 byte packet.
pub fn metex14_valid(buf: &[u8]) -> bool {
    buf.len() >= METEX14_SIZE && buf[13] == b'\r' && metex14_flags(buf, false).valid()
}

/// True for four Metex packets in a row (the four display meters).
pub fn metex14_x4_valid(buf: &[u8]) -> bool {
    buf.len() >= 4 * METEX14_SIZE && buf.chunks(METEX14_SIZE).take(4).all(metex14_valid)
}

/// Parses a Metex 14 byte packet.
pub fn metex14_parse(buf: &[u8]) -> Option<Reading> {
    if buf.len() < METEX14_SIZE {
        return None;
    }
    let text = squeeze(&buf[2..9]);
    let (value, exponent, logic) = if is_over_limit(&text) {
        (f32::INFINITY, 0, false)
    } else if text == "READY" || text == "FLOAT" {
        (f32::INFINITY, 0, true)
    } else if text == "Hi" {
        (1.0, 0, true)
    } else if text == "Lo" {
        (0.0, 0, true)
    } else {
        let (v, e) = number(&text)?;
        (v, e, false)
    };
    Some(metex14_flags(buf, logic).apply(value, exponent))
}

/// Parses four Metex packets, one reading per display.
pub fn metex14_x4_parse(buf: &[u8]) -> Vec<Reading> {
    buf.chunks(METEX14_SIZE)
        .take(4)
        .enumerate()
        .filter_map(|(i, chunk)| {
            metex14_parse(chunk).map(|mut r| {
                r.channel = i;
                r
            })
        })
        .collect()
}

/// True for a VC-96 packet.
pub fn vc96_valid(buf: &[u8]) -> bool {
    buf.len() >= VC96_SIZE && buf[11] == b'\r' && buf[12] == b'\n' && vc96_flags(buf).valid()
}

#[derive(Clone, Copy, Debug, Default)]
struct Vc96Flags {
    prefix: Prefix,
    ac: bool,
    dc: bool,
    ohm: bool,
    diode: bool,
    hfe: bool,
    ampere: bool,
    volt: bool,
}

fn vc96_flags(buf: &[u8]) -> Vc96Flags {
    let mode = &buf[0..3];
    let mut f = Vc96Flags {
        ac: &mode[..2] == b"AC",
        dc: &mode[..2] == b"DC",
        ohm: mode == b"OHM" || mode == b"BEP",
        diode: mode == b"DIO",
        hfe: mode == b"hfe",
        ..Default::default()
    };
    let unit = squeeze(&buf[9..11]);
    let p = &mut f.prefix;
    match unit.to_ascii_lowercase().as_str() {
        "ma" => (p.milli, f.ampere) = (true, true),
        "ua" => (p.micro, f.ampere) = (true, true),
        "a" => f.ampere = true,
        "mv" => (p.milli, f.volt) = (true, true),
        "v" => f.volt = true,
        u if u.starts_with('k') => p.kilo = true,
        u if u.contains('m') => p.mega = true,
        _ => {}
    }
    f
}

impl Vc96Flags {
    fn valid(&self) -> bool {
        self.prefix.is_single() && count(&[self.ac, self.dc, self.diode]) <= 1
    }
}

/// Parses a VC-96 packet.
pub fn vc96_parse(buf: &[u8]) -> Option<Reading> {
    if buf.len() < VC96_SIZE {
        return None;
    }
    let text = squeeze(&buf[3..9]);
    let (value, exponent) = if is_over_limit(&text) { (f32::INFINITY, 0) } else { number(&text)? };
    let f = vc96_flags(buf);
    let (v, digits) = prefixed(value, exponent, f.prefix);
    let mut r = Reading::new(v, Mq::Gain, Unit::Unitless, digits);
    if f.volt {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
    }
    if f.ampere {
        (r.mq, r.unit) = (Mq::Current, Unit::Ampere);
    }
    if f.ohm {
        (r.mq, r.unit) = (Mq::Resistance, Unit::Ohm);
    }
    if f.diode {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
        r.flags |= MqFlags::DIODE | MqFlags::DC;
    }
    if f.hfe {
        (r.mq, r.unit) = (Mq::Gain, Unit::Unitless);
    }
    if f.ac {
        r.flags |= MqFlags::AC;
    }
    if f.dc {
        r.flags |= MqFlags::DC;
    }
    Some(r)
}

/// The unit and flags of an ASYC-II packet, bytes 7 to 14.
#[derive(Clone, Copy, Debug, Default)]
struct AsyciiFlags {
    prefix: Prefix,
    ampere: bool,
    volt_ampere: bool,
    volt: bool,
    resistance: bool,
    capacitance: bool,
    gain: bool,
    frequency: bool,
    duty_cycle: bool,
    pulse_count: bool,
    pulse_width: bool,
    ac: bool,
    dc: bool,
    ac_and_dc: bool,
    diode: bool,
    peak_max: bool,
    peak_min: bool,
    invalid: bool,
}

/// Strips `prefix` from `u` when present.
fn take(u: &mut &str, prefix: &str) -> bool {
    if let Some(rest) = u.strip_prefix(prefix) {
        *u = rest;
        true
    } else {
        false
    }
}

/// A `+` or `-` must follow a signed unit; its absence is an error.
fn signed(u: &mut &str, invalid: &mut bool) {
    if !(take(u, "+") || take(u, "-")) {
        *invalid = true;
    }
}

fn asycii_flags(buf: &[u8]) -> AsyciiFlags {
    let text = squeeze(&buf[7..15]);
    let mut u = text.as_str();
    let mut f = AsyciiFlags::default();
    let p = &mut f.prefix;
    let first = u.chars().next();
    match first {
        Some('p') => p.pico = true,
        Some('n') => p.nano = true,
        Some('u') => p.micro = true,
        Some('m') => p.milli = true,
        Some('k') => p.kilo = true,
        Some('M') => p.mega = true,
        _ => {}
    }
    if matches!(first, Some('p' | 'n' | 'u' | 'm' | 'k' | 'M')) {
        u = &u[1..];
    }
    if take(&mut u, "A") {
        f.ampere = true;
    } else if take(&mut u, "VA") {
        f.volt_ampere = true;
    } else if take(&mut u, "V") {
        f.volt = true;
    } else if take(&mut u, "ohm") {
        f.resistance = true;
    } else if take(&mut u, "F") {
        f.capacitance = true;
    } else if take(&mut u, "dB") {
        f.gain = true;
    } else if take(&mut u, "Hz") {
        f.frequency = true;
    } else if take(&mut u, "%") {
        f.duty_cycle = true;
        signed(&mut u, &mut f.invalid);
    } else if take(&mut u, "Cnt") {
        f.pulse_count = true;
        signed(&mut u, &mut f.invalid);
    } else if take(&mut u, "s") {
        f.pulse_width = true;
        signed(&mut u, &mut f.invalid);
    } else {
        f.invalid = true;
    }
    if take(&mut u, "ac+dc") {
        f.ac_and_dc = true;
    } else if take(&mut u, "ac") {
        f.ac = true;
    } else if take(&mut u, "dc") {
        f.dc = true;
    } else if take(&mut u, "d") {
        f.diode = true;
    } else if take(&mut u, "Pk") {
        if take(&mut u, "+") {
            f.peak_max = true;
        } else if take(&mut u, "-") {
            f.peak_min = true;
        } else {
            f.invalid = true;
        }
    }
    if !u.is_empty() {
        f.invalid = true;
    }
    f
}

impl AsyciiFlags {
    fn valid(&self) -> bool {
        !self.invalid
            && self.prefix.is_single()
            && count(&[
                self.volt || self.diode,
                self.volt_ampere,
                self.ampere,
                self.gain,
                self.resistance,
                self.capacitance,
                self.frequency,
                self.duty_cycle,
                self.pulse_width,
                self.pulse_count,
            ]) <= 1
            && count(&[self.ac, self.ac_and_dc, self.dc]) <= 1
    }
}

/// True for an ASYC-II packet.
pub fn asycii_valid(buf: &[u8]) -> bool {
    buf.len() >= ASYCII_SIZE && buf[15] == b'\r' && asycii_flags(buf).valid()
}

/// Parses an ASYC-II packet.
pub fn asycii_parse(buf: &[u8]) -> Option<Reading> {
    if buf.len() < ASYCII_SIZE {
        return None;
    }
    let text = squeeze(&buf[0..7]);
    let over = {
        let s = text.strip_prefix('-').unwrap_or(&text);
        s.eq_ignore_ascii_case(".OL") || s.eq_ignore_ascii_case("O.L") || s.starts_with(['X', 'x'])
    };
    let (value, exponent) = if over { (f32::INFINITY, 0) } else { number(&text)? };
    let f = asycii_flags(buf);
    let (v, digits) = prefixed(value, exponent, f.prefix);
    let mut r = Reading::new(v, Mq::Gain, Unit::Unitless, digits);
    if f.volt {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
    }
    if f.volt_ampere {
        (r.mq, r.unit) = (Mq::Power, Unit::VoltAmpere);
    }
    if f.ampere {
        (r.mq, r.unit) = (Mq::Current, Unit::Ampere);
    }
    if f.frequency {
        (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
    }
    if f.duty_cycle {
        (r.mq, r.unit) = (Mq::DutyCycle, Unit::Percentage);
    }
    if f.pulse_width {
        (r.mq, r.unit) = (Mq::PulseWidth, Unit::Second);
    }
    if f.pulse_count {
        (r.mq, r.unit) = (Mq::Count, Unit::Unitless);
    }
    if f.resistance {
        (r.mq, r.unit) = (Mq::Resistance, Unit::Ohm);
    }
    if f.capacitance {
        (r.mq, r.unit) = (Mq::Capacitance, Unit::Farad);
    }
    if f.diode {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
        r.flags |= MqFlags::DIODE | MqFlags::DC;
    }
    if f.gain {
        (r.mq, r.unit) = (Mq::Gain, Unit::DecibelVolt);
    }
    if f.ac {
        r.flags |= MqFlags::AC;
    }
    if f.ac_and_dc {
        r.flags |= MqFlags::AC | MqFlags::DC;
    }
    if f.dc {
        r.flags |= MqFlags::DC;
    }
    if f.peak_max {
        r.flags |= MqFlags::MAX;
    }
    if f.peak_min {
        r.flags |= MqFlags::MIN;
    }
    Some(r)
}

/// True for an M2110 line: a number or `OVERRNG`, then CR LF.
pub fn m2110_valid(buf: &[u8]) -> bool {
    buf.len() >= M2110_SIZE
        && buf[7] == b'\r'
        && buf[8] == b'\n'
        && (&buf[..7] == b"OVERRNG" || m2110_number(buf).is_some())
}

fn m2110_number(buf: &[u8]) -> Option<f32> {
    let text: String = buf[..7].iter().map(|b| char::from(*b)).collect();
    text.trim().parse().ok()
}

/// Parses an M2110 line. The unit is not sent, so the reading is a
/// unitless gain.
pub fn m2110_parse(buf: &[u8]) -> Option<Reading> {
    if buf.len() < M2110_SIZE {
        return None;
    }
    if &buf[..7] == b"OVERRNG" {
        return Some(Reading::new(f32::INFINITY, Mq::Gain, Unit::Unitless, 0));
    }
    let value = m2110_number(buf)?;
    let digits = buf[..7].iter().position(|b| *b == b'.').map_or(0, |dot| 6 - dot as i32);
    Some(Reading::new(value, Mq::Gain, Unit::Unitless, digits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metex14_packets() {
        let p = b"DC -1.234 V  \r";
        assert!(metex14_valid(p));
        let r = metex14_parse(p).unwrap();
        assert_eq!((r.mq, r.unit, r.flags, r.digits), (Mq::Voltage, Unit::Volt, MqFlags::DC, 3));
        assert!((r.value + 1.234).abs() < 1e-5);
        let p = b"OH  O.L  KOhm\r";
        let r = metex14_parse(p).unwrap();
        assert!(r.value.is_infinite());
        assert_eq!((r.mq, r.digits), (Mq::Resistance, -3));
        let p = b"FR 12.34  KHz\r";
        let r = metex14_parse(p).unwrap();
        assert_eq!((r.mq, r.unit, r.digits), (Mq::Frequency, Unit::Hertz, -1));
        assert!((r.value - 12340.0).abs() < 1e-2);
        let p = b"   Hi        \r";
        let r = metex14_parse(p).unwrap();
        assert_eq!((r.value, r.mq), (1.0, Mq::Gain));
        // Four packets, one per display.
        let mut four = Vec::new();
        for _ in 0..4 {
            four.extend_from_slice(b"DC  1.000 V  \r");
        }
        assert!(metex14_x4_valid(&four));
        let rs = metex14_x4_parse(&four);
        assert_eq!(rs.len(), 4);
        assert_eq!(rs[3].channel, 3);
        assert!(!metex14_valid(b"DC -1.234 V    "));
    }

    #[test]
    fn vc96_and_asycii_and_m2110() {
        let p = b"DC -1.234 V\r\n";
        assert!(vc96_valid(p));
        let r = vc96_parse(p).unwrap();
        assert_eq!((r.mq, r.flags, r.digits), (Mq::Voltage, MqFlags::DC, 3));
        let p = b"BEP 0.123 K\r\n";
        let r = vc96_parse(p).unwrap();
        assert_eq!((r.mq, r.digits), (Mq::Resistance, 0));
        assert!((r.value - 123.0).abs() < 1e-3);

        let p = b"  1.234 Vac    \r";
        assert!(asycii_valid(p), "{}", String::from_utf8_lossy(p));
        let r = asycii_parse(p).unwrap();
        assert_eq!((r.mq, r.flags, r.digits), (Mq::Voltage, MqFlags::AC, 3));
        let p = b" 12.34  kohm   \r";
        let r = asycii_parse(p).unwrap();
        assert_eq!((r.mq, r.digits), (Mq::Resistance, -1));
        assert!((r.value - 12340.0).abs() < 1e-2);
        let p = b"-X.XXX  Vd     \r";
        let r = asycii_parse(p).unwrap();
        assert!(r.value.is_infinite() && r.flags.contains(MqFlags::DIODE));
        let p = b"  50.0  %+     \r";
        assert!(asycii_valid(p));
        assert_eq!(asycii_parse(p).unwrap().mq, Mq::DutyCycle);
        assert!(!asycii_valid(b"  1.234 Vxx    \r"));

        assert!(m2110_valid(b" 1.2345\r\n"));
        let r = m2110_parse(b" 1.2345\r\n").unwrap();
        assert_eq!((r.value, r.digits), (1.2345, 4));
        assert!(m2110_parse(b"OVERRNG\r\n").unwrap().value.is_infinite());
        assert!(!m2110_valid(b"abcdefg\r\n"));
    }
}
