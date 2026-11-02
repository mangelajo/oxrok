//! The transaction log: one line per completed transfer, in the
//! dialect of oxrok's USB fixtures (`docs/USB.md`), `# section NAME`
//! markers between the phases of a run, and `! violation: ...` lines
//! for what the host did outside the protocol. Transfers the device
//! holds with a NAK are not logged until they complete, so a log does
//! not depend on how often the host polls; unlinks and other events
//! are `#` comments, written only in verbose mode.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use crate::model::{BULK, CONTROL, INTERRUPT, Reply, Setup};

/// The log.
pub struct TxLog {
    out: Option<BufWriter<File>>,
    verbose: bool,
    violations: usize,
    section: String,
}

impl TxLog {
    /// A log to `path`, or to nowhere; events go in only when
    /// `verbose`. Violations are also printed to stderr.
    pub fn new(path: Option<&Path>, verbose: bool) -> io::Result<TxLog> {
        let out = match path {
            Some(p) => Some(BufWriter::new(File::create(p)?)),
            None => None,
        };
        Ok(TxLog { out, verbose, violations: 0, section: String::new() })
    }

    fn line(&mut self, s: &str) {
        if let Some(o) = &mut self.out {
            // Errors writing the log are not worth stopping the device.
            let _ = writeln!(o, "{s}");
            let _ = o.flush();
        }
    }

    /// Starts a section.
    pub fn section(&mut self, name: &str) {
        name.clone_into(&mut self.section);
        self.line(&format!("# section {name}"));
    }

    /// The current section's name.
    pub fn current_section(&self) -> &str {
        &self.section
    }

    /// One completed transfer.
    pub fn transfer(&mut self, line: &str) {
        self.line(line);
    }

    /// A violation, counted and also printed to stderr.
    pub fn violation(&mut self, message: &str) {
        self.violations += 1;
        eprintln!("violation [{}]: {message}", self.section);
        self.line(&format!("! violation: {message}"));
    }

    /// An event, in verbose mode only.
    pub fn event(&mut self, message: &str) {
        if self.verbose {
            eprintln!("emu: {message}");
            self.line(&format!("# {message}"));
        }
    }

    /// How many violations were logged.
    pub fn violations(&self) -> usize {
        self.violations
    }
}

/// Bytes as hex pairs, `-` when empty.
pub fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".to_owned();
    }
    bytes.iter().fold(String::with_capacity(2 * bytes.len()), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn result(reply: &Reply, is_in: bool) -> String {
    match reply {
        Reply::Data(d) if is_in => hex(d),
        Reply::Data(_) => "ok".to_owned(),
        Reply::Nak => "nak".to_owned(),
        Reply::Stall => "err stall".to_owned(),
    }
}

/// The line of a control transfer: the fixture format, the direction
/// bit of `bmRequestType` left to the `ctl_in`/`ctl_out` word.
pub fn control_line(setup: Setup, data: &[u8], reply: &Reply) -> String {
    let kind = setup.request_type & 0x7f;
    if setup.is_in() {
        format!(
            "ctl_in {kind:02x} {:02x} {:04x} {:04x} {} -> {}",
            setup.request,
            setup.value,
            setup.index,
            setup.length,
            result(reply, true)
        )
    } else {
        format!(
            "ctl_out {kind:02x} {:02x} {:04x} {:04x} {} -> {}",
            setup.request,
            setup.value,
            setup.index,
            hex(data),
            result(reply, false)
        )
    }
}

/// The line of a bulk, interrupt or isochronous transfer on endpoint
/// `address`; `max` is the length asked for an IN, `data` the payload
/// of an OUT.
pub fn transfer_line(
    transfer_type: u8,
    address: u8,
    max: usize,
    data: &[u8],
    reply: &Reply,
) -> String {
    let word = match transfer_type {
        BULK => "bulk",
        INTERRUPT => "int",
        CONTROL => "ctl",
        _ => "iso",
    };
    if address & 0x80 != 0 {
        format!("{word}_in {address:02x} {max} -> {}", result(reply, true))
    } else {
        let taken = match reply {
            Reply::Data(d) if d.is_empty() => data.len().to_string(),
            Reply::Data(d) => d.len().to_string(),
            other => result(other, false),
        };
        format!("{word}_out {address:02x} {} -> {taken}", hex(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_follow_the_fixture_dialect() {
        let s = Setup { request_type: 0xa1, request: 1, value: 0x0300, index: 0, length: 8 };
        assert_eq!(
            control_line(s, &[], &Reply::Data(vec![0x41, 0x42])),
            "ctl_in 21 01 0300 0000 8 -> 4142"
        );
        let s = Setup { request_type: 0x21, request: 9, value: 0x0300, index: 0, length: 2 };
        assert_eq!(
            control_line(s, &[0xff, 1], &Reply::ack()),
            "ctl_out 21 09 0300 0000 ff01 -> ok"
        );
        assert_eq!(
            control_line(s, &[0xff, 1], &Reply::Stall),
            "ctl_out 21 09 0300 0000 ff01 -> err stall"
        );
        assert_eq!(
            transfer_line(INTERRUPT, 0x81, 64, &[], &Reply::Data(vec![1])),
            "int_in 81 64 -> 01"
        );
        assert_eq!(transfer_line(BULK, 0x02, 0, &[1, 2], &Reply::ack()), "bulk_out 02 0102 -> 2");
    }
}
