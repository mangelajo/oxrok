// Counterpart in sigrok:
//   libsigrokdecode (the decoder API, instance handling and the sample
//   matching of wait()).
// This file is an independent implementation written for oxrok from the
//   documented decoder API and the observed behaviour.

//! The protocol decoder runtime.
//!
//! A [`Decoder`] describes itself with a [`DecoderInfo`] and runs its
//! [`Decoder::decode`] loop against a [`Context`], which hands out
//! samples through `wait` and takes annotations, binary data and items
//! for stacked decoders through `put_*`. The API mirrors
//! libsigrokdecode's Python API, so Python decoders run on the same
//! runtime through a thin host and native decoders read like them.
//!
//! A [`Stack`] runs instances on their own threads, fed with logic
//! packets, and delivers their output in order.

#![forbid(unsafe_code)]

pub mod info;
pub mod stream;

pub use info::{AnnClass, AnnRow, BinClass, ChannelDesc, DecoderInfo, OptionDesc, OptionValue};
pub use stream::{Cond, CondKind, Pins, Stream, StreamWriter};

use oxrok_core::{Error, Result};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

/// Output kinds a decoder registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    /// Annotations: a class index and texts, longest first.
    Annotation,
    /// Items for a stacked decoder.
    Python,
    /// Binary data of a binary class.
    Binary,
    /// Metadata.
    Meta,
}

/// An item passed from one decoder to the one stacked on it.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A string, e.g. a state name.
    Str(String),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// A list of items.
    List(Vec<Item>),
    /// Raw bytes.
    Bytes(Vec<u8>),
    /// A boolean.
    Bool(bool),
    /// Nothing.
    None,
}

/// What a decoder emitted.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// An annotation over `ss..es`.
    Annotation {
        /// Instance id, e.g. `uart-1`.
        instance: String,
        /// Start sample.
        ss: u64,
        /// End sample.
        es: u64,
        /// Index into `DecoderInfo::annotations`.
        class: usize,
        /// Texts, longest first.
        texts: Vec<String>,
    },
    /// Binary output.
    Binary {
        /// Instance id.
        instance: String,
        /// Start sample.
        ss: u64,
        /// End sample.
        es: u64,
        /// Index into `DecoderInfo::binary`.
        class: usize,
        /// The bytes.
        data: Vec<u8>,
    },
    /// An item for stacked decoders.
    Python {
        /// Instance id.
        instance: String,
        /// Start sample.
        ss: u64,
        /// End sample.
        es: u64,
        /// The item.
        item: Item,
    },
    /// Metadata.
    Meta {
        /// Instance id.
        instance: String,
        /// Start sample.
        ss: u64,
        /// End sample.
        es: u64,
        /// The value.
        item: Item,
    },
    /// The instance's decode loop ended, with an error message when it
    /// failed.
    End {
        /// Instance id.
        instance: String,
        /// The error, if any.
        error: Option<String>,
    },
}

/// What a decoder sees while decoding.
pub trait Context {
    /// Waits for one of the condition groups to match (each group is a
    /// conjunction). Returns the pin levels at that sample, or
    /// [`Error::Eof`] at the end of the data. With no groups, advances
    /// one sample.
    fn wait(&mut self, conds: &[&[Cond]]) -> Result<Pins>;

    /// The current sample number.
    fn samplenum(&self) -> u64;

    /// Which groups matched at the last `wait`.
    fn matched(&self) -> &[bool];

    /// True when the decoder channel is assigned.
    fn has_channel(&self, channel: usize) -> bool;

    /// Emits an annotation.
    fn put_ann(&mut self, ss: u64, es: u64, class: usize, texts: Vec<String>);

    /// Emits binary data.
    fn put_binary(&mut self, ss: u64, es: u64, class: usize, data: Vec<u8>);

    /// Emits an item for stacked decoders.
    fn put_python(&mut self, ss: u64, es: u64, item: Item);

    /// Emits metadata.
    fn put_meta(&mut self, ss: u64, es: u64, item: Item);
}

/// The configuration of one instance.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// Option values by id, parsed to the option's type.
    pub options: HashMap<String, OptionValue>,
    /// Decoder channel (required then optional, in declaration order)
    /// to logic channel index, when assigned.
    pub channels: Vec<Option<usize>>,
}

impl Config {
    /// An option as integer, or its default.
    pub fn int(&self, id: &str, default: i64) -> i64 {
        match self.options.get(id) {
            Some(OptionValue::Int(v)) => *v,
            Some(OptionValue::Float(v)) => *v as i64,
            _ => default,
        }
    }

