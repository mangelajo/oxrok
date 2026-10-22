//! Recording a USB session to a fixture and replaying it.
//!
//! A fixture is a text file, one call per line, hand editable:
//!
//! ```text
//! # oxrok usb fixture 1
//! device vid=0403 pid=dc48 version=0400 bus=2 addr=7 product="LogicPort Logic Analyzer"
//! claim 0 -> ok
//! ctl_out 40 00 0000 0001 - -> ok
//! ctl_in 40 0c 0000 0001 1 -> 18
//! bulk_out 02 04 -> 1
//! bulk_in 81 64 -> 3160
//! bulk_in 81 64 -> err timeout
//! ```
//!
//! Control requests are `bmRequestType bRequest wValue wIndex` in hex,
//! then the data (`-` when empty) or the length. Data is hex pairs; a
//! run of the same byte may be written `00*4096`. Results are `ok`, a
//! number, hex data, or `err <kind> ["message"]` with kind `timeout`,
//! `busy`, `device`, `io` or `unsupported`.

use crate::{ControlRequest, Result, Transport, UsbDeviceInfo};
use oxrok_core::Error;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A call to the transport, without its result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    /// `claim_interface`.
    Claim(u8),
    /// `release_interface`.
    Release(u8),
    /// `reset`.
    Reset,
    /// `max_packet_size`.
    MaxPacketSize(u8),
    /// `control_out`.
    ControlOut(ControlRequest, Vec<u8>),
    /// `control_in` with the requested length.
    ControlIn(ControlRequest, usize),
    /// `bulk_out`.
    BulkOut(u8, Vec<u8>),
    /// `bulk_in` with the requested length.
    BulkIn(u8, usize),
}

/// What a call returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// `Ok(())`.
    Ok,
    /// A byte count.
    Count(usize),
    /// Data read.
    Data(Vec<u8>),
    /// An error, by kind name and message.
    Err(String, String),
}

impl Outcome {
    fn from_error(e: &Error) -> Self {
        let (kind, msg) = match e {
            Error::Timeout => ("timeout", String::new()),
            Error::Busy => ("busy", String::new()),
            Error::Io(e) => ("io", e.to_string()),
            Error::NotSupported(m) => ("unsupported", m.clone()),
            other => ("device", other.to_string()),
        };
        Outcome::Err(kind.to_owned(), msg)
    }

    fn to_error(&self) -> Error {
        match self {
            Outcome::Err(kind, msg) => match kind.as_str() {
                "timeout" => Error::Timeout,
                "busy" => Error::Busy,
                "io" => Error::Io(std::io::Error::other(msg.clone())),
                "unsupported" => Error::NotSupported(msg.clone()),
                _ => Error::Device(msg.clone()),
            },
            _ => Error::device("not an error"),
        }
    }
}

/// One recorded call and its result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// The call.
    pub call: Call,
    /// Its result.
    pub outcome: Outcome,
}

/// A recorded session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fixture {
    /// The device as enumeration described it.
    pub device: UsbDeviceInfo,
    /// The calls, in order.
    pub steps: Vec<Step>,
}

const MAGIC: &str = "# oxrok usb fixture 1";

