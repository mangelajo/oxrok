// Counterpart in sigrok:
//   libsigrok/src/dmm/ut71x.c,
//   libsigrok/src/dmm/es519xx.c,
//   libsigrok/src/dmm/vc870.c,
//   Copyright (C) 2013-2014 Uwe Hermann <uwe@hermann-uwe.de>,
//   Copyright (C) 2014 Aurelien Jacobs <aurel@gnuage.org>,
//   with commits by Bert Vermeulen, Daniel Elstner, Gerhard Sittig,
//   Martin Ling, Frank Stettner, Soeren Apel
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! Protocols that send digits plus a range index, with the decimal
//! exponent looked up per function and range: the UNI-T UT71x, the
//! Cyrustek ES519xx family and the Voltcraft VC-870.

use crate::{Reading, count, scaled};
use oxrok_core::{Mq, MqFlags, Unit};

/// UT71x: 11 bytes.
pub const UT71X_SIZE: usize = 11;
/// ES519xx with 11 byte packets: the packet is sent twice.
pub const ES519XX_11B_SIZE: usize = 22;
/// ES519xx with 14 byte packets.
pub const ES519XX_14B_SIZE: usize = 14;
/// VC-870: 23 bytes.
pub const VC870_SIZE: usize = 23;

const UT71X_EXPONENTS: [[i32; 8]; 16] = [
    [-5, 0, 0, 0, 0, 0, 0, 0],
    [0, -4, -3, -2, -1, 0, 0, 0],
    [0, -4, -3, -2, -1, 0, 0, 0],
    [-5, 0, 0, 0, 0, 0, 0, 0],
    [0, -2, -1, 0, 1, 2, 3, 0],
    [0, -12, -11, -10, -9, -8, -7, -6],
    [-1, 0, 0, 0, 0, 0, 0, 0],
    [-8, -7, 0, 0, 0, 0, 0, 0],
    [-6, -5, 0, 0, 0, 0, 0, 0],
    [0, -3, 0, 0, 0, 0, 0, 0],
    [-1, 0, 0, 0, 0, 0, 0, 0],
    [-4, 0, 0, 0, 0, 0, 0, 0],
    [-3, -2, -1, 0, 1, 2, 3, 4],
    [-1, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [-2, 0, 0, 0, 0, 0, 0, 0],
];

fn digit(b: u8) -> Option<u32> {
    b.is_ascii_digit().then(|| u32::from(b - b'0'))
}

/// True for a UT71x packet.
pub fn ut71x_valid(buf: &[u8]) -> bool {
    buf.len() >= UT71X_SIZE && buf[9] == b'\r' && buf[10] == b'\n' && ut71x_mode(buf).is_some()
}

/// The function, flags and validity of a UT71x packet.
struct Ut71xMode {
    function: usize,
    ac: bool,
    dc: bool,
    auto: bool,
    sign: bool,
    duty_cycle: bool,
}

fn ut71x_mode(buf: &[u8]) -> Option<Ut71xMode> {
    let function = usize::from(buf[6].checked_sub(b'0')?);
    if function > 15 {
        return None;
    }
    let auto = buf[8] & 1 != 0;
    let manual = buf[8] & 2 != 0;
    if auto && manual {
        return None;
    }
    let mut sign = buf[8] & 4 != 0;
    let mut duty_cycle = false;
    if function == 12 && sign {
        duty_cycle = true;
        sign = false;
    }
    Some(Ut71xMode { function, ac: buf[7] & 1 != 0, dc: buf[7] & 2 != 0, auto, sign, duty_cycle })
}

/// Parses a UT71x packet.
pub fn ut71x_parse(buf: &[u8]) -> Option<Reading> {
    if !ut71x_valid(buf) {
        return None;
    }
    let m = ut71x_mode(buf)?;
    let value = if &buf[..5] == b"::0<:" || &buf[..5] == b":<0::" {
        f32::INFINITY
    } else {
        let n = if buf[4] == b':' { 4 } else { 5 };
        let mut v = 0u32;
        for &c in &buf[..n] {
            v = v * 10 + digit(c)?;
        }
        if n == 4 {
            v *= 10;
        }
        v as f32 * if m.sign { -1.0 } else { 1.0 }
    };
    let range = usize::from(buf[5].checked_sub(b'0')?);
    if range > 7 {
        return None;
    }
    let (v, digits) = scaled(value, UT71X_EXPONENTS[m.function][range]);
    let mut r = Reading::new(v, Mq::Gain, Unit::Unitless, digits);
    (r.mq, r.unit) = match m.function {
        0..=3 => (Mq::Voltage, Unit::Volt),
        4 => (Mq::Resistance, Unit::Ohm),
        5 => (Mq::Capacitance, Unit::Farad),
        6 => (Mq::Temperature, Unit::Celsius),
        7..=9 => (Mq::Current, Unit::Ampere),
        10 => {
            r.value = if r.value < 0.0 || r.value > 60.0 { 0.0 } else { 1.0 };
            (Mq::Continuity, Unit::Boolean)
        }
        11 => (Mq::Voltage, Unit::Volt),
        12 if m.duty_cycle => (Mq::DutyCycle, Unit::Percentage),
        12 => (Mq::Frequency, Unit::Hertz),
        13 => (Mq::Temperature, Unit::Fahrenheit),
        14 => (Mq::Power, Unit::Watt),
        _ => (Mq::Current, Unit::Percentage),
    };
    if m.ac {
        r.flags |= MqFlags::AC | MqFlags::RMS;
    }
    if m.dc {
        r.flags |= MqFlags::DC;
    }
    if m.auto {
        r.flags |= MqFlags::AUTORANGE;
    }
    if m.function == 11 {
        r.flags |= MqFlags::DIODE | MqFlags::DC;
    }
    Some(r)
}

/// The ES519xx variants: baud rate, packet length and which option
/// bytes the meter sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Es519xx {
    /// 2400 baud, 11 bytes.
    Baud2400x11,
    /// 2400 baud, 11 bytes, alternate function codes.
    Baud2400x11AltFn,
    /// 19200 baud, 11 bytes, 5 digit display.
    Baud19200x11x5Digits,
    /// 19200 baud, 11 bytes, clamp meter.
    Baud19200x11Clamp,
    /// 19200 baud, 11 bytes.
    Baud19200x11,
    /// 19200 baud, 14 bytes.
    Baud19200x14,
    /// 19200 baud, 14 bytes, selectable low pass filter.
    Baud19200x14SelLpf,
}

