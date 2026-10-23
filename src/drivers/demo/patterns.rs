// Counterpart in sigrok:
//   libsigrok/src/hardware/demo/protocol.c.
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like it;
//   the logo bitmap was replaced by oxrok's own pattern.

//! The signal patterns of the demo device.

use std::f64::consts::PI;

/// Logic patterns, by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicPattern {
    /// The word "oxrok" as a bitmap, one column per sample, D0 at the
    /// top. Text pixels are low, the background is high.
    Oxrok,
    /// Pseudo random bits on every channel.
    Random,
    /// A counter: D0 toggles every sample, D1 every two, and so on.
    Incremental,
    /// A single high bit moving up the channels, with an all low sample
    /// between rounds.
    WalkingOne,
    /// A single low bit moving up the channels.
    WalkingZero,
    /// Every channel low.
    AllLow,
    /// Every channel high.
    AllHigh,
    /// A Gray code counter, like a rotary encoder.
    Graycode,
}

impl LogicPattern {
    /// Every pattern, in the order `list()` reports them.
    pub const ALL: [LogicPattern; 8] = [
        LogicPattern::Oxrok,
        LogicPattern::Random,
        LogicPattern::Incremental,
        LogicPattern::WalkingOne,
        LogicPattern::WalkingZero,
        LogicPattern::AllLow,
        LogicPattern::AllHigh,
        LogicPattern::Graycode,
    ];

    /// The id used with `-c pattern=`.
    pub fn id(self) -> &'static str {
        match self {
            LogicPattern::Oxrok => "oxrok",
            LogicPattern::Random => "random",
            LogicPattern::Incremental => "incremental",
            LogicPattern::WalkingOne => "walking-one",
            LogicPattern::WalkingZero => "walking-zero",
            LogicPattern::AllLow => "all-low",
            LogicPattern::AllHigh => "all-high",
            LogicPattern::Graycode => "graycode",
        }
    }

    /// The pattern with `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.id() == id)
    }
}

/// Analog patterns, by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalogPattern {
    /// A square wave, 10 samples per period, starting low.
    Square,
    /// A sine, 20 samples per period.
    Sine,
    /// A triangle, 20 samples per period.
    Triangle,
    /// A sawtooth, 20 samples per period.
    Sawtooth,
    /// Pseudo random values.
    Random,
}

impl AnalogPattern {
    /// Every pattern, in the order `list()` reports them. Channels get
    /// them in this order too: A0 square, A1 sine, and so on.
    pub const ALL: [AnalogPattern; 5] = [
        AnalogPattern::Square,
        AnalogPattern::Sine,
        AnalogPattern::Triangle,
        AnalogPattern::Sawtooth,
        AnalogPattern::Random,
    ];

    /// The id used with `-c pattern=`.
    pub fn id(self) -> &'static str {
        match self {
            AnalogPattern::Square => "square",
            AnalogPattern::Sine => "sine",
            AnalogPattern::Triangle => "triangle",
            AnalogPattern::Sawtooth => "sawtooth",
            AnalogPattern::Random => "random",
        }
    }

    /// The pattern with `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.id() == id)
    }
}

/// Samples per period of the periodic analog patterns.
pub const ANALOG_SAMPLES_PER_PERIOD: usize = 20;
/// Length of the precomputed analog tables, a whole number of periods.
pub const ANALOG_TABLE_LEN: usize = 1020;

/// The unit amplitude table of an analog pattern (random is generated on
/// the fly and returns an empty table).
pub fn analog_table(pattern: AnalogPattern) -> Vec<f32> {
    let n = ANALOG_TABLE_LEN;
    // The tables are built for a 200 kHz rate and a period of 20 samples,
    // written out as rate and time so the rounding matches the reference
    // generator sample for sample (sin(pi) comes out slightly negative).
    let rate = 200_000.0f64;
    let frequency = rate / ANALOG_SAMPLES_PER_PERIOD as f64;
    let t = |i: usize| i as f64 / rate;
    match pattern {
        AnalogPattern::Square => {
            // Toggles every five samples, starting low.
            let mut value = 1.0f32;
            (0..n)
                .map(|i| {
                    if i % 5 == 0 {
                        value = -value;
                    }
                    value
                })
                .collect()
        }
        AnalogPattern::Sine => (0..n).map(|i| (2.0 * PI * frequency * t(i)).sin() as f32).collect(),
        AnalogPattern::Triangle => (0..n)
            .map(|i| ((2.0 / PI) * (2.0 * PI * frequency * t(i)).sin().asin()) as f32)
            .collect(),
        AnalogPattern::Sawtooth => (0..n)
            .map(|i| {
                let x = t(i) * frequency;
                (2.0 * (x - (f64::from(0.5f32) + x).floor())) as f32
            })
            .collect(),
        AnalogPattern::Random => Vec::new(),
    }
}

