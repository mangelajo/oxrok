//! The sigrok session file format (`.sr`).
//!
//! A session file is a zip archive with:
//!
//! - `version`: the text `2` (or `1` for old files);
//! - `metadata`: an INI file with a `[global]` section and a `[device 1]`
//!   section that names the capture file, the channel count and names,
//!   the sample rate and the logic unit size;
//! - `logic-1-1`, `logic-1-2`, ...: raw logic samples, `unitsize` bytes
//!   each, in order (old files have a single `logic-1`);
//! - `analog-1-<n>-1`, `analog-1-<n>-2`, ...: raw little endian f32
//!   samples of the analog channel whose 1-based position is `n`.
//!
//! [`SessionWriter`] writes one progressively; [`SessionReader`] reads one
//! back as packets. Both match what libsigrok writes and reads, so
//! `PulseView` and sigrok-cli open our files and we open theirs.

use oxrok_core::util::{parse_sizestring, samplerate_string};
use oxrok_core::{AnalogData, Channel, ChannelKind, Error, Key, Mq, MqFlags, Packet, Result, Unit};
use std::fmt::Write as _;
use std::io::{Read, Seek, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// Samples per logic chunk in a written file (bytes, like libsigrok).
const CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// Analog values the reader sends per packet.
const READ_CHUNK_FLOATS: usize = CHUNK_BYTES / 4;
/// Analog data from a session file carries no precision; libsigrok
/// reports two digits.
const ANALOG_DIGITS: i32 = 2;

/// The metadata of a session file, as read or as about to be written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    /// The writer's version string.
    pub writer: String,
    /// The capture file name, `logic-1`, when the file has logic data.
    pub capturefile: Option<String>,
    /// The number of logic channels (`total probes`).
    pub total_probes: usize,
    /// The number of analog channels (`total analog`).
    pub total_analog: usize,
    /// The sample rate in Hz, if known.
    pub samplerate: Option<u64>,
    /// Bytes per logic sample, when the file has logic data.
    pub unitsize: Option<usize>,
    /// Named logic channels: (1-based position, name).
    pub probes: Vec<(usize, String)>,
    /// Named analog channels: (1-based channel index, name).
    pub analogs: Vec<(usize, String)>,
}

impl Metadata {
    /// Parses the INI text of a `metadata` entry.
    pub fn parse(text: &str) -> Result<Metadata> {
        let mut m = Metadata::default();
        let mut section = String::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                name.trim().clone_into(&mut section);
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            if section == "global" {
                if k == "sigrok version" {
                    v.clone_into(&mut m.writer);
                }
                continue;
            }
            if !section.starts_with("device") {
                continue;
            }
            let bad = || Error::Arg(format!("bad session metadata: {k}={v}"));
            match k {
                "capturefile" => m.capturefile = Some(v.to_owned()),
                "total probes" => m.total_probes = v.parse().map_err(|_| bad())?,
                "total analog" => m.total_analog = v.parse().map_err(|_| bad())?,
                "samplerate" => m.samplerate = Some(parse_sizestring(v)?),
                "unitsize" => m.unitsize = Some(v.parse().map_err(|_| bad())?),
                _ => {
                    if let Some(n) = k.strip_prefix("probe") {
                        let n: usize = n.parse().map_err(|_| bad())?;
                        m.probes.push((n, v.to_owned()));
                    } else if let Some(n) = k.strip_prefix("analog") {
                        let n: usize = n.parse().map_err(|_| bad())?;
                        m.analogs.push((n, v.to_owned()));
                    }
                }
            }
        }
        Ok(m)
    }

    /// The INI text, laid out like libsigrok writes it.
    pub fn to_ini(&self) -> String {
        let mut s = format!("[global]\nsigrok version={}\n\n[device 1]\n", self.writer);
        if let Some(cf) = &self.capturefile {
            let _ = writeln!(s, "capturefile={cf}");
            let _ = writeln!(s, "total probes={}", self.total_probes);
        }
        if let Some(rate) = self.samplerate {
            let _ = writeln!(s, "samplerate={}", samplerate_string(rate));
        }
        let _ = writeln!(s, "total analog={}", self.total_analog);
        for (n, name) in &self.probes {
            let _ = writeln!(s, "probe{n}={name}");
        }
        for (n, name) in &self.analogs {
            let _ = writeln!(s, "analog{n}={name}");
        }
        if let Some(u) = self.unitsize {
            let _ = writeln!(s, "unitsize={u}");
        }
        s
    }

    /// The channels the metadata describes, in libsigrok order: logic
    /// channels first (index 0..), then analog. Only named channels are
    /// enabled; unnamed ones keep their number as name.
    pub fn channels(&self) -> Vec<Channel> {
        let mut channels: Vec<Channel> = (0..self.total_probes)
            .map(|i| {
                let mut c = Channel::new(i, ChannelKind::Logic, i.to_string());
                c.enabled = false;
                c
            })
            .collect();
        for i in 0..self.total_analog {
            let index = self.total_probes + i;
            let mut c = Channel::new(index, ChannelKind::Analog, index.to_string());
            c.enabled = false;
            channels.push(c);
        }
        for (n, name) in &self.probes {
            if let Some(c) = n.checked_sub(1).and_then(|p| channels.get_mut(p)) {
                c.name.clone_from(name);
                c.enabled = true;
            }
        }
        for (n, name) in &self.analogs {
            if let Some(c) =
                n.checked_sub(1).and_then(|i| channels.iter_mut().find(|c| c.index == i))
            {
                c.name.clone_from(name);
                c.enabled = true;
            }
        }
        channels
    }
}

