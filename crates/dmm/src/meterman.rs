// Counterpart in sigrok:
//   libsigrok/src/dmm/mm38xr.c,
//   Copyright (C) 2021 Christian Kulp <cs@kulp.net>,
//   with commits by Gerhard Sittig
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! The Meterman 38XR: 15 byte packets of hex digits, with the range
//! deciding the scale per function.

use crate::Reading;
use oxrok_core::{Mq, MqFlags, Unit};

/// 38XR: 15 bytes.
pub const MM38XR_SIZE: usize = 15;

const OVERLOAD: u32 = 0xb0dd;
const BAD_JACK: u32 = 0xbaab;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Volts,
    Ohms,
    MicroAmps,
    MilliAmps,
    Amps,
    Capacitance,
    Diode,
    TempC,
    TempF,
    Frequency,
    InductanceH,
    InductanceMh,
    Dbm,
    Duty,
    Continuity,
}

impl Mode {
    fn decimals(self) -> [i32; 7] {
        match self {
            Mode::Volts => [1, 3, 2, 1, 0, 0, 0],
            Mode::Ohms => [2, 3, 4, 2, 3, 1, 0],
            Mode::MicroAmps => [2, 1, 0, 0, 0, 0, 0],
            Mode::MilliAmps | Mode::InductanceMh => [3, 2, 1, 0, 0, 0, 0],
            Mode::Amps => [3, 0, 0, 0, 0, 0, 0],
            Mode::Capacitance => [2, 1, 3, 2, 1, 0, 0],
            Mode::Diode => [0, 3, 0, 0, 0, 0, 0],
            Mode::TempC | Mode::TempF => [0; 7],
            Mode::Frequency => [2, 1, 3, 2, 1, 3, 2],
            Mode::InductanceH => [0, 0, 0, 3, 2, 0, 0],
            Mode::Dbm | Mode::Duty => [2; 7],
            Mode::Continuity => [0, 0, 0, 0, 0, 1, 0],
        }
    }

    fn exponents(self) -> [i32; 7] {
        match self {
            Mode::Volts => [-3, 0, 0, 0, 0, 0, 0],
            Mode::Ohms => [6, 6, 6, 3, 3, 0, 0],
            Mode::MicroAmps => [-6, -6, 0, 0, 0, 0, 0],
            Mode::MilliAmps | Mode::InductanceMh => [-3, -3, -3, 0, 0, 0, 0],
            Mode::Capacitance => [-9, -9, -6, -6, -6, 0, 0],
            Mode::Frequency => [0, 0, 3, 3, 3, 6, 6],
            _ => [0; 7],
        }
    }
}

fn nibble(c: u8) -> u32 {
    if c <= b'9' { u32::from(c - b'0') } else { u32::from(c - b'A' + 10) }
}

/// True for a 38XR packet: 13 hex digits, CR LF, a function code 1 to
/// 16.
pub fn mm38xr_valid(buf: &[u8]) -> bool {
    if buf.len() < MM38XR_SIZE || buf[13] != b'\r' || buf[14] != b'\n' {
        return false;
    }
    if !buf[..13].iter().all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(c)) {
        return false;
    }
    let code = nibble(buf[0]) << 4 | nibble(buf[1]);
    (1..=0x10).contains(&code)
}

