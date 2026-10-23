// Counterpart in sigrok:
//   libsigrok/src/hardware/uni-t-ut32x/api.c,
//   libsigrok/src/hardware/uni-t-ut32x/protocol.c,
//   libsigrok/src/hardware/uni-t-ut32x/protocol.h,
//   Copyright (C) 2013 Bert Vermeulen <bert@biot.com>,
//   with commits by Gerhard Sittig, Uwe Hermann, Aurelien Jacobs
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is an independent implementation written for oxrok from the
//   observed behaviour and the public interfaces.

//! The UNI-T UT32x thermometers: three channels (`T1`, `T2`, `T1-T2`),
//! live readings or the stored ones, on a UT-D04 cable or a serial
//! port. `conn=` is required: the cable cannot be told from any other
//! UT-D04.

#![allow(clippy::doc_markdown)]

use oxrok_core::{
    Access, AnalogData, Channel, ChannelKind, Device, DeviceClass, DeviceInfo, Driver, Error, Key,
    Mq, MqFlags, OptionDesc, Packet, PacketSink, Result, Unit, Value, ValueList,
};
use oxrok_serial::{Params, Port};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SCAN_OPTIONS: [Key; 2] = [Key::Conn, Key::Serialcomm];
const CLASSES: [DeviceClass; 1] = [DeviceClass::Thermometer];
static DEV_OPTIONS: [OptionDesc; 4] = [
    OptionDesc::new(Key::Continuous, Access::NONE),
    OptionDesc::new(Key::LimitSamples, Access::GET_SET),
    OptionDesc::new(Key::LimitMsec, Access::GET_SET),
    OptionDesc::new(Key::DataSource, Access::GET_SET_LIST),
];
const DATA_SOURCES: [&str; 2] = ["Live", "Memory"];
const CHANNEL_NAMES: [&str; 3] = ["T1", "T2", "T1-T2"];
/// Packet size.
pub const PACKET_SIZE: usize = 19;
const CMD_GET_LIVE: u8 = 1;
const CMD_STOP: u8 = 2;
const CMD_GET_STORED: u8 = 7;
const READ_TIMEOUT: Duration = Duration::from_millis(100);

/// The driver.
pub struct Ut32xDriver;

impl Driver for Ut32xDriver {
    fn id(&self) -> &'static str {
        "uni-t-ut32x"
    }

    fn long_name(&self) -> &'static str {
        "UNI-T UT32x"
    }

    fn scan_options(&self) -> &'static [Key] {
        &SCAN_OPTIONS
    }

    fn classes(&self) -> &'static [DeviceClass] {
        &CLASSES
    }

    fn scan(&self, options: &[(Key, Value)]) -> Result<Vec<Box<dyn Device>>> {
        let mut conn = None;
        let mut serialcomm = "2400/8n1".to_owned();
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
        let Some(conn) = conn else {
            return Ok(Vec::new());
        };
        let conn = super::hid_conn(&conn);
        let params = Params::parse(&serialcomm)?;
        // The device cannot be queried; opening the port must do.
        let port = oxrok_serial::open(&conn, params)?;
        Ok(vec![Box::new(Ut32xDevice::new(conn, params, Some(port)))])
    }
}

/// One thermometer.
pub struct Ut32xDevice {
    info: DeviceInfo,
    channels: Vec<Channel>,
    conn: String,
    params: Params,
    limit_samples: u64,
    limit_msec: u64,
    memory: bool,
    pending: Option<Box<dyn Port>>,
    port: Arc<Mutex<Option<Box<dyn Port>>>>,
    worker: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
}

impl Ut32xDevice {
    /// A device on `conn`, with an already open port when given.
    pub fn new(conn: String, params: Params, port: Option<Box<dyn Port>>) -> Ut32xDevice {
        Ut32xDevice {
            info: DeviceInfo {
                driver: "uni-t-ut32x",
                vendor: "UNI-T".to_owned(),
                model: "UT32x".to_owned(),
                version: String::new(),
                serial: String::new(),
                conn: None,
            },
            channels: CHANNEL_NAMES
                .iter()
                .enumerate()
                .map(|(i, n)| Channel::new(i, ChannelKind::Analog, *n))
                .collect(),
            conn,
            params,
            limit_samples: 0,
            limit_msec: 0,
            memory: false,
            pending: port,
            port: Arc::new(Mutex::new(None)),
            worker: None,
        }
    }
}

