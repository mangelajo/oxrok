// Counterpart in sigrok:
//   libsigrok/src/dmm/eev121gw.c,
//   Copyright (C) 2018-2019 Gerhard Sittig <gerhard.sittig@gmx.net>,
//   with commits by Uwe Hermann, Frank Stettner
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! The EEVblog 121GW: 19 byte binary packets with a main display, a
//! secondary display and a bargraph, each its own channel.

use crate::core::{Mq, MqFlags, Unit};
use crate::dmm::Reading;

/// 121GW: 19 bytes.
pub const EEV121GW_SIZE: usize = 19;
/// Displays per packet.
pub const EEV121GW_DISPLAYS: usize = 3;
/// Channel names.
pub const EEV121GW_CHANNELS: [&str; 3] = ["main", "sub", "bar"];

const OFF_MAIN_MODE: usize = 5;
const OFF_MAIN_RANGE: usize = 6;
const OFF_MAIN_VAL: usize = 7;
const OFF_SUB_MODE: usize = 9;
const OFF_SUB_RANGE: usize = 10;
const OFF_SUB_VAL: usize = 11;
const OFF_BAR_STATUS: usize = 13;
const OFF_BAR_VALUE: usize = 14;
const OFF_ICON1: usize = 15;
const OFF_ICON2: usize = 16;
const OFF_ICON3: usize = 17;
const OFF_CHECKSUM: usize = 18;

/// True for a 121GW packet: the start byte, zero reserved bits and the
/// XOR checksum.
pub fn eev121gw_valid(buf: &[u8]) -> bool {
    if buf.len() < EEV121GW_SIZE || buf[0] != 0xf2 {
        return false;
    }
    if buf[OFF_MAIN_MODE] & 0x20 != 0
        || buf[OFF_SUB_RANGE] & 0x08 != 0
        || buf[OFF_BAR_STATUS] & 0xe0 != 0
        || buf[OFF_BAR_VALUE] & 0xc0 != 0
        || buf[OFF_ICON3] & 0x80 != 0
    {
        return false;
    }
    buf[..OFF_CHECKSUM].iter().fold(0u8, |a, b| a ^ b) == buf[OFF_CHECKSUM]
}

/// Modes of the main display (0 to 24) and the extra ones of the sub
/// display (100 and up).
mod mode {
    pub const LOW_Z: u8 = 0;
    pub const DC_V: u8 = 1;
    pub const AC_V: u8 = 2;
    pub const DC_MV: u8 = 3;
    pub const AC_MV: u8 = 4;
    pub const TEMP: u8 = 5;
    pub const FREQ: u8 = 6;
    pub const PERIOD: u8 = 7;
    pub const DUTY: u8 = 8;
    pub const RES: u8 = 9;
    pub const CONT: u8 = 10;
    pub const DIODE: u8 = 11;
    pub const CAP: u8 = 12;
    pub const AC_UVA: u8 = 13;
    pub const AC_MVA: u8 = 14;
    pub const AC_VA: u8 = 15;
    pub const AC_UA: u8 = 16;
    pub const DC_UA: u8 = 17;
    pub const AC_MA: u8 = 18;
    pub const DC_MA: u8 = 19;
    pub const AC_A: u8 = 20;
    pub const DC_A: u8 = 21;
    pub const DC_UVA: u8 = 22;
    pub const DC_MVA: u8 = 23;
    pub const DC_VA: u8 = 24;
    pub const SUB_TEMPC: u8 = 100;
    pub const SUB_TEMPF: u8 = 105;
    pub const SUB_BATT: u8 = 110;
    pub const SUB_APO_ON: u8 = 120;
    pub const SUB_APO_OFF: u8 = 125;
    pub const SUB_YEAR: u8 = 130;
    pub const SUB_DATE: u8 = 135;
    pub const SUB_TIME: u8 = 140;
    pub const SUB_B_VOLT: u8 = 150;
    pub const SUB_LCD: u8 = 160;
    pub const SUB_CONT_PARM_0: u8 = 170;
    pub const SUB_CONT_PARM_3: u8 = 173;
    pub const SUB_DBM: u8 = 180;
}

