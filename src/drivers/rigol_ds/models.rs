// Counterpart in sigrok:
//   libsigrok/src/hardware/rigol-ds/api.c (the series and model tables),
//   Copyright (C) 2012 Martin Ling <martin-sigrok@earth.li>,
//   Copyright (C) 2013 Bert Vermeulen <bert@biot.com>,
//   Copyright (C) 2013 Mathias Grimmberger <mgri@zaphod.sax.de>,
//   with commits by Uwe Hermann, Aurelien Jacobs, Daniel Elstner,
//   Gerhard Sittig, Soeren Apel, Frank Stettner, Guido Trentalancia,
//   Lars-Peter Clausen, Sylvain Munaut, Timo Kokkonen, Wolfram Sang,
//   Alexandru Gagniuc, Marc Schink
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! The supported scopes: vendors, series and models.

/// How a series talks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Protocol {
    /// VS5000.
    V1,
    /// DS1000.
    V2,
    /// DS2000, DSO1000.
    V3,
    /// DS1000Z, DS4000, MSO7000A.
    V4,
    /// MSO5000.
    V5,
}

/// How waveform data comes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Bare bytes: VS5000, and DS1000 firmware before 0.2.4.
    Raw,
    /// An IEEE 488.2 definite length block.
    Ieee488,
}

/// A manufacturer.
pub struct Vendor {
    /// Short name, as sigrok shows it.
    pub name: &'static str,
    /// The name in `*IDN?`.
    pub full_name: &'static str,
}

/// A product series.
pub struct Series {
    /// The vendor.
    pub vendor: &'static Vendor,
    /// Series name.
    pub name: &'static str,
    /// Protocol generation.
    pub protocol: Protocol,
    /// Waveform data format.
    pub format: Format,
    /// Slowest timebase, as a rational of seconds.
    pub max_timebase: (u64, u64),
    /// Smallest vertical scale, as a rational of volts.
    pub min_vdiv: (u64, u64),
    /// Horizontal divisions on screen.
    pub num_hdiv: i32,
    /// Samples per channel in a live capture.
    pub live_samples: usize,
    /// Samples in the acquisition memory, 0 when it varies.
    pub buffer_samples: usize,
}

/// One model.
pub struct Model {
    /// Its series.
    pub series: &'static Series,
    /// The name in `*IDN?`.
    pub name: &'static str,
    /// Fastest timebase.
    pub min_timebase: (u64, u64),
    /// Analog channels.
    pub analog_channels: usize,
    /// Whether it has the 16 digital inputs.
    pub has_digital: bool,
    /// Commands for the horizontal trigger position (query, set with a
    /// `{}` placeholder).
    pub triggerpos_cmds: (&'static str, &'static str),
}

/// Rigol.
pub static RIGOL: Vendor = Vendor { name: "Rigol", full_name: "Rigol Technologies" };
/// Agilent.
pub static AGILENT: Vendor = Vendor { name: "Agilent", full_name: "Agilent Technologies" };

#[allow(clippy::too_many_arguments)]
const fn series(
    vendor: &'static Vendor,
    name: &'static str,
    protocol: Protocol,
    format: Format,
    max_timebase: (u64, u64),
    min_vdiv: (u64, u64),
    num_hdiv: i32,
    live_samples: usize,
    buffer_samples: usize,
) -> Series {
    Series {
        vendor,
        name,
        protocol,
        format,
        max_timebase,
        min_vdiv,
        num_hdiv,
        live_samples,
        buffer_samples,
    }
}

/// The VS5000 series.
pub static VS5000: Series =
    series(&RIGOL, "VS5000", Protocol::V1, Format::Raw, (50, 1), (2, 1000), 14, 2048, 0);
/// The DS1000 series (DS1052E, DS1102E, ...).
pub static DS1000: Series =
    series(&RIGOL, "DS1000", Protocol::V2, Format::Ieee488, (50, 1), (2, 1000), 12, 600, 1_048_576);