/// "oxrok" as 64 columns of 7 rows, bit 0 at the top. Letters are 5
/// columns wide with a blank column between them; the rest is blank.
const OXROK_COLUMNS: [u8; 64] = {
    let o = [0x38, 0x44, 0x44, 0x44, 0x38];
    let x = [0x44, 0x28, 0x10, 0x28, 0x44];
    let r = [0x7c, 0x08, 0x04, 0x04, 0x08];
    let k = [0x7f, 0x10, 0x28, 0x44, 0x00];
    let letters = [o, x, r, o, k];
    let mut cols = [0u8; 64];
    let mut l = 0;
    while l < letters.len() {
        let mut c = 0;
        while c < 5 {
            cols[l * 6 + c] = letters[l][c];
            c += 1;
        }
        l += 1;
    }
    cols
};

/// A small xorshift generator, so the random patterns need no crate.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// A generator seeded from `seed` (zero is replaced).
    pub fn new(seed: u64) -> Self {
        Rng(if seed == 0 { 0x9e37_79b9_7f4a_7c15 } else { seed })
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A float in [0, 1).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Generates logic samples. Keeps the pattern position between calls.
#[derive(Clone, Debug)]
pub struct LogicGen {
    pattern: LogicPattern,
    channels: usize,
    unit_size: usize,
    step: u64,
    rng: Rng,
}

impl LogicGen {
    /// A generator for `channels` logic channels.
    pub fn new(pattern: LogicPattern, channels: usize, seed: u64) -> Self {
        LogicGen {
            pattern,
            channels,
            unit_size: crate::core::channel::unit_size(channels),
            step: 0,
            rng: Rng::new(seed),
        }
    }

    /// Bytes per sample.
    pub fn unit_size(&self) -> usize {
        self.unit_size
    }

    fn mask(&self) -> u64 {
        if self.channels >= 64 { u64::MAX } else { (1u64 << self.channels) - 1 }
    }

    /// Appends `count` samples to `out`.
    pub fn generate(&mut self, count: usize, out: &mut Vec<u8>) {
        let top = 1u64 << (self.channels.max(1) - 1);
        for _ in 0..count {
            let value: u64 = match self.pattern {
                LogicPattern::Oxrok => {
                    let col = OXROK_COLUMNS[(self.step % 64) as usize];
                    self.step += 1;
                    // Repeat the column in every byte, inverted, so the
                    // word shows on D0..D7 of any channel count.
                    let byte = u64::from(!col);
                    let mut v = 0u64;
                    for b in 0..self.unit_size.min(8) {
                        v |= byte << (8 * b);
                    }
                    v
                }
                LogicPattern::Random => self.rng.next_u64(),
                LogicPattern::Incremental => {
                    let v = self.step;
                    self.step = self.step.wrapping_add(1);
                    v
                }
                LogicPattern::WalkingOne | LogicPattern::WalkingZero => {
                    let v = self.step;
                    self.step = if self.step == 0 {
                        1
                    } else if self.step == top {
                        0
                    } else {
                        self.step << 1
                    };
                    if self.pattern == LogicPattern::WalkingOne { v } else { !v }
                }
                LogicPattern::AllLow => 0,
                LogicPattern::AllHigh => u64::MAX,
                LogicPattern::Graycode => {
                    self.step = (self.step + 1) & self.mask();
                    self.step ^ (self.step >> 1)
                }
            };
            let value = value & self.mask();
            out.extend_from_slice(&value.to_le_bytes()[..self.unit_size]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(pattern: LogicPattern, channels: usize, n: usize) -> Vec<u8> {
        let mut g = LogicGen::new(pattern, channels, 1);
        let mut out = Vec::new();
        g.generate(n, &mut out);
        out
    }

    #[test]
    fn walking_patterns_match_the_reference() {
        // Four channels: all low, then one bit walking up, repeat.
        assert_eq!(bits(LogicPattern::WalkingOne, 4, 6), [0, 1, 2, 4, 8, 0]);
        assert_eq!(bits(LogicPattern::WalkingZero, 4, 6), [15, 14, 13, 11, 7, 15]);
        assert_eq!(bits(LogicPattern::Graycode, 4, 5), [1, 3, 2, 6, 7]);
        assert_eq!(bits(LogicPattern::Incremental, 4, 5), [0, 1, 2, 3, 4]);
    }

    #[test]
    fn wide_units_are_little_endian() {
        let b = bits(LogicPattern::Incremental, 12, 300);
        assert_eq!(&b[2 * 256..2 * 256 + 2], [0x00, 0x01]);
        let b = bits(LogicPattern::AllHigh, 12, 1);
        assert_eq!(b, [0xff, 0x0f]);
    }

    #[test]
    fn analog_tables_match_the_reference() {
        let sq = analog_table(AnalogPattern::Square);
        assert_eq!(&sq[..10], &[-1.0, -1.0, -1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        let sine = analog_table(AnalogPattern::Sine);
        assert!((sine[1] * 10.0 - 3.0902).abs() < 1e-4);
        assert!((sine[5] - 1.0).abs() < 1e-6);
        let tri = analog_table(AnalogPattern::Triangle);
        assert!((tri[1] * 10.0 - 2.0).abs() < 1e-4);
        let saw = analog_table(AnalogPattern::Sawtooth);
        assert!((saw[1] * 10.0 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn oxrok_pattern_spells_the_name() {
        // Column 0 of 'o' has rows 3, 4 and 5 set; inverted in the data.
        assert_eq!(bits(LogicPattern::Oxrok, 8, 1), [!0x38]);
    }
}
