// Counterpart in sigrok:
//   libsigrok/src/hardware/serial-dmm/api.c,
//   libsigrok/src/hardware/serial-dmm/protocol.c,
//   libsigrok/src/hardware/serial-dmm/protocol.h,
//   Copyright (C) 2012-2013 Uwe Hermann <uwe@hermann-uwe.de>,
//   Copyright (C) 2012 Alexandru Gagniuc <mr.nuke.me@gmail.com>,
//   with commits by Bert Vermeulen, Gerhard Sittig, Daniel Elstner,
//   Martin Ling, Aurelien Jacobs, Matthias Heidbrink, Janne Huttunen,
//   Frank Stettner, Soeren Apel, Rainer Schmid, Mike McCauley,
//   Alexander Zakharov, Guido Trentalancia, Sebastian Bissbort
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is an independent implementation written for oxrok from the
//   observed behaviour and the public interfaces.

//! Handheld multimeters on a serial cable.
//!
//! Every supported meter is its own driver id (`uni-t-ut61e-ser`,
//! `voltcraft-vc820-ser`, ...), as in sigrok, since nothing in the
//! byte stream says which meter is talking. A scan needs `conn=` with
//! the serial port, except for the meters whose cable is a USB HID
//! adapter (`hid/bu86x`, `hid/victor`), which are found on their own;
//! `serialcomm=` overrides the cable's parameters. The
//! scan listens for up to three seconds for a packet that the chipset
//! parser accepts, and acquisition then streams one analog packet per
//! reading until the sample or time limit, or until stopped.
//!
//! `OXROK_SERIAL_RECORD=FILE` records the session for replay.

#![forbid(unsafe_code)]

pub mod models;

use models::Model;
use oxrok_core::{
    Access, AnalogData, Channel, ChannelKind, Device, DeviceClass, DeviceInfo, Driver, Error, Key,
    OptionDesc, Packet, PacketSink, Result, Value, ValueList,
};
use oxrok_dmm::Protocol;
use oxrok_serial::{Params, Port};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SCAN_OPTIONS: [Key; 2] = [Key::Conn, Key::Serialcomm];
const CLASSES: [DeviceClass; 1] = [DeviceClass::Multimeter];
// sigrok lists conn for these meters but cannot read it back, so its
// show output has neither the option nor the connection in the header.
static DEV_OPTIONS: [OptionDesc; 3] = [
    OptionDesc::new(Key::Continuous, Access::NONE),
    OptionDesc::new(Key::LimitSamples, Access::GET_SET),
    OptionDesc::new(Key::LimitMsec, Access::GET_SET),
];

/// How long a scan listens for a valid packet.
const DETECT_TIMEOUT: Duration = Duration::from_secs(3);
/// How long one read waits for data.
const READ_TIMEOUT: Duration = Duration::from_millis(100);
/// The receive buffer; when it fills without a packet it is dropped.
const BUF_SIZE: usize = 128;
/// The default wait for a requested packet before asking again.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

/// The driver of one meter.
pub struct SerialDmmDriver(pub &'static Model);

/// Registers a driver for every supported meter.
pub fn register_all(registry: &mut oxrok_core::Registry) {
    for model in models::MODELS {
        registry.register(Box::new(SerialDmmDriver(model)));
    }
}

impl Driver for SerialDmmDriver {
    fn id(&self) -> &'static str {
        self.0.id
    }

    fn long_name(&self) -> &'static str {
        self.0.long_name()
    }

    fn scan_options(&self) -> &'static [Key] {
        &SCAN_OPTIONS
    }

    fn classes(&self) -> &'static [DeviceClass] {
        &CLASSES
    }

    fn scan(&self, options: &[(Key, Value)]) -> Result<Vec<Box<dyn Device>>> {
        let mut conn = None;
        let mut serialcomm = self.0.serialcomm.to_owned();
        for (key, value) in options {
            match key {
                Key::Conn => conn = value.as_str().map(str::to_owned),
                Key::Serialcomm => {
                    value
                        .as_str()
                        .ok_or_else(|| Error::arg("serialcomm"))?
                        .clone_into(&mut serialcomm);
                }
                other => return Err(Error::UnknownKey(other.id().to_owned())),
            }
        }
        let given = conn.is_some();
        let conn = match conn {
            Some(c) => c,
            None if !self.0.conn.is_empty() => self.0.conn.to_owned(),
            None => return Ok(Vec::new()),
        };
        let params = Params::parse(&serialcomm)?;
        let mut port = match oxrok_serial::open(&conn, params) {
            Ok(p) => p,
            // A default cable that is not plugged in is not an error.
            Err(e) if !given => {
                log::debug!("{}: {e}", self.0.id);
                return Ok(Vec::new());
            }
            Err(e) => return Err(e),
        };
        if let Some(after_open) = self.0.after_open {
            after_open(port.as_mut())?;
        }
        let conn = port.name().to_owned();
        log::info!("Probing serial port {conn}.");
        let found = detect(port.as_mut(), &self.0.protocol, DETECT_TIMEOUT)?;
        if !found {
            log::info!("No {} packets on {conn}.", self.0.id);
            return Ok(Vec::new());
        }
        log::info!("Found device on port {conn}.");
        // The port stays open for the device: reopening it would toggle
        // the handshake lines again, which resets some adapters.
        Ok(vec![Box::new(DmmDevice::new(self.0, conn, params, Some(port)))])
    }
}

