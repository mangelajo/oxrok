//! The output side shared by captures and file inputs: builds the output
//! module, feeds it packets, writes the text.

use crate::args::Args;
use crate::parse;
use oxrok_core::{Channel, Error, Packet, Result};
use oxrok_formats::output::{self, Options, Output, OutputContext};
use std::io::Write;

const DEFAULT_OUTPUT_NOFILE: &str = "bits:width=64";
const DEFAULT_OUTPUT_FILE: &str = "srzip";

/// A running output pipeline.
pub struct Pipeline {
    main: Box<dyn Output>,
    analog: Option<Box<dyn Output>>,
    sink: Option<Box<dyn Write>>,
    buf: Vec<u8>,
    wait_trigger: bool,
    triggered: bool,
    /// Logic and analog samples received.
    pub received_logic: u64,
    pub received_analog: u64,
}

impl Pipeline {
    /// Builds the output named by `-O` (or the default) for `channels`.
    pub fn new(
        args: &Args,
        channels: &[Channel],
        samplerate: Option<u64>,
        title: &str,
    ) -> Result<Self> {
        let spec = args.output_format.clone().unwrap_or_else(|| {
            if args.output_file.is_some() { DEFAULT_OUTPUT_FILE } else { DEFAULT_OUTPUT_NOFILE }
                .to_owned()
        });
        let parsed = parse::generic_args(&spec, true);
        let id = parsed.scope.ok_or_else(|| Error::arg("Invalid output format."))?;
        let module = output::find(&id)
            .ok_or_else(|| Error::Arg(format!("Unknown output module '{id}'.")))?;
        let options: Options =
            parsed.items.into_iter().map(|(k, v)| (k, v.unwrap_or_default())).collect();
        let ctx = OutputContext {
            channels,
            samplerate,
            title: title.to_owned(),
            filename: args.output_file.as_ref().map(|p| p.display().to_string()),
        };
        let main = module.instantiate(&ctx, &options)?;
        let analog = if module.analog {
            None
        } else {
            let mut opts = Options::new();
            opts.insert("meta".to_owned(), "false".to_owned());
            output::find("analog").map(|m| m.instantiate(&ctx, &opts)).transpose()?
        };
        let sink: Option<Box<dyn Write>> = if main.writes_file() {
            None
        } else {
            Some(match &args.output_file {
                Some(path) => {
                    Box::new(std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| {
                        Error::Arg(format!("Cannot write to output file '{}': {e}", path.display()))
                    })?))
                }
                None => Box::new(std::io::stdout()),
            })
        };
        Ok(Pipeline {
            main,
            analog,
            sink,
            buf: Vec::new(),
            wait_trigger: args.wait_trigger,
            triggered: false,
            received_logic: 0,
            received_analog: 0,
        })
    }

    /// Handles one packet. False when the output can no longer be written.
    pub fn handle(&mut self, packet: &Packet) -> bool {
        match packet {
            Packet::Trigger => self.triggered = true,
            Packet::Logic(l) => self.received_logic += l.len() as u64,
            Packet::Analog(a) => self.received_analog += a.len() as u64,
            _ => {}
        }
        if self.wait_trigger && !self.triggered && matches!(packet, Packet::Logic(_)) {
            return true;
        }
        self.buf.clear();
        match self.main.receive(packet, &mut self.buf) {
            Ok(()) => {}
            Err(e) => {
                log::error!("output: {e}");
                return false;
            }
        }
        if self.buf.is_empty()
            && let Some(a) = &mut self.analog
        {
            let _ = a.receive(packet, &mut self.buf);
        }
        if !self.buf.is_empty()
            && let Some(sink) = &mut self.sink
            && (sink.write_all(&self.buf).is_err() || sink.flush().is_err())
        {
            return false;
        }
        true
    }

    /// Warns when fewer samples than asked arrived.
    pub fn check_limit(&self, limit: u64) {
        if limit == 0 {
            return;
        }
        if self.received_logic > 0 && self.received_logic < limit {
            log::warn!("Device only sent {} samples.", self.received_logic);
        } else if self.received_analog > 0 && self.received_analog < limit {
            log::warn!("Device only sent {} samples.", self.received_analog);
        }
    }
}
