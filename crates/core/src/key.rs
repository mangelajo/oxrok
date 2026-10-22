//! Configuration keys.
//!
//! The well known keys are the ones sigrok defines, with the same ids,
//! so `-c samplerate=1m` and `.sr` metadata mean the same thing here. A
//! driver that needs a setting sigrok has no key for publishes a
//! [`Key::Custom`] with its own id; front ends treat both alike.

use crate::value::ValueKind;
use std::fmt;

/// A configuration key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    /// `conn`: Connection.
    Conn,
    /// `serialcomm`: Serial communication.
    Serialcomm,
    /// `modbusaddr`: Modbus slave address.
    Modbusaddr,
    /// `force_detect`: Forced detection.
    ForceDetect,
    /// `probe_names`: Names of device's probes.
    ProbeNames,
    /// `samplerate`: Sample rate.
    Samplerate,
    /// `captureratio`: Pre-trigger capture ratio.
    CaptureRatio,
    /// `pattern`: Pattern.
    PatternMode,
    /// `rle`: Run length encoding.
    Rle,
    /// `triggerslope`: Trigger slope.
    TriggerSlope,
    /// `averaging`: Averaging.
    Averaging,
    /// `avg_samples`: Number of samples to average over.
    AvgSamples,
    /// `triggersource`: Trigger source.
    TriggerSource,
    /// `horiz_triggerpos`: Horizontal trigger position.
    HorizTriggerpos,
    /// `buffersize`: Buffer size.
    Buffersize,
    /// `timebase`: Time base.
    Timebase,
    /// `filter`: Filter.
    Filter,
    /// `vdiv`: Volts/div.
    Vdiv,
    /// `coupling`: Coupling.
    Coupling,
    /// `triggermatch`: Trigger matches.
    TriggerMatch,
    /// `sample_interval`: Sample interval.
    SampleInterval,
    /// `num_hdiv`: Number of horizontal divisions.
    NumHdiv,
    /// `num_vdiv`: Number of vertical divisions.
    NumVdiv,
    /// `spl_weight_freq`: Sound pressure level frequency weighting.
    SplWeightFreq,
    /// `spl_weight_time`: Sound pressure level time weighting.
    SplWeightTime,
    /// `spl_meas_range`: Sound pressure level measurement range.
    SplMeasurementRange,
    /// `hold_max`: Hold max.
    HoldMax,
    /// `hold_min`: Hold min.
    HoldMin,
    /// `voltage_threshold`: Voltage threshold.
    VoltageThreshold,
    /// `external_clock`: External clock mode.
    ExternalClock,
    /// `swap`: Swap channel order.
    Swap,
    /// `center_frequency`: Center frequency.
    CenterFrequency,
    /// `logic_channels`: Number of logic channels.
    NumLogicChannels,
    /// `analog_channels`: Number of analog channels.
    NumAnalogChannels,
    /// `voltage`: Current voltage.
    Voltage,
    /// `voltage_target`: Voltage target.
    VoltageTarget,
    /// `current`: Current current.
    Current,
    /// `current_limit`: Current limit.
    CurrentLimit,
    /// `enabled`: Channel enabled.
    Enabled,
    /// `channel_config`: Channel modes.
    ChannelConfig,
    /// `ovp_enabled`: Over-voltage protection enabled.
    OverVoltageProtectionEnabled,
    /// `ovp_active`: Over-voltage protection active.
    OverVoltageProtectionActive,
    /// `ovp_threshold`: Over-voltage protection threshold.
    OverVoltageProtectionThreshold,
    /// `ocp_enabled`: Over-current protection enabled.
    OverCurrentProtectionEnabled,
    /// `ocp_active`: Over-current protection active.
    OverCurrentProtectionActive,
    /// `ocp_threshold`: Over-current protection threshold.
    OverCurrentProtectionThreshold,
    /// `clock_edge`: Clock edge.
    ClockEdge,
    /// `amplitude`: Amplitude.
    Amplitude,
    /// `regulation`: Channel regulation.
    Regulation,
    /// `otp`: Over-temperature protection.
    OverTemperatureProtection,
    /// `output_frequency`: Output frequency.
    OutputFrequency,
    /// `output_frequency_target`: Output frequency target.
    OutputFrequencyTarget,
    /// `measured_quantity`: Measured quantity.
    MeasuredQuantity,
    /// `equiv_circuit_model`: Equivalent circuit model.
    EquivCircuitModel,
    /// `otp_active`: Over-temperature protection active.
    OverTemperatureProtectionActive,
    /// `uvc`: Under-voltage condition.
    UnderVoltageCondition,
    /// `uvc_active`: Under-voltage condition active.
    UnderVoltageConditionActive,
    /// `uvc_threshold`: Under-voltage condition threshold.
    UnderVoltageConditionThreshold,
    /// `triggerlevel`: Trigger level.
    TriggerLevel,
    /// `external_clock_source`: External clock source.
    ExternalClockSource,
    /// `offset`: Offset.
    Offset,
    /// `triggerpattern`: Trigger pattern.
    TriggerPattern,
    /// `highresolution`: High resolution.
    HighResolution,
    /// `peakdetection`: Peak detection.
    PeakDetection,
    /// `logic_threshold`: Logic threshold (predefined).
    LogicThreshold,
    /// `logic_threshold_custom`: Logic threshold (custom).
    LogicThresholdCustom,
    /// `range`: Range.
    Range,
    /// `digits`: Digits.
    Digits,
    /// `phase`: Phase.
    Phase,
    /// `output_duty_cycle`: Duty Cycle.
    DutyCycle,
    /// `power`: Power.
    Power,
    /// `power_target`: Power Target.
    PowerTarget,
    /// `resistance_target`: Resistance Target.
    ResistanceTarget,
    /// `ocp_delay`: Over-current protection delay.
    OverCurrentProtectionDelay,
    /// `inverted`: Signal inverted.
    Inverted,
    /// `sessionfile`: Session file.
    Sessionfile,
    /// `capturefile`: Capture file.
    Capturefile,
    /// `capture_unitsize`: Capture unitsize.
    CaptureUnitsize,
    /// `power_off`: Power off.
    PowerOff,
    /// `data_source`: Data source.
    DataSource,
    /// `probe_factor`: Probe factor.
    ProbeFactor,
    /// `nplc`: Number of ADC powerline cycles.
    AdcPowerlineCycles,
    /// `limit_time`: Time limit.
    LimitMsec,
    /// `limit_samples`: Sample limit.
    LimitSamples,
    /// `limit_frames`: Frame limit.
    LimitFrames,
    /// `continuous`: Continuous sampling.
    Continuous,
    /// `datalog`: Datalog.
    Datalog,
    /// `device_mode`: Device mode.
    DeviceMode,
    /// `test_mode`: Test mode.
    TestMode,
    /// `opp_enabled`: Over-power protection enabled.
    OverPowerProtectionEnabled,
    /// `opp_active`: Over-power protection active.
    OverPowerProtectionActive,
    /// `opp_threshold`: Over-power protection threshold.
    OverPowerProtectionThreshold,
    /// `resistance`: Resistance.
    Resistance,
    /// `gate_time`: Gate time.
    GateTime,
    /// A driver specific key. The id must be unique within the driver
    /// and is used on the command line like a well known id.
    Custom(&'static CustomKey),
}

