// Counterpart in sigrok:
//   libsigrok/src/dmm/bm52x.c,
//   libsigrok/src/dmm/bm86x.c,
//   Copyright (C) 2018 Sven Schnelle <svens@stackframe.org>,
//   Copyright (C) 2019-2020 Gerhard Sittig <gerhard.sittig@gmx.net>,
//   Copyright (C) 2020 Aurelien Jacobs <aurel@gnuage.org>,
//   with commits by Uwe Hermann, Frank Stettner
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! Brymen meters on the BU-86X infrared adapter: the BM520s series
//! (BM521s, BM525s), the BM820s series and the BM860s series (BM867s,
//! BM869s). The meter answers a request with a 24 byte packet that
//! holds both displays as seven segment digits plus indicator bits.
//!
//! Only live readings are supported; the recordings the BM52x series
//! stores in memory are not downloaded.

use crate::core::{Mq, MqFlags, Unit};
use crate::dmm::{Reading, scaled};

/// Packet size of all three series.
pub const BRYMEN_SIZE: usize = 24;
/// Displays per packet.
pub const BRYMEN_DISPLAYS: usize = 2;
/// The request for a live reading from a BM52x.
pub const BM52X_REQUEST: &[u8] = &[0x00, 0x00, 0x52, 0x66];
/// The request for a live reading from a BM82x.
pub const BM82X_REQUEST: &[u8] = &[0x00, 0x00, 0x82, 0x66];
/// The request for a live reading from a BM86x.
pub const BM86X_REQUEST: &[u8] = &[0x00, 0x00, 0x86, 0x66];

fn has_marker(buf: &[u8], marker: u8) -> bool {
    buf.len() >= BRYMEN_SIZE && buf[16..20].iter().all(|b| *b == marker)
}

/// True for a BM52x packet: bytes 16 to 19 are the model id 0x52.
pub fn bm52x_valid(buf: &[u8]) -> bool {
    has_marker(buf, 0x52)
}

/// True for a BM82x packet: bytes 16 to 19 are the model id 0x82.
pub fn bm82x_valid(buf: &[u8]) -> bool {
    has_marker(buf, 0x82)
}

/// True for a BM86x packet: bytes 16 to 19 are the model id 0x86.
pub fn bm86x_valid(buf: &[u8]) -> bool {
    has_marker(buf, 0x86)
}

/// The text of one display: its characters, the temperature unit shown
/// in the last position and the digits after the decimal point.
#[derive(Debug, Default, PartialEq, Eq)]
struct Text {
    chars: String,
    temp_unit: Option<char>,
    digits: i32,
}

impl Text {
    fn value(&self) -> Option<f32> {
        self.chars.parse().ok()
    }

    fn is_over_limit(&self) -> bool {
        self.chars.contains("0L") || self.chars.contains("0.L")
    }
}

/// Reads the display at `pkt[1..]` with the sign bit `sign` of
/// `pkt[0]`, `len` digit bytes, using `digit` for the segments and
/// `point` for where the decimal point of a position is encoded.
fn read_text(
    pkt: &[u8],
    len: usize,
    sign: u8,
    digit: fn(u8) -> Option<char>,
    point: fn(usize, u8) -> bool,
    point_first: bool,
    unit_pos: usize,
) -> Text {
    let mut t = Text { digits: i32::MIN, ..Text::default() };
    if pkt[0] & sign != 0 {
        t.chars.push('-');
    }
    for pos in 0..len {
        let byte = pkt[1 + pos];
        if point_first && point(pos, byte) {
            t.chars.push('.');
            t.digits = 0;
        }
        match digit(byte) {
            Some('\0') => {}
            Some(c @ ('C' | 'F')) if pos == unit_pos => t.temp_unit = Some(c),
            Some(c) => {
                t.chars.push(c);
                t.digits += 1;
            }
            None => log::warn!("Unknown encoding for digit: {byte:#04x}."),
        }
        if !point_first && point(pos, byte) {
            t.chars.push('.');
            t.digits = 0;
        }
    }
    t.digits = t.digits.max(0);
    t
}