/// True when the file looks like a session file: a zip with a `version`
/// entry.
pub fn is_session_file(path: &Path) -> bool {
    let Ok(f) = std::fs::File::open(path) else { return false };
    let Ok(mut a) = ZipArchive::new(f) else { return false };
    a.by_name("version").is_ok() && a.by_name("metadata").is_ok()
}

/// Reads a session file.
pub struct SessionReader<R: Read + Seek> {
    archive: ZipArchive<R>,
    metadata: Metadata,
    channels: Vec<Channel>,
}

impl SessionReader<std::fs::File> {
    /// Opens the session file at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        SessionReader::new(std::fs::File::open(path)?)
    }
}

impl<R: Read + Seek> SessionReader<R> {
    /// Reads the metadata of a session file from `reader`.
    pub fn new(reader: R) -> Result<Self> {
        let zip_err = |e: zip::result::ZipError| Error::Arg(format!("not a session file: {e}"));
        let mut archive = ZipArchive::new(reader).map_err(zip_err)?;
        let mut version = String::new();
        archive.by_name("version").map_err(zip_err)?.read_to_string(&mut version)?;
        let version: u64 = version.trim().parse().unwrap_or(0);
        if version == 0 || version > 2 {
            return Err(Error::Arg(format!("cannot handle session file version {version}")));
        }
        let mut text = String::new();
        archive.by_name("metadata").map_err(zip_err)?.read_to_string(&mut text)?;
        let metadata = Metadata::parse(&text)?;
        let channels = metadata.channels();
        Ok(SessionReader { archive, metadata, channels })
    }