/// Description of a driver specific key.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct CustomKey {
    /// The id, e.g. `state_qualifier`. Lower case, no spaces.
    pub id: &'static str,
    /// A display name, e.g. `State qualifier`.
    pub name: &'static str,
    /// What the key means, one sentence.
    pub description: &'static str,
    /// The value type.
    pub kind: ValueKind,
}

/// The table of well known keys: key, id, name, kind.
const KNOWN: &[(Key, &str, &str, ValueKind)] = &[
    (Key::Conn, "conn", "Connection", ValueKind::Str),
    (Key::Serialcomm, "serialcomm", "Serial communication", ValueKind::Str),
    (Key::Modbusaddr, "modbusaddr", "Modbus slave address", ValueKind::U64),
    (Key::ForceDetect, "force_detect", "Forced detection", ValueKind::Str),
    (Key::ProbeNames, "probe_names", "Names of device's probes", ValueKind::Str),
    (Key::Samplerate, "samplerate", "Sample rate", ValueKind::U64),
    (Key::CaptureRatio, "captureratio", "Pre-trigger capture ratio", ValueKind::U64),
    (Key::PatternMode, "pattern", "Pattern", ValueKind::Str),
    (Key::Rle, "rle", "Run length encoding", ValueKind::Bool),
    (Key::TriggerSlope, "triggerslope", "Trigger slope", ValueKind::Str),
    (Key::Averaging, "averaging", "Averaging", ValueKind::Bool),
    (Key::AvgSamples, "avg_samples", "Number of samples to average over", ValueKind::U64),
    (Key::TriggerSource, "triggersource", "Trigger source", ValueKind::Str),
    (Key::HorizTriggerpos, "horiz_triggerpos", "Horizontal trigger position", ValueKind::F64),
    (Key::Buffersize, "buffersize", "Buffer size", ValueKind::U64),
    (Key::Timebase, "timebase", "Time base", ValueKind::RationalPeriod),
    (Key::Filter, "filter", "Filter", ValueKind::Bool),
    (Key::Vdiv, "vdiv", "Volts/div", ValueKind::RationalVolt),
    (Key::Coupling, "coupling", "Coupling", ValueKind::Str),
    (Key::TriggerMatch, "triggermatch", "Trigger matches", ValueKind::I32),
    (Key::SampleInterval, "sample_interval", "Sample interval", ValueKind::U64),
    (Key::NumHdiv, "num_hdiv", "Number of horizontal divisions", ValueKind::I32),
    (Key::NumVdiv, "num_vdiv", "Number of vertical divisions", ValueKind::I32),
    (
        Key::SplWeightFreq,
        "spl_weight_freq",
        "Sound pressure level frequency weighting",
        ValueKind::Str,
    ),
    (Key::SplWeightTime, "spl_weight_time", "Sound pressure level time weighting", ValueKind::Str),
    (
        Key::SplMeasurementRange,
        "spl_meas_range",
        "Sound pressure level measurement range",
        ValueKind::RangeU64,
    ),
    (Key::HoldMax, "hold_max", "Hold max", ValueKind::Bool),
    (Key::HoldMin, "hold_min", "Hold min", ValueKind::Bool),
    (Key::VoltageThreshold, "voltage_threshold", "Voltage threshold", ValueKind::RangeF64),
    (Key::ExternalClock, "external_clock", "External clock mode", ValueKind::Bool),
    (Key::Swap, "swap", "Swap channel order", ValueKind::Bool),
    (Key::CenterFrequency, "center_frequency", "Center frequency", ValueKind::U64),
    (Key::NumLogicChannels, "logic_channels", "Number of logic channels", ValueKind::I32),
    (Key::NumAnalogChannels, "analog_channels", "Number of analog channels", ValueKind::I32),
    (Key::Voltage, "voltage", "Current voltage", ValueKind::F64),
    (Key::VoltageTarget, "voltage_target", "Voltage target", ValueKind::F64),
    (Key::Current, "current", "Current current", ValueKind::F64),
    (Key::CurrentLimit, "current_limit", "Current limit", ValueKind::F64),
    (Key::Enabled, "enabled", "Channel enabled", ValueKind::Bool),
    (Key::ChannelConfig, "channel_config", "Channel modes", ValueKind::Str),
    (
        Key::OverVoltageProtectionEnabled,
        "ovp_enabled",
        "Over-voltage protection enabled",
        ValueKind::Bool,
    ),
    (
        Key::OverVoltageProtectionActive,
        "ovp_active",
        "Over-voltage protection active",
        ValueKind::Bool,
    ),
    (
        Key::OverVoltageProtectionThreshold,
        "ovp_threshold",
        "Over-voltage protection threshold",
        ValueKind::F64,
    ),
    (
        Key::OverCurrentProtectionEnabled,
        "ocp_enabled",
        "Over-current protection enabled",
        ValueKind::Bool,
    ),
    (
        Key::OverCurrentProtectionActive,
        "ocp_active",
        "Over-current protection active",
        ValueKind::Bool,
    ),
    (
        Key::OverCurrentProtectionThreshold,
        "ocp_threshold",
        "Over-current protection threshold",
        ValueKind::F64,
    ),
    (Key::ClockEdge, "clock_edge", "Clock edge", ValueKind::Str),
    (Key::Amplitude, "amplitude", "Amplitude", ValueKind::F64),
    (Key::Regulation, "regulation", "Channel regulation", ValueKind::Str),
    (Key::OverTemperatureProtection, "otp", "Over-temperature protection", ValueKind::Bool),
    (Key::OutputFrequency, "output_frequency", "Output frequency", ValueKind::F64),
    (
        Key::OutputFrequencyTarget,
        "output_frequency_target",
        "Output frequency target",
        ValueKind::F64,
    ),
    (Key::MeasuredQuantity, "measured_quantity", "Measured quantity", ValueKind::Mq),
    (Key::EquivCircuitModel, "equiv_circuit_model", "Equivalent circuit model", ValueKind::Str),
    (
        Key::OverTemperatureProtectionActive,
        "otp_active",
        "Over-temperature protection active",
        ValueKind::Bool,
    ),
    (Key::UnderVoltageCondition, "uvc", "Under-voltage condition", ValueKind::Bool),
    (
        Key::UnderVoltageConditionActive,
        "uvc_active",
        "Under-voltage condition active",
        ValueKind::Bool,
    ),
    (
        Key::UnderVoltageConditionThreshold,
        "uvc_threshold",
        "Under-voltage condition threshold",
        ValueKind::F64,
    ),
    (Key::TriggerLevel, "triggerlevel", "Trigger level", ValueKind::F64),
    (Key::ExternalClockSource, "external_clock_source", "External clock source", ValueKind::Str),
    (Key::Offset, "offset", "Offset", ValueKind::F64),
    (Key::TriggerPattern, "triggerpattern", "Trigger pattern", ValueKind::Str),
    (Key::HighResolution, "highresolution", "High resolution", ValueKind::Bool),
    (Key::PeakDetection, "peakdetection", "Peak detection", ValueKind::Bool),
    (Key::LogicThreshold, "logic_threshold", "Logic threshold (predefined)", ValueKind::Str),
    (
        Key::LogicThresholdCustom,
        "logic_threshold_custom",
        "Logic threshold (custom)",
        ValueKind::F64,
    ),
    (Key::Range, "range", "Range", ValueKind::Str),
    (Key::Digits, "digits", "Digits", ValueKind::Str),
    (Key::Phase, "phase", "Phase", ValueKind::F64),
    (Key::DutyCycle, "output_duty_cycle", "Duty Cycle", ValueKind::F64),
    (Key::Power, "power", "Power", ValueKind::F64),
    (Key::PowerTarget, "power_target", "Power Target", ValueKind::F64),
    (Key::ResistanceTarget, "resistance_target", "Resistance Target", ValueKind::F64),
    (Key::OverCurrentProtectionDelay, "ocp_delay", "Over-current protection delay", ValueKind::F64),
    (Key::Inverted, "inverted", "Signal inverted", ValueKind::Bool),
    (Key::Sessionfile, "sessionfile", "Session file", ValueKind::Str),
    (Key::Capturefile, "capturefile", "Capture file", ValueKind::Str),
    (Key::CaptureUnitsize, "capture_unitsize", "Capture unitsize", ValueKind::U64),
    (Key::PowerOff, "power_off", "Power off", ValueKind::Bool),
    (Key::DataSource, "data_source", "Data source", ValueKind::Str),
    (Key::ProbeFactor, "probe_factor", "Probe factor", ValueKind::U64),
    (Key::AdcPowerlineCycles, "nplc", "Number of ADC powerline cycles", ValueKind::F64),
    (Key::LimitMsec, "limit_time", "Time limit", ValueKind::U64),
    (Key::LimitSamples, "limit_samples", "Sample limit", ValueKind::U64),
    (Key::LimitFrames, "limit_frames", "Frame limit", ValueKind::U64),
    (Key::Continuous, "continuous", "Continuous sampling", ValueKind::Bool),
    (Key::Datalog, "datalog", "Datalog", ValueKind::Bool),
    (Key::DeviceMode, "device_mode", "Device mode", ValueKind::Str),
    (Key::TestMode, "test_mode", "Test mode", ValueKind::Str),
    (
        Key::OverPowerProtectionEnabled,
        "opp_enabled",
        "Over-power protection enabled",
        ValueKind::Bool,
    ),
    (Key::OverPowerProtectionActive, "opp_active", "Over-power protection active", ValueKind::Bool),
    (
        Key::OverPowerProtectionThreshold,
        "opp_threshold",
        "Over-power protection threshold",
        ValueKind::F64,
    ),
    (Key::Resistance, "resistance", "Resistance", ValueKind::F64),
    (Key::GateTime, "gate_time", "Gate time", ValueKind::RationalPeriod),
];

