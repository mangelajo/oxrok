//! Parsing and formatting helpers shared by the CLI and the formats.
//!
//! The formats match sigrok's, so files and command lines written for
//! sigrok keep working: `200k` is 200000, a sample rate prints as
//! `200 kHz`, a time is `1s` or `100ms`.

use crate::error::{Error, Result};

const UNITS: [u64; 7] = [
    1,
    1_000,
    1_000_000,
    1_000_000_000,
    1_000_000_000_000,
    1_000_000_000_000_000,
    1_000_000_000_000_000_000,
];
const PREFIXES: [&str; 7] = ["", "k", "M", "G", "T", "P", "E"];

/// Formats `x` with an SI prefix and `unit`, trimming trailing zeros:
/// `200 kHz`, `1.5 MHz`, `1 Hz`, `7 ` for an empty unit.
pub fn si_string_u64(x: u64, unit: &str) -> String {
    let mut i = 0;
    while i + 1 < UNITS.len() && x / UNITS[i] >= 1000 {
        i += 1;
    }
    let quot = x / UNITS[i];
    let mut fract = String::new();
    if i > 0 {
        let rem = x % UNITS[i];
        if rem != 0 {
            let digits = i * 3;
            fract = format!(".{rem:0digits$}");
            while fract.ends_with('0') {
                fract.pop();
            }
            if fract.ends_with('.') {
                fract.pop();
            }
        }
    }
    format!("{quot}{fract} {}{unit}", PREFIXES[i])
}

/// A sample rate as `200 kHz`.
pub fn samplerate_string(rate: u64) -> String {
    si_string_u64(rate, "Hz")
}

/// Parses a number with an optional fraction and SI suffix: `100`,
/// `1.5k`, `200 k`, `24m`, `1g`, optionally followed by `Hz`.
///
/// Lower and upper case suffixes mean the same thing (`m` is mega, not
/// milli), as in sigrok-cli.
pub fn parse_sizestring(text: &str) -> Result<u64> {
    let bad = || Error::Arg(format!("invalid number '{text}'"));
    let s = text.trim();
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (int_part, mut rest) = s.split_at(end);
    if int_part.is_empty() {
        return Err(bad());
    }
    let mut value: u64 = int_part.parse().map_err(|_| bad())?;
    let mut frac = 0.0f64;
    if let Some(after) = rest.strip_prefix('.') {
        let end = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
        let digits = &after[..end];
        if !digits.is_empty() {
            frac = format!("0.{digits}").parse().map_err(|_| bad())?;
        }
        rest = &after[end..];
    }
    rest = rest.trim_start();
    let mut multiplier = 1u64;
    if let Some(c) = rest.chars().next() {
        let m = match c.to_ascii_lowercase() {
            'k' => Some(UNITS[1]),
            'm' => Some(UNITS[2]),
            'g' => Some(UNITS[3]),
            't' => Some(UNITS[4]),
            'p' => Some(UNITS[5]),
            'e' => Some(UNITS[6]),
            _ => None,
        };
        if let Some(m) = m {
            multiplier = m;
            rest = &rest[c.len_utf8()..];
        }
    }
    let rest = rest.trim();
    if !rest.is_empty() && !rest.eq_ignore_ascii_case("hz") {
        return Err(bad());
    }
    value = value.checked_mul(multiplier).ok_or_else(bad)?;
    if frac != 0.0 {
        if multiplier == 1 {
            return Err(bad());
        }
        value = value.checked_add((frac * multiplier as f64).round() as u64).ok_or_else(bad)?;
    }
    Ok(value)
}

/// Parses a duration as milliseconds: `100`, `100ms`, `2s`. Returns
/// `None` for anything else.
pub fn parse_timestring(text: &str) -> Option<u64> {
    let s = text.trim();
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let value: u64 = s[..end].parse().ok()?;
    match s[end..].trim() {
        "" | "ms" => Some(value),
        "s" => value.checked_mul(1000),
        _ => None,
    }
}