    /// The parsed metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// The channels, with the names from the file.
    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }

    /// Mutable channels, so a front end can disable some before reading.
    pub fn channels_mut(&mut self) -> &mut [Channel] {
        &mut self.channels
    }

    /// Bytes per logic sample.
    pub fn unit_size(&self) -> usize {
        self.metadata.unitsize.unwrap_or_else(|| self.metadata.total_probes.div_ceil(8))
    }

    /// Sends every packet of the file to `sink`: header, the logic
    /// chunks, then each analog channel's chunks, then end. The sample
    /// rate is in [`SessionReader::metadata`], not in a meta packet, as
    /// with libsigrok's session driver.
    pub fn read(&mut self, sink: &mut dyn FnMut(Packet)) -> Result<()> {
        sink(Packet::header());
        let unit_size = self.unit_size();
        if let Some(cf) = self.metadata.capturefile.clone() {
            if unit_size == 0 {
                log::warn!("session file has logic data but no unit size");
            } else {
                let names = self.chunk_names(&cf);
                for name in names {
                    let mut data = Vec::new();
                    self.archive
                        .by_name(&name)
                        .map_err(|e| Error::Arg(format!("{name}: {e}")))?
                        .read_to_end(&mut data)?;
                    if data.len() % unit_size != 0 {
                        log::warn!(
                            "{name}: size {} not a multiple of the unit size {unit_size}",
                            data.len()
                        );
                        data.truncate(data.len() / unit_size * unit_size);
                    }
                    for chunk in data.chunks(CHUNK_BYTES / unit_size * unit_size) {
                        sink(Packet::logic(unit_size, chunk.to_vec()));
                    }
                }
            }
        }
        for i in 0..self.metadata.total_analog {
            let index = self.metadata.total_probes + i;
            let base = format!("analog-1-{}", index + 1);
            for name in self.chunk_names(&base) {
                let mut data = Vec::new();
                self.archive
                    .by_name(&name)
                    .map_err(|e| Error::Arg(format!("{name}: {e}")))?
                    .read_to_end(&mut data)?;
                let floats: Vec<f32> = data
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                for chunk in floats.chunks(READ_CHUNK_FLOATS) {
                    sink(Packet::Analog(AnalogData {
                        channels: vec![index],
                        samples: chunk.to_vec(),
                        mq: Mq::Voltage,
                        flags: MqFlags::DC,
                        unit: Unit::Volt,
                        digits: ANALOG_DIGITS,
                    }));
                }
            }
        }
        sink(Packet::End);
        Ok(())
    }

    /// `base` itself if it exists, else `base-1`, `base-2`, ... while they
    /// exist.
    fn chunk_names(&mut self, base: &str) -> Vec<String> {
        if self.archive.by_name(base).is_ok() {
            return vec![base.to_owned()];
        }
        let mut names = Vec::new();
        let mut n = 1;
        loop {
            let name = format!("{base}-{n}");
            if self.archive.by_name(&name).is_err() {
                break;
            }
            names.push(name);
            n += 1;
        }
        names
    }
}

/// Writes a session file progressively from packets.
///
/// The archive is created when the first data packet arrives, since the
/// unit size and the sample rate are only certain then. Logic data is
/// collected into chunks of a few megabytes, analog data per channel, and
/// [`SessionWriter::finish`] (or the end packet) writes what is left.
pub struct SessionWriter<W: Write + Seek> {
    zip: ZipWriter<W>,
    channels: Vec<Channel>,
    samplerate: Option<u64>,
    created: bool,
    unit_size: usize,
    logic_chunk: u32,
    logic_buf: Vec<u8>,
    /// Per enabled analog channel: index, 1-based name number, buffer, chunk counter.
    analog: Vec<AnalogBuf>,
}

struct AnalogBuf {
    index: usize,
    number: usize,
    buf: Vec<f32>,
    chunk: u32,
}

impl SessionWriter<std::fs::File> {
    /// Creates the file at `path`, replacing any existing one.
    pub fn create(path: &Path, channels: &[Channel], samplerate: Option<u64>) -> Result<Self> {
        SessionWriter::new(std::fs::File::create(path)?, channels, samplerate)
    }
}