    /// An option as float, or its default.
    pub fn float(&self, id: &str, default: f64) -> f64 {
        match self.options.get(id) {
            Some(OptionValue::Float(v)) => *v,
            Some(OptionValue::Int(v)) => *v as f64,
            _ => default,
        }
    }

    /// An option as string, or its default.
    pub fn str<'a>(&'a self, id: &str, default: &'a str) -> &'a str {
        match self.options.get(id) {
            Some(OptionValue::Str(v)) => v,
            _ => default,
        }
    }
}

/// A protocol decoder.
pub trait Decoder: Send {
    /// What this decoder is.
    fn info(&self) -> &'static DecoderInfo;

    /// Prepares for a run with `config`. Called once before `decode`.
    fn start(&mut self, config: &Config) -> Result<()>;

    /// The sample rate of the logic input, before `decode`.
    fn metadata(&mut self, samplerate: u64) {
        let _ = samplerate;
    }

    /// The decode loop of a logic-input decoder: runs until `wait`
    /// returns [`Error::Eof`].
    fn decode(&mut self, ctx: &mut dyn Context) -> Result<()>;

    /// One item from the decoder below, for stacked decoders.
    fn decode_item(&mut self, ss: u64, es: u64, item: &Item, ctx: &mut dyn Context) -> Result<()> {
        let _ = (ss, es, item, ctx);
        Err(Error::not_supported("this decoder takes logic input"))
    }
}

/// How to create a decoder.
pub struct DecoderDef {
    /// Its description.
    pub info: &'static DecoderInfo,
    /// Creates a fresh instance.
    pub new: fn() -> Box<dyn Decoder>,
}

/// The decoders a program knows about.
#[derive(Default)]
pub struct Registry {
    defs: Vec<&'static DecoderDef>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Registry::default()
    }

    /// Adds decoders. Kept sorted by id.
    pub fn register(&mut self, defs: &[&'static DecoderDef]) {
        self.defs.extend_from_slice(defs);
        self.defs.sort_by_key(|d| d.info.id);
        self.defs.dedup_by_key(|d| d.info.id);
    }

    /// All decoders, sorted by id.
    pub fn decoders(&self) -> impl Iterator<Item = &'static DecoderDef> + '_ {
        self.defs.iter().copied()
    }

    /// The decoder with `id`.
    pub fn find(&self, id: &str) -> Option<&'static DecoderDef> {
        self.defs.iter().copied().find(|d| d.info.id == id)
    }
}

/// Parses `key=value` options against a decoder's option table; keys
/// that name channels assign them by logic channel name.
///
/// As in libsigrokdecode, decoder channels left unassigned default to
/// the logic channel of the same index when there is one, and a name
/// that matches no channel is a warning, not an error.
pub fn parse_spec(
    info: &DecoderInfo,
    pairs: &[(String, String)],
    channel_names: &[String],
) -> Result<Config> {
    let count = info.channels.len() + info.optional_channels.len();
    let mut config = Config {
        options: info.options.iter().map(|o| (o.id.to_owned(), o.default.clone())).collect(),
        channels: (0..count).map(|i| (i < channel_names.len()).then_some(i)).collect(),
    };
    let mut assigned = vec![false; count];
    for (key, value) in pairs {
        if let Some(pos) = info
            .channels
            .iter()
            .chain(info.optional_channels.iter())
            .position(|c| c.id.eq_ignore_ascii_case(key))
        {
            assigned[pos] = true;
            config.channels[pos] = channel_names.iter().position(|n| n == value);
            if config.channels[pos].is_none() {
                log::warn!("No channel with name \"{value}\" found.");
            }
            continue;
        }
        let opt = info.options.iter().find(|o| o.id == key).ok_or_else(|| {
            Error::Arg(format!("Unknown option or channel '{key}' for decoder '{}'.", info.id))
        })?;
        let parsed = opt.parse(value)?;
        if !opt.values.is_empty() && !opt.values.contains(&parsed) {
            return Err(Error::Arg(format!(
                "Invalid value '{value}' for option '{key}' of decoder '{}'.",
                info.id
            )));
        }
        config.options.insert(key.clone(), parsed);
    }
    // A user mapping replaces the default for every channel.
    if assigned.iter().any(|&a| a) {
        for (i, a) in assigned.iter().enumerate() {
            if !a {
                config.channels[i] = None;
            }
        }
    }
    for (i, c) in info.channels.iter().enumerate() {
        if config.channels[i].is_none() {
            return Err(Error::Arg(format!(
                "Decoder '{}' needs channel '{}' assigned.",
                info.id, c.id
            )));
        }
    }
    Ok(config)
}