impl Model {
    fn long_name(&self) -> &'static str {
        // Built once per model: the table is static, the names are few.
        static NAMES: std::sync::OnceLock<std::sync::Mutex<Vec<(&'static str, &'static str)>>> =
            std::sync::OnceLock::new();
        let names = NAMES.get_or_init(|| std::sync::Mutex::new(Vec::new()));
        let mut names = names.lock().expect("names lock");
        if let Some((_, n)) = names.iter().find(|(id, _)| *id == self.id) {
            return n;
        }
        let n: &'static str = Box::leak(format!("{} {}", self.vendor, self.model).into_boxed_str());
        names.push((self.id, n));
        n
    }
}

/// Reads from `port` until a valid packet of `protocol` appears or
/// `timeout` passes. Returns whether one did; leftover bytes are
/// dropped.
pub fn detect(port: &mut dyn Port, protocol: &Protocol, timeout: Duration) -> Result<bool> {
    if let Some(req) = protocol.request {
        port.write(req)?;
    }
    let start = Instant::now();
    let mut buf = Vec::with_capacity(BUF_SIZE);
    let mut byte = [0u8; 1];
    while start.elapsed() < timeout {
        let n = port.read(&mut byte, READ_TIMEOUT)?;
        if n == 0 {
            continue;
        }
        buf.push(byte[0]);
        if protocol.find_packet(&buf).is_some() {
            return Ok(true);
        }
        if buf.len() >= BUF_SIZE {
            buf.drain(..BUF_SIZE / 2);
        }
    }
    Ok(false)
}

/// One meter.
pub struct DmmDevice {
    model: &'static Model,
    info: DeviceInfo,
    channels: Vec<Channel>,
    conn: String,
    params: Params,
    limit_samples: u64,
    limit_msec: u64,
    /// A port to use instead of opening `conn`, for replays.
    pending: Option<Box<dyn Port>>,
    port: Arc<std::sync::Mutex<Option<Box<dyn Port>>>>,
    worker: Option<Worker>,
}

struct Worker {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl DmmDevice {
    fn new(
        model: &'static Model,
        conn: String,
        params: Params,
        port: Option<Box<dyn Port>>,
    ) -> Self {
        let p = &model.protocol;
        let channels = (0..p.channels)
            .map(|i| {
                let name = p
                    .channel_names
                    .get(i)
                    .map_or_else(|| format!("P{}", i + 1), |n| (*n).to_owned());
                Channel::new(i, ChannelKind::Analog, name)
            })
            .collect();
        DmmDevice {
            model,
            info: DeviceInfo {
                driver: model.id,
                vendor: model.vendor.to_owned(),
                model: model.model.to_owned(),
                version: String::new(),
                serial: String::new(),
                conn: None,
            },
            channels,
            conn,
            params,
            limit_samples: 0,
            limit_msec: 0,
            pending: port,
            port: Arc::new(std::sync::Mutex::new(None)),
            worker: None,
        }
    }

    /// A device of `model` on an already open port, e.g. a replay. No
    /// detection is run.
    pub fn from_port(model: &'static Model, port: Box<dyn Port>) -> Self {
        let name = port.name().to_owned();
        let params = Params::parse(model.serialcomm).expect("table parameters parse");
        Self::new(model, name, params, Some(port))
    }
}

impl Device for DmmDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn classes(&self) -> &[DeviceClass] {
        &CLASSES
    }

    fn channels(&self) -> &[Channel] {
        &self.channels
    }

    fn enable_channel(&mut self, pos: usize, enabled: bool) -> Result<()> {
        let ch = self.channels.get_mut(pos).ok_or(Error::Arg(format!("No channel {pos}.")))?;
        ch.enabled = enabled;
        Ok(())
    }

    fn rename_channel(&mut self, pos: usize, name: &str) -> Result<()> {
        let ch = self.channels.get_mut(pos).ok_or(Error::Arg(format!("No channel {pos}.")))?;
        name.clone_into(&mut ch.name);
        Ok(())
    }

    fn options(&self, group: Option<&str>) -> Result<Vec<OptionDesc>> {
        match group {
            None => Ok(DEV_OPTIONS.to_vec()),
            Some(g) => Err(Error::ChannelGroup(g.to_owned())),
        }
    }

    fn get(&self, key: &Key, group: Option<&str>) -> Result<Value> {
        if let Some(g) = group {
            return Err(Error::ChannelGroup(g.to_owned()));
        }
        Ok(match key {
            Key::LimitSamples => Value::U64(self.limit_samples),
            Key::LimitMsec => Value::U64(self.limit_msec),
            other => return Err(Error::UnknownKey(other.id().to_owned())),
        })
    }