impl Fixture {
    /// Formats the fixture as text.
    pub fn to_text(&self) -> String {
        let mut s = String::new();
        s.push_str(MAGIC);
        s.push('\n');
        let d = &self.device;
        let _ = write!(
            s,
            "device vid={:04x} pid={:04x} version={:04x} bus={} addr={}",
            d.vendor_id, d.product_id, d.device_version, d.bus, d.address
        );
        for (key, v) in
            [("manufacturer", &d.manufacturer), ("product", &d.product), ("serial", &d.serial)]
        {
            if let Some(v) = v {
                let _ = write!(s, " {key}={}", quote(v));
            }
        }
        s.push('\n');
        for step in &self.steps {
            match &step.call {
                Call::Claim(i) => {
                    let _ = write!(s, "claim {i}");
                }
                Call::Release(i) => {
                    let _ = write!(s, "release {i}");
                }
                Call::Reset => s.push_str("reset"),
                Call::MaxPacketSize(ep) => {
                    let _ = write!(s, "mps {ep:02x}");
                }
                Call::ControlOut(r, data) => {
                    let _ = write!(
                        s,
                        "ctl_out {:02x} {:02x} {:04x} {:04x} {}",
                        r.request_type,
                        r.request,
                        r.value,
                        r.index,
                        hex(data)
                    );
                }
                Call::ControlIn(r, len) => {
                    let _ = write!(
                        s,
                        "ctl_in {:02x} {:02x} {:04x} {:04x} {len}",
                        r.request_type, r.request, r.value, r.index
                    );
                }
                Call::BulkOut(ep, data) => {
                    let _ = write!(s, "bulk_out {ep:02x} {}", hex(data));
                }
                Call::BulkIn(ep, len) => {
                    let _ = write!(s, "bulk_in {ep:02x} {len}");
                }
            }
            s.push_str(" -> ");
            match &step.outcome {
                Outcome::Ok => s.push_str("ok"),
                Outcome::Count(n) => {
                    let _ = write!(s, "{n}");
                }
                Outcome::Data(d) => s.push_str(&hex(d)),
                Outcome::Err(kind, msg) => {
                    let _ = write!(s, "err {kind}");
                    if !msg.is_empty() {
                        let _ = write!(s, " {}", quote(msg));
                    }
                }
            }
            s.push('\n');
        }
        s
    }

    /// Parses fixture text.
    pub fn parse(text: &str) -> Result<Fixture> {
        let mut lines = text.lines().enumerate();
        let Some((_, first)) = lines.next() else {
            return Err(Error::arg("empty fixture"));
        };
        if first.trim() != MAGIC {
            return Err(Error::arg("not an oxrok usb fixture"));
        }
        let mut fixture = Fixture::default();
        let mut have_device = false;
        for (n, line) in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let bad = |what: &str| Error::Arg(format!("fixture line {}: {what}", n + 1));
            if let Some(rest) = line.strip_prefix("device ") {
                fixture.device = parse_device(rest).map_err(|e| bad(&e))?;
                have_device = true;
                continue;
            }
            let (call, outcome) = line.split_once(" -> ").ok_or_else(|| bad("missing ->"))?;
            let call = parse_call(call).map_err(|e| bad(&e))?;
            let outcome = parse_outcome(outcome, &call).map_err(|e| bad(&e))?;
            fixture.steps.push(Step { call, outcome });
        }
        if !have_device {
            return Err(Error::arg("fixture has no device line"));
        }
        Ok(fixture)
    }
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Reads a quoted string at the start of `s`; returns it and the rest.
fn unquote(s: &str) -> std::result::Result<(String, &str), String> {
    let rest = s.strip_prefix('"').ok_or("expected a quoted string")?;
    let mut out = String::new();
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                let (_, e) = chars.next().ok_or("unterminated escape")?;
                out.push(e);
            }
            '"' => return Ok((out, &rest[i + 1..])),
            c => out.push(c),
        }
    }
    Err("unterminated string".into())
}

fn parse_device(s: &str) -> std::result::Result<UsbDeviceInfo, String> {
    let mut d = UsbDeviceInfo::default();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let (key, after) = rest.split_once('=').ok_or("expected key=value")?;
        let (value, after) = if after.starts_with('"') {
            unquote(after)?
        } else {
            let end = after.find(' ').unwrap_or(after.len());
            (after[..end].to_owned(), &after[end..])
        };
        rest = after.trim_start();
        let hex16 = |v: &str| u16::from_str_radix(v, 16).map_err(|_| format!("bad {key}"));
        match key {
            "vid" => d.vendor_id = hex16(&value)?,
            "pid" => d.product_id = hex16(&value)?,
            "version" => d.device_version = hex16(&value)?,
            "bus" => d.bus = value.parse().map_err(|_| "bad bus")?,
            "addr" => d.address = value.parse().map_err(|_| "bad addr")?,
            "manufacturer" => d.manufacturer = Some(value),
            "product" => d.product = Some(value),
            "serial" => d.serial = Some(value),
            other => return Err(format!("unknown device field {other}")),
        }
    }
    Ok(d)
}