/// The scale of a main display mode per range: the power of ten the
/// mantissa is divided by, which is also the digit count.
fn main_scale(m: u8, range: usize) -> Option<i32> {
    let ranges: &[i32] = match m {
        mode::LOW_Z => &[1],
        mode::DC_V | mode::AC_V => &[4, 3, 2, 1],
        mode::DC_MV | mode::AC_MV => &[6, 5],
        mode::TEMP => &[1],
        mode::FREQ => &[3, 2, 1, 0, -1],
        mode::PERIOD => &[7, 6, 5],
        mode::DUTY => &[1],
        mode::RES => &[3, 2, 1, 0, -1, -2, -3],
        mode::CONT => &[2],
        mode::DIODE => &[4, 3],
        mode::CAP => &[11, 10, 9, 8, 7, 5],
        mode::DC_VA | mode::AC_VA => &[4, 3, 3, 2],
        mode::DC_MVA | mode::AC_MVA => &[6, 5, 5, 4],
        mode::DC_UVA | mode::AC_UVA => &[8, 7, 7, 6],
        mode::DC_A | mode::AC_A => &[5, 4, 3],
        mode::DC_MA | mode::AC_MA => &[7, 6],
        mode::DC_UA | mode::AC_UA => &[9, 8],
        _ => return None,
    };
    ranges.get(range).copied()
}

/// The scale of a sub display mode per "range", which is the position
/// of the decimal point there. `None` entries are unused positions.
fn sub_scale(m: u8, range: usize) -> Option<i32> {
    let ranges: &[Option<i32>] = match m {
        mode::DC_V | mode::AC_V => &[None, None, None, Some(3), Some(4)],
        mode::DC_A | mode::AC_A | mode::DC_MA | mode::AC_MA => {
            &[None, None, Some(5), Some(6), Some(7)]
        }
        mode::DC_UA | mode::AC_UA => &[None, None, None, None, Some(7)],
        mode::FREQ => &[None, Some(1), Some(2), Some(3)],
        mode::DIODE => &[Some(0)],
        mode::SUB_TEMPC | mode::SUB_TEMPF => &[None, Some(1)],
        mode::SUB_BATT => &[None, Some(1)],
        mode::SUB_DBM => &[None, Some(1), Some(2), Some(3)],
        _ => return None,
    };
    ranges.get(range).copied().flatten()
}

fn reading(mq: Mq, unit: Unit) -> Reading {
    Reading::new(0.0, mq, unit, 0)
}

fn apply(mut r: Reading, mantissa: u32, factor: i32, digits: i32, ofl: bool, neg: bool) -> Reading {
    let mut v = mantissa as f32;
    if factor != 0 {
        v *= 10f32.powi(-factor);
    }
    if ofl {
        v = f32::INFINITY;
    }
    if neg {
        v = -v;
    }
    r.value = v;
    r.digits = digits;
    r
}

