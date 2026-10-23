//! How a decoder describes itself.

use crate::core::{Error, Result};
use std::borrow::Cow;

/// A decoder input channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelDesc {
    /// The id used in `-P dec:id=D0`, e.g. `rx`.
    pub id: &'static str,
    /// A short name, e.g. `RX`.
    pub name: &'static str,
    /// What it carries.
    pub desc: &'static str,
}

/// An option value. The default sets the type.
#[derive(Clone, Debug, PartialEq)]
pub enum OptionValue {
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// A string.
    Str(Cow<'static, str>),
}

impl OptionValue {
    /// A static string value, usable in `static` tables.
    pub const fn s(v: &'static str) -> OptionValue {
        OptionValue::Str(Cow::Borrowed(v))
    }

    /// An integer value.
    pub const fn i(v: i64) -> OptionValue {
        OptionValue::Int(v)
    }

    /// A float value.
    pub const fn f(v: f64) -> OptionValue {
        OptionValue::Float(v)
    }

    /// The value as text, as `--show` prints it: strings quoted, floats
    /// with one decimal at least.
    pub fn show(&self) -> String {
        match self {
            OptionValue::Int(v) => v.to_string(),
            OptionValue::Float(v) => {
                if v.fract() == 0.0 {
                    format!("{v:.1}")
                } else {
                    v.to_string()
                }
            }
            OptionValue::Str(s) => format!("'{s}'"),
        }
    }
}

/// A decoder option.
#[derive(Clone, Debug, PartialEq)]
pub struct OptionDesc {
    /// The id used in `-P dec:id=value`.
    pub id: &'static str,
    /// What it does.
    pub desc: &'static str,
    /// The default, which also fixes the type.
    pub default: OptionValue,
    /// Allowed values, empty for any.
    pub values: &'static [OptionValue],
}

impl OptionDesc {
    /// Parses `text` to the option's type.
    pub fn parse(&self, text: &str) -> Result<OptionValue> {
        let bad = || Error::Arg(format!("Invalid value '{text}' for option '{}'.", self.id));
        Ok(match self.default {
            OptionValue::Int(_) => OptionValue::Int(text.parse().map_err(|_| bad())?),
            OptionValue::Float(_) => OptionValue::Float(text.parse().map_err(|_| bad())?),
            OptionValue::Str(_) => OptionValue::Str(Cow::Owned(text.to_owned())),
        })
    }
}

/// An annotation class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnClass {
    /// The id used in `-A dec=id`.
    pub id: &'static str,
    /// A description.
    pub desc: &'static str,
}

/// A row grouping annotation classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnRow {
    /// The id.
    pub id: &'static str,
    /// A description.
    pub desc: &'static str,
    /// Indices into the annotation classes.
    pub classes: &'static [usize],
}

/// A binary output class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinClass {
    /// The id used in `-B dec=id`.
    pub id: &'static str,
    /// A description.
    pub desc: &'static str,
}

/// Everything static about a decoder.
#[derive(Clone, Debug, PartialEq)]
pub struct DecoderInfo {
    /// The id, e.g. `uart`.
    pub id: &'static str,
    /// A short name.
    pub name: &'static str,
    /// The long name.
    pub longname: &'static str,
    /// One line.
    pub desc: &'static str,
    /// License of this implementation.
    pub license: &'static str,
    /// Input ids it accepts, `logic` or another decoder's output id.
    pub inputs: &'static [&'static str],
    /// Output ids it produces.
    pub outputs: &'static [&'static str],
    /// Tags, e.g. `Embedded/industrial`.
    pub tags: &'static [&'static str],
    /// Required channels.
    pub channels: &'static [ChannelDesc],
    /// Optional channels.
    pub optional_channels: &'static [ChannelDesc],
    /// Options.
    pub options: &'static [OptionDesc],
    /// Annotation classes.
    pub annotations: &'static [AnnClass],
    /// Annotation rows.
    pub annotation_rows: &'static [AnnRow],
    /// Binary classes.
    pub binary: &'static [BinClass],
    /// Longer documentation.
    pub doc: &'static str,
}

impl DecoderInfo {
    /// The annotation classes selected by `id`: a class id or a row id.
    pub fn ann_classes(&self, id: &str) -> Vec<usize> {
        if let Some(i) = self.annotations.iter().position(|a| canon_eq(a.id, id)) {
            return vec![i];
        }
        if let Some(r) = self.annotation_rows.iter().find(|r| canon_eq(r.id, id)) {
            return r.classes.to_vec();
        }
        Vec::new()
    }
}

/// Compares ids ignoring case and treating `-` and `_` alike, as
/// sigrok-cli does.
pub fn canon_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.chars().zip(b.chars()).all(|(x, y)| {
            let x = if x == '-' { '_' } else { x.to_ascii_lowercase() };
            let y = if y == '-' { '_' } else { y.to_ascii_lowercase() };
            x == y
        })
}