    fn set(&mut self, key: &Key, value: &Value, group: Option<&str>) -> Result<()> {
        if let Some(g) = group {
            return Err(Error::ChannelGroup(g.to_owned()));
        }
        let bad = || Error::Arg(format!("invalid value for {}", key.id()));
        match key {
            Key::LimitSamples => self.limit_samples = value.as_u64().ok_or_else(bad)?,
            Key::LimitMsec => self.limit_msec = value.as_u64().ok_or_else(bad)?,
            other => return Err(Error::UnknownKey(other.id().to_owned())),
        }
        Ok(())
    }

    fn list(&self, key: &Key, group: Option<&str>) -> Result<ValueList> {
        if let Some(g) = group {
            return Err(Error::ChannelGroup(g.to_owned()));
        }
        Err(Error::UnknownKey(key.id().to_owned()))
    }

    fn open(&mut self) -> Result<()> {
        if self.port.lock().expect("port lock").is_some() {
            return Ok(());
        }
        let port = match self.pending.take() {
            Some(p) => p,
            None => oxrok_serial::open(&self.conn, self.params)?,
        };
        *self.port.lock().expect("port lock") = Some(port);
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        self.stop()?;
        self.port.lock().expect("port lock").take();
        Ok(())
    }

    fn start(&mut self, sink: PacketSink) -> Result<()> {
        if self.worker.is_some() {
            return Err(Error::Busy);
        }
        let port = self.port.lock().expect("port lock").take();
        let Some(port) = port else {
            return Err(Error::device("device is not open"));
        };
        let acq = Acquisition {
            model: self.model,
            enabled: self.channels.iter().map(|c| c.enabled).collect(),
            limit_samples: self.limit_samples,
            limit_msec: self.limit_msec,
        };
        let slot = Arc::clone(&self.port);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name(format!("{} acquisition", self.model.id))
            .spawn(move || {
                let mut port = port;
                if let Err(e) = acq.run(port.as_mut(), &sink, &stop2) {
                    log::error!("Acquisition failed: {e}.");
                }
                sink.send(Packet::End);
                *slot.lock().expect("port lock") = Some(port);
            })
            .map_err(Error::Io)?;
        self.worker = Some(Worker { stop, handle });
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(w) = self.worker.take() {
            w.stop.store(true, Ordering::SeqCst);
            let _ = w.handle.join();
        }
        Ok(())
    }
}

/// One acquisition, on its own thread.
struct Acquisition {
    model: &'static Model,
    enabled: Vec<bool>,
    limit_samples: u64,
    limit_msec: u64,
}

impl Acquisition {
    fn run(&self, port: &mut dyn Port, sink: &PacketSink, stop: &AtomicBool) -> Result<()> {
        let p = &self.model.protocol;
        let request_timeout = if self.model.request_timeout_ms > 0 {
            Duration::from_millis(self.model.request_timeout_ms)
        } else {
            REQUEST_TIMEOUT
        };
        let request_delay = Duration::from_millis(self.model.request_delay_ms);
        sink.send(Packet::header());
        let started = Instant::now();
        let mut samples = 0u64;
        let mut buf: Vec<u8> = Vec::with_capacity(BUF_SIZE);
        let mut chunk = [0u8; BUF_SIZE];
        let mut last_request = None;
        if let Some(req) = p.request {
            port.write(req)?;
            last_request = Some(Instant::now());
        }
        loop {
            if stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            if self.limit_msec > 0 && started.elapsed() >= Duration::from_millis(self.limit_msec) {
                return Ok(());
            }
            let room = BUF_SIZE - buf.len();
            let n = port.read(&mut chunk[..room.max(1)], READ_TIMEOUT)?;
            if n == 0 {
                if let (Some(req), Some(at)) = (p.request, last_request)
                    && at.elapsed() >= request_timeout
                {
                    port.write(req)?;
                    last_request = Some(Instant::now());
                }
                continue;
            }
            buf.extend_from_slice(&chunk[..n]);
            let mut pos = 0;
            while let Some((skip, len)) = p.find_packet(&buf[pos..]) {
                pos += skip;
                let packet = &buf[pos..pos + len];
                log::debug!("DMM packet: {}", hex(packet));
                let mut sent = false;
                for r in (p.parse)(packet) {
                    if !self.enabled.get(r.channel).copied().unwrap_or(false) {
                        continue;
                    }
                    let data = AnalogData {
                        channels: vec![r.channel],
                        samples: vec![r.value],
                        mq: r.mq,
                        flags: r.flags,
                        unit: r.unit,
                        digits: r.digits,
                    };
                    if !sink.send(Packet::Analog(data)) {
                        return Ok(());
                    }
                    sent = true;
                }
                if sent {
                    samples += 1;
                    if self.limit_samples > 0 && samples >= self.limit_samples {
                        return Ok(());
                    }
                }
                pos += len;
                if let Some(req) = p.request {
                    if !request_delay.is_zero() {
                        std::thread::sleep(request_delay);
                    }
                    port.write(req)?;
                    last_request = Some(Instant::now());
                }
            }
            buf.drain(..pos);
            if buf.len() >= BUF_SIZE {
                log::info!("Dropping unparsed data to resync.");
                buf.clear();
            }
        }
    }
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}