/// Hex pairs with `xx*N` runs.
pub fn hex(data: &[u8]) -> String {
    if data.is_empty() {
        return "-".into();
    }
    let mut s = String::new();
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        let mut run = 1;
        while i + run < data.len() && data[i + run] == b {
            run += 1;
        }
        if run >= 4 {
            let _ = write!(s, "{b:02x}*{run}");
            i += run;
        } else {
            let _ = write!(s, "{b:02x}");
            i += 1;
        }
    }
    s
}

/// The inverse of [`hex`].
pub fn unhex(s: &str) -> std::result::Result<Vec<u8>, String> {
    let mut out = Vec::new();
    if s == "-" {
        return Ok(out);
    }
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 2 > bytes.len() {
            return Err("odd hex length".into());
        }
        let b = u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "bad hex")?;
        i += 2;
        if i < bytes.len() && bytes[i] == b'*' {
            let end =
                s[i + 1..].find(|c: char| !c.is_ascii_digit()).map_or(bytes.len(), |e| i + 1 + e);
            let n: usize = s[i + 1..end].parse().map_err(|_| "bad run length")?;
            out.extend(std::iter::repeat_n(b, n));
            i = end;
        } else {
            out.push(b);
        }
    }
    Ok(out)
}

fn parse_call(s: &str) -> std::result::Result<Call, String> {
    let words: Vec<&str> = s.split_whitespace().collect();
    let kind = *words.first().ok_or("empty call")?;
    let field = |i: usize| words.get(i).copied().ok_or_else(|| format!("{kind}: missing field"));
    let h8 = |i: usize| {
        let w = field(i)?;
        u8::from_str_radix(w, 16).map_err(|_| format!("bad byte {w}"))
    };
    let h16 = |i: usize| {
        let w = field(i)?;
        u16::from_str_radix(w, 16).map_err(|_| format!("bad word {w}"))
    };
    let num = |i: usize| {
        let w = field(i)?;
        w.parse::<usize>().map_err(|_| format!("bad number {w}"))
    };
    let request = || {
        Ok::<_, String>(ControlRequest {
            // The direction bit is implied by the call.
            request_type: h8(1)? & 0x7f,
            request: h8(2)?,
            value: h16(3)?,
            index: h16(4)?,
        })
    };
    Ok(match kind {
        "claim" => Call::Claim(num(1)? as u8),
        "release" => Call::Release(num(1)? as u8),
        "reset" => Call::Reset,
        "mps" => Call::MaxPacketSize(h8(1)?),
        "ctl_out" => Call::ControlOut(request()?, unhex(field(5)?)?),
        "ctl_in" => Call::ControlIn(request()?, num(5)?),
        "bulk_out" => Call::BulkOut(h8(1)?, unhex(field(2)?)?),
        "bulk_in" => Call::BulkIn(h8(1)?, num(2)?),
        other => return Err(format!("unknown call {other}")),
    })
}

fn parse_outcome(s: &str, call: &Call) -> std::result::Result<Outcome, String> {
    let s = s.trim();
    if s == "ok" {
        return Ok(Outcome::Ok);
    }
    if let Some(rest) = s.strip_prefix("err ") {
        let (kind, msg) = rest.split_once(' ').unwrap_or((rest, ""));
        let msg = if msg.is_empty() { String::new() } else { unquote(msg.trim())?.0 };
        return Ok(Outcome::Err(kind.to_owned(), msg));
    }
    Ok(match call {
        Call::ControlIn(..) | Call::BulkIn(..) => Outcome::Data(unhex(s)?),
        _ => Outcome::Count(s.parse().map_err(|_| format!("bad count {s}"))?),
    })
}

