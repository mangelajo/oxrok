// Counterpart in sigrok:
//   sigrok-cli/decode.c,
//   Copyright (C) 2013 Bert Vermeulen <bert@biot.com>,
//   with commits by Gerhard Sittig, Uwe Hermann and others (see the
//   sigrok-cli history).
// This file is an independent implementation written for oxrok from the
//   observed behaviour and the public interfaces.

//! Protocol decoding in the command line: `-P`, `-A`, `-B` and `-M`.

use crate::args::Args;
use crate::parse;
use oxrok_core::{Channel, Error, Packet, Result};
use oxrok_decode::{Config, Decoder, DecoderInfo, Output, Registry, Stack};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write;
use std::sync::mpsc::Receiver;

/// Every decoder this binary ships.
pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(&oxrok_decoders::ALL);
    r
}

/// Which annotation classes of a decoder are shown: `None` for all.
type Visible = HashMap<&'static str, Option<Vec<usize>>>;
/// An instance waiting for the first logic packet: id, configuration
/// and constructor.
type Pending = (String, Config, fn() -> Box<dyn Decoder>);

struct Instance {
    id: String,
    info: &'static DecoderInfo,
}

/// The decoders of one run and what to print.
#[allow(clippy::struct_excessive_bools)]
pub struct Decoding {
    instances: Vec<Instance>,
    stack: Option<Stack>,
    outputs: Option<Receiver<Output>>,
    ann_visible: Visible,
    binary: Option<(&'static str, usize)>,
    meta_visible: Vec<&'static str>,
    show_snum: bool,
    show_quotes: bool,
    show_class: bool,
    show_abbrev: bool,
    samplerate: u64,
    pending: Vec<Pending>,
    ended: usize,
}

impl Decoding {
    /// Parses the decoder flags; `None` without `-P`.
    #[allow(clippy::too_many_lines)]
    pub fn from_args(
        args: &Args,
        channels: &[Channel],
        samplerate: Option<u64>,
    ) -> Result<Option<Self>> {
        if args.decoders.is_empty() {
            return Ok(None);
        }
        let registry = registry();
        let names: Vec<String> = channels.iter().map(|c| c.name.clone()).collect();
        let mut instances = Vec::new();
        let mut pending = Vec::new();
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for spec in &args.decoders {
            let mut parts = spec.split(',');
            let first = parts.next().unwrap_or_default();
            if parts.next().is_some() {
                return Err(Error::not_supported("stacked decoders are not implemented yet"));
            }
            let parsed = parse::generic_args(first, true);
            let id = parsed.scope.ok_or_else(|| Error::arg("Invalid decoder specification."))?;
            let def = registry
                .find(&id)
                .ok_or_else(|| Error::Arg(format!("Protocol decoder '{id}' not found.")))?;
            let pairs: Vec<(String, String)> =
                parsed.items.into_iter().map(|(k, v)| (k, v.unwrap_or_default())).collect();
            let config = oxrok_decode::parse_spec(def.info, &pairs, &names)?;
            let n = counts.entry(def.info.id).or_insert(0);
            *n += 1;
            let inst_id = format!("{}-{n}", def.info.id);
            instances.push(Instance { id: inst_id.clone(), info: def.info });
            pending.push((inst_id, config, def.new));
        }
        let mut ann_visible: Visible = HashMap::new();
        if args.decoder_annotations.is_empty()
            && args.decoder_binary.is_none()
            && args.decoder_meta.is_empty()
        {
            for i in &instances {
                ann_visible.insert(i.info.id, None);
            }
        }
        for spec in &args.decoder_annotations {
            for item in spec.split(',') {
                let (dec, classes) =
                    item.split_once('=').map_or((item, None), |(d, c)| (d, Some(c)));
                let info =
                    instances.iter().find(|i| i.info.id == dec).map(|i| i.info).ok_or_else(
                        || Error::Arg(format!("Protocol decoder '{dec}' not found.")),
                    )?;
                let Some(classes) = classes else {
                    ann_visible.insert(info.id, None);
                    continue;
                };
                let mut list = Vec::new();
                for c in classes.split(':') {
                    let found = info.ann_classes(c);
                    if found.is_empty() {
                        return Err(Error::Arg(format!(
                            "Annotation class '{c}' not found for decoder '{dec}'."
                        )));
                    }
                    list.extend(found);
                }
                ann_visible
                    .entry(info.id)
                    .and_modify(|v| {
                        if let Some(v) = v {
                            v.extend(list.iter().copied());
                        }
                    })
                    .or_insert(Some(list));
            }
        }
        let binary = match &args.decoder_binary {
            None => None,
            Some(spec) => {
                let (dec, class) = spec
                    .split_once('=')
                    .ok_or_else(|| Error::arg("Binary output needs decoder=class."))?;
                let info =
                    instances.iter().find(|i| i.info.id == dec).map(|i| i.info).ok_or_else(
                        || Error::Arg(format!("Protocol decoder '{dec}' not found.")),
                    )?;
                let idx = info.binary.iter().position(|b| b.id == class).ok_or_else(|| {
                    Error::Arg(format!("Binary class '{class}' not found for decoder '{dec}'."))
                })?;
                Some((info.id, idx))
            }
        };
        let mut meta_visible = Vec::new();
        for spec in &args.decoder_meta {
            for dec in spec.split(',') {
                let info =
                    instances.iter().find(|i| i.info.id == dec).map(|i| i.info).ok_or_else(
                        || Error::Arg(format!("Protocol decoder '{dec}' not found.")),
                    )?;
                meta_visible.push(info.id);
            }
        }
        let verbose = args.loglevel > 2;
        Ok(Some(Decoding {
            instances,
            stack: None,
            outputs: None,
            ann_visible,
            binary,
            meta_visible,
            show_snum: args.decoder_samplenum || verbose,
            show_quotes: verbose,
            show_class: args.loglevel > 3 || args.decoder_ann_class,
            show_abbrev: args.loglevel > 3,
            samplerate: samplerate.unwrap_or(0),
            pending,
            ended: 0,
        }))
    }

