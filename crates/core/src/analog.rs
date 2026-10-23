// Counterpart in sigrok:
//   libsigrok/src/analog.c,
//   Copyright (C) 2014 Bert Vermeulen <bert@biot.com>,
//   with commits by Uwe Hermann, Martin Ling, Aurelien Jacobs, Stefan Brüns,
//   Daniel Elstner, Alexandru Gagniuc, Frank Stettner, Gerhard Sittig,
//   Marek Vasut, Timo Kokkonen
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like
//   it.

//! Measured quantities, their flags and units, for analog data.

use std::fmt;

macro_rules! id_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $id:literal, $label:literal;)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum $name {
            $($(#[$vmeta])* $variant,)*
        }

        impl $name {
            /// Every variant.
            pub const ALL: &'static [$name] = &[$($name::$variant,)*];

            /// The id used on the command line and in files.
            pub fn id(self) -> &'static str {
                match self { $($name::$variant => $id,)* }
            }

            /// The display name.
            pub fn name(self) -> &'static str {
                match self { $($name::$variant => $label,)* }
            }

            /// The variant with `id`.
            pub fn from_id(id: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.id() == id)
            }
        }
    };
}

id_enum! {
    /// What an analog channel measures.
    Mq {
        /// Voltage.
        Voltage => "voltage", "Voltage";
        /// Current.
        Current => "current", "Current";
        /// Resistance.
        Resistance => "resistance", "Resistance";
        /// Capacitance.
        Capacitance => "capacitance", "Capacitance";
        /// Temperature.
        Temperature => "temperature", "Temperature";
        /// Frequency.
        Frequency => "frequency", "Frequency";
        /// Duty cycle.
        DutyCycle => "duty_cycle", "Duty cycle";
        /// Continuity.
        Continuity => "continuity", "Continuity";
        /// Pulse width.
        PulseWidth => "pulse_width", "Pulse width";
        /// Conductance.
        Conductance => "conductance", "Conductance";
        /// Power.
        Power => "power", "Power";
        /// Gain.
        Gain => "gain", "Gain";
        /// Sound pressure level.
        SoundPressureLevel => "spl", "Sound pressure level";
        /// Carbon monoxide.
        CarbonMonoxide => "co", "Carbon monoxide";
        /// Relative humidity.
        RelativeHumidity => "rh", "Relative humidity";
        /// Time.
        Time => "time", "Time";
        /// Wind speed.
        WindSpeed => "wind_speed", "Wind speed";
        /// Pressure.
        Pressure => "pressure", "Pressure";
        /// Parallel inductance.
        ParallelInductance => "parallel_inductance", "Parallel inductance";
        /// Parallel capacitance.
        ParallelCapacitance => "parallel_capacitance", "Parallel capacitance";
        /// Parallel resistance.
        ParallelResistance => "parallel_resistance", "Parallel resistance";
        /// Series inductance.
        SeriesInductance => "series_inductance", "Series inductance";
        /// Series capacitance.
        SeriesCapacitance => "series_capacitance", "Series capacitance";
        /// Series resistance.
        SeriesResistance => "series_resistance", "Series resistance";
        /// Dissipation factor.
        DissipationFactor => "dissipation_factor", "Dissipation factor";
        /// Quality factor.
        QualityFactor => "quality_factor", "Quality factor";
        /// Phase angle.
        PhaseAngle => "phase_angle", "Phase angle";
        /// Difference.
        Difference => "difference", "Difference";
        /// Count.
        Count => "count", "Count";
        /// Power factor.
        PowerFactor => "power_factor", "Power factor";
        /// Apparent power.
        ApparentPower => "apparent_power", "Apparent power";
        /// Mass.
        Mass => "mass", "Mass";
        /// Harmonic ratio.
        HarmonicRatio => "harmonic_ratio", "Harmonic ratio";
        /// Energy.
        Energy => "energy", "Energy";
        /// Electric charge.
        ElectricCharge => "electric_charge", "Electric charge";
    }
}

