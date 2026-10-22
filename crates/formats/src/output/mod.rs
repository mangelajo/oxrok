//! Output modules turn packets into text or bytes.

use oxrok_core::{Channel, ChannelKind, Error, Key, Packet, Result};
use std::collections::HashMap;

mod analog;
mod ascii;
mod bits;
mod hex;

/// Options given to an output module, e.g. `width=64`.
pub type Options = HashMap<String, String>;

/// What an output module needs to know about the device.
#[derive(Clone, Debug)]
pub struct OutputContext<'a> {
    /// All channels of the device, enabled or not.
    pub channels: &'a [Channel],
    /// The sample rate, if known before the first packet.
    pub samplerate: Option<u64>,
}

impl OutputContext<'_> {
    /// The enabled logic channels as (index, name), in device order.
    fn enabled_logic(&self) -> Vec<(usize, String)> {
        self.channels
            .iter()
            .filter(|c| c.kind == ChannelKind::Logic && c.enabled)
            .map(|c| (c.index, c.name.clone()))
            .collect()
    }
}

/// A running output.
pub trait Output {
    /// Handles one packet, appending any text to `out`.
    fn receive(&mut self, packet: &Packet, out: &mut Vec<u8>) -> Result<()>;
}

/// One option an output module accepts.
#[derive(Clone, Copy, Debug)]
pub struct OutputOption {
    /// The id, e.g. `width`.
    pub id: &'static str,
    /// A display name.
    pub name: &'static str,
    /// What it does.
    pub description: &'static str,
    /// The default, as text.
    pub default: &'static str,
}

/// An output module.
pub struct OutputModule {
    /// The id, as used with `-O`.
    pub id: &'static str,
    /// A display name.
    pub name: &'static str,
    /// A one line description.
    pub description: &'static str,
    /// The options it accepts.
    pub options: &'static [OutputOption],
    /// File name extensions it suggests.
    pub extensions: &'static [&'static str],
    /// Creates a running output.
    pub create: fn(&OutputContext, &Options) -> Result<Box<dyn Output>>,
}

impl OutputModule {
    /// Creates a running output, checking the option names.
    pub fn instantiate(&self, ctx: &OutputContext, options: &Options) -> Result<Box<dyn Output>> {
        for key in options.keys() {
            if !self.options.iter().any(|o| o.id == key) {
                log::warn!("Unknown keyword: {key}.");
            }
        }
        (self.create)(ctx, options)
    }
}

static MODULES: [&OutputModule; 4] = [&analog::MODULE, &ascii::MODULE, &bits::MODULE, &hex::MODULE];

/// All output modules, sorted by id.
pub fn modules() -> &'static [&'static OutputModule] {
    &MODULES
}

/// The output module with `id`.
pub fn find(id: &str) -> Option<&'static OutputModule> {
    MODULES.iter().copied().find(|m| m.id == id)
}

/// The header the logic outputs print before the first data.
fn header(ctx: &OutputContext, enabled: usize, samplerate: Option<u64>) -> String {
    let mut s = format!(
        "oxrok {}\nAcquisition with {}/{} channels",
        oxrok_core::VERSION,
        enabled,
        ctx.channels.len()
    );
    if let Some(rate) = samplerate.filter(|r| *r != 0) {
        s.push_str(" at ");
        s.push_str(&oxrok_core::util::samplerate_string(rate));
    }
    s.push('\n');
    s
}

/// The sample rate from a meta packet, if it carries one.
fn meta_samplerate(packet: &Packet) -> Option<u64> {
    match packet {
        Packet::Meta(items) => {
            items.iter().find(|(k, _)| *k == Key::Samplerate).and_then(|(_, v)| v.as_u64())
        }
        _ => None,
    }
}

/// Parses an unsigned option with a default.
fn option_u32(options: &Options, id: &str, default: u32) -> Result<u32> {
    match options.get(id) {
        None => Ok(default),
        Some(v) => v.parse().map_err(|_| Error::Arg(format!("invalid value '{v}' for '{id}'"))),
    }
}