fn bm52x_digit(b: u8) -> Option<char> {
    Some(match b & !0x10 {
        0x40 => '-',
        0xaf => '0',
        0xa0 => '1',
        0xcb => '2',
        0xe9 => '3',
        0xe4 => '4',
        0x6d => '5',
        0x6f => '6',
        0xa8 => '7',
        0xef => '8',
        0xed => '9',
        0x0f => 'C',
        0x4e => 'F',
        0x07 => 'L',
        0xe3 => 'd',
        0x20 => 'i',
        0x63 => 'o',
        0xee => 'A',
        0x23 => 'u',
        0x47 => 't',
        0x00 => '\0',
        _ => return None,
    })
}

fn bm52x_point(pos: usize, byte: u8) -> bool {
    pos < 3 && byte & 0x10 != 0
}

fn bm52x_text(pkt: &[u8], sign: u8) -> Text {
    read_text(pkt, 4, sign, bm52x_digit, bm52x_point, false, 3)
}

fn prefix(flags: [(bool, i32); 5]) -> i32 {
    // The last set prefix wins, as in the C code.
    flags.iter().rev().find(|(set, _)| *set).map_or(0, |(_, e)| *e)
}

fn finish(mut r: Reading, value: Option<f32>, digits: i32, exponent: i32, over: bool) -> Reading {
    let (v, d) = scaled(value.unwrap_or(0.0), exponent);
    r.value = if over { f32::INFINITY } else { v };
    r.digits = digits + d;
    r
}