/// Parses `true`, `yes`, `on` and `1` as true (case insensitive); anything
/// else is false.
pub fn parse_boolstring(text: &str) -> bool {
    let t = text.trim();
    t.eq_ignore_ascii_case("true")
        || t.eq_ignore_ascii_case("yes")
        || t.eq_ignore_ascii_case("on")
        || t == "1"
}

/// Parses a period like `10ms`, `2.5 us`, `1 s` into a rational number of
/// seconds (p, q).
pub fn parse_period(text: &str) -> Option<(u64, u64)> {
    parse_rational(text, &[("s", 1), ("ms", 1_000), ("us", 1_000_000), ("ns", 1_000_000_000)])
}

/// Parses a voltage like `500mV`, `2 V` into a rational number of volts.
pub fn parse_voltage(text: &str) -> Option<(u64, u64)> {
    parse_rational(text, &[("v", 1), ("mv", 1_000)])
}

fn parse_rational(text: &str, units: &[(&str, u64)]) -> Option<(u64, u64)> {
    let s = text.trim();
    let end = s.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(s.len());
    let number = &s[..end];
    let unit = s[end..].trim().to_ascii_lowercase();
    let (_, divisor) = units.iter().find(|(u, _)| *u == unit)?;
    let (int, frac) = number.split_once('.').unwrap_or((number, ""));
    let int: u64 = if int.is_empty() { 0 } else { int.parse().ok()? };
    // value = (int + frac / scale) / divisor
    let (mut p, mut q) = (int, *divisor);
    if !frac.is_empty() {
        let scale = 10u64.checked_pow(frac.len() as u32)?;
        p = p.checked_mul(scale)?.checked_add(frac.parse::<u64>().ok()?)?;
        q = q.checked_mul(scale)?;
    }
    // Reduce p/q so that 10 ms is 1/100, not 10/1000.
    let g = gcd(p, q);
    Some(if g > 1 { (p / g, q / g) } else { (p, q) })
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sample_rates_like_sigrok() {
        assert_eq!(samplerate_string(200_000), "200 kHz");
        assert_eq!(samplerate_string(1_000_000_000), "1 GHz");
        assert_eq!(samplerate_string(1), "1 Hz");
        assert_eq!(samplerate_string(1_500_000), "1.5 MHz");
        assert_eq!(samplerate_string(24_000_000), "24 MHz");
        assert_eq!(samplerate_string(921_600), "921.6 kHz");
    }

    #[test]
    fn parses_sizes_like_sigrok() {
        assert_eq!(parse_sizestring("100").unwrap(), 100);
        assert_eq!(parse_sizestring("200k").unwrap(), 200_000);
        assert_eq!(parse_sizestring("1.5k").unwrap(), 1_500);
        assert_eq!(parse_sizestring("24m").unwrap(), 24_000_000);
        assert_eq!(parse_sizestring("1 MHz").unwrap(), 1_000_000);
        assert_eq!(parse_sizestring("1g").unwrap(), 1_000_000_000);
        assert!(parse_sizestring("").is_err());
        assert!(parse_sizestring("12x").is_err());
        assert!(parse_sizestring("1.5").is_err());
    }

    #[test]
    fn parses_times_and_bools() {
        assert_eq!(parse_timestring("100"), Some(100));
        assert_eq!(parse_timestring("100ms"), Some(100));
        assert_eq!(parse_timestring("2 s"), Some(2000));
        assert_eq!(parse_timestring("2h"), None);
        assert!(parse_boolstring("yes"));
        assert!(parse_boolstring("On"));
        assert!(!parse_boolstring("no"));
    }

    #[test]
    fn parses_rationals() {
        assert_eq!(parse_period("10ms"), Some((1, 100)));
        assert_eq!(parse_period("2.5 us"), Some((1, 400_000)));
        assert_eq!(parse_voltage("500mV"), Some((1, 2)));
        assert_eq!(parse_voltage("2 V"), Some((2, 1)));
        assert_eq!(parse_voltage("2 A"), None);
    }
}
