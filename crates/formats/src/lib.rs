//! File formats and text outputs of oxrok.
//!
//! The text outputs (`bits`, `hex`, `ascii`, `analog`) print the data
//! feed the way sigrok-cli does, byte for byte after the header line, so
//! scripts built on sigrok-cli keep working and the two tools can be
//! diffed against each other.

#![forbid(unsafe_code)]

pub mod output;

pub use output::{Options, Output, OutputContext, OutputModule, OutputOption};