impl Key {
    /// The id, as used on the command line and in files.
    pub fn id(&self) -> &'static str {
        match self {
            Key::Custom(c) => c.id,
            _ => KNOWN.iter().find(|k| &k.0 == self).map_or("", |k| k.1),
        }
    }

    /// The display name.
    pub fn name(&self) -> &'static str {
        match self {
            Key::Custom(c) => c.name,
            _ => KNOWN.iter().find(|k| &k.0 == self).map_or("", |k| k.2),
        }
    }

    /// The value type.
    pub fn kind(&self) -> ValueKind {
        match self {
            Key::Custom(c) => c.kind,
            _ => KNOWN.iter().find(|k| &k.0 == self).map_or(ValueKind::Str, |k| k.3),
        }
    }

    /// The well known key with `id`, if any. Custom keys are resolved by
    /// the device that owns them, see
    /// [`Device::options`](crate::Device::options).
    pub fn from_id(id: &str) -> Option<Key> {
        KNOWN.iter().find(|k| k.1 == id).map(|k| k.0.clone())
    }

    /// Every well known key.
    pub fn known() -> impl Iterator<Item = &'static Key> {
        KNOWN.iter().map(|k| &k.0)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_keys_round_trip() {
        assert_eq!(Key::Samplerate.id(), "samplerate");
        assert_eq!(Key::Samplerate.kind(), ValueKind::U64);
        assert_eq!(Key::from_id("limit_time"), Some(Key::LimitMsec));
        assert_eq!(Key::VoltageThreshold.kind(), ValueKind::RangeF64);
        assert_eq!(Key::PatternMode.id(), "pattern");
        assert_eq!(Key::from_id("nope"), None);
        for k in Key::known() {
            assert_eq!(Key::from_id(k.id()).as_ref(), Some(k), "{k:?}");
        }
    }

    #[test]
    fn custom_keys_describe_themselves() {
        static QUAL: CustomKey = CustomKey {
            id: "state_qualifier",
            name: "State qualifier",
            description: "Only store samples while the qualifier matches.",
            kind: ValueKind::Str,
        };
        let k = Key::Custom(&QUAL);
        assert_eq!(k.id(), "state_qualifier");
        assert_eq!(k.to_string(), "state_qualifier");
    }
}