/// A transport that logs every call to a [`Fixture`].
pub struct Recorder {
    inner: Box<dyn Transport>,
    fixture: Arc<Mutex<Fixture>>,
}

/// A view of a [`Recorder`]'s fixture that outlives the recorder being
/// moved into a driver.
#[derive(Clone)]
pub struct RecordingHandle(Arc<Mutex<Fixture>>);

impl RecordingHandle {
    /// A copy of the recording so far.
    pub fn fixture(&self) -> Fixture {
        self.0.lock().expect("recording lock").clone()
    }
}

impl Recorder {
    /// Wraps `inner`.
    pub fn new(inner: Box<dyn Transport>) -> Self {
        let device = inner.info().clone();
        Recorder { inner, fixture: Arc::new(Mutex::new(Fixture { device, steps: Vec::new() })) }
    }

    /// Wraps `inner` and returns a handle to read the recording later.
    pub fn with_handle(inner: Box<dyn Transport>) -> (Self, RecordingHandle) {
        let r = Self::new(inner);
        let h = RecordingHandle(Arc::clone(&r.fixture));
        (r, h)
    }

    /// A copy of the recording so far.
    pub fn fixture(&self) -> Fixture {
        self.fixture.lock().expect("recording lock").clone()
    }

    /// Stops recording and returns the fixture and the transport.
    pub fn finish(self) -> (Fixture, Box<dyn Transport>) {
        let fixture = self.fixture();
        (fixture, self.inner)
    }

    fn log<T>(&mut self, call: Call, r: Result<T>, ok: impl FnOnce(&T) -> Outcome) -> Result<T> {
        let outcome = match &r {
            Ok(v) => ok(v),
            Err(e) => Outcome::from_error(e),
        };
        self.fixture.lock().expect("recording lock").steps.push(Step { call, outcome });
        r
    }
}

impl Transport for Recorder {
    fn info(&self) -> &UsbDeviceInfo {
        self.inner.info()
    }

    fn claim_interface(&mut self, interface: u8) -> Result<()> {
        let r = self.inner.claim_interface(interface);
        self.log(Call::Claim(interface), r, |()| Outcome::Ok)
    }

    fn release_interface(&mut self, interface: u8) -> Result<()> {
        let r = self.inner.release_interface(interface);
        self.log(Call::Release(interface), r, |()| Outcome::Ok)
    }

    fn reset(&mut self) -> Result<()> {
        let r = self.inner.reset();
        self.log(Call::Reset, r, |()| Outcome::Ok)
    }

    fn max_packet_size(&mut self, endpoint: u8) -> Result<usize> {
        let r = self.inner.max_packet_size(endpoint);
        self.log(Call::MaxPacketSize(endpoint), r, |n| Outcome::Count(*n))
    }

    fn control_out(&mut self, req: ControlRequest, data: &[u8], timeout: Duration) -> Result<()> {
        let r = self.inner.control_out(req, data, timeout);
        self.log(Call::ControlOut(req, data.to_vec()), r, |()| Outcome::Ok)
    }

    fn control_in(
        &mut self,
        req: ControlRequest,
        len: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        let r = self.inner.control_in(req, len, timeout);
        self.log(Call::ControlIn(req, len), r, |d| Outcome::Data(d.clone()))
    }

    fn bulk_out(&mut self, endpoint: u8, data: &[u8], timeout: Duration) -> Result<usize> {
        let r = self.inner.bulk_out(endpoint, data, timeout);
        self.log(Call::BulkOut(endpoint, data.to_vec()), r, |n| Outcome::Count(*n))
    }

    fn bulk_in(&mut self, endpoint: u8, len: usize, timeout: Duration) -> Result<Vec<u8>> {
        let r = self.inner.bulk_in(endpoint, len, timeout);
        self.log(Call::BulkIn(endpoint, len), r, |d| Outcome::Data(d.clone()))
    }
}

/// A transport that answers from a [`Fixture`] and fails on any call
/// that differs from the recording.
pub struct Replay {
    fixture: Fixture,
    pos: usize,
}

