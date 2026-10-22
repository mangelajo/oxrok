//! File formats and text outputs of oxrok.
//!
//! - [`output`]: the text outputs (`bits`, `hex`, `ascii`, `analog`,
//!   `csv`, `vcd`, `binary`) print the data feed the way sigrok-cli does,
//!   byte for byte after the header line, and `srzip` writes session
//!   files.
//! - [`input`]: readers for VCD and raw binary files.
//! - [`sr`]: the sigrok session file format, read and written.

#![forbid(unsafe_code)]

pub mod input;
pub mod output;
pub mod sr;

pub use input::{Input, InputModule, InputOption};
pub use output::{Options, Output, OutputContext, OutputModule, OutputOption};