fn main_display(buf: &[u8]) -> Option<Reading> {
    let m = buf[OFF_MAIN_MODE] & 0x1f;
    let range = usize::from(buf[OFF_MAIN_RANGE] & 0x0f);
    let scale = main_scale(m, range)?;
    let mantissa = u32::from(u16::from_be_bytes([buf[OFF_MAIN_VAL], buf[OFF_MAIN_VAL + 1]]))
        | u32::from(buf[OFF_MAIN_MODE] >> 6) << 16;
    let ofl = buf[OFF_MAIN_RANGE] & 0x80 != 0;
    let icon1 = buf[OFF_ICON1];
    let icon2 = buf[OFF_ICON2];
    let icon3 = buf[OFF_ICON3];
    let use_sign = match m {
        mode::LOW_Z => {
            let mut dc = icon3 & 0x01 != 0;
            if icon3 & 0x02 != 0 {
                dc = false;
            }
            dc
        }
        mode::DC_V
        | mode::DC_MV
        | mode::TEMP
        | mode::DC_UVA
        | mode::DC_MVA
        | mode::DC_VA
        | mode::DC_UA
        | mode::DC_MA
        | mode::DC_A => true,
        _ => false,
    };
    let neg = use_sign && buf[OFF_MAIN_RANGE] & 0x40 != 0;
    let mut r = match m {
        mode::LOW_Z => reading(Mq::Voltage, Unit::Volt),
        mode::DC_V | mode::DC_MV => {
            let mut r = reading(Mq::Voltage, Unit::Volt);
            r.flags |= MqFlags::DC;
            r
        }
        mode::AC_V | mode::AC_MV => {
            let mut r = reading(Mq::Voltage, Unit::Volt);
            r.flags |= MqFlags::AC;
            r
        }
        mode::TEMP => {
            let unit = if buf[OFF_MAIN_RANGE] & 0x10 != 0 {
                Unit::Fahrenheit
            } else if buf[OFF_MAIN_RANGE] & 0x20 != 0 {
                Unit::Celsius
            } else {
                Unit::Unitless
            };
            reading(Mq::Temperature, unit)
        }
        mode::FREQ => reading(Mq::Frequency, Unit::Hertz),
        mode::PERIOD => reading(Mq::Time, Unit::Second),
        mode::DUTY => reading(Mq::DutyCycle, Unit::Percentage),
        mode::RES => reading(Mq::Resistance, Unit::Ohm),
        mode::CONT => reading(Mq::Continuity, Unit::Ohm),
        mode::DIODE => {
            let mut r = reading(Mq::Voltage, Unit::Volt);
            r.flags |= MqFlags::DIODE | MqFlags::DC;
            r
        }
        mode::CAP => reading(Mq::Capacitance, Unit::Farad),
        mode::AC_UVA | mode::AC_MVA | mode::AC_VA => {
            let mut r = reading(Mq::Power, Unit::VoltAmpere);
            r.flags |= MqFlags::AC;
            r
        }
        mode::DC_UVA | mode::DC_MVA | mode::DC_VA => {
            let mut r = reading(Mq::Power, Unit::VoltAmpere);
            r.flags |= MqFlags::DC;
            r
        }
        mode::AC_UA | mode::AC_MA | mode::AC_A => {
            let mut r = reading(Mq::Current, Unit::Ampere);
            r.flags |= MqFlags::AC;
            r
        }
        mode::DC_UA | mode::DC_MA | mode::DC_A => {
            let mut r = reading(Mq::Current, Unit::Ampere);
            r.flags |= MqFlags::DC;
            r
        }
        _ => return None,
    };
    match (icon1 >> 3) & 3 {
        1 => r.flags |= MqFlags::DC,
        2 => r.flags |= MqFlags::AC,
        3 => r.flags |= MqFlags::AC | MqFlags::DC,
        _ => {}
    }
    if icon1 & 0x04 != 0 {
        r.flags |= MqFlags::AUTORANGE;
    }
    if icon2 & 0x20 != 0 {
        // Taken for the 20 mA loop current indicator, shown in percent.
        r.unit = Unit::Percentage;
    }
    if icon2 & 0x10 != 0 {
        r.flags |= MqFlags::RELATIVE;
    }
    if (icon3 >> 4) & 3 != 0 {
        r.flags |= MqFlags::REFERENCE;
    }
    if (icon3 >> 2) & 3 != 0 {
        r.flags |= MqFlags::HOLD;
    }
    if icon3 & 0x02 != 0 {
        r.flags |= MqFlags::AC;
    }
    if icon3 & 0x01 != 0 {
        r.flags |= MqFlags::DC;
    }
    Some(apply(r, mantissa, scale, scale, ofl, neg))
}