/// One running decoder instance.
struct Running {
    instance: String,
    handle: Option<JoinHandle<()>>,
    writer: StreamWriter,
}

/// A set of decoder instances fed from one logic stream. Stacking is
/// not implemented yet: every instance reads the logic input.
pub struct Stack {
    running: Vec<Running>,
    tx: Sender<Output>,
    rx: Option<Receiver<Output>>,
    unit_size: usize,
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}

impl Stack {
    /// An empty stack.
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Stack { running: Vec::new(), tx, rx: Some(rx), unit_size: 0 }
    }

    /// Adds an instance and starts its thread. `unit_size` is the bytes
    /// per sample of the logic input, `samplerate` its rate.
    pub fn add(
        &mut self,
        instance: String,
        mut decoder: Box<dyn Decoder>,
        config: &Config,
        unit_size: usize,
        samplerate: u64,
    ) -> Result<()> {
        self.unit_size = unit_size;
        decoder.start(config)?;
        if samplerate > 0 {
            decoder.metadata(samplerate);
        }
        let (stream, writer) = Stream::new(unit_size, config.channels.clone());
        let tx = self.tx.clone();
        let name = instance.clone();
        let handle = std::thread::Builder::new()
            .name(format!("decoder {instance}"))
            .spawn(move || {
                let mut ctx = ThreadContext {
                    stream,
                    tx: tx.clone(),
                    instance: name.clone(),
                    matched: Vec::new(),
                };
                let r = decoder.decode(&mut ctx);
                let error = match r {
                    Ok(()) | Err(Error::Eof) => None,
                    Err(e) => Some(e.to_string()),
                };
                let _ = tx.send(Output::End { instance: name, error });
            })
            .map_err(Error::Io)?;
        self.running.push(Running { instance, handle: Some(handle), writer });
        Ok(())
    }

    /// The instances' ids, in order.
    pub fn instances(&self) -> Vec<String> {
        self.running.iter().map(|r| r.instance.clone()).collect()
    }

    /// Feeds logic samples to every instance.
    pub fn feed(&mut self, data: &[u8]) {
        if !data.len().is_multiple_of(self.unit_size.max(1)) {
            log::warn!("logic data is not a whole number of samples");
        }
        for r in &self.running {
            r.writer.push(data);
        }
    }

    /// Ends the input; the decoders finish and send `Output::End`.
    pub fn end(&mut self) {
        for r in &self.running {
            r.writer.end();
        }
    }

    /// Takes the output receiver. Outputs arrive in emission order per
    /// instance.
    pub fn outputs(&mut self) -> Option<Receiver<Output>> {
        self.rx.take()
    }

    /// Waits for every instance to finish.
    pub fn join(&mut self) {
        for r in &mut self.running {
            if let Some(h) = r.handle.take() {
                let _ = h.join();
            }
        }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        self.end();
        self.join();
    }
}

struct ThreadContext {
    stream: Stream,
    tx: Sender<Output>,
    instance: String,
    matched: Vec<bool>,
}

impl Context for ThreadContext {
    fn wait(&mut self, conds: &[&[Cond]]) -> Result<Pins> {
        let (pins, matched) = self.stream.wait(conds)?;
        self.matched = matched;
        Ok(pins)
    }

    fn samplenum(&self) -> u64 {
        self.stream.samplenum()
    }

    fn matched(&self) -> &[bool] {
        &self.matched
    }

    fn has_channel(&self, channel: usize) -> bool {
        self.stream.has_channel(channel)
    }

    fn put_ann(&mut self, ss: u64, es: u64, class: usize, texts: Vec<String>) {
        let _ = self.tx.send(Output::Annotation {
            instance: self.instance.clone(),
            ss,
            es,
            class,
            texts,
        });
    }

    fn put_binary(&mut self, ss: u64, es: u64, class: usize, data: Vec<u8>) {
        let _ =
            self.tx.send(Output::Binary { instance: self.instance.clone(), ss, es, class, data });
    }

    fn put_python(&mut self, ss: u64, es: u64, item: Item) {
        let _ = self.tx.send(Output::Python { instance: self.instance.clone(), ss, es, item });
    }

    fn put_meta(&mut self, ss: u64, es: u64, item: Item) {
        let _ = self.tx.send(Output::Meta { instance: self.instance.clone(), ss, es, item });
    }
}
