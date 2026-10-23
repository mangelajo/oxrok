// Counterpart in sigrok:
//   libsigrok/src/output/srzip.c.
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like it.

//! `srzip`: a sigrok session file. Needs a file name.

use super::{Output, OutputModule};
use crate::core::{Error, Packet, Result};
use crate::formats::sr::SessionWriter;
use std::path::Path;

pub(super) static MODULE: OutputModule = OutputModule {
    id: "srzip",
    name: "srzip",
    description: "srzip session file format data",
    options: &[],
    extensions: &["sr"],
    analog: true,
    create: |ctx, _options| {
        let Some(filename) = &ctx.filename else {
            return Err(Error::arg("srzip output module requires a file name, cannot save."));
        };
        let writer = SessionWriter::create(Path::new(filename), ctx.channels, ctx.samplerate)?;
        Ok(Box::new(Srzip { writer: Some(writer) }))
    },
};

struct Srzip {
    writer: Option<SessionWriter<std::fs::File>>,
}

impl Output for Srzip {
    fn receive(&mut self, packet: &Packet, _out: &mut Vec<u8>) -> Result<()> {
        let Some(w) = &mut self.writer else { return Ok(()) };
        w.receive(packet)?;
        if matches!(packet, Packet::End)
            && let Some(w) = self.writer.take()
        {
            w.finish()?;
        }
        Ok(())
    }

    fn writes_file(&self) -> bool {
        true
    }
}