fn sub_display(buf: &[u8]) -> Option<Reading> {
    let m = buf[OFF_SUB_MODE];
    let range = usize::from(buf[OFF_SUB_RANGE] & 0x07);
    let scale = sub_scale(m, range)?;
    let mut factor = scale;
    let mut digits = scale;
    let mut mantissa = u32::from(u16::from_be_bytes([buf[OFF_SUB_VAL], buf[OFF_SUB_VAL + 1]]));
    let mut ofl = buf[OFF_SUB_RANGE] & 0x80 != 0;
    let use_sign = matches!(
        m,
        mode::DC_V
            | mode::AC_V
            | mode::DC_A
            | mode::AC_A
            | mode::SUB_TEMPC
            | mode::SUB_TEMPF
            | mode::SUB_B_VOLT
            | mode::SUB_DBM
    );
    let mut neg = use_sign && buf[OFF_SUB_RANGE] & 0x40 != 0;
    let is_k = buf[OFF_SUB_RANGE] & 0x20 != 0;
    let mut r = match m {
        mode::DC_V => reading(Mq::Voltage, Unit::Volt),
        mode::DC_A => reading(Mq::Current, Unit::Ampere),
        mode::FREQ => {
            if is_k {
                factor -= 3;
                digits -= 3;
            }
            ofl = false;
            reading(Mq::Frequency, Unit::Hertz)
        }
        mode::SUB_TEMPC => reading(Mq::Temperature, Unit::Celsius),
        mode::SUB_TEMPF => reading(Mq::Temperature, Unit::Fahrenheit),
        mode::SUB_BATT => reading(Mq::Voltage, Unit::Volt),
        mode::SUB_DBM => reading(Mq::Gain, Unit::DecibelVolt),
        mode::SUB_CONT_PARM_0..=mode::SUB_CONT_PARM_3 => {
            let code = m - mode::SUB_CONT_PARM_0;
            mantissa = if code & 0x02 != 0 { 300 } else { 30 };
            neg = false;
            ofl = false;
            factor = 0;
            digits = 0;
            reading(Mq::Resistance, Unit::Ohm)
        }
        mode::DIODE => reading(Mq::Voltage, Unit::Volt),
        mode::SUB_APO_ON
        | mode::SUB_APO_OFF
        | mode::SUB_LCD
        | mode::SUB_YEAR
        | mode::SUB_DATE
        | mode::SUB_TIME => return None,
        _ => return None,
    };
    r.channel = 1;
    Some(apply(r, mantissa, factor, digits, ofl, neg))
}

fn bar_display(buf: &[u8]) -> Option<Reading> {
    let status = buf[OFF_BAR_STATUS];
    let value = buf[OFF_BAR_VALUE];
    if status & 0x10 != 0 || value & 0x20 != 0 {
        return None;
    }
    let ticks = u32::from(value & 0x1f).min(25);
    let neg = status & 0x04 != 0;
    let (mantissa, factor, digits) = match status & 3 {
        0 => (ticks * (5000 / 25), 3, 1),
        1 => (ticks * (50 / 25), 0, 0),
        2 => (ticks * (500 / 25), 0, -1),
        _ => (ticks * (1000 / 25), 0, -1),
    };
    let mut r = reading(Mq::Count, Unit::Unitless);
    r.channel = 2;
    Some(apply(r, mantissa, factor, digits, false, neg))
}