impl Mq {
    /// The unit a quantity is normally measured in.
    pub fn default_unit(self) -> Unit {
        match self {
            Mq::Voltage => Unit::Volt,
            Mq::Current => Unit::Ampere,
            Mq::Resistance | Mq::Continuity | Mq::ParallelResistance | Mq::SeriesResistance => {
                Unit::Ohm
            }
            Mq::Capacitance | Mq::ParallelCapacitance | Mq::SeriesCapacitance => Unit::Farad,
            Mq::Temperature => Unit::Celsius,
            Mq::Frequency => Unit::Hertz,
            Mq::DutyCycle | Mq::PulseWidth => Unit::Percentage,
            Mq::Conductance => Unit::Siemens,
            Mq::Power => Unit::Watt,
            Mq::SoundPressureLevel => Unit::DecibelSpl,
            Mq::CarbonMonoxide => Unit::Concentration,
            Mq::RelativeHumidity => Unit::Humidity293K,
            Mq::Time => Unit::Second,
            Mq::WindSpeed => Unit::MeterSecond,
            Mq::Pressure => Unit::Hectopascal,
            Mq::ParallelInductance | Mq::SeriesInductance => Unit::Henry,
            Mq::PhaseAngle => Unit::Degree,
            Mq::Count => Unit::Piece,
            Mq::ApparentPower => Unit::VoltAmpere,
            Mq::Mass => Unit::Gram,
            Mq::Energy => Unit::Joule,
            Mq::ElectricCharge => Unit::Coulomb,
            Mq::Gain
            | Mq::DissipationFactor
            | Mq::QualityFactor
            | Mq::Difference
            | Mq::PowerFactor
            | Mq::HarmonicRatio => Unit::Unitless,
        }
    }
}

id_enum! {
    /// The unit of analog samples.
    Unit {
        /// Volt.
        Volt => "V", "Volt";
        /// Ampere.
        Ampere => "A", "Ampere";
        /// Ohm.
        Ohm => "\u{2126}", "Ohm";
        /// Farad.
        Farad => "F", "Farad";
        /// Kelvin.
        Kelvin => "K", "Kelvin";
        /// Degree Celsius.
        Celsius => "\u{b0}C", "Celsius";
        /// Degree Fahrenheit.
        Fahrenheit => "\u{b0}F", "Fahrenheit";
        /// Hertz.
        Hertz => "Hz", "Hertz";
        /// Percent.
        Percentage => "%", "Percent";
        /// A boolean.
        Boolean => "", "Boolean";
        /// Second.
        Second => "s", "Second";
        /// Siemens.
        Siemens => "S", "Siemens";
        /// Decibel relative to one milliwatt.
        DecibelMw => "dBm", "dBm";
        /// Decibel relative to one volt.
        DecibelVolt => "dBV", "dBV";
        /// No unit.
        Unitless => "", "Unitless";
        /// Decibel of sound pressure.
        DecibelSpl => "dB", "dB SPL";
        /// Parts per million.
        Concentration => "ppm", "Concentration";
        /// Revolutions per minute.
        RevolutionsPerMinute => "RPM", "RPM";
        /// Volt ampere.
        VoltAmpere => "VA", "Volt ampere";
        /// Watt.
        Watt => "W", "Watt";
        /// Watt hour.
        WattHour => "Wh", "Watt hour";
        /// Meters per second.
        MeterSecond => "m/s", "Meters per second";
        /// Hectopascal.
        Hectopascal => "hPa", "Hectopascal";
        /// Relative humidity at 293 K.
        Humidity293K => "%rF", "Humidity";
        /// Degree of arc.
        Degree => "\u{b0}", "Degree";
        /// Henry.
        Henry => "H", "Henry";
        /// Gram.
        Gram => "g", "Gram";
        /// Carat.
        Carat => "ct", "Carat";
        /// Ounce.
        Ounce => "oz", "Ounce";
        /// Troy ounce.
        TroyOunce => "oz t", "Troy ounce";
        /// Pound.
        Pound => "lb", "Pound";
        /// Pennyweight.
        Pennyweight => "dwt", "Pennyweight";
        /// Grain.
        Grain => "gr", "Grain";
        /// Tael.
        Tael => "tael", "Tael";
        /// Momme.
        Momme => "momme", "Momme";
        /// Tola.
        Tola => "tola", "Tola";
        /// Piece.
        Piece => "pcs", "Piece";
        /// Joule.
        Joule => "J", "Joule";
        /// Coulomb.
        Coulomb => "C", "Coulomb";
        /// Ampere hour.
        AmpereHour => "Ah", "Ampere hour";
    }
}

impl Unit {
    /// True for units that read well with an SI prefix (`mV`, `kHz`).
    pub fn si_prefix_friendly(self) -> bool {
        matches!(
            self,
            Unit::Volt
                | Unit::Ampere
                | Unit::Ohm
                | Unit::Farad
                | Unit::Kelvin
                | Unit::Hertz
                | Unit::Second
                | Unit::Siemens
                | Unit::VoltAmpere
                | Unit::Watt
                | Unit::WattHour
                | Unit::MeterSecond
                | Unit::Henry
                | Unit::Gram
        )
    }
}

/// Flags that qualify a measurement, e.g. DC or RMS. Several may apply.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct MqFlags(u32);

