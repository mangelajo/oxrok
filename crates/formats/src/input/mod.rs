//! Input modules read files into the packet stream.

use oxrok_core::{Channel, Packet, Result};
use std::collections::HashMap;

mod binary;
mod vcd;

/// Options given to an input module, e.g. `samplerate=1m`.
pub type Options = HashMap<String, String>;

/// A running input: feed it bytes, collect packets.
///
/// Packets start with a header once the module knows the channels, so
/// [`Input::channels`] is `Some` from the first header on.
pub trait Input {
    /// Consumes more of the file. Packets are appended to `out`.
    fn feed(&mut self, data: &[u8], out: &mut Vec<Packet>) -> Result<()>;
    /// The file is complete. The last packets, ending with
    /// [`Packet::End`], are appended to `out`.
    fn end(&mut self, out: &mut Vec<Packet>) -> Result<()>;
    /// The channels, once the header was parsed.
    fn channels(&self) -> Option<&[Channel]>;
    /// Disables channels the user did not select, by position.
    fn enable_channel(&mut self, pos: usize, enabled: bool);
}

/// The value type of an input option, for parsing the command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputOptionKind {
    /// A size string like `200k`.
    U64,
    /// A small count.
    I32,
    /// Text.
    Str,
    /// `true`/`false`.
    Bool,
}

/// One option an input module accepts.
#[derive(Clone, Copy, Debug)]
pub struct InputOption {
    /// The id, e.g. `samplerate`.
    pub id: &'static str,
    /// A display name.
    pub name: &'static str,
    /// What it does.
    pub description: &'static str,
    /// The value type.
    pub kind: InputOptionKind,
    /// The default, as text.
    pub default: &'static str,
}

/// An input module.
pub struct InputModule {
    /// The id, as used with `-I`.
    pub id: &'static str,
    /// A display name.
    pub name: &'static str,
    /// A one line description.
    pub description: &'static str,
    /// File name extensions it handles.
    pub extensions: &'static [&'static str],
    /// The options it accepts.
    pub options: &'static [InputOption],
    /// Creates a running input.
    pub create: fn(&Options) -> Result<Box<dyn Input>>,
    /// Looks at the first bytes of a file and says how confident the
    /// module is that it can read it: 0 is no, 100 is certain.
    pub detect: fn(head: &[u8], filename: Option<&str>) -> u8,
}

impl InputModule {
    /// Creates a running input, warning about unknown option names.
    pub fn instantiate(&self, options: &Options) -> Result<Box<dyn Input>> {
        for key in options.keys() {
            if !self.options.iter().any(|o| o.id == key) {
                log::warn!("Unknown keyword: {key}.");
            }
        }
        (self.create)(options)
    }
}

static MODULES: [&InputModule; 2] = [&binary::MODULE, &vcd::MODULE];

/// All input modules, sorted by id.
pub fn modules() -> &'static [&'static InputModule] {
    &MODULES
}

/// The input module with `id`.
pub fn find(id: &str) -> Option<&'static InputModule> {
    MODULES.iter().copied().find(|m| m.id == id)
}

/// The module most confident it can read a file that starts with `head`.
pub fn detect(head: &[u8], filename: Option<&str>) -> Option<&'static InputModule> {
    MODULES
        .iter()
        .copied()
        .map(|m| ((m.detect)(head, filename), m))
        .filter(|(c, _)| *c > 0)
        .max_by_key(|(c, _)| *c)
        .map(|(_, m)| m)
}

/// Parses an option as an unsigned size string, with a default.
fn option_u64(options: &Options, id: &str, default: u64) -> Result<u64> {
    match options.get(id) {
        None => Ok(default),
        Some(v) => oxrok_core::util::parse_sizestring(v),
    }
}
