// Counterpart in sigrok:
//   libsigrok/src/dmm/ms2115b.c,
//   libsigrok/src/dmm/ms8250d.c,
//   Copyright (C) 2019 Dave Buechi <db@pflutsch.ch>,
//   Copyright (C) 2018 Stefan Mandl <stefan.mandl@gmail.com>,
//   with commits by Uwe Hermann, Gerhard Sittig, Frank Stettner
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is an independent implementation written for oxrok from the
//   observed behaviour and the public interfaces.

//! Mastech meters: the MS2115B clamp meter (9 byte binary packets, two
//! displays) and the MS8250D (18 byte segment packets).

use crate::{Reading, scaled};
use oxrok_core::{Mq, MqFlags, Unit};

/// MS2115B: 9 bytes.
pub const MS2115B_SIZE: usize = 9;
/// MS2115B displays.
pub const MS2115B_DISPLAYS: usize = 2;
/// MS2115B channel names.
pub const MS2115B_CHANNELS: [&str; 2] = ["main", "sub"];
/// MS8250D: 18 bytes.
pub const MS8250D_SIZE: usize = 18;

/// True for an MS2115B packet: it starts with 0x55.
pub fn ms2115b_valid(buf: &[u8]) -> bool {
    buf.len() >= MS2115B_SIZE && buf[0] == 0x55
}

const MS2115B_RES_EXP: [i32; 6] = [-1, 0, 1, 2, 3, 4];
const MS2115B_CAP_EXP: [i32; 8] = [-12, -11, -10, -9, -8, -7, -6, -5];
const MS2115B_HZ_EXP: [i32; 7] = [-2, -1, 0, 1, 2, 3, 4];
const MS2115B_V_EXP: [i32; 5] = [-4, -3, -2, -1, 0];