/// The reading in a 38XR packet.
pub fn mm38xr_parse(buf: &[u8]) -> Option<Reading> {
    if !mm38xr_valid(buf) {
        return None;
    }
    let code = nibble(buf[0]) << 4 | nibble(buf[1]);
    let reading = if buf[2] > b'A' {
        nibble(buf[2]) << 12 | nibble(buf[3]) << 8 | nibble(buf[4]) << 4 | nibble(buf[5])
    } else {
        nibble(buf[2]) * 1000 + nibble(buf[3]) * 100 + nibble(buf[4]) * 10 + nibble(buf[5])
    };
    let range = nibble(buf[8]) as usize;
    if range > 6 {
        return None;
    }
    let amps_function = nibble(buf[9]);
    let peak = nibble(buf[10]);
    let rflag_h = nibble(buf[11]);
    let rflag_l = nibble(buf[12]);
    let current_acdc = || match amps_function {
        1 => MqFlags::AC,
        2 => MqFlags::AC | MqFlags::DC,
        _ => MqFlags::DC,
    };
    let (mode, acdc) = match code {
        0x02 => (Mode::TempF, MqFlags::empty()),
        0x03 | 0x0e => (Mode::MilliAmps, current_acdc()),
        0x04 => (Mode::Diode, MqFlags::DC),
        0x05 => (Mode::InductanceH, MqFlags::empty()),
        0x06 => (Mode::TempC, MqFlags::empty()),
        0x07 => (Mode::MicroAmps, current_acdc()),
        0x08 => (if rflag_l == 0x08 { Mode::Continuity } else { Mode::Ohms }, MqFlags::empty()),
        0x09 => (Mode::InductanceMh, MqFlags::empty()),
        0x0a => (Mode::Amps, current_acdc()),
        0x0b => (Mode::Capacitance, MqFlags::empty()),
        0x0c => {
            (Mode::Volts, if rflag_l == 0x02 { MqFlags::AC | MqFlags::DC } else { MqFlags::DC })
        }
        0x0f => (if rflag_h == 0x0b { Mode::Duty } else { Mode::Frequency }, MqFlags::empty()),
        0x10 => {
            (if rflag_l == 0x04 || rflag_l == 0x05 { Mode::Dbm } else { Mode::Volts }, MqFlags::AC)
        }
        _ => return None,
    };
    let (mq, unit) = match mode {
        Mode::Volts => (Mq::Voltage, Unit::Volt),
        Mode::Ohms => (Mq::Resistance, Unit::Ohm),
        Mode::MicroAmps | Mode::MilliAmps | Mode::Amps => (Mq::Current, Unit::Ampere),
        Mode::Capacitance => (Mq::Capacitance, Unit::Farad),
        Mode::Diode => (Mq::Voltage, Unit::Volt),
        Mode::TempC => (Mq::Temperature, Unit::Celsius),
        Mode::TempF => (Mq::Temperature, Unit::Fahrenheit),
        Mode::Frequency => (Mq::Frequency, Unit::Hertz),
        Mode::InductanceH | Mode::InductanceMh => (Mq::SeriesInductance, Unit::Henry),
        Mode::Dbm => (Mq::Voltage, Unit::DecibelMw),
        Mode::Duty => (Mq::DutyCycle, Unit::Percentage),
        Mode::Continuity => (Mq::Continuity, Unit::Boolean),
    };
    let mut r = Reading::new(0.0, mq, unit, 0);
    r.flags |= acdc;
    if mode == Mode::Diode {
        r.flags |= MqFlags::DIODE;
    }
    if mode == Mode::Dbm {
        r.flags |= MqFlags::AC;
    }
    if peak == 0x02 || peak == 0x0a {
        r.flags |= MqFlags::MAX;
    }
    if peak == 0x03 || peak == 0x0b {
        r.flags |= MqFlags::MIN;
    }
    if rflag_h == 0x0a || peak == 0x0b {
        r.flags |= MqFlags::AUTORANGE;
    }
    if mode == Mode::Continuity {
        r.value = if reading == OVERLOAD { 0.0 } else { 1.0 };
        return Some(r);
    }
    if reading == OVERLOAD || reading == BAD_JACK {
        // Over the limit the C code returns before it sets the unit,
        // so nothing is sent.
        return None;
    }
    let exponent = mode.exponents()[range] - mode.decimals()[range];
    let negative = rflag_l == 0x01 || (mode == Mode::Dbm && rflag_l == 0x05);
    r.value = reading as f32 * if negative { -1.0 } else { 1.0 } * 10f32.powi(exponent);
    r.digits = -exponent;
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(
        code: &str,
        digits: &str,
        range: char,
        amps: char,
        peak: char,
        rh: char,
        rl: char,
    ) -> Vec<u8> {
        format!("{code}{digits}00{range}{amps}{peak}{rh}{rl}\r\n").into_bytes()
    }

    #[test]
    fn mm38xr_packets() {
        // DC volts, range 1 (x.xxx V), 1234 -> 1.234 V, autorange.
        let p = packet("0C", "1234", '1', '0', '0', 'A', '0');
        assert!(mm38xr_valid(&p));
        let r = mm38xr_parse(&p).unwrap();
        assert_eq!((r.mq, r.unit, r.digits), (Mq::Voltage, Unit::Volt, 3));
        assert!((r.value - 1.234).abs() < 1e-5);
        assert!(r.flags.contains(MqFlags::DC | MqFlags::AUTORANGE));
        // Negative mA, AC, range 0: 123.4 -> 12.34 mA? range 0 has 3
        // decimals and exponent -3: 1234 -> 1.234 mA.
        let p = packet("0E", "1234", '0', '1', '0', '0', '1');
        let r = mm38xr_parse(&p).unwrap();
        assert_eq!(r.mq, Mq::Current);
        assert!((r.value + 1.234e-3).abs() < 1e-8);
        assert!(r.flags.contains(MqFlags::AC));
        assert_eq!(r.digits, 6);
        // Resistance over limit: nothing. Continuity over limit: 0.
        let p = packet("08", "B0DD", '2', '0', '0', '0', '0');
        assert!(mm38xr_parse(&p).is_none());
        let p = packet("08", "B0DD", '2', '0', '0', '0', '8');
        let r = mm38xr_parse(&p).unwrap();
        assert_eq!((r.mq, r.value), (Mq::Continuity, 0.0));
        // dBm negative, max hold.
        let p = packet("10", "0150", '3', '0', '2', '0', '5');
        let r = mm38xr_parse(&p).unwrap();
        assert_eq!(r.unit, Unit::DecibelMw);
        assert!((r.value + 1.5).abs() < 1e-5);
        assert!(r.flags.contains(MqFlags::MAX | MqFlags::AC));
        // Invalid: a lowercase digit, a bad terminator, code 0.
        assert!(!mm38xr_valid(b"0c123400100000\r\n"));
        assert!(!mm38xr_valid(b"0C1234001000000\n\n"));
        assert!(!mm38xr_valid(b"0012340010000\r\n"));
    }
}