/// The DS2000 series.
pub static DS2000: Series = series(
    &RIGOL,
    "DS2000",
    Protocol::V3,
    Format::Ieee488,
    (500, 1),
    (500, 1_000_000),
    14,
    1400,
    14000,
);
/// The DS2000A and MSO2000A series.
pub static DS2000A: Series = series(
    &RIGOL,
    "DS2000A",
    Protocol::V3,
    Format::Ieee488,
    (1000, 1),
    (500, 1_000_000),
    14,
    1400,
    14000,
);
/// The Agilent DSO1000A series.
pub static DSO1000: Series =
    series(&AGILENT, "DSO1000", Protocol::V3, Format::Ieee488, (50, 1), (2, 1000), 12, 600, 20480);
/// The Agilent DSO1000B series.
pub static DSO1000B: Series =
    series(&AGILENT, "DSO1000", Protocol::V3, Format::Ieee488, (50, 1), (2, 1000), 12, 600, 20480);
/// The DS1000Z and MSO1000Z series.
pub static DS1000Z: Series = series(
    &RIGOL,
    "DS1000Z",
    Protocol::V4,
    Format::Ieee488,
    (50, 1),
    (1, 1000),
    12,
    1200,
    12_000_000,
);
/// The DS4000 series.
pub static DS4000: Series =
    series(&RIGOL, "DS4000", Protocol::V4, Format::Ieee488, (1000, 1), (1, 1000), 14, 1400, 0);
/// The MSO5000 series.
pub static MSO5000: Series = series(
    &RIGOL,
    "MSO5000",
    Protocol::V5,
    Format::Ieee488,
    (1000, 1),
    (500, 1_000_000),
    10,
    1000,
    0,
);
/// The Agilent MSO7000A series.
pub static MSO7000A: Series = series(
    &AGILENT,
    "MSO7000A",
    Protocol::V4,
    Format::Ieee488,
    (50, 1),
    (2, 1000),
    10,
    1000,
    8_000_000,
);

const STD_CMDS: (&str, &str) = (":TIM:OFFS?", ":TIM:OFFS {}");
const MSO7000A_CMDS: (&str, &str) = (":TIM:POS?", ":TIM:POS {}");

const fn m(
    series: &'static Series,
    name: &'static str,
    min_ns: u64,
    analog_channels: usize,
    has_digital: bool,
) -> Model {
    Model {
        series,
        name,
        min_timebase: (min_ns, 1_000_000_000),
        analog_channels,
        has_digital,
        triggerpos_cmds: STD_CMDS,
    }
}