/// The readings in a 121GW packet: main display, sub display and
/// bargraph on channels 0, 1 and 2, each only when it shows a value.
pub fn eev121gw_parse(buf: &[u8]) -> Vec<Reading> {
    if !eev121gw_valid(buf) {
        return Vec::new();
    }
    main_display(buf).into_iter().chain(sub_display(buf)).chain(bar_display(buf)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn packet(
        main_mode: u8,
        main_range: u8,
        main_val: u32,
        sub_mode: u8,
        sub_range: u8,
        sub_val: u16,
        bar: [u8; 2],
        icons: [u8; 3],
    ) -> [u8; 19] {
        let mut p = [0u8; 19];
        p[0] = 0xf2;
        p[OFF_MAIN_MODE] = main_mode | ((main_val >> 16) as u8) << 6;
        p[OFF_MAIN_RANGE] = main_range;
        p[OFF_MAIN_VAL..OFF_MAIN_VAL + 2].copy_from_slice(&(main_val as u16).to_be_bytes());
        p[OFF_SUB_MODE] = sub_mode;
        p[OFF_SUB_RANGE] = sub_range;
        p[OFF_SUB_VAL..OFF_SUB_VAL + 2].copy_from_slice(&sub_val.to_be_bytes());
        p[OFF_BAR_STATUS] = bar[0];
        p[OFF_BAR_VALUE] = bar[1];
        p[OFF_ICON1..OFF_ICON1 + 3].copy_from_slice(&icons);
        p[OFF_CHECKSUM] = p[..OFF_CHECKSUM].iter().fold(0, |a, b| a ^ b);
        p
    }

    #[test]
    fn eev121gw_three_displays() {
        // DC 5.0000 V range, 12345 -> 1.2345 V, autorange, sub shows
        // 50.00 Hz, bargraph 10 of 25 ticks on the 5.0 range.
        let p = packet(mode::DC_V, 0, 12345, mode::FREQ, 2, 5000, [0x00, 10], [0x04, 0, 0]);
        assert!(eev121gw_valid(&p));
        let r = eev121gw_parse(&p);
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].mq, r[0].unit, r[0].digits), (Mq::Voltage, Unit::Volt, 4));
        assert!((r[0].value - 1.2345).abs() < 1e-5);
        assert!(r[0].flags.contains(MqFlags::DC | MqFlags::AUTORANGE));
        assert_eq!((r[1].channel, r[1].mq, r[1].digits), (1, Mq::Frequency, 2));
        assert!((r[1].value - 50.0).abs() < 1e-4);
        assert_eq!((r[2].channel, r[2].mq, r[2].digits), (2, Mq::Count, 1));
        assert!((r[2].value - 2.0).abs() < 1e-5);
        // Bad checksum.
        let mut q = p;
        q[OFF_CHECKSUM] ^= 1;
        assert!(!eev121gw_valid(&q));
    }

    #[test]
    fn eev121gw_sign_overflow_and_blank_sub() {
        // Negative DC mA (range 1: 50.000 mA), overflow flag, the sub
        // display shows the time (ignored), bargraph blanked.
        let p =
            packet(mode::DC_MA, 0x80 | 0x40 | 1, 123, mode::SUB_TIME, 0, 0, [0x10, 0], [0, 0, 0]);
        let r = eev121gw_parse(&p);
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].mq, r[0].digits), (Mq::Current, 6));
        assert!(r[0].value.is_infinite() && r[0].value < 0.0);
        // AC volts with a kHz sub display: 9.999 kHz range is point 3,
        // k flag shifts by three.
        let p = packet(mode::AC_V, 1, 23000, mode::FREQ, 0x20 | 3, 9999, [0x10, 0], [0, 0, 0]);
        let r = eev121gw_parse(&p);
        assert_eq!(r.len(), 2);
        assert!((r[0].value - 23.0).abs() < 1e-4);
        assert!(r[0].flags.contains(MqFlags::AC));
        assert!((r[1].value - 9999.0).abs() < 1e-2);
        assert_eq!(r[1].digits, 0);
        // Continuity parameters on the sub display have no scale
        // table entry, so they give no reading, as in the C code.
        let p = packet(mode::CONT, 0, 100, 172, 0, 0, [0x10, 0], [0, 0, 0]);
        let r = eev121gw_parse(&p);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].mq, Mq::Continuity);
        // An unknown main mode gives no reading.
        let p = packet(25, 0, 0, mode::SUB_TIME, 0, 0, [0x10, 0], [0, 0, 0]);
        assert!(eev121gw_parse(&p).is_empty());
    }
}