/// A temperature reading from a packet, or `None` when it carries no
/// measurement; `Err` for a packet that is not one at all.
pub fn parse_packet(pkt: &[u8]) -> std::result::Result<Option<(usize, f32, Unit, MqFlags)>, ()> {
    if pkt.len() != PACKET_SIZE || pkt[17] != b'\r' || pkt[18] != b'\n' {
        return Err(());
    }
    if pkt[8] != b'0' || pkt[16] != b'1' {
        return Err(());
    }
    if pkt[1..5] == *b";;;;" {
        return Ok(None);
    }
    let mut value = 0.0f32;
    let mut negative = false;
    for &c in &pkt[1..5] {
        match c {
            b':' => {}
            b';' => {
                if negative {
                    return Ok(None);
                }
                negative = true;
            }
            b'0'..=b'9' => value = value * 10.0 + f32::from(c - b'0'),
            _ => return Ok(None),
        }
    }
    value /= 10.0;
    if negative {
        value = -value;
    }
    let unit = match pkt[5] {
        b'1' => Unit::Celsius,
        b'2' => Unit::Fahrenheit,
        b'3' => Unit::Kelvin,
        _ => Unit::Unitless,
    };
    let (channel, flags) = match pkt[13] {
        b'0' => (0, MqFlags::empty()),
        b'1' => (1, MqFlags::empty()),
        b'2' | b'3' => (2, MqFlags::RELATIVE),
        _ => {
            log::error!("Unknown channel {:#04x}.", pkt[13]);
            return Ok(None);
        }
    };
    Ok(Some((channel, value, unit, flags)))
}

impl Device for Ut32xDevice {
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

    fn get(&self, key: &Key, _group: Option<&str>) -> Result<Value> {
        Ok(match key {
            Key::LimitSamples => Value::U64(self.limit_samples),
            Key::LimitMsec => Value::U64(self.limit_msec),
            Key::DataSource => Value::Str(DATA_SOURCES[usize::from(self.memory)].to_owned()),
            other => return Err(Error::UnknownKey(other.id().to_owned())),
        })
    }

    fn set(&mut self, key: &Key, value: &Value, _group: Option<&str>) -> Result<()> {
        let bad = || Error::Arg(format!("invalid value for {}", key.id()));
        match key {
            Key::LimitSamples => self.limit_samples = value.as_u64().ok_or_else(bad)?,
            Key::LimitMsec => self.limit_msec = value.as_u64().ok_or_else(bad)?,
            Key::DataSource => {
                self.memory = match value.as_str().ok_or_else(bad)? {
                    "Live" => false,
                    "Memory" => true,
                    _ => return Err(bad()),
                };
            }
            other => return Err(Error::UnknownKey(other.id().to_owned())),
        }
        Ok(())
    }

    fn list(&self, key: &Key, _group: Option<&str>) -> Result<ValueList> {
        match key {
            Key::DataSource => {
                Ok(ValueList::Str(DATA_SOURCES.iter().map(|s| (*s).to_owned()).collect()))
            }
            other => Err(Error::UnknownKey(other.id().to_owned())),
        }
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
        let enabled: Vec<bool> = self.channels.iter().map(|c| c.enabled).collect();
        let (limit_samples, limit_msec, memory) =
            (self.limit_samples, self.limit_msec, self.memory);
        let slot = Arc::clone(&self.port);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("uni-t-ut32x acquisition".to_owned())
            .spawn(move || {
                let mut port = port;
                if let Err(e) =
                    run(port.as_mut(), &sink, &stop2, &enabled, limit_samples, limit_msec, memory)
                {
                    log::error!("Acquisition failed: {e}.");
                }
                sink.send(Packet::End);
                *slot.lock().expect("port lock") = Some(port);
            })
            .map_err(Error::Io)?;
        self.worker = Some((stop, handle));
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some((stop, handle)) = self.worker.take() {
            stop.store(true, Ordering::SeqCst);
            let _ = handle.join();
        }
        Ok(())
    }
}