/// All supported models, as libsigrok lists them.
pub static MODELS: &[Model] = &[
    m(&VS5000, "VS5022", 20, 2, false),
    m(&VS5000, "VS5042", 10, 2, false),
    m(&VS5000, "VS5062", 5, 2, false),
    m(&VS5000, "VS5102", 2, 2, false),
    m(&VS5000, "VS5202", 2, 2, false),
    m(&VS5000, "VS5022D", 20, 2, true),
    m(&VS5000, "VS5042D", 10, 2, true),
    m(&VS5000, "VS5062D", 5, 2, true),
    m(&VS5000, "VS5102D", 2, 2, true),
    m(&VS5000, "VS5202D", 2, 2, true),
    m(&DS1000, "DS1052E", 5, 2, false),
    m(&DS1000, "DS1102E", 2, 2, false),
    m(&DS1000, "DS1152E", 2, 2, false),
    m(&DS1000, "DS1152E-EDU", 2, 2, false),
    m(&DS1000, "DS1052D", 5, 2, true),
    m(&DS1000, "DS1102D", 2, 2, true),
    m(&DS1000, "DS1152D", 2, 2, true),
    m(&DS2000, "DS2072", 5, 2, false),
    m(&DS2000, "DS2102", 5, 2, false),
    m(&DS2000, "DS2202", 2, 2, false),
    m(&DS2000, "DS2302", 1, 2, false),
    m(&DS2000A, "DS2072A", 5, 2, false),
    m(&DS2000A, "DS2102A", 5, 2, false),
    m(&DS2000A, "DS2202A", 2, 2, false),
    m(&DS2000A, "DS2302A", 1, 2, false),
    m(&DS2000A, "MSO2072A", 5, 2, true),
    m(&DS2000A, "MSO2102A", 5, 2, true),
    m(&DS2000A, "MSO2202A", 2, 2, true),
    m(&DS2000A, "MSO2302A", 1, 2, true),
    m(&DSO1000, "DSO1002A", 5, 2, false),
    m(&DSO1000, "DSO1004A", 5, 4, false),
    m(&DSO1000, "DSO1012A", 2, 2, false),
    m(&DSO1000, "DSO1014A", 2, 4, false),
    m(&DSO1000, "DSO1022A", 2, 2, false),
    m(&DSO1000, "DSO1024A", 2, 4, false),
    m(&DSO1000B, "DSO1052B", 2, 2, false),
    m(&DSO1000B, "DSO1072B", 2, 2, false),
    m(&DSO1000B, "DSO1102B", 2, 2, false),
    m(&DSO1000B, "DSO1152B", 2, 2, false),
    m(&DS1000Z, "DS1054Z", 5, 4, false),
    m(&DS1000Z, "DS1074Z", 5, 4, false),
    m(&DS1000Z, "DS1104Z", 5, 4, false),
    m(&DS1000Z, "DS1074Z-S", 5, 4, false),
    m(&DS1000Z, "DS1104Z-S", 5, 4, false),
    m(&DS1000Z, "DS1074Z Plus", 5, 4, true),
    m(&DS1000Z, "DS1104Z Plus", 5, 4, true),
    m(&DS1000Z, "DS1102Z-E", 2, 2, false),
    m(&DS1000Z, "DS1202Z-E", 2, 2, false),
    m(&DS1000Z, "MSO1074Z", 5, 4, true),
    m(&DS1000Z, "MSO1104Z", 5, 4, true),
    m(&DS1000Z, "MSO1074Z-S", 5, 4, true),
    m(&DS1000Z, "MSO1104Z-S", 5, 4, true),
    m(&DS4000, "DS4014", 1, 4, false),
    m(&DS4000, "DS4024", 1, 4, false),
    m(&MSO5000, "MSO5072", 1, 2, true),
    m(&MSO5000, "MSO5074", 1, 4, true),
    m(&MSO5000, "MSO5102", 1, 2, true),
    m(&MSO5000, "MSO5104", 1, 4, true),
    m(&MSO5000, "MSO5204", 1, 4, true),
    m(&MSO5000, "MSO5354", 1, 4, true),
    // Digital channels are not yet supported on the MSO7000A, as in
    // libsigrok.
    Model {
        series: &MSO7000A,
        name: "MSO7034A",
        min_timebase: (2, 1_000_000_000),
        analog_channels: 4,
        has_digital: false,
        triggerpos_cmds: MSO7000A_CMDS,
    },
];

/// The timebases any model may offer, fastest first.
pub static TIMEBASES: &[(u64, u64)] = &[
    (1, 1_000_000_000),
    (2, 1_000_000_000),
    (5, 1_000_000_000),
    (10, 1_000_000_000),
    (20, 1_000_000_000),
    (50, 1_000_000_000),
    (100, 1_000_000_000),
    (500, 1_000_000_000),
    (1, 1_000_000),
    (2, 1_000_000),
    (5, 1_000_000),
    (10, 1_000_000),
    (20, 1_000_000),
    (50, 1_000_000),
    (100, 1_000_000),
    (200, 1_000_000),
    (500, 1_000_000),
    (1, 1000),
    (2, 1000),
    (5, 1000),
    (10, 1000),
    (20, 1000),
    (50, 1000),
    (100, 1000),
    (200, 1000),
    (500, 1000),
    (1, 1),
    (2, 1),
    (5, 1),
    (10, 1),
    (20, 1),
    (50, 1),
    (100, 1),
    (200, 1),
    (500, 1),
    (1000, 1),
];