fn ms2115b_main(buf: &[u8]) -> Option<Reading> {
    let mode = buf[1] & 7;
    let func = buf[1] & 8 != 0;
    let range = usize::from(buf[2] & 7);
    let mut r = Reading::new(0.0, Mq::Gain, Unit::Unitless, 0);
    let mut signed = false;
    let mut up_limit = 6000.0f32;
    let acdc = |r: &mut Reading| {
        r.flags |= if func { MqFlags::DC } else { MqFlags::AC };
    };
    let exponent = match mode {
        0 | 1 => {
            signed = true;
            (r.mq, r.unit) = (Mq::Current, Unit::Ampere);
            acdc(&mut r);
            if mode == 0 { -1 } else { -2 }
        }
        2 => {
            signed = true;
            (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
            acdc(&mut r);
            *MS2115B_V_EXP.get(range)?
        }
        3 => {
            if func {
                up_limit = 2500.0;
                (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
                r.flags |= MqFlags::DIODE | MqFlags::DC;
                -3
            } else {
                (r.mq, r.unit) = (Mq::Continuity, Unit::Boolean);
                0
            }
        }
        4 => {
            (r.mq, r.unit) = (Mq::Resistance, Unit::Ohm);
            *MS2115B_RES_EXP.get(range)?
        }
        5 => {
            (r.mq, r.unit) = (Mq::Capacitance, Unit::Farad);
            *MS2115B_CAP_EXP.get(range)?
        }
        6 => {
            (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
            *MS2115B_HZ_EXP.get(usize::from(buf[3] & 7))?
        }
        _ => return None,
    };
    let raw = u16::from_le_bytes([buf[4], buf[5]]);
    let value = if signed { f32::from(raw as i16) } else { f32::from(raw) };
    if buf[8] & 1 == 0 {
        r.flags |= MqFlags::AUTORANGE;
    }
    // Over the limit the C code sends nothing: it never gets to the
    // unit.
    if value.abs() > up_limit {
        return None;
    }
    let (v, d) = scaled(value, exponent);
    r.value = if mode == 3 && !func { 1.0 } else { v };
    r.digits = d;
    Some(r)
}

fn ms2115b_sub(buf: &[u8]) -> Option<Reading> {
    let mode = buf[1] & 7;
    let func = buf[1] & 8 != 0;
    let mut r = Reading::new(0.0, Mq::Gain, Unit::Unitless, 0);
    r.channel = 1;
    let exponent = match mode {
        0..=2 => {
            if func {
                return None;
            }
            (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
            -2
        }
        6 => {
            (r.mq, r.unit) = (Mq::DutyCycle, Unit::Percentage);
            -1
        }
        _ => return None,
    };
    let value = f32::from(u16::from_le_bytes([buf[6], buf[7]]));
    if value > 6000.0 {
        return None;
    }
    (r.value, r.digits) = scaled(value, exponent);
    Some(r)
}

/// The readings in an MS2115B packet: the main display on channel 0,
/// the sub display (frequency or duty cycle) on channel 1.
pub fn ms2115b_parse(buf: &[u8]) -> Vec<Reading> {
    if !ms2115b_valid(buf) {
        return Vec::new();
    }
    ms2115b_main(buf).into_iter().chain(ms2115b_sub(buf)).collect()
}

/// An MS8250D main display digit from its segment word; 0xf is the
/// overflow pattern.
fn ms8250d_digit(w: u16) -> Option<u8> {
    Some(match w {
        0x000 | 0x533 => 0,
        0x430 => 0xf,
        0x003 => 1,
        0x721 => 2,
        0x703 => 3,
        0x213 => 4,
        0x712 => 5,
        0x732 => 6,
        0x103 => 7,
        0x733 => 8,
        0x713 => 9,
        _ => return None,
    })
}

struct Ms8250dFlags {
    volt: bool,
    ohm: bool,
    ampere: bool,
    hz: bool,
    farad: bool,
    micro: bool,
    nano: bool,
    milli: bool,
    kilo: bool,
    mega: bool,
    rs232: bool,
    ac: bool,
    dc: bool,
    auto: bool,
    min: bool,
    max: bool,
    rel: bool,
    hold: bool,
    diode: bool,
    beep: bool,
}

fn ms8250d_flags(buf: &[u8]) -> Ms8250dFlags {
    let farad = buf[10] & 0x02 != 0;
    Ms8250dFlags {
        volt: buf[9] & 0x10 != 0,
        ohm: buf[9] & 0x40 != 0,
        ampere: buf[10] & 0x01 != 0,
        hz: buf[10] & 0x04 != 0,
        farad,
        micro: if farad { buf[9] & 0x02 != 0 } else { buf[8] & 0x10 != 0 },
        nano: buf[8] & 0x20 != 0,
        milli: buf[9] & 0x01 != 0,
        kilo: buf[9] & 0x04 != 0,
        mega: buf[8] & 0x40 != 0,
        rs232: buf[1] & 0x02 != 0,
        ac: buf[1] & 0x10 != 0,
        dc: buf[2] & 0x02 != 0,
        auto: buf[16] & 0x10 != 0,
        min: buf[16] & 0x04 != 0,
        max: buf[16] & 0x02 != 0,
        rel: buf[15] & 0x80 != 0,
        hold: buf[16] & 0x08 != 0,
        diode: buf[11] & 0x01 != 0,
        beep: buf[11] & 0x02 != 0,
    }
}

/// True for an MS8250D packet: a zero last byte, the RS232 flag, one
/// prefix and one function at most, not both AC and DC.
pub fn ms8250d_valid(buf: &[u8]) -> bool {
    if buf.len() < MS8250D_SIZE || buf[17] != 0 {
        return false;
    }
    let f = ms8250d_flags(buf);
    f.rs232
        && crate::count(&[f.nano, f.micro, f.milli, f.kilo, f.mega]) <= 1
        && crate::count(&[f.hz, f.ohm, f.farad, f.ampere, f.volt]) <= 1
        && !(f.ac && f.dc)
}

/// The main display of an MS8250D packet. The second display is not
/// reported, as in sigrok.
pub fn ms8250d_parse(buf: &[u8]) -> Option<Reading> {
    if !ms8250d_valid(buf) {
        return None;
    }
    let w = |hi: u8, mid: u8, lo: u8| (u16::from(hi) << 8) | u16::from(mid) | u16::from(lo);
    let d1 = ms8250d_digit(w(buf[3] & 0x07, buf[2] & 0x30, (buf[3] & 0x30) >> 4))?;
    let d2 = ms8250d_digit((u16::from(buf[4] & 0x73) << 4) | u16::from(buf[5] & 0x03))?;
    let d3 = ms8250d_digit(w(buf[6] & 0x07, buf[5] & 0x30, (buf[6] & 0x30) >> 4))?;
    let d4 = ms8250d_digit((u16::from(buf[7] & 0x73) << 4) | u16::from(buf[8] & 0x03))?;
    let mut exponent = if buf[3] & 0x40 != 0 {
        -3
    } else if buf[5] & 0x40 != 0 {
        -2
    } else if buf[7] & 0x04 != 0 {
        -1
    } else {
        0
    };
    let f = ms8250d_flags(buf);
    let sign = if buf[0] & 0x04 != 0 { -1.0 } else { 1.0 };
    let mut value = sign
        * f32::from(
            u16::from(d1) * 1000 + u16::from(d2) * 100 + u16::from(d3) * 10 + u16::from(d4),
        );
    if f.nano {
        exponent -= 9;
    }
    if f.micro {
        exponent -= 6;
    }
    if f.milli {
        exponent -= 3;
    }
    if f.kilo {
        exponent += 3;
    }
    if f.mega {
        exponent += 6;
    }
    value *= 10f32.powi(exponent);
    let mut r = Reading::new(value, Mq::Gain, Unit::Unitless, -exponent);
    if f.volt {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
    }
    if f.ampere {
        (r.mq, r.unit) = (Mq::Current, Unit::Ampere);
    }
    if f.ohm {
        (r.mq, r.unit) = (Mq::Resistance, Unit::Ohm);
    }
    if f.hz {
        (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
    }
    if f.farad {
        (r.mq, r.unit) = (Mq::Capacitance, Unit::Farad);
    }
    if f.beep {
        (r.mq, r.unit) = (Mq::Continuity, Unit::Boolean);
        r.value = 1.0;
    }
    if f.diode {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
        r.flags |= MqFlags::DIODE | MqFlags::DC;
    }
    if r.mq == Mq::Gain {
        return None;
    }
    if f.ac {
        r.flags |= MqFlags::AC;
    }
    if f.dc {
        r.flags |= MqFlags::DC;
    }
    if f.auto {
        r.flags |= MqFlags::AUTORANGE;
    }
    if f.hold {
        r.flags |= MqFlags::HOLD;
    }
    if f.rel {
        r.flags |= MqFlags::RELATIVE;
    }
    if f.min {
        r.flags |= MqFlags::MIN;
    }
    if f.max {
        r.flags |= MqFlags::MAX;
    }
    if d3 == 0xf {
        r.value = f32::INFINITY;
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ms2115b_volts_and_frequency() {
        // AC volts, 6.000 V range (1), value 1234 -> 1.234 V, with
        // 50.00 Hz on the sub display, autorange.
        let p = [0x55, 2, 1, 0, 0xd2, 0x04, 0x88, 0x13, 0];
        assert!(ms2115b_valid(&p));
        let r = ms2115b_parse(&p);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].mq, r[0].unit, r[0].digits), (Mq::Voltage, Unit::Volt, 3));
        assert!((r[0].value - 1.234).abs() < 1e-5);
        assert!(r[0].flags.contains(MqFlags::AC | MqFlags::AUTORANGE));
        assert_eq!((r[1].channel, r[1].mq, r[1].digits), (1, Mq::Frequency, 2));
        assert!((r[1].value - 50.0).abs() < 1e-4);
        // DC amps 600 A range, negative value, manual range: no sub.
        let p = [0x55, 8, 0, 0, 0x2e, 0xfb, 0, 0, 1];
        let r = ms2115b_parse(&p);
        assert_eq!(r.len(), 1);
        assert!((r[0].value + 123.4).abs() < 1e-3);
        assert!(r[0].flags.contains(MqFlags::DC) && !r[0].flags.contains(MqFlags::AUTORANGE));
        // Over the limit: no main reading, the sub display still shows.
        let p = [0x55, 2, 1, 0, 0x71, 0x17, 0, 0, 0];
        let r = ms2115b_parse(&p);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].channel, 1);
        // Continuity reads 1, capacitance 60.00 nF range.
        let p = [0x55, 3, 0, 0, 10, 0, 0, 0, 0];
        assert_eq!((ms2115b_parse(&p)[0].mq, ms2115b_parse(&p)[0].value), (Mq::Continuity, 1.0));
        let p = [0x55, 5, 1, 0, 0xe8, 0x03, 0, 0, 0];
        let r = ms2115b_parse(&p);
        assert!((r[0].value - 10.0e-9).abs() < 1e-12);
        assert_eq!(r[0].digits, 11);
    }

    /// An MS8250D packet showing digits `d` with a decimal point after
    /// digit `point` (1 to 3).
    fn ms8250d(d: [u16; 4], point: u8) -> [u8; 18] {
        let mut p = [0u8; 18];
        p[1] = 0x02; // RS232
        p[3] |= ((d[0] >> 8) & 7) as u8;
        p[2] |= (d[0] & 0x30) as u8;
        p[3] |= ((d[0] & 0x03) << 4) as u8;
        p[4] |= ((d[1] >> 4) & 0x73) as u8;
        p[5] |= (d[1] & 0x03) as u8;
        p[6] |= ((d[2] >> 8) & 7) as u8;
        p[5] |= (d[2] & 0x30) as u8;
        p[6] |= ((d[2] & 0x03) << 4) as u8;
        p[7] |= ((d[3] >> 4) & 0x73) as u8;
        p[8] |= (d[3] & 0x03) as u8;
        match point {
            1 => p[3] |= 0x40,
            2 => p[5] |= 0x40,
            3 => p[7] |= 0x04,
            _ => {}
        }
        p
    }

    #[test]
    fn ms8250d_packets() {
        // 12.34 V DC auto.
        let mut p = ms8250d([0x003, 0x721, 0x703, 0x213], 2);
        p[9] |= 0x10;
        p[2] |= 0x02;
        p[16] |= 0x10;
        assert!(ms8250d_valid(&p));
        let r = ms8250d_parse(&p).unwrap();
        assert_eq!((r.mq, r.unit, r.digits), (Mq::Voltage, Unit::Volt, 2));
        assert!((r.value - 12.34).abs() < 1e-4);
        assert!(r.flags.contains(MqFlags::DC | MqFlags::AUTORANGE));
        // -5.678 kOhm hold.
        let mut p = ms8250d([0x712, 0x732, 0x103, 0x733], 1);
        p[9] |= 0x40 | 0x04;
        p[0] |= 0x04;
        p[16] |= 0x08;
        let r = ms8250d_parse(&p).unwrap();
        assert_eq!((r.mq, r.digits), (Mq::Resistance, 0));
        assert!((r.value + 5678.0).abs() < 1e-2);
        assert!(r.flags.contains(MqFlags::HOLD));
        // Overflow in the third digit.
        let mut p = ms8250d([0x000, 0x533, 0x430, 0x533], 0);
        p[9] |= 0x40;
        assert!(ms8250d_parse(&p).unwrap().value.is_infinite());
        // No RS232 flag, both AC and DC, two functions: invalid.
        let mut q = p;
        q[1] = 0;
        assert!(!ms8250d_valid(&q));
        let mut q = p;
        q[1] |= 0x10;
        q[2] |= 0x02;
        assert!(!ms8250d_valid(&q));
        let mut q = p;
        q[9] |= 0x10;
        assert!(!ms8250d_valid(&q));
    }
}