impl Es519xx {
    fn packet_size(self) -> usize {
        match self {
            Es519xx::Baud19200x14 | Es519xx::Baud19200x14SelLpf => 14,
            _ => 11,
        }
    }

    /// Bytes per protocol packet, as the serial stream carries them.
    pub fn stream_size(self) -> usize {
        if self.packet_size() == 11 { ES519XX_11B_SIZE } else { ES519XX_14B_SIZE }
    }
}

const ES_2400_11B: [[i32; 8]; 9] = [
    [-4, -3, -2, -1, 0, 0, 0, 0],
    [-7, -6, 0, 0, 0, 0, 0, 0],
    [-5, -4, 0, 0, 0, 0, 0, 0],
    [-2, 0, 0, 0, 0, 0, 0, 0],
    [1, 2, 3, 4, 5, 6, 0, 0],
    [-1, 0, 1, 2, 3, 4, 0, 0],
    [0, 1, 2, 3, 4, 5, 0, 0],
    [-12, -11, -10, -9, -8, -7, -6, -5],
    [-3, 0, 0, 0, 0, 0, 0, 0],
];
const ES_19200_11B_5DIGITS: [[i32; 8]; 9] = [
    [-4, -3, -2, -1, -5, 0, 0, 0],
    [-8, -7, 0, 0, 0, 0, 0, 0],
    [-6, -5, 0, 0, 0, 0, 0, 0],
    [0, -3, 0, 0, 0, 0, 0, 0],
    [-4, -3, -2, -1, 0, 0, 0, 0],
    [-2, -1, 0, 1, 2, 3, 4, 0],
    [-1, 0, 0, 1, 2, 3, 4, 0],
    [-12, -11, -10, -9, -8, -7, -6, -5],
    [-4, 0, 0, 0, 0, 0, 0, 0],
];
const ES_19200_11B_CLAMP: [[i32; 8]; 9] = [
    [-3, -2, -1, 0, -4, 0, 0, 0],
    [-7, -6, 0, 0, 0, 0, 0, 0],
    [-5, -4, 0, 0, 0, 0, 0, 0],
    [-2, 0, 0, 0, 0, 0, 0, 0],
    [-3, -2, -1, 0, 0, 0, 0, 0],
    [-1, 0, 1, 2, 3, 4, 0, 0],
    [-1, 0, 0, 1, 2, 3, 4, 0],
    [-12, -11, -10, -9, -8, -7, -6, -5],
    [-3, 0, 0, 0, 0, 0, 0, 0],
];
const ES_19200_11B: [[i32; 8]; 9] = [
    [-3, -2, -1, 0, -4, 0, 0, 0],
    [-7, -6, 0, 0, 0, 0, 0, 0],
    [-5, -4, 0, 0, 0, 0, 0, 0],
    [-3, -2, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [-1, 0, 1, 2, 3, 4, 0, 0],
    [0, 1, 2, 3, 4, 0, 0, 0],
    [-12, -11, -10, -9, -8, -7, -6, 0],
    [-3, 0, 0, 0, 0, 0, 0, 0],
];
const ES_19200_14B: [[i32; 8]; 9] = [
    [-4, -3, -2, -1, -5, 0, 0, 0],
    [-8, -7, 0, 0, 0, 0, 0, 0],
    [-6, -5, 0, 0, 0, 0, 0, 0],
    [-3, 0, 0, 0, 0, 0, 0, 0],
    [-4, -3, -2, -1, 0, 0, 0, 0],
    [-2, -1, 0, 1, 2, 3, 4, 0],
    [-2, -1, 0, 0, 1, 2, 3, 4],
    [-12, -11, -10, -9, -8, -7, -6, -5],
    [-4, 0, 0, 0, 0, 0, 0, 0],
];

#[derive(Clone, Copy, Debug, Default)]
#[allow(clippy::struct_excessive_bools)]
struct EsFlags {
    sign: bool,
    ol: bool,
    ul: bool,
    judge: bool,
    digit4: bool,
    max: bool,
    min: bool,
    rel: bool,
    dc: bool,
    ac: bool,
    auto: bool,
    vahz: bool,
    hold: bool,
    vbar: bool,
    vasel: bool,
    voltage: bool,
    current: bool,
    micro: bool,
    milli: bool,
    resistance: bool,
    continuity: bool,
    diode: bool,
    frequency: bool,
    rpm: bool,
    duty_cycle: bool,
    capacitance: bool,
    temperature: bool,
    celsius: bool,
    fahrenheit: bool,
}

fn es_flags(buf: &[u8], v: Es519xx) -> EsFlags {
    let size = v.packet_size();
    let function = if size == 14 { 6 } else { 5 };
    let status = buf[function + 1];
    let alt = v == Es519xx::Baud2400x11AltFn;
    let baud2400 = matches!(v, Es519xx::Baud2400x11 | Es519xx::Baud2400x11AltFn);
    let mut f = EsFlags::default();
    if alt {
        f.sign = status & 8 != 0;
        f.ol = status & 3 != 0;
    } else {
        f.judge = status & 8 != 0;
        f.sign = status & 4 != 0;
        f.ol = status & 1 != 0;
    }
    if size == 14 {
        f.max = buf[8] & 8 != 0;
        f.min = buf[8] & 4 != 0;
        f.rel = buf[8] & 2 != 0;
        f.ul = buf[9] & 8 != 0;
        f.dc = buf[10] & 8 != 0;
        f.ac = buf[10] & 4 != 0;
        f.auto = buf[10] & 2 != 0;
        f.vahz = buf[10] & 1 != 0;
        if v == Es519xx::Baud19200x14SelLpf {
            f.hold = buf[11] & 8 != 0;
            f.vbar = buf[11] & 4 != 0;
        } else {
            f.vbar = buf[11] & 4 != 0;
            f.hold = buf[11] & 2 != 0;
        }
    } else if alt {
        f.dc = buf[8] & 8 != 0;
        f.auto = buf[8] & 4 != 0;
        f.ac = !f.dc;
    } else {
        match v {
            Es519xx::Baud2400x11 => f.vahz = buf[7] & 1 != 0,
            Es519xx::Baud19200x11x5Digits => {
                f.ul = buf[7] & 8 != 0;
                f.digit4 = buf[7] & 1 != 0;
            }
            Es519xx::Baud19200x11Clamp => {
                f.ul = buf[7] & 8 != 0;
                f.vasel = buf[7] & 4 != 0;
                f.vbar = buf[7] & 2 != 0;
            }
            _ => {
                f.hold = buf[7] & 8 != 0;
                f.max = buf[7] & 4 != 0;
                f.min = buf[7] & 2 != 0;
            }
        }
        f.dc = buf[8] & 8 != 0;
        f.ac = buf[8] & 4 != 0;
        f.auto = buf[8] & 2 != 0;
        if !baud2400 {
            f.vahz = buf[8] & 1 != 0;
        }
    }
    let freq_like = |f: &mut EsFlags| {
        if size == 14 {
            if f.judge {
                f.duty_cycle = true;
            } else {
                f.frequency = true;
            }
        } else if f.judge {
            f.rpm = true;
        } else {
            f.frequency = true;
        }
    };
    if alt {
        match buf[function] {
            0x3f => (f.current, f.auto) = (true, true),
            0x3e => (f.current, f.micro, f.auto) = (true, true, true),
            0x3d => (f.current, f.milli, f.auto) = (true, true, true),
            0x3c => f.voltage = true,
            0x37 => f.resistance = true,
            0x36 => f.continuity = true,
            0x3b => f.diode = true,
            0x3a => f.frequency = true,
            _ => {}
        }
    } else {
        match buf[function] {
            0x3b => f.voltage = true,
            0x3d => (f.current, f.micro, f.auto) = (true, true, true),
            0x3f => (f.current, f.milli, f.auto) = (true, true, true),
            0x30 => (f.current, f.auto) = (true, true),
            0x39 => (f.current, f.auto) = (true, false),
            0x33 => f.resistance = true,
            0x35 => f.continuity = true,
            0x31 => f.diode = true,
            0x32 => freq_like(&mut f),
            0x36 => f.capacitance = true,
            0x34 => {
                f.temperature = true;
                if f.judge {
                    f.celsius = true;
                } else {
                    f.fahrenheit = true;
                }
            }
            _ => {}
        }
    }
    if f.vahz && (f.voltage || f.current) {
        f.voltage = false;
        f.current = false;
        f.milli = false;
        f.micro = false;
        freq_like(&mut f);
    }
    if f.current && (f.micro || f.milli) && f.vasel {
        f.current = false;
        f.auto = false;
        f.voltage = true;
    }
    if baud2400 && f.current && (f.milli || !f.auto) {
        f.milli = !f.milli;
        f.auto = true;
    }
    f
}

impl EsFlags {
    /// Exactly one function: libsigrok also accepts none, which lets
    /// any 14 bytes ending in CR LF through (a boot banner, say), and
    /// a packet without a function yields no reading anyway.
    fn valid(&self) -> bool {
        count(&[self.micro, self.milli]) <= 1
            && count(&[
                self.voltage,
                self.current,
                self.resistance,
                self.frequency,
                self.capacitance,
                self.temperature,
                self.continuity,
                self.diode,
                self.rpm,
                self.duty_cycle,
            ]) == 1
            && !(self.ac && self.dc)
    }
}

/// True for a packet of the ES519xx variant `v`.
pub fn es519xx_valid(buf: &[u8], v: Es519xx) -> bool {
    let s = v.packet_size();
    if buf.len() < v.stream_size() {
        return false;
    }
    if s == 11 && buf[..s] != buf[s..2 * s] {
        return false;
    }
    buf[s - 2] == b'\r' && buf[s - 1] == b'\n' && es_flags(buf, v).valid()
}

/// Parses a packet of the ES519xx variant `v`.
pub fn es519xx_parse(buf: &[u8], v: Es519xx) -> Option<Reading> {
    if !es519xx_valid(buf, v) {
        return None;
    }
    let f = es_flags(buf, v);
    let ndigits = if v.packet_size() == 14 { 5 } else { 4 };
    let value = if f.ol || f.ul {
        f32::INFINITY
    } else {
        let mut n = u32::from(f.digit4);
        for &c in &buf[1..=ndigits] {
            n = n * 10 + digit(c)?;
        }
        n as f32 * if f.sign { -1.0 } else { 1.0 }
    };
    let range = usize::from(buf[0].checked_sub(b'0')?);
    if range > 7 {
        return None;
    }
    let mode = if f.voltage {
        0
    } else if f.current && f.micro {
        1
    } else if f.current && f.milli {
        2
    } else if f.current && f.auto {
        3
    } else if f.current || f.rpm {
        4
    } else if f.resistance || f.continuity {
        5
    } else if f.frequency {
        6
    } else if f.capacitance {
        7
    } else if f.diode {
        8
    } else if f.duty_cycle {
        0
    } else {
        return None;
    };
    let exponent = if f.vbar {
        if f.micro {
            [-1, 0][range.min(1)]
        } else if f.milli {
            [-2, -1][range.min(1)]
        } else {
            0
        }
    } else if f.duty_cycle {
        -1
    } else {
        match v {
            Es519xx::Baud2400x11 | Es519xx::Baud2400x11AltFn => ES_2400_11B[mode][range],
            Es519xx::Baud19200x11x5Digits => ES_19200_11B_5DIGITS[mode][range],
            Es519xx::Baud19200x11Clamp => ES_19200_11B_CLAMP[mode][range],
            Es519xx::Baud19200x11 => ES_19200_11B[mode][range],
            Es519xx::Baud19200x14 | Es519xx::Baud19200x14SelLpf => ES_19200_14B[mode][range],
        }
    };
    let (val, digits) = scaled(value, exponent);
    let mut r = Reading::new(val, Mq::Gain, Unit::Unitless, digits);
    if f.voltage {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
    }
    if f.current {
        (r.mq, r.unit) = (Mq::Current, Unit::Ampere);
    }
    if f.resistance {
        (r.mq, r.unit) = (Mq::Resistance, Unit::Ohm);
    }
    if f.frequency {
        (r.mq, r.unit) = (Mq::Frequency, Unit::Hertz);
    }
    if f.capacitance {
        (r.mq, r.unit) = (Mq::Capacitance, Unit::Farad);
    }
    if f.temperature {
        r.mq = Mq::Temperature;
        r.unit = if f.celsius { Unit::Celsius } else { Unit::Fahrenheit };
    }
    if f.continuity {
        (r.mq, r.unit) = (Mq::Continuity, Unit::Boolean);
        r.value = if r.value < 0.0 || r.value > 25.0 { 0.0 } else { 1.0 };
    }
    if f.diode {
        (r.mq, r.unit) = (Mq::Voltage, Unit::Volt);
    }
    if f.rpm {
        (r.mq, r.unit) = (Mq::Frequency, Unit::RevolutionsPerMinute);
    }
    if f.duty_cycle {
        (r.mq, r.unit) = (Mq::DutyCycle, Unit::Percentage);
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
    if f.diode {
        r.flags |= MqFlags::DIODE | MqFlags::DC;
    }
    if f.hold {
        r.flags |= MqFlags::HOLD;
    }
    if f.max {
        r.flags |= MqFlags::MAX;
    }
    if f.min {
        r.flags |= MqFlags::MIN;
    }
    if f.rel {
        r.flags |= MqFlags::RELATIVE;
    }
    Some(r)
}

const VC870_EXPONENTS: [[i32; 8]; 19] = [
    [-4, -3, -2, -1, 0, 0, 0, 0],
    [-3, -2, -1, 0, 0, 0, 0, 0],
    [-5, 0, 0, 0, 0, 0, 0, 0],
    [-1, 0, 0, 0, 0, 0, 0, 0],
    [-2, -1, 1, 2, 3, 4, 0, 0],
    [-2, 0, 0, 0, 0, 0, 0, 0],
    [-12, -11, -10, -9, -8, -7, -6, 0],
    [-4, 0, 0, 0, 0, 0, 0, 0],
    [-3, -2, -1, 0, 1, 2, 3, 4],
    [-2, 0, 0, 0, 0, 0, 0, 0],
    [-8, -7, 0, 0, 0, 0, 0, 0],
    [-7, -6, 0, 0, 0, 0, 0, 0],
    [-6, -5, 0, 0, 0, 0, 0, 0],
    [-5, -4, 0, 0, 0, 0, 0, 0],
    [-3, 0, 0, 0, 0, 0, 0, 0],
    [-3, 0, 0, 0, 0, 0, 0, 0],
    [-1, 0, 0, 0, 0, 0, 0, 0],
    [-3, 0, 0, 0, 0, 0, 0, 0],
    [-1, 0, 0, 0, 0, 0, 0, 0],
];

/// True for a VC-870 packet.
pub fn vc870_valid(buf: &[u8]) -> bool {
    buf.len() >= VC870_SIZE && buf[21] == b'\r' && buf[22] == b'\n'
}

/// Parses a VC-870 packet: the main display only.
pub fn vc870_parse(buf: &[u8]) -> Option<Reading> {
    if !vc870_valid(buf) {
        return None;
    }
    let sel = buf[1];
    let (mode, mq, unit) = match (buf[0], sel) {
        (0x30, 0x30) => (0, Mq::Voltage, Unit::Volt),
        (0x30, 0x31) => (1, Mq::Voltage, Unit::Volt),
        (0x31, 0x30) => (2, Mq::Voltage, Unit::Volt),
        (0x31, 0x31) => (3, Mq::Temperature, Unit::Celsius),
        (0x32, 0x30) => (4, Mq::Resistance, Unit::Ohm),
        (0x32, 0x31) => (4, Mq::Continuity, Unit::Boolean),
        (0x33, 0x30) => (6, Mq::Capacitance, Unit::Farad),
        (0x34, 0x30) => (7, Mq::Voltage, Unit::Volt),
        (0x35, 0x30) => (8, Mq::Frequency, Unit::Hertz),
        (0x35, 0x31) => (9, Mq::Current, Unit::Percentage),
        (0x36, 0x30) => (10, Mq::Current, Unit::Ampere),
        (0x36, 0x31) => (11, Mq::Current, Unit::Ampere),
        (0x37, 0x30) => (12, Mq::Current, Unit::Ampere),
        (0x37, 0x31) => (13, Mq::Current, Unit::Ampere),
        (0x38, 0x30) => (14, Mq::Current, Unit::Ampere),
        (0x38, 0x31) => (15, Mq::Current, Unit::Ampere),
        (0x39, 0x30) => (16, Mq::Power, Unit::Watt),
        (0x39, 0x31) => (17, Mq::PowerFactor, Unit::Unitless),
        (0x39, 0x32) => (18, Mq::Voltage, Unit::Volt),
        _ => return None,
    };
    let range = usize::from(buf[2].checked_sub(b'0')?);
    if range > 7 {
        return None;
    }
    let sign1 = buf[15] & 4 != 0;
    let ol1 = buf[15] & 1 != 0;
    let open = buf[17] & 4 != 0;
    let value = if open || ol1 {
        f32::INFINITY
    } else {
        let mut n = 0u32;
        for &c in &buf[3..8] {
            n = n * 10 + digit(c)?;
        }
        n as f32 * if sign1 { -1.0 } else { 1.0 }
    };
    let (v, digits) = scaled(value, VC870_EXPONENTS[mode][range]);
    let mut r = Reading::new(v, mq, unit, digits);
    if mq == Mq::Continuity {
        r.value = if r.value < 0.0 || r.value > 20.0 { 0.0 } else { 1.0 };
    }
    let dc = matches!(mode, 0 | 2 | 10 | 12 | 14);
    let ac = matches!(mode, 1 | 11 | 13 | 15);
    if ac {
        r.flags |= MqFlags::AC;
    }
    if dc {
        r.flags |= MqFlags::DC;
    }
    if mode == 18 {
        r.flags |= MqFlags::RMS;
    }
    if buf[17] & 2 == 0 {
        r.flags |= MqFlags::AUTORANGE;
    }
    if mode == 7 {
        r.flags |= MqFlags::DIODE | MqFlags::DC;
    }
    if buf[17] & 1 != 0 {
        r.flags |= MqFlags::HOLD;
    }
    if buf[16] & 8 != 0 {
        r.flags |= MqFlags::MAX;
    }
    if buf[16] & 4 != 0 {
        r.flags |= MqFlags::MIN;
    }
    if buf[16] & 1 != 0 {
        r.flags |= MqFlags::RELATIVE;
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut71x_packets() {
        // 12345 range 1 function 1 (DC V): 1.2345 V, auto.
        let p = b"12345110\x01\r\n";
        let p = &[p[0], p[1], p[2], p[3], p[4], b'1', b'1', 2, 1, b'\r', b'\n'];
        assert!(ut71x_valid(p));
        let r = ut71x_parse(p).unwrap();
        assert_eq!((r.mq, r.unit, r.digits), (Mq::Voltage, Unit::Volt, 4));
        assert!((r.value - 1.2345).abs() < 1e-5);
        assert!(r.flags.contains(MqFlags::DC | MqFlags::AUTORANGE));
        // Over limit, resistance, 4 digit meter.
        let p = [b':', b':', b'0', b'<', b':', b'2', b'4', 0, 2, b'\r', b'\n'];
        let r = ut71x_parse(&p).unwrap();
        assert!(r.value.is_infinite());
        assert_eq!(r.mq, Mq::Resistance);
        // Frequency with the sign bit means duty cycle.
        let p = [b'5', b'0', b'0', b'0', b'0', b'0', b'<', 0, 4, b'\r', b'\n'];
        let r = ut71x_parse(&p).unwrap();
        assert_eq!((r.mq, r.unit), (Mq::DutyCycle, Unit::Percentage));
        assert!((r.value - 50.0).abs() < 1e-4);
        // Auto and manual together is not a packet.
        let p = [b'1', b'2', b'3', b'4', b'5', b'1', b'1', 2, 3, b'\r', b'\n'];
        assert!(!ut71x_valid(&p));
    }

    #[test]
    fn es519xx_packets() {
        // 19200 14 byte (UT61E): range 1, 12345, function V, status 0,
        // options: DC and auto. 1.2345 V with range 1 -> exponent -3?
        let mut p = b"112345".to_vec();
        p.extend([0x3b, 0x30, 0x30, 0x30, 0x30 | 8 | 2, 0x30, b'\r', b'\n']);
        assert!(es519xx_valid(&p, Es519xx::Baud19200x14));
        let r = es519xx_parse(&p, Es519xx::Baud19200x14).unwrap();
        assert_eq!((r.mq, r.unit, r.digits), (Mq::Voltage, Unit::Volt, 3));
        assert!((r.value - 12.345).abs() < 1e-4);
        assert!(r.flags.contains(MqFlags::DC | MqFlags::AUTORANGE));
        // 2400 11 byte sent twice: range 0, 1234, function resistance,
        // status overflow.
        let mut one = b"01234".to_vec();
        one.extend([0x33, 0x31, 0x30, 0x30, b'\r', b'\n']);
        let mut p = one.clone();
        p.extend(one);
        assert!(es519xx_valid(&p, Es519xx::Baud2400x11));
        let r = es519xx_parse(&p, Es519xx::Baud2400x11).unwrap();
        assert!(r.value.is_infinite());
        assert_eq!((r.mq, r.digits), (Mq::Resistance, 1));
        // Not repeated: not a packet.
        p[12] = b'9';
        assert!(!es519xx_valid(&p, Es519xx::Baud2400x11));
        // Text that happens to end in CR LF is not a packet either.
        assert!(!es519xx_valid(b"SIGGEN READY\r\n", Es519xx::Baud19200x14));
    }

    #[test]
    fn vc870_packet() {
        // DC V, range 1, 12345 -> 12.345 V; status, options, dual, CR LF.
        let mut p = vec![0x30, 0x30, b'1'];
        p.extend(b"12345");
        p.extend(b"00000");
        p.extend([0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, b'\r', b'\n']);
        assert_eq!(p.len(), VC870_SIZE);
        assert!(vc870_valid(&p));
        let r = vc870_parse(&p).unwrap();
        assert_eq!((r.mq, r.digits), (Mq::Voltage, 3));
        assert!((r.value - 12.345).abs() < 1e-4);
        assert!(r.flags.contains(MqFlags::DC | MqFlags::AUTORANGE));
    }
}