impl<W: Write + Seek> SessionWriter<W> {
    /// A writer over `out` for `channels`. Disabled channels are left out
    /// of the metadata; logic data still uses the device's unit size.
    pub fn new(out: W, channels: &[Channel], samplerate: Option<u64>) -> Result<Self> {
        let total_logic = channels.iter().filter(|c| c.kind == ChannelKind::Logic).count();
        let any_logic = channels.iter().any(|c| c.kind == ChannelKind::Logic && c.enabled);
        let first_analog = if any_logic { total_logic + 1 } else { 1 };
        let analog = channels
            .iter()
            .filter(|c| c.kind == ChannelKind::Analog && c.enabled)
            .enumerate()
            .map(|(i, c)| AnalogBuf {
                index: c.index,
                number: first_analog + i,
                buf: Vec::new(),
                chunk: 1,
            })
            .collect();
        Ok(SessionWriter {
            zip: ZipWriter::new(out),
            channels: channels.to_vec(),
            samplerate,
            created: false,
            unit_size: total_logic.div_ceil(8),
            logic_chunk: 1,
            logic_buf: Vec::new(),
            analog,
        })
    }

    /// The sample rate to record, if a meta packet carries one.
    pub fn set_samplerate(&mut self, rate: u64) {
        if !self.created {
            self.samplerate = Some(rate);
        }
    }

    fn options() -> SimpleFileOptions {
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
    }

    fn zip_err(e: zip::result::ZipError) -> Error {
        Error::Io(std::io::Error::other(e))
    }

    fn add(&mut self, name: &str, data: &[u8]) -> Result<()> {
        self.zip.start_file(name, Self::options()).map_err(Self::zip_err)?;
        self.zip.write_all(data)?;
        Ok(())
    }

    fn create_archive(&mut self) -> Result<()> {
        let any_logic = self.channels.iter().any(|c| c.kind == ChannelKind::Logic && c.enabled);
        let total_logic = self.channels.iter().filter(|c| c.kind == ChannelKind::Logic).count();
        let meta = Metadata {
            writer: format!("oxrok {}", oxrok_core::VERSION),
            capturefile: any_logic.then(|| "logic-1".to_owned()),
            total_probes: total_logic,
            total_analog: self.analog.len(),
            samplerate: self.samplerate,
            unitsize: any_logic.then_some(self.unit_size),
            probes: self
                .channels
                .iter()
                .filter(|c| c.kind == ChannelKind::Logic && c.enabled)
                .map(|c| (c.index + 1, c.name.clone()))
                .collect(),
            analogs: self
                .analog
                .iter()
                .map(|a| {
                    let name = self
                        .channels
                        .iter()
                        .find(|c| c.index == a.index)
                        .map(|c| c.name.clone())
                        .unwrap_or_default();
                    (a.number, name)
                })
                .collect(),
        };
        self.add("version", b"2")?;
        self.add("metadata", meta.to_ini().as_bytes())?;
        self.created = true;
        Ok(())
    }

    fn flush_logic(&mut self) -> Result<()> {
        if self.logic_buf.is_empty() {
            return Ok(());
        }
        let name = format!("logic-1-{}", self.logic_chunk);
        let data = std::mem::take(&mut self.logic_buf);
        self.add(&name, &data)?;
        self.logic_chunk += 1;
        Ok(())
    }

    fn flush_analog(&mut self, i: usize) -> Result<()> {
        if self.analog[i].buf.is_empty() {
            return Ok(());
        }
        let name = format!("analog-1-{}-{}", self.analog[i].number, self.analog[i].chunk);
        let buf = std::mem::take(&mut self.analog[i].buf);
        let bytes: Vec<u8> = buf.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.add(&name, &bytes)?;
        self.analog[i].chunk += 1;
        Ok(())
    }