impl Replay {
    /// Replays `fixture` from the start.
    pub fn new(fixture: Fixture) -> Self {
        Replay { fixture, pos: 0 }
    }

    /// Replays fixture text.
    pub fn from_text(text: &str) -> Result<Self> {
        Ok(Self::new(Fixture::parse(text)?))
    }

    /// The number of recorded steps not yet replayed.
    pub fn remaining(&self) -> usize {
        self.fixture.steps.len() - self.pos
    }

    /// Fails when steps remain, for the end of a test.
    pub fn finish(&self) -> Result<()> {
        match self.remaining() {
            0 => Ok(()),
            n => Err(Error::Device(format!(
                "replay: {n} recorded steps were not replayed, next is {:?}",
                self.fixture.steps[self.pos].call
            ))),
        }
    }

    fn step(&mut self, call: &Call) -> Result<Outcome> {
        let Some(step) = self.fixture.steps.get(self.pos) else {
            return Err(Error::Device(format!(
                "replay: past the end of the recording at {call:?}"
            )));
        };
        if step.call != *call {
            return Err(Error::Device(format!(
                "replay: step {} expected {:?}, got {call:?}",
                self.pos + 1,
                step.call
            )));
        }
        self.pos += 1;
        Ok(step.outcome.clone())
    }

    fn unit(&mut self, call: &Call) -> Result<()> {
        match self.step(call)? {
            Outcome::Ok => Ok(()),
            e @ Outcome::Err(..) => Err(e.to_error()),
            other => Err(Error::Device(format!("replay: {call:?} recorded {other:?}"))),
        }
    }

    fn count(&mut self, call: &Call) -> Result<usize> {
        match self.step(call)? {
            Outcome::Count(n) => Ok(n),
            e @ Outcome::Err(..) => Err(e.to_error()),
            other => Err(Error::Device(format!("replay: {call:?} recorded {other:?}"))),
        }
    }

    fn data(&mut self, call: &Call) -> Result<Vec<u8>> {
        match self.step(call)? {
            Outcome::Data(d) => Ok(d),
            e @ Outcome::Err(..) => Err(e.to_error()),
            other => Err(Error::Device(format!("replay: {call:?} recorded {other:?}"))),
        }
    }
}

impl Transport for Replay {
    fn info(&self) -> &UsbDeviceInfo {
        &self.fixture.device
    }

    fn claim_interface(&mut self, interface: u8) -> Result<()> {
        self.unit(&Call::Claim(interface))
    }

    fn release_interface(&mut self, interface: u8) -> Result<()> {
        self.unit(&Call::Release(interface))
    }

    fn reset(&mut self) -> Result<()> {
        self.unit(&Call::Reset)
    }

    fn max_packet_size(&mut self, endpoint: u8) -> Result<usize> {
        self.count(&Call::MaxPacketSize(endpoint))
    }

    fn control_out(&mut self, req: ControlRequest, data: &[u8], _timeout: Duration) -> Result<()> {
        self.unit(&Call::ControlOut(req, data.to_vec()))
    }

    fn control_in(
        &mut self,
        req: ControlRequest,
        len: usize,
        _timeout: Duration,
    ) -> Result<Vec<u8>> {
        self.data(&Call::ControlIn(req, len))
    }

    fn bulk_out(&mut self, endpoint: u8, data: &[u8], _timeout: Duration) -> Result<usize> {
        self.count(&Call::BulkOut(endpoint, data.to_vec()))
    }