/// The main display of a BM52x or BM82x packet.
fn bm52x_main(buf: &[u8]) -> Option<Reading> {
    let second = bm52x_text(&buf[7..], 0);
    let is_diode = second.chars == "diod";
    let main = bm52x_text(&buf[2..], 0x80);
    let value = main.value();
    let over = main.is_over_limit();
    let no_temp = main.chars == "---C" || main.chars == "---F";
    if value.is_none() && !over {
        return None;
    }
    let mut r = Reading::new(0.0, Mq::Gain, Unit::Unitless, 0);
    let is_db = buf[6] & 0x10 != 0;
    let mut is_milli = buf[14] & 0x40 != 0;
    if buf[14] & 0x20 != 0 {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
        if is_diode {
            r.flags |= MqFlags::DIODE | MqFlags::DC;
        }
    } else if buf[14] & 0x10 != 0 {
        (r.mq, r.unit) = (Mq::Current, Unit::Ampere);
    } else if buf[14] & 0x01 != 0 {
        (r.mq, r.unit) = (Mq::Capacitance, Unit::Farad);
    } else if buf[14] & 0x02 != 0 {
        (r.mq, r.unit) = (Mq::Conductance, Unit::Siemens);
    } else if buf[13] & 0x10 != 0 {
        (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
    } else if buf[7] & 0x01 != 0 {
        (r.mq, r.unit) = (Mq::Continuity, Unit::Ohm);
    } else if buf[13] & 0x20 != 0 {
        (r.mq, r.unit) = (Mq::Resistance, Unit::Ohm);
    } else if is_db && is_milli {
        (r.mq, r.unit) = (Mq::Power, Unit::DecibelMw);
    } else if buf[14] & 0x04 != 0 {
        (r.mq, r.unit) = (Mq::DutyCycle, Unit::Percentage);
    } else if buf[2] & 0x09 != 0 && main.temp_unit.is_some() {
        if no_temp {
            return None;
        }
        r.mq = Mq::Temperature;
        r.unit = if main.temp_unit == Some('F') { Unit::Fahrenheit } else { Unit::Celsius };
    } else {
        return None;
    }
    // MIN, MAX and AVG all lit means recording with live readings
    // shown; MAX and MIN together is the peak to peak reading, which
    // has no flag.
    let mut is_max = buf[1] & 0x01 != 0;
    let mut is_min = buf[1] & 0x08 != 0;
    let mut is_avg = buf[1] & 0x02 != 0;
    if is_max && is_min {
        // Recording with live readings shown, or the peak to peak
        // reading: neither maps to flags.
        (is_max, is_min, is_avg) = (false, false, false);
    }
    if buf[1] & 0x20 != 0 {
        r.flags |= MqFlags::DC;
    }
    if buf[1] & 0x10 != 0 {
        r.flags |= MqFlags::AC;
    }
    if buf[20] & 0x10 != 0 {
        r.flags |= MqFlags::AUTORANGE;
    }
    if buf[20] & 0x80 != 0 {
        r.flags |= MqFlags::HOLD;
    }
    if is_max {
        r.flags |= MqFlags::MAX;
    }
    if is_min {
        r.flags |= MqFlags::MIN;
    }
    if is_avg {
        r.flags |= MqFlags::AVG;
    }
    if buf[2] & 0x40 != 0 {
        r.flags |= MqFlags::RELATIVE;
    }
    // The "m" of dBm is not a milli prefix.
    if is_db {
        is_milli = false;
    }
    let e = prefix([
        (buf[14] & 0x08 != 0, -9),
        (buf[14] & 0x80 != 0, -6),
        (is_milli, -3),
        (buf[13] & 0x80 != 0, 3),
        (buf[13] & 0x40 != 0, 6),
    ]);
    Some(finish(r, value, main.digits, e, over))
}

/// The secondary display of a BM52x or BM82x packet.
fn bm52x_secondary(buf: &[u8]) -> Option<Reading> {
    let main = bm52x_text(&buf[2..], 0x80);
    let second = bm52x_text(&buf[7..], 0x20);
    if second.chars == "diod" || second.chars == "Auto" {
        return None;
    }
    if second.chars == "---C" || second.chars == "---F" {
        return None;
    }
    let value = second.value()?;
    let mut r = Reading::new(0.0, Mq::Gain, Unit::Unitless, 0);
    r.channel = 1;
    if buf[12] & 0x10 != 0 {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
    } else if buf[12] & 0x20 != 0 {
        r.mq = Mq::Current;
        r.unit = if buf[11] & 0x10 != 0 { Unit::Percentage } else { Unit::Ampere };
    } else if buf[13] & 0x02 != 0 {
        (r.mq, r.unit) = (Mq::Resistance, Unit::Ohm);
    } else if buf[12] & 0x02 != 0 {
        (r.mq, r.unit) = (Mq::Conductance, Unit::Siemens);
    } else if buf[12] & 0x01 != 0 {
        (r.mq, r.unit) = (Mq::Capacitance, Unit::Farad);
    } else if buf[7] & 0x06 != 0 {
        if second.chars.contains("---") {
            return None;
        }
        r.mq = Mq::Temperature;
        r.unit = if main.temp_unit == Some('F') { Unit::Fahrenheit } else { Unit::Celsius };
    } else if buf[13] & 0x01 != 0 {
        (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
    } else if buf[11] & 0x08 != 0 {
        (r.mq, r.unit) = (Mq::DutyCycle, Unit::Percentage);
    } else {
        return None;
    }
    if buf[7] & 0x80 != 0 {
        r.flags |= MqFlags::DC;
    }
    if buf[7] & 0x40 != 0 {
        r.flags |= MqFlags::AC;
    }
    let e = prefix([
        (buf[12] & 0x04 != 0, -9),
        (buf[12] & 0x40 != 0, -6),
        (buf[12] & 0x80 != 0, -3),
        (buf[13] & 0x04 != 0, 3),
        (buf[13] & 0x08 != 0, 6),
    ]);
    Some(finish(r, Some(value), second.digits, e, false))
}

/// The readings in a BM52x or BM82x packet: the main display on
/// channel 0, the secondary on channel 1 when it shows a value.
pub fn bm52x_parse(buf: &[u8]) -> Vec<Reading> {
    if buf.len() < BRYMEN_SIZE {
        return Vec::new();
    }
    if buf[7] & 0x08 != 0 {
        log::warn!("Battery is low.");
    }
    bm52x_main(buf).into_iter().chain(bm52x_secondary(buf)).collect()
}

fn bm86x_digit(b: u8) -> Option<char> {
    Some(match b >> 1 {
        0x20 => '-',
        0x5f => '0',
        0x50 => '1',
        0x6d => '2',
        0x7c => '3',
        0x72 => '4',
        0x3e => '5',
        0x3f => '6',
        0x54 => '7',
        0x7f => '8',
        0x7e => '9',
        0x0f => 'C',
        0x27 => 'F',
        0x0b => 'L',
        0x79 => 'd',
        0x10 => 'i',
        0x39 => 'o',
        0x00 => '\0',
        _ => return None,
    })
}

/// The BM86x keeps a position's decimal point in the next byte.
fn bm86x_point(pos: usize, byte: u8) -> bool {
    pos > 0 && pos < 5 && byte & 0x01 != 0
}

fn bm86x_main_text(buf: &[u8]) -> Text {
    read_text(&buf[2..], 6, 0x80, bm86x_digit, bm86x_point, true, 5)
}

fn bm86x_second_text(buf: &[u8], sign: u8) -> Text {
    read_text(&buf[9..], 4, sign, bm86x_digit, bm86x_point, true, usize::MAX)
}

/// The main display of a BM86x packet.
fn bm86x_main(buf: &[u8]) -> Option<Reading> {
    let is_diode = bm86x_second_text(buf, 0).chars == "diod";
    let main = bm86x_main_text(buf);
    let value = main.value();
    let over = main.is_over_limit();
    if value.is_none() && !over {
        return None;
    }
    let mut r = Reading::new(0.0, Mq::Gain, Unit::Unitless, 0);
    if buf[8] & 0x01 != 0 {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
        if is_diode {
            r.flags |= MqFlags::DIODE | MqFlags::DC;
        }
    } else if buf[14] & 0x80 != 0 {
        (r.mq, r.unit) = (Mq::Current, Unit::Ampere);
    } else if buf[14] & 0x20 != 0 {
        (r.mq, r.unit) = (Mq::Capacitance, Unit::Farad);
    } else if buf[14] & 0x10 != 0 {
        (r.mq, r.unit) = (Mq::Conductance, Unit::Siemens);
    } else if buf[15] & 0x01 != 0 {
        (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
    } else if buf[10] & 0x01 != 0 {
        (r.mq, r.unit) = (Mq::Continuity, Unit::Ohm);
    } else if buf[15] & 0x10 != 0 {
        (r.mq, r.unit) = (Mq::Resistance, Unit::Ohm);
    } else if buf[15] & 0x02 != 0 {
        (r.mq, r.unit) = (Mq::Power, Unit::DecibelMw);
    } else if buf[15] & 0x80 != 0 {
        (r.mq, r.unit) = (Mq::DutyCycle, Unit::Percentage);
    } else if buf[2] & 0x0a != 0 && main.temp_unit.is_some() {
        r.mq = Mq::Temperature;
        r.unit = if main.temp_unit == Some('F') { Unit::Fahrenheit } else { Unit::Celsius };
    } else {
        return None;
    }
    // MIN, MAX and AVG all lit at once mean none of them.
    let mut ind1 = buf[1];
    if ind1 & 0xe0 == 0xe0 {
        ind1 &= !0xe0;
    }
    if buf[1] & 0x10 != 0 {
        r.flags |= MqFlags::DC;
    }
    if buf[2] & 0x01 != 0 {
        r.flags |= MqFlags::AC;
    }
    if buf[1] & 0x01 != 0 {
        r.flags |= MqFlags::AUTORANGE;
    }
    if buf[1] & 0x08 != 0 {
        r.flags |= MqFlags::HOLD;
    }
    if ind1 & 0x20 != 0 {
        r.flags |= MqFlags::MAX;
    }
    if ind1 & 0x40 != 0 {
        r.flags |= MqFlags::MIN;
    }
    if ind1 & 0x80 != 0 {
        r.flags |= MqFlags::AVG;
    }
    if buf[3] & 0x01 != 0 {
        r.flags |= MqFlags::RELATIVE;
    }
    // The "m" of dBm is not a milli prefix.
    let mut ind15 = buf[15];
    if ind15 & 0x02 != 0 {
        ind15 &= !0x04;
    }
    let e = prefix([
        (buf[14] & 0x40 != 0, -9),
        (buf[15] & 0x08 != 0, -6),
        (ind15 & 0x04 != 0, -3),
        (buf[15] & 0x40 != 0, 3),
        (buf[15] & 0x20 != 0, 6),
    ]);
    Some(finish(r, value, main.digits, e, over))
}

/// The secondary display of a BM86x packet.
fn bm86x_secondary(buf: &[u8]) -> Option<Reading> {
    let main = bm86x_main_text(buf);
    let second = bm86x_second_text(buf, 0x10);
    let value = second.value()?;
    let mut r = Reading::new(0.0, Mq::Gain, Unit::Unitless, 0);
    r.channel = 1;
    if buf[14] & 0x08 != 0 {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
    } else if buf[9] & 0x04 != 0 {
        (r.mq, r.unit) = (Mq::Current, Unit::Ampere);
    } else if buf[9] & 0x08 != 0 {
        (r.mq, r.unit) = (Mq::Current, Unit::Percentage);
    } else if buf[14] & 0x04 != 0 {
        (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
    } else if buf[9] & 0x40 != 0 && main.temp_unit.is_some() {
        r.mq = Mq::Temperature;
        r.unit = if main.temp_unit == Some('F') { Unit::Fahrenheit } else { Unit::Celsius };
    } else {
        return None;
    }
    if buf[9] & 0x20 != 0 {
        r.flags |= MqFlags::AC;
    }
    let e = prefix([
        (false, 0),
        (buf[9] & 0x01 != 0, -6),
        (buf[9] & 0x02 != 0, -3),
        (buf[14] & 0x02 != 0, 3),
        (buf[14] & 0x01 != 0, 6),
    ]);
    Some(finish(r, Some(value), second.digits, e, false))
}

/// The readings in a BM86x packet: the main display on channel 0, the
/// secondary on channel 1 when it shows a value.
pub fn bm86x_parse(buf: &[u8]) -> Vec<Reading> {
    if buf.len() < BRYMEN_SIZE {
        return Vec::new();
    }
    if buf[9] & 0x80 != 0 {
        log::warn!("Battery is low.");
    }
    bm86x_main(buf).into_iter().chain(bm86x_secondary(buf)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The BM52x segment byte of a character, with the point bit.
    fn seg52(c: char, point: bool) -> u8 {
        let b = match c {
            '-' => 0x40,
            '0' => 0xaf,
            '1' => 0xa0,
            '2' => 0xcb,
            '3' => 0xe9,
            '4' => 0xe4,
            '5' => 0x6d,
            '6' => 0x6f,
            '7' => 0xa8,
            '8' => 0xef,
            '9' => 0xed,
            'C' => 0x0f,
            'L' => 0x07,
            'd' => 0xe3,
            'i' => 0x20,
            'o' => 0x63,
            ' ' => 0x00,
            _ => panic!("{c}"),
        };
        b | u8::from(point) << 4
    }

    /// A BM52x packet showing `main` (four characters, a '.' after
    /// position `point`) and `second`.
    fn bm52x(main: &str, point: Option<usize>, second: &str) -> [u8; 24] {
        let mut p = [0u8; 24];
        p[16..20].copy_from_slice(&[0x52; 4]);
        for (i, c) in main.chars().enumerate() {
            p[3 + i] = seg52(c, point == Some(i));
        }
        for (i, c) in second.chars().enumerate() {
            p[8 + i] = seg52(c, false);
        }
        p
    }

    #[test]
    fn bm52x_voltage_and_frequency() {
        // 12.34 V DC auto on the main display, 50.00 Hz on the second.
        let mut p = bm52x("1234", Some(1), "5000");
        p[14] |= 0x20; // V
        p[1] |= 0x20; // DC
        p[20] |= 0x10; // auto
        p[13] |= 0x01; // second: Hz
        p[9] |= 0x10; // second point after position 1
        assert!(bm52x_valid(&p));
        assert!(!bm82x_valid(&p));
        let r = bm52x_parse(&p);
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[0].channel, r[0].mq, r[0].unit, r[0].digits),
            (0, Mq::Voltage, Unit::Volt, 2)
        );
        assert!((r[0].value - 12.34).abs() < 1e-4);
        assert!(r[0].flags.contains(MqFlags::DC | MqFlags::AUTORANGE));
        assert_eq!(
            (r[1].channel, r[1].mq, r[1].unit, r[1].digits),
            (1, Mq::Frequency, Unit::Hertz, 2)
        );
        assert!((r[1].value - 50.0).abs() < 1e-4);
    }

    #[test]
    fn bm52x_overload_milli_and_diode() {
        // "0.L" in kOhm: over limit.
        let mut p = bm52x(" 0L ", Some(1), "    ");
        p[13] |= 0x20 | 0x80;
        let r = bm52x_parse(&p);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].unit, Unit::Ohm);
        assert!(r[0].value.is_infinite());
        // 123.4 mA, negative; the second display says "diod" for a
        // voltage reading, which gives the diode flag and no second
        // reading.
        let mut p = bm52x("1234", Some(2), "diod");
        p[2] |= 0x80;
        p[14] |= 0x10 | 0x40;
        let r = bm52x_parse(&p);
        assert_eq!(r.len(), 1);
        assert!((r[0].value + 0.1234).abs() < 1e-6);
        assert_eq!(r[0].digits, 4);
        let mut p = bm52x("0612", Some(0), "diod");
        p[14] |= 0x20;
        let r = bm52x_parse(&p);
        assert_eq!(r.len(), 1);
        assert!(r[0].flags.contains(MqFlags::DIODE | MqFlags::DC));
        // No function bits at all: nothing.
        assert!(bm52x_parse(&bm52x("1234", None, "    ")).is_empty());
    }

    #[test]
    fn bm52x_temperature() {
        // "25.0C" shows as 250 with the unit in the last position and
        // the temperature indicator in byte 2.
        let mut p = bm52x("250C", Some(1), "    ");
        p[2] |= 0x01;
        let r = bm52x_parse(&p);
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].mq, r[0].unit), (Mq::Temperature, Unit::Celsius));
        assert!((r[0].value - 25.0).abs() < 1e-4);
        let mut p = bm52x("---C", None, "    ");
        p[2] |= 0x01;
        assert!(bm52x_parse(&p).is_empty());
    }

    /// The BM86x segment byte of a character.
    fn seg86(c: char) -> u8 {
        let b: u8 = match c {
            '-' => 0x20,
            '0' => 0x5f,
            '1' => 0x50,
            '2' => 0x6d,
            '3' => 0x7c,
            '4' => 0x72,
            '5' => 0x3e,
            '6' => 0x3f,
            '7' => 0x54,
            '8' => 0x7f,
            '9' => 0x7e,
            'C' => 0x0f,
            'F' => 0x27,
            'L' => 0x0b,
            'd' => 0x79,
            'i' => 0x10,
            'o' => 0x39,
            ' ' => 0x00,
            _ => panic!("{c}"),
        };
        b << 1
    }

    /// A BM86x packet with `main` (six positions) and `second` (four);
    /// a point before position `i` is bit 0 of that position's byte.
    fn bm86x(main: &str, point: Option<usize>, second: &str, point2: Option<usize>) -> [u8; 24] {
        let mut p = [0u8; 24];
        p[16..20].copy_from_slice(&[0x86; 4]);
        for (i, c) in main.chars().enumerate() {
            p[3 + i] = seg86(c) | u8::from(point == Some(i));
        }
        for (i, c) in second.chars().enumerate() {
            p[10 + i] = seg86(c) | u8::from(point2 == Some(i));
        }
        p
    }

    #[test]
    fn bm86x_dual_display() {
        // 1.2345 V AC with 50.0 Hz on the second display, hold.
        let mut p = bm86x("12345 ", Some(1), "500 ", Some(2));
        p[8] |= 0x01; // V
        p[2] |= 0x01; // AC
        p[1] |= 0x08; // hold
        p[14] |= 0x04; // second: Hz
        assert!(bm86x_valid(&p));
        let r = bm86x_parse(&p);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].mq, r[0].unit, r[0].digits), (Mq::Voltage, Unit::Volt, 4));
        assert!((r[0].value - 1.2345).abs() < 1e-5);
        assert!(r[0].flags.contains(MqFlags::AC | MqFlags::HOLD));
        assert_eq!((r[1].channel, r[1].mq, r[1].digits), (1, Mq::Frequency, 1));
        assert!((r[1].value - 50.0).abs() < 1e-4);
    }

    #[test]
    fn bm86x_prefixes_temperature_and_overload() {
        // 47.0 nF.
        let mut p = bm86x("470   ", Some(2), "    ", None);
        p[14] |= 0x20 | 0x40;
        let r = bm86x_parse(&p);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].unit, Unit::Farad);
        assert!((r[0].value - 47.0e-9).abs() < 1e-12);
        assert_eq!(r[0].digits, 10);
        // -12.5 F with the unit in the last position.
        let mut p = bm86x("125  F", Some(2), "    ", None);
        p[2] |= 0x80 | 0x02;
        let r = bm86x_parse(&p);
        assert_eq!((r[0].mq, r[0].unit), (Mq::Temperature, Unit::Fahrenheit));
        assert!((r[0].value + 12.5).abs() < 1e-4);
        // dBm: the m is not a prefix.
        let mut p = bm86x("  100 ", Some(4), "    ", None);
        p[15] |= 0x02 | 0x04;
        let r = bm86x_parse(&p);
        assert_eq!(r[0].unit, Unit::DecibelMw);
        assert!((r[0].value - 10.0).abs() < 1e-4);
        // Overload in MOhm, all three of MIN MAX AVG lit.
        let mut p = bm86x(" 0L   ", Some(2), "    ", None);
        p[15] |= 0x10 | 0x20;
        p[1] |= 0xe0;
        let r = bm86x_parse(&p);
        assert!(r[0].value.is_infinite());
        assert!(!r[0].flags.contains(MqFlags::MAX));
        assert!(!bm86x_valid(&[0u8; 24]));
    }
}