    /// Handles one packet.
    pub fn receive(&mut self, packet: &Packet) -> Result<()> {
        match packet {
            Packet::Meta(items) => {
                if let Some((_, v)) = items.iter().find(|(k, _)| *k == Key::Samplerate)
                    && let Some(rate) = v.as_u64()
                {
                    self.set_samplerate(rate);
                }
            }
            Packet::Logic(logic) => {
                if !self.created {
                    self.create_archive()?;
                }
                if self.unit_size == 0 {
                    return Ok(());
                }
                let copy = logic.unit_size.min(self.unit_size);
                for sample in logic.data.chunks_exact(logic.unit_size) {
                    self.logic_buf.extend_from_slice(&sample[..copy]);
                    self.logic_buf.extend(std::iter::repeat_n(0, self.unit_size - copy));
                    if self.logic_buf.len() >= CHUNK_BYTES / self.unit_size * self.unit_size {
                        self.flush_logic()?;
                    }
                }
            }
            Packet::Analog(a) => {
                if !self.created {
                    self.create_archive()?;
                }
                let n = a.channels.len();
                for (c, index) in a.channels.iter().enumerate() {
                    let Some(i) = self.analog.iter().position(|b| b.index == *index) else {
                        continue;
                    };
                    for s in 0..a.len() {
                        self.analog[i].buf.push(a.samples[s * n + c]);
                    }
                    if self.analog[i].buf.len() >= CHUNK_BYTES / 4 {
                        self.flush_analog(i)?;
                    }
                }
            }
            Packet::End => self.flush_all()?,
            _ => {}
        }
        Ok(())
    }

    fn flush_all(&mut self) -> Result<()> {
        if !self.created {
            return Ok(());
        }
        self.flush_logic()?;
        for i in 0..self.analog.len() {
            self.flush_analog(i)?;
        }
        Ok(())
    }

    /// Writes what is buffered and closes the archive.
    pub fn finish(mut self) -> Result<()> {
        if !self.created {
            self.create_archive()?;
        }
        self.flush_all()?;
        self.zip.finish().map_err(Self::zip_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn metadata_round_trips() {
        let text = "[global]\nsigrok version=0.5.2\n\n[device 1]\ncapturefile=logic-1\ntotal probes=4\nsamplerate=200 kHz\ntotal analog=2\nprobe1=D0\nprobe2=D1\nprobe3=D2\nprobe4=D3\nanalog5=A0\nanalog6=A1\nunitsize=1\n";
        let m = Metadata::parse(text).unwrap();
        assert_eq!(m.samplerate, Some(200_000));
        assert_eq!(m.unitsize, Some(1));
        assert_eq!(m.to_ini(), text);
        let ch = m.channels();
        let names: Vec<&str> = ch.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["D0", "D1", "D2", "D3", "A0", "A1"]);
        assert_eq!(ch[4].index, 4);
        assert!(ch.iter().all(|c| c.enabled));
    }

    #[test]
    fn write_then_read() {
        let mut channels = vec![Channel::logic(0), Channel::logic(1), Channel::logic(2)];
        channels[1].enabled = false;
        channels.push(Channel::new(3, ChannelKind::Analog, "A0"));
        let mut w = SessionWriter::new(Cursor::new(Vec::new()), &channels, Some(1000)).unwrap();
        w.receive(&Packet::logic(1, vec![1, 5, 4])).unwrap();
        w.receive(&Packet::Analog(AnalogData {
            channels: vec![3],
            samples: vec![1.5, -2.0],
            mq: Mq::Voltage,
            flags: MqFlags::DC,
            unit: Unit::Volt,
            digits: 4,
        }))
        .unwrap();
        w.receive(&Packet::End).unwrap();
        let cursor = {
            let zip = w.zip;
            zip.finish().unwrap()
        };
        let mut r = SessionReader::new(Cursor::new(cursor.into_inner())).unwrap();
        let names: Vec<(String, bool)> =
            r.channels().iter().map(|c| (c.name.clone(), c.enabled)).collect();
        assert_eq!(
            names,
            [("D0".into(), true), ("1".into(), false), ("D2".into(), true), ("A0".into(), true)]
        );
        assert_eq!(r.metadata().samplerate, Some(1000));
        let mut packets = Vec::new();
        r.read(&mut |p| packets.push(p)).unwrap();
        assert_eq!(packets[1], Packet::logic(1, vec![1, 5, 4]));
        let Packet::Analog(a) = &packets[2] else { panic!("{:?}", packets[2]) };
        assert_eq!(a.channels, [3]);
        assert_eq!(a.samples, [1.5, -2.0]);
        assert_eq!(packets[3], Packet::End);
    }
}