    fn start(&mut self, unit_size: usize) -> Result<()> {
        let mut stack = Stack::new();
        for (id, config, new) in self.pending.drain(..) {
            stack.add(id, new(), &config, unit_size, self.samplerate)?;
        }
        self.outputs = stack.outputs();
        self.stack = Some(stack);
        Ok(())
    }

    /// Feeds a packet; prints what the decoders produced so far.
    pub fn handle(&mut self, packet: &Packet, out: &mut dyn Write) -> Result<()> {
        match packet {
            Packet::Meta(items) => {
                // File inputs report the sample rate after the header.
                if self.stack.is_none()
                    && let Some((_, v)) =
                        items.iter().find(|(k, _)| *k == oxrok_core::Key::Samplerate)
                    && let Some(rate) = v.as_u64()
                {
                    self.samplerate = rate;
                }
                Ok(())
            }
            Packet::Logic(l) => {
                if self.stack.is_none() {
                    self.start(l.unit_size)?;
                }
                if let Some(s) = &mut self.stack {
                    s.feed(&l.data);
                }
                self.drain(out, false)
            }
            Packet::End => {
                if self.stack.is_none() {
                    self.start(1)?;
                }
                if let Some(s) = &mut self.stack {
                    s.end();
                }
                self.drain(out, true)?;
                if let Some(s) = &mut self.stack {
                    s.join();
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Prints available output; with `all`, waits until every instance
    /// has ended.
    fn drain(&mut self, out: &mut dyn Write, all: bool) -> Result<()> {
        let Some(rx) = &self.outputs else { return Ok(()) };
        loop {
            let next = if all { rx.recv().ok() } else { rx.try_recv().ok() };
            let Some(o) = next else { return Ok(()) };
            self.print(&o, out)?;
            if matches!(o, Output::End { .. }) {
                self.ended += 1;
                if all && self.ended >= self.instances.len() {
                    return Ok(());
                }
            }
        }
    }

    fn info_of(&self, instance: &str) -> Option<&'static DecoderInfo> {
        self.instances.iter().find(|i| i.id == instance).map(|i| i.info)
    }

    fn print(&self, o: &Output, out: &mut dyn Write) -> Result<()> {
        match o {
            Output::Annotation { instance, ss, es, class, texts } => {
                let Some(info) = self.info_of(instance) else { return Ok(()) };
                let Some(visible) = self.ann_visible.get(info.id) else { return Ok(()) };
                if let Some(list) = visible
                    && !list.contains(class)
                {
                    return Ok(());
                }
                let mut line = String::new();
                if self.show_snum {
                    let _ = write!(line, "{ss}-{es} ");
                }
                let _ = write!(line, "{instance}: ");
                if self.show_class {
                    let desc = info.annotations.get(*class).map_or("?", |a| a.id);
                    let _ = write!(line, "{desc}: ");
                }
                let q = if self.show_quotes { "\"" } else { "" };
                let shown: &[String] =
                    if self.show_abbrev { texts } else { &texts[..texts.len().min(1)] };
                for (i, t) in shown.iter().enumerate() {
                    if i > 0 {
                        line.push(' ');
                    }
                    let _ = write!(line, "{q}{t}{q}");
                }
                line.push('\n');
                out.write_all(line.as_bytes())?;
                out.flush()?;
            }
            Output::Binary { instance, class, data, .. } => {
                if let Some((dec, idx)) = self.binary
                    && self.info_of(instance).is_some_and(|i| i.id == dec)
                    && *class == idx
                {
                    out.write_all(data)?;
                    out.flush()?;
                }
            }
            Output::Meta { instance, ss, es, name, item } => {
                if self.info_of(instance).is_some_and(|i| self.meta_visible.contains(&i.id)) {
                    let mut line = String::new();
                    if self.show_snum {
                        let _ = write!(line, "{ss}-{es} ");
                    }
                    let _ = writeln!(line, "{instance}: {name}: {item}");
                    out.write_all(line.as_bytes())?;
                }
            }
            Output::Python { .. } => {}
            Output::End { instance, error } => {
                if let Some(e) = error {
                    log::error!("{instance}: {e}");
                }
            }
        }
        Ok(())
    }
}

/// `-P decoder --show`.
pub fn decoder_detail(args: &Args) -> Result<()> {
    let spec = args.decoders.first().map(String::as_str).unwrap_or_default();
    let id = parse::generic_args(spec.split(',').next().unwrap_or_default(), true)
        .scope
        .unwrap_or_default();
    let registry = registry();
    let def = registry
        .find(&id)
        .ok_or_else(|| Error::Arg(format!("Protocol decoder '{id}' not found.")))?;
    let d = def.info;
    println!(
        "ID: {}\nName: {}\nLong name: {}\nDescription: {}\nLicense: {}",
        d.id, d.name, d.longname, d.desc, d.license
    );
    println!("Possible decoder input IDs:");
    for i in d.inputs {
        println!("- {i}");
    }
    println!("Possible decoder output IDs:");
    for o in d.outputs {
        println!("- {o}");
    }
    println!("Decoder tags:");
    for t in d.tags {
        println!("- {t}");
    }
    println!("Annotation classes:");
    for a in d.annotations {
        println!("- {}: {}", a.id, a.desc);
    }
    println!("Annotation rows:");
    for r in d.annotation_rows {
        let classes: Vec<&str> =
            r.classes.iter().filter_map(|&c| d.annotations.get(c).map(|a| a.id)).collect();
        println!("- {} ({}): {}", r.id, r.desc, classes.join(", "));
    }
    println!("Binary classes:");
    for b in d.binary {
        println!("- {}: {}", b.id, b.desc);
    }
    println!("Required channels:");
    if d.channels.is_empty() {
        println!("None.");
    }
    for c in d.channels {
        println!("- {} ({}): {}", c.id, c.name, c.desc);
    }
    println!("Optional channels:");
    if d.optional_channels.is_empty() {
        println!("None.");
    }
    for c in d.optional_channels {
        println!("- {} ({}): {}", c.id, c.name, c.desc);
    }
    println!("Options:");
    for o in d.options {
        if o.values.is_empty() {
            println!("- {}: {} (default {})", o.id, o.desc, o.default.show());
        } else {
            let values: Vec<String> =
                o.values.iter().map(oxrok_decode::OptionValue::show).collect();
            println!(
                "- {}: {} ({}, default {})",
                o.id,
                o.desc,
                values.join(", "),
                o.default.show()
            );
        }
    }
    println!("Documentation:\n{}", d.doc);
    Ok(())
}