// BM85x: a framed text protocol over the serial cable, see
// libsigrok/src/dmm/bm85x.c, Copyright (C) 2019 Gerhard Sittig.

/// The shortest BM85x packet: a complete header.
pub const BM85X_MIN_SIZE: usize = 4;
/// The request for a reading: DLE STX, command 0 with two zero
/// arguments, their XOR checksum, DLE ETX.
pub const BM85X_REQUEST: &[u8] = &[0x10, 0x02, 0x00, 0x00, 0x00, 0x00, 0x10, 0x03];

const BM85X_HEAD: usize = 4;
const BM85X_TAIL: usize = 3;
const BM85X_DATA_MAX: usize = 15;
const BM85X_BFUNC: usize = 4;

/// Frames a BM85x packet at the start of `buf`: header, payload of the
/// announced length, checksum and trailer.
pub fn bm85x_frame(buf: &[u8]) -> crate::dmm::Validity {
    use crate::dmm::Validity::{Invalid, Need, Valid};
    if buf.len() < BM85X_HEAD {
        return Need;
    }
    if buf[0] != 0x10 || buf[1] != 0x02 {
        return Invalid;
    }
    // Command 1 happens with OL pending at connect time; it is a
    // reading too.
    if buf[2] != 0 && buf[2] != 1 {
        return Invalid;
    }
    let plen = usize::from(buf[3]);
    if plen > BM85X_DATA_MAX {
        return Invalid;
    }
    let total = BM85X_HEAD + plen + BM85X_TAIL;
    if buf.len() < total {
        return Need;
    }
    let crc = buf[BM85X_HEAD..BM85X_HEAD + plen].iter().fold(0u8, |a, b| a ^ b);
    let tail = &buf[BM85X_HEAD + plen..total];
    if tail != [crc, 0x10, 0x03] {
        return Invalid;
    }
    Valid(total)
}