fn run(
    port: &mut dyn Port,
    sink: &PacketSink,
    stop: &AtomicBool,
    enabled: &[bool],
    limit_samples: u64,
    limit_msec: u64,
    memory: bool,
) -> Result<()> {
    sink.send(Packet::header());
    port.write(&[if memory { CMD_GET_STORED } else { CMD_GET_LIVE }])?;
    let started = Instant::now();
    let mut count = 0u64;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 64];
    let result = loop {
        if stop.load(Ordering::SeqCst) {
            break Ok(());
        }
        if limit_msec > 0 && started.elapsed() >= Duration::from_millis(limit_msec) {
            break Ok(());
        }
        let n = match port.read(&mut chunk, READ_TIMEOUT) {
            Ok(n) => n,
            Err(e) => break Err(e),
        };
        if n == 0 {
            continue;
        }
        buf.extend_from_slice(&chunk[..n]);
        log::debug!("UT32x: {n} bytes: {}", super::hex(&chunk[..n]));
        let mut done = false;
        // Packets end in CR LF; anything up to such a marker is tried.
        while let Some(end) = buf.windows(2).position(|w| w == b"\r\n") {
            let pkt: Vec<u8> = buf.drain(..end + 2).collect();
            let parsed = parse_packet(&pkt);
            log::debug!("UT32x packet {}: {parsed:?}", super::hex(&pkt));
            if let Ok(reading) = parsed {
                if let Some((ch, value, unit, flags)) = reading
                    && enabled.get(ch).copied().unwrap_or(false)
                {
                    let data = AnalogData {
                        channels: vec![ch],
                        samples: vec![value],
                        mq: Mq::Temperature,
                        flags,
                        unit,
                        digits: 1,
                    };
                    if !sink.send(Packet::Analog(data)) {
                        done = true;
                        break;
                    }
                }
                // Every packet counts, so a limit on the memory source
                // also passes over the empty slots.
                count += 1;
                if limit_samples > 0 && count >= limit_samples {
                    done = true;
                    break;
                }
            }
        }
        if buf.len() > 4 * PACKET_SIZE {
            buf.clear();
        }
        if done {
            break Ok(());
        }
    };
    let _ = port.write(&[CMD_STOP]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packet with the four temperature characters, the unit digit,
    /// the channel digit and the two fixed markers.
    fn packet(temp: &[u8; 4], unit: u8, channel: u8, marker16: u8) -> Vec<u8> {
        let mut p = vec![0u8];
        p.extend_from_slice(temp);
        p.push(unit);
        p.extend(b"00");
        p.push(b'0');
        p.extend(b"0000");
        p.push(channel);
        p.extend(b"00");
        p.push(marker16);
        p.extend(b"\r\n");
        p
    }

    #[test]
    fn packets_parse() {
        // 23.5 C on T1.
        let p = packet(b"0235", b'1', b'0', b'1');
        assert_eq!(p.len(), PACKET_SIZE);
        assert_eq!(parse_packet(&p).unwrap(), Some((0, 23.5, Unit::Celsius, MqFlags::empty())));
        // -1.2 F on T2.
        let (ch, v, unit, _) = parse_packet(&packet(b";:12", b'2', b'1', b'1')).unwrap().unwrap();
        assert_eq!((ch, unit), (1, Unit::Fahrenheit));
        assert!((v + 1.2).abs() < 1e-5);
        // The difference channel, in kelvin, is relative.
        let (ch, _, unit, flags) =
            parse_packet(&packet(b"0010", b'3', b'2', b'1')).unwrap().unwrap();
        assert_eq!((ch, unit, flags), (2, Unit::Kelvin, MqFlags::RELATIVE));
        // No measurement, and not a packet.
        assert_eq!(parse_packet(&packet(b";;;;", b'1', b'0', b'1')).unwrap(), None);
        assert!(parse_packet(&packet(b"0235", b'1', b'0', b'0')).is_err());
    }
}