    fn bulk_in(&mut self, endpoint: u8, len: usize, _timeout: Duration) -> Result<Vec<u8>> {
        self.data(&Call::BulkIn(endpoint, len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# oxrok usb fixture 1\n\
        device vid=0403 pid=dc48 version=0400 bus=2 addr=7 manufacturer=\"Intronix\" product=\"LogicPort \\\"LA\\\"\" serial=\"80806096\"\n\
        claim 0 -> ok\n\
        mps 81 -> 64\n\
        ctl_out 40 00 0000 0001 - -> ok\n\
        ctl_in 40 0c 0000 0001 1 -> 18\n\
        bulk_out 02 00*5aa -> 6\n\
        bulk_in 81 64 -> 3160\n\
        bulk_in 81 64 -> err timeout\n\
        reset -> err device \"device disconnected\"\n";

    #[test]
    fn hex_runs() {
        assert_eq!(hex(&[]), "-");
        assert_eq!(hex(&[0, 0, 0, 0, 0xaa]), "00*4aa");
        assert_eq!(hex(&[0, 0, 0, 0xaa]), "000000aa");
        assert_eq!(unhex("00*4aa").unwrap(), [0, 0, 0, 0, 0xaa]);
        assert_eq!(unhex("-").unwrap(), Vec::<u8>::new());
        assert!(unhex("0").is_err());
        assert!(unhex("zz").is_err());
    }

    #[test]
    fn parse_and_format_round_trip() {
        let f = Fixture::parse(TEXT).unwrap();
        assert_eq!(f.device.product.as_deref(), Some("LogicPort \"LA\""));
        assert_eq!(f.device.conn(), "2.7");
        assert_eq!(f.steps.len(), 8);
        assert_eq!(f.steps[4].call, Call::BulkOut(2, vec![0, 0, 0, 0, 0, 0xaa]));
        assert_eq!(f.to_text(), TEXT);
        assert_eq!(Fixture::parse(&f.to_text()).unwrap(), f);
    }

    #[test]
    fn replay_follows_the_recording() {
        let mut r = Replay::from_text(TEXT).unwrap();
        let t = Duration::from_millis(10);
        r.claim_interface(0).unwrap();
        assert_eq!(r.max_packet_size(0x81).unwrap(), 64);
        r.control_out(ControlRequest::vendor(0, 0, 1), &[], t).unwrap();
        assert_eq!(r.control_in(ControlRequest::vendor(0x0c, 0, 1), 1, t).unwrap(), [0x18]);
        assert_eq!(r.bulk_out(2, &[0, 0, 0, 0, 0, 0xaa], t).unwrap(), 6);
        assert_eq!(r.bulk_in(0x81, 64, t).unwrap(), [0x31, 0x60]);
        assert!(matches!(r.bulk_in(0x81, 64, t), Err(Error::Timeout)));
        assert_eq!(r.remaining(), 1);
        assert!(r.finish().is_err());
        let e = r.reset().unwrap_err().to_string();
        assert_eq!(e, "device error: device disconnected");
        r.finish().unwrap();
        assert!(r.reset().unwrap_err().to_string().contains("past the end"));
    }

    #[test]
    fn replay_rejects_a_different_call() {
        let mut r = Replay::from_text(TEXT).unwrap();
        let e = r.claim_interface(1).unwrap_err().to_string();
        assert!(e.contains("step 1 expected Claim(0), got Claim(1)"), "{e}");
    }

    #[test]
    fn recorder_logs_what_replay_answers() {
        let inner = Replay::from_text(TEXT).unwrap();
        let mut rec = Recorder::new(Box::new(inner));
        let t = Duration::from_millis(10);
        rec.claim_interface(0).unwrap();
        assert_eq!(rec.max_packet_size(0x81).unwrap(), 64);
        rec.control_out(ControlRequest::vendor(0, 0, 1), &[], t).unwrap();
        let (f, _) = rec.finish();
        assert_eq!(f.steps.len(), 3);
        assert_eq!(f.steps[1].outcome, Outcome::Count(64));
        assert!(f.to_text().starts_with(MAGIC));
    }

    #[test]
    fn parse_errors_name_the_line() {
        assert!(Fixture::parse("").is_err());
        assert!(Fixture::parse("# other\n").is_err());
        let e = Fixture::parse("# oxrok usb fixture 1\ndevice vid=0403 pid=1\nclaim x -> ok\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains("fixture line 3"), "{e}");
        assert!(Fixture::parse("# oxrok usb fixture 1\nclaim 0 -> ok\n").is_err());
    }
}