/// The reading in a framed BM85x packet.
#[allow(clippy::similar_names)]
pub fn bm85x_parse(buf: &[u8]) -> Option<Reading> {
    let crate::dmm::Validity::Valid(total) = bm85x_frame(buf) else {
        return None;
    };
    let payload = &buf[BM85X_HEAD..total - BM85X_TAIL];
    if payload.len() < BM85X_BFUNC {
        return None;
    }
    let bfunc = &payload[..BM85X_BFUNC];
    let mut text = String::from_utf8_lossy(&payload[BM85X_BFUNC..]).into_owned();
    // The function bits; reserved ones must be clear.
    let is_batt = bfunc[3] & 0x80 != 0;
    if bfunc[3] & 0x7f != 0 || bfunc[2] != 0 || bfunc[1] & 0xd0 != 0 {
        return None;
    }
    let is_dbm = bfunc[1] & 0x20 != 0;
    let is_perc = bfunc[1] & 0x08 != 0;
    let is_hz = bfunc[1] & 0x04 != 0;
    let is_amp = bfunc[1] & 0x02 != 0;
    let is_beep = bfunc[1] & 0x01 != 0;
    let is_ohm = bfunc[0] & 0x80 != 0;
    let is_temp_f = bfunc[0] & 0x40 != 0;
    let is_temp_c = bfunc[0] & 0x20 != 0;
    let is_diode = bfunc[0] & 0x10 != 0;
    let is_cap = bfunc[0] & 0x08 != 0;
    let is_volt = bfunc[0] & 0x04 != 0;
    let is_dc = bfunc[0] & 0x02 != 0;
    let is_ac = bfunc[0] & 0x01 != 0;
    if is_batt {
        log::warn!("Low battery!");
    }
    if is_dbm && is_ohm {
        // The dBm reference resistance comes as " 0.600 E" and the like.
        text = text.strip_prefix(" 0.")?.to_owned();
        let end = text.find(" E")?;
        text.truncate(end);
    }
    if is_temp_f || is_temp_c {
        let unit = if is_temp_f { 'F' } else { 'C' };
        let pos = text.find(unit)?;
        text.replace_range(pos..=pos, " ");
    }
    let (mut value, mut digits) = if text.contains("+OL") {
        (f64::INFINITY, 0)
    } else if text.contains("-OL") {
        (f64::NEG_INFINITY, 0)
    } else if text.contains("OL") {
        (f64::INFINITY, 0)
    } else {
        let compact: String = text.chars().filter(|c| *c != ' ').collect();
        let v: f64 = compact.parse().ok()?;
        let digits = compact.find('.').map_or(0, |p| (compact.len() - p - 1) as i32);
        (v, digits)
    };
    let mut r = Reading::new(0.0, Mq::Gain, Unit::Unitless, 0);
    if is_volt {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
    }
    if is_amp {
        (r.mq, r.unit) = (Mq::Current, Unit::Ampere);
    }
    if is_ohm {
        r.mq = if is_beep && !is_dbm { Mq::Continuity } else { Mq::Resistance };
        r.unit = Unit::Ohm;
    }
    if is_hz {
        (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
    }
    if is_perc {
        (r.mq, r.unit) = (Mq::DutyCycle, Unit::Percentage);
    }
    if is_cap {
        (r.mq, r.unit) = (Mq::Capacitance, Unit::Farad);
    }
    if is_temp_f {
        (r.mq, r.unit) = (Mq::Temperature, Unit::Fahrenheit);
    }
    if is_temp_c {
        (r.mq, r.unit) = (Mq::Temperature, Unit::Celsius);
    }
    if is_dbm && !is_ohm {
        // dBm readings come scaled by a thousand.
        (r.mq, r.unit) = (Mq::Power, Unit::DecibelMw);
        value *= 1000.0;
        digits -= 3;
    }
    if is_diode {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
        r.flags |= MqFlags::DIODE | MqFlags::DC;
    }
    if r.mq == Mq::Gain {
        return None;
    }
    if is_ac {
        r.flags |= MqFlags::AC;
    }
    if is_dc {
        r.flags |= MqFlags::DC;
    }
    r.value = value as f32;
    r.digits = digits;
    Some(r)
}

#[cfg(test)]
mod bm85x_tests {
    use super::*;
    use crate::dmm::Validity;

    fn packet(bfunc: [u8; 4], text: &str) -> Vec<u8> {
        let mut payload = bfunc.to_vec();
        payload.extend_from_slice(text.as_bytes());
        let crc = payload.iter().fold(0u8, |a, b| a ^ b);
        let mut p = vec![0x10, 0x02, 0x00, payload.len() as u8];
        p.extend(&payload);
        p.extend([crc, 0x10, 0x03]);
        p
    }

    #[test]
    fn bm85x_frames_and_parses() {
        let p = packet([0x06, 0, 0, 0], " 12.345 ");
        assert_eq!(bm85x_frame(&p), Validity::Valid(p.len()));
        assert_eq!(bm85x_frame(&p[..3]), Validity::Need);
        assert_eq!(bm85x_frame(&p[..p.len() - 1]), Validity::Need);
        assert_eq!(bm85x_frame(&p[1..]), Validity::Invalid);
        let mut bad = p.clone();
        bad[6] ^= 1;
        assert_eq!(bm85x_frame(&bad), Validity::Invalid);
        let r = bm85x_parse(&p).unwrap();
        assert_eq!((r.mq, r.unit, r.digits), (Mq::Voltage, Unit::Volt, 3));
        assert!((r.value - 12.345).abs() < 1e-4);
        assert!(r.flags.contains(MqFlags::DC));
        // Temperature with the unit letter in the text, and a dBm value.
        let p = packet([0x20, 0, 0, 0], " 23.5C");
        let r = bm85x_parse(&p).unwrap();
        assert_eq!((r.mq, r.unit), (Mq::Temperature, Unit::Celsius));
        assert!((r.value - 23.5).abs() < 1e-4);
        let p = packet([0x00, 0x20, 0, 0], " 0.0123");
        let r = bm85x_parse(&p).unwrap();
        assert_eq!((r.mq, r.unit, r.digits), (Mq::Power, Unit::DecibelMw, 1));
        assert!((r.value - 12.3).abs() < 1e-3);
        // Overload, negative.
        let p = packet([0x82, 0, 0, 0], "  -OL  ");
        let r = bm85x_parse(&p).unwrap();
        assert!(r.value.is_infinite() && r.value < 0.0);
        // A reserved bit set: nothing.
        assert!(bm85x_parse(&packet([0x06, 0x10, 0, 0], " 1.000 ")).is_none());
        assert_eq!(BM85X_REQUEST.len(), 8);
    }
}