/// The vertical scales any model may offer, smallest first.
pub static VDIVS: &[(u64, u64)] = &[
    (500, 1_000_000),
    (1, 1000),
    (2, 1000),
    (5, 1000),
    (10, 1000),
    (20, 1000),
    (50, 1000),
    (100, 1000),
    (200, 1000),
    (500, 1000),
    (1, 1),
    (2, 1),
    (5, 1),
    (10, 1),
    (20, 1),
    (50, 1),
    (100, 1),
];

/// Trigger sources: the analog channels, EXT and AC Line, then the
/// digital inputs for models that have them.
pub static TRIGGER_SOURCES_2: &[&str] = &[
    "CH1", "CH2", "EXT", "AC Line", "D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7", "D8", "D9",
    "D10", "D11", "D12", "D13", "D14", "D15",
];
/// Trigger sources of the four channel models.
pub static TRIGGER_SOURCES_4: &[&str] = &[
    "CH1", "CH2", "CH3", "CH4", "EXT", "AC Line", "D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7",
    "D8", "D9", "D10", "D11", "D12", "D13", "D14", "D15",
];
/// Trigger slopes.
pub static TRIGGER_SLOPES: &[&str] = &["r", "f"];
/// Couplings.
pub static COUPLINGS: &[&str] = &["AC", "DC", "GND"];
/// Probe factors.
pub static PROBE_FACTORS: &[u64] = &[1, 2, 5, 10, 20, 50, 100, 200, 500, 1000];
/// Data sources.
pub static DATA_SOURCES: &[&str] = &["Live", "Memory", "Segmented"];

impl Model {
    /// The trigger sources this model offers.
    pub fn trigger_sources(&self) -> &'static [&'static str] {
        let all = if self.analog_channels == 4 { TRIGGER_SOURCES_4 } else { TRIGGER_SOURCES_2 };
        if self.has_digital { all } else { &all[..self.analog_channels + 2] }
    }

    /// The timebases this model offers.
    pub fn timebases(&self) -> &'static [(u64, u64)] {
        let first = TIMEBASES.iter().position(|t| *t == self.min_timebase).unwrap_or(0);
        let last = TIMEBASES
            .iter()
            .position(|t| *t == self.series.max_timebase)
            .unwrap_or(TIMEBASES.len() - 1);
        &TIMEBASES[first..=last]
    }

    /// The vertical scales this model offers.
    pub fn vdivs(&self) -> &'static [(u64, u64)] {
        let first = VDIVS.iter().position(|v| *v == self.series.min_vdiv).unwrap_or(0);
        &VDIVS[first..]
    }

    /// The data sources this model offers.
    pub fn data_sources(&self) -> &'static [&'static str] {
        match self.series.protocol {
            Protocol::V1 => &DATA_SOURCES[..1],
            Protocol::V2 => &DATA_SOURCES[..2],
            _ => DATA_SOURCES,
        }
    }
}

/// The model matching an `*IDN?` answer.
pub fn find(manufacturer: &str, model: &str) -> Option<&'static Model> {
    MODELS
        .iter()
        .find(|m| m.series.vendor.full_name.eq_ignore_ascii_case(manufacturer) && m.name == model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables() {
        let m = find("Rigol Technologies", "DS1102E").unwrap();
        assert_eq!(m.series.protocol, Protocol::V2);
        assert_eq!(m.timebases().first(), Some(&(2, 1_000_000_000)));
        assert_eq!(m.timebases().last(), Some(&(50, 1)));
        assert_eq!(m.vdivs().first(), Some(&(2, 1000)));
        assert_eq!(m.trigger_sources(), &["CH1", "CH2", "EXT", "AC Line"]);
        assert_eq!(m.data_sources(), &["Live", "Memory"]);
        assert!(find("RIGOL TECHNOLOGIES", "DS1102E").is_some());
        assert!(find("Rigol Technologies", "DS9999").is_none());
        let z = find("Rigol Technologies", "MSO1104Z").unwrap();
        assert_eq!(z.trigger_sources().len(), 22);
        assert_eq!(MODELS.len(), 61);
    }
}