macro_rules! flags {
    ($($(#[$meta:meta])* $name:ident = $bit:expr, $id:literal, $suffix:literal;)*) => {
        impl MqFlags {
            $($(#[$meta])* pub const $name: MqFlags = MqFlags(1 << $bit);)*

            const TABLE: &'static [(MqFlags, &'static str, &'static str)] = &[
                $((MqFlags::$name, $id, $suffix),)*
            ];
        }
    };
}

flags! {
    /// Alternating current.
    AC = 0, "ac", " AC";
    /// Direct current.
    DC = 1, "dc", " DC";
    /// Root mean square.
    RMS = 2, "rms", " RMS";
    /// Diode test.
    DIODE = 3, "diode", " DIODE";
    /// Held reading.
    HOLD = 4, "hold", " HOLD";
    /// Maximum.
    MAX = 5, "max", " MAX";
    /// Minimum.
    MIN = 6, "min", " MIN";
    /// Auto ranging.
    AUTORANGE = 7, "auto_range", " AUTO";
    /// Relative to a reference.
    RELATIVE = 8, "relative", " REL";
    /// Sound level, A weighted.
    SPL_FREQ_WEIGHT_A = 9, "spl_freq_weight_a", "(A)";
    /// Sound level, C weighted.
    SPL_FREQ_WEIGHT_C = 10, "spl_freq_weight_c", "(C)";
    /// Sound level, Z weighted.
    SPL_FREQ_WEIGHT_Z = 11, "spl_freq_weight_z", "(Z)";
    /// Sound level, flat.
    SPL_FREQ_WEIGHT_FLAT = 12, "spl_freq_weight_flat", "(SPL)";
    /// Sound level, slow time weighting.
    SPL_TIME_WEIGHT_S = 13, "spl_time_weight_s", " S";
    /// Sound level, fast time weighting.
    SPL_TIME_WEIGHT_F = 14, "spl_time_weight_f", " F";
    /// Sound level, time averaged.
    SPL_LAT = 15, "spl_time_average", " LAT";
    /// Sound level, percent over alarm.
    SPL_PCT_OVER_ALARM = 16, "spl_pct_over_alarm", "%oA";
    /// A duration.
    DURATION = 17, "duration", " DURATION";
    /// Averaged.
    AVG = 18, "average", " AVG";
    /// A reference value.
    REFERENCE = 19, "reference", " REF";
    /// An unstable reading.
    UNSTABLE = 20, "unstable", " UNSTABLE";
    /// Four wire measurement.
    FOUR_WIRE = 21, "four_wire", " 4-WIRE";
}

impl MqFlags {
    /// No flags.
    pub const fn empty() -> Self {
        MqFlags(0)
    }

    /// True when no flag is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True when every flag of `other` is set.
    pub const fn contains(self, other: MqFlags) -> bool {
        self.0 & other.0 == other.0
    }

    /// The flag with `id`.
    pub fn from_id(id: &str) -> Option<MqFlags> {
        Self::TABLE.iter().find(|f| f.1 == id).map(|f| f.0)
    }

    /// The set flags, one at a time.
    pub fn iter(self) -> impl Iterator<Item = MqFlags> {
        Self::TABLE.iter().map(|f| f.0).filter(move |f| self.contains(*f))
    }

    /// The id of a single flag, e.g. `dc`.
    pub fn id(self) -> &'static str {
        Self::TABLE.iter().find(|f| f.0 == self).map_or("", |f| f.1)
    }

    /// The suffixes appended to a unit for the set flags, e.g. ` DC`.
    pub fn suffix(self) -> String {
        Self::TABLE.iter().filter(|f| self.contains(f.0)).map(|f| f.2).collect()
    }
}

impl std::ops::BitOr for MqFlags {
    type Output = MqFlags;
    fn bitor(self, rhs: MqFlags) -> MqFlags {
        MqFlags(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for MqFlags {
    fn bitor_assign(&mut self, rhs: MqFlags) {
        self.0 |= rhs.0;
    }
}

impl fmt::Debug for MqFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter().map(MqFlags::id)).finish()
    }
}

/// The unit text of a measurement with its flags, as printed after a
/// value: `V DC`, `Hz`, `dB(A) S`.
pub fn unit_string(unit: Unit, flags: MqFlags) -> String {
    format!("{}{}", unit.id(), flags.suffix())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_flags() {
        assert_eq!(unit_string(Unit::Volt, MqFlags::DC), "V DC");
        assert_eq!(unit_string(Unit::Volt, MqFlags::AC | MqFlags::RMS), "V AC RMS");
        assert_eq!(Mq::from_id("voltage"), Some(Mq::Voltage));
        assert_eq!(MqFlags::from_id("dc"), Some(MqFlags::DC));
        assert_eq!(format!("{:?}", MqFlags::DC | MqFlags::MAX), "[\"dc\", \"max\"]");
        assert!(Unit::Volt.si_prefix_friendly());
        assert!(!Unit::Percentage.si_prefix_friendly());
    }
}
