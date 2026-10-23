// Counterpart in sigrok:
//   libsigrok/src/hardware/fx2lafw/api.c,
//   libsigrok/src/hardware/fx2lafw/protocol.c,
//   sigrok-firmware-fx2lafw (the command set).
// This file is a port of the sigrok counterpart, GPL-3.0-or-later like it.

//! fx2lafw: logic analyzers built on the Cypress FX2 running the sigrok
//! `fx2lafw` firmware. The FX2 streams its GPIO port over a bulk
//! endpoint at up to 24 MS/s (8 channels) or 12 MS/s (16 channels);
//! triggers run on the host.
//!
//! Written from the firmware's public command set: `0xb0` firmware
//! version, `0xb1` start acquisition with a flags byte and a sample
//! delay, `0xb2` chip revision. Devices without the firmware in EEPROM
//! enumerate as the bare chip or the vendor's product; the firmware is
//! uploaded over [`crate::ezusb`] and the device renumerates.

#![forbid(unsafe_code)]

use crate::core::{
    Access, Channel, ChannelGroup, Device, DeviceClass, DeviceInfo, Driver, Error, Key, OptionDesc,
    Packet, PacketSink, PreTriggerBuffer, Result, SoftTrigger, Trigger, TriggerMatch, Value,
    ValueList,
};
use crate::usb::{ControlRequest, Transport, UsbDeviceInfo, UsbId};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const CMD_GET_FW_VERSION: u8 = 0xb0;
const CMD_START: u8 = 0xb1;
const CMD_GET_REVID_VERSION: u8 = 0xb2;
const FLAG_WIDE: u8 = 1 << 5;
const FLAG_CLK_48MHZ: u8 = 1 << 6;
const REQUIRED_MAJOR: u8 = 1;
const MAX_SAMPLE_DELAY: u64 = 6 * 256;
const MAX_8BIT_RATE: u64 = 24_000_000;
const MAX_16BIT_RATE: u64 = 12_000_000;
const CONFIGURATION: u8 = 1;
const INTERFACE: u8 = 0;
const ENDPOINT_IN: u8 = 0x82;
const CONTROL_TIMEOUT: Duration = Duration::from_millis(100);
const RENUM_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_TRANSFERS: usize = 32;
/// Consecutive empty or failed transfers after which the FX2 is taken
/// to have given up.
const MAX_EMPTY_TRANSFERS: usize = MAX_TRANSFERS * 2;

/// The sample rates the firmware can produce.
pub const SAMPLERATES: [u64; 17] = [
    20_000, 25_000, 50_000, 100_000, 200_000, 250_000, 500_000, 1_000_000, 2_000_000, 3_000_000,
    4_000_000, 6_000_000, 8_000_000, 12_000_000, 16_000_000, 24_000_000, 48_000_000,
];

/// A known device: its USB id, names, firmware file and channel count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    /// Vendor and product id.
    pub id: UsbId,
    /// Vendor name.
    pub vendor: &'static str,
    /// Model name.
    pub model: &'static str,
    /// The firmware file in the firmware directories.
    pub firmware: &'static str,
    /// 16 channels instead of 8.
    pub wide: bool,
}

const fn profile(
    vendor_id: u16,
    product_id: u16,
    vendor: &'static str,
    model: &'static str,
    firmware: &'static str,
    wide: bool,
) -> Profile {
    Profile { id: UsbId { vendor: vendor_id, product: product_id }, vendor, model, firmware, wide }
}

/// The devices this driver serves.
pub static PROFILES: [Profile; 10] = [
    profile(0x08a9, 0x0014, "CWAV", "USBee AX", "fx2lafw-cwav-usbeeax.fw", false),
    profile(0x08a9, 0x0015, "CWAV", "USBee DX", "fx2lafw-cwav-usbeedx.fw", true),
    profile(0x08a9, 0x0009, "CWAV", "USBee SX", "fx2lafw-cwav-usbeesx.fw", false),
    profile(0x08a9, 0x0005, "CWAV", "USBee ZX", "fx2lafw-cwav-usbeezx.fw", false),
    profile(0x0925, 0x3881, "Saleae", "Logic", "fx2lafw-saleae-logic.fw", false),
    profile(0x04b4, 0x8613, "Cypress", "FX2", "fx2lafw-cypress-fx2.fw", true),
    profile(
        0x16d0,
        0x0498,
        "Braintechnology",
        "USB-LPS",
        "fx2lafw-braintechnology-usb-lps.fw",
        true,
    ),
    profile(0x1d50, 0x608c, "sigrok", "FX2 LA (8ch)", "fx2lafw-sigrok-fx2-8ch.fw", false),
    profile(0x1d50, 0x608d, "sigrok", "FX2 LA (16ch)", "fx2lafw-sigrok-fx2-16ch.fw", true),
    profile(0x1d50, 0x608f, "sigrok", "usb-c-grok", "fx2lafw-usb-c-grok.fw", false),
];

fn find_profile(id: UsbId) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.id == id)
}

/// True when the device's strings say the firmware is running.
pub fn has_firmware(info: &UsbDeviceInfo) -> bool {
    info.manufacturer.as_deref() == Some("sigrok")
        && info.product.as_deref().is_some_and(|p| p.starts_with("fx2lafw"))
}

/// The `0xb1` command for `samplerate`: flags byte and the GPIF sample
/// delay, or an error for a rate the clocks cannot divide to.
pub fn start_command(samplerate: u64, wide: bool) -> Result<[u8; 3]> {
    let mut flags = 0u8;
    let mut delay = 0u64;
    if samplerate > 0 && 48_000_000 % samplerate == 0 {
        flags = FLAG_CLK_48MHZ;
        delay = 48_000_000 / samplerate - 1;
        if delay > MAX_SAMPLE_DELAY {
            delay = 0;
        }
    }
    if delay == 0 && samplerate > 0 && 30_000_000 % samplerate == 0 && samplerate != 48_000_000 {
        flags = 0;
        delay = 30_000_000 / samplerate - 1;
    }
    let ok = samplerate == 48_000_000 || delay > 0;
    if !ok || delay > MAX_SAMPLE_DELAY {
        return Err(Error::Arg(format!("Unable to sample at {samplerate} Hz.")));
    }
    if wide {
        flags |= FLAG_WIDE;
    }
    Ok([flags, (delay >> 8) as u8, (delay & 0xff) as u8])
}

/// Transfer size (10 ms of data, a multiple of 512), queue depth
/// (about 500 ms in flight, at most 32) and the timeout of one
/// transfer for a rate and sample width.
pub fn transfer_plan(samplerate: u64, unit_size: usize) -> (usize, usize, Duration) {
    let bytes_per_ms = ((samplerate / 1000).max(1) as usize) * unit_size;
    let size = (10 * bytes_per_ms).div_ceil(512).max(1) * 512;
    let depth = (500 * bytes_per_ms / size).clamp(1, MAX_TRANSFERS);
    let total_ms = (size * depth / bytes_per_ms) as u64;
    (size, depth, Duration::from_millis(total_ms + total_ms / 4 + 100))
}

/// The driver.
pub struct Fx2lafwDriver;

static CLASSES: [DeviceClass; 1] = [DeviceClass::LogicAnalyzer];
static SCAN_OPTIONS: [Key; 2] = [Key::Conn, Key::ProbeNames];
static DEV_OPTIONS: [OptionDesc; 7] = [
    OptionDesc::new(Key::Continuous, Access::NONE),
    OptionDesc::new(Key::LimitFrames, Access::GET_SET),
    OptionDesc::new(Key::LimitSamples, Access::GET_SET),
    OptionDesc::new(Key::Conn, Access::GET),
    OptionDesc::new(Key::Samplerate, Access::GET_SET_LIST),
    OptionDesc::new(Key::TriggerMatch, Access::LIST),
    OptionDesc::new(Key::CaptureRatio, Access::GET_SET),
];
static TRIGGER_MATCHES: [TriggerMatch; 5] = [
    TriggerMatch::Zero,
    TriggerMatch::One,
    TriggerMatch::Rising,
    TriggerMatch::Falling,
    TriggerMatch::Edge,
];

impl Driver for Fx2lafwDriver {
    fn id(&self) -> &'static str {
        "fx2lafw"
    }

    fn long_name(&self) -> &'static str {
        "fx2lafw (generic driver for FX2 based LAs)"
    }

    fn scan_options(&self) -> &'static [Key] {
        &SCAN_OPTIONS
    }

    fn classes(&self) -> &'static [DeviceClass] {
        &CLASSES
    }

    fn scan(&self, options: &[(Key, Value)]) -> Result<Vec<Box<dyn Device>>> {
        let mut conn = None;
        let mut probe_names = None;
        for (key, value) in options {
            match key {
                Key::Conn => conn = value.as_str().map(str::to_owned),
                Key::ProbeNames => probe_names = value.as_str().map(str::to_owned),
                other => return Err(Error::UnknownKey(other.id().to_owned())),
            }
        }
        let ids: Vec<UsbId> = PROFILES.iter().map(|p| p.id).collect();
        let mut devices: Vec<Box<dyn Device>> = Vec::new();
        for info in crate::usb::find_devices(&ids)? {
            if conn.as_deref().is_some_and(|c| !info.matches_conn(c)) {
                continue;
            }
            let profile = find_profile(info.id()).expect("found by id");
            let mut dev = Fx2lafwDevice::new(info, profile, None);
            if let Some(spec) = &probe_names {
                dev.name_channels(spec);
            }
            devices.push(Box::new(dev));
        }
        Ok(devices)
    }
}

/// Settings kept between acquisitions.
#[derive(Clone, Debug)]
struct Settings {
    samplerate: u64,
    limit_samples: u64,
    limit_frames: u64,
    capture_ratio: u64,
}

/// One FX2 based analyzer.
pub struct Fx2lafwDevice {
    info: DeviceInfo,
    usb: UsbDeviceInfo,
    profile: &'static Profile,
    channels: Vec<Channel>,
    groups: Vec<ChannelGroup>,
    settings: Settings,
    trigger: Option<Trigger>,
    pending: Option<Box<dyn Transport>>,
    transport: Arc<Mutex<Option<Box<dyn Transport>>>>,
    worker: Option<Worker>,
}

struct Worker {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl Fx2lafwDevice {
    fn new(
        usb: UsbDeviceInfo,
        profile: &'static Profile,
        transport: Option<Box<dyn Transport>>,
    ) -> Self {
        let count = if profile.wide { 16 } else { 8 };
        let channels = (0..count).map(Channel::logic).collect();
        let groups = vec![ChannelGroup::new("Logic", (0..count).collect())];
        Fx2lafwDevice {
            info: DeviceInfo {
                driver: "fx2lafw",
                vendor: profile.vendor.into(),
                model: profile.model.into(),
                version: String::new(),
                serial: usb.serial.clone().unwrap_or_default(),
                conn: Some(usb.conn()),
            },
            usb,
            profile,
            channels,
            groups,
            settings: Settings {
                samplerate: SAMPLERATES[0],
                limit_samples: 0,
                limit_frames: 1,
                capture_ratio: 0,
            },
            trigger: None,
            pending: transport,
            transport: Arc::new(Mutex::new(None)),
            worker: None,
        }
    }

    /// A device on an already open transport (a replayed recording).
    /// The firmware is assumed to be running; the profile comes from
    /// the transport's USB id.
    pub fn from_transport(transport: Box<dyn Transport>) -> Result<Self> {
        let usb = transport.info().clone();
        let profile =
            find_profile(usb.id()).ok_or_else(|| Error::arg("not a known fx2lafw device"))?;
        Ok(Self::new(usb, profile, Some(transport)))
    }

    /// Applies a `probe_names` spec (see
    /// `crate::core::util::parse_probe_names`); a spec starting with `-`
    /// leaves only the named channels.
    pub fn name_channels(&mut self, spec: &str) {
        let defaults: Vec<&str> = self.channels.iter().map(|c| c.name.as_str()).collect();
        let names = crate::core::util::parse_probe_names(spec, &defaults);
        self.channels.truncate(names.len());
        for (c, n) in self.channels.iter_mut().zip(names) {
            c.name = n;
        }
        let count = self.channels.len();
        self.groups = vec![ChannelGroup::new("Logic", (0..count).collect())];
    }

    /// True when a channel above D7 is enabled on a 16 channel device.
    fn wide(&self) -> bool {
        self.profile.wide && self.channels.iter().any(|c| c.enabled && c.index >= 8)
    }
}

/// Opens the device and claims its interface, retrying for a while:
/// right after renumeration the OS may not have set the configuration
/// yet, and the interface is not there to claim.
fn reopen_when_ready(usb: &UsbDeviceInfo) -> Result<Box<dyn Transport>> {
    let mut last = Error::device("device not ready");
    for _ in 0..30 {
        match crate::usb::open(usb) {
            Ok(mut t) => {
                match t.set_configuration(CONFIGURATION).and_then(|()| t.claim_interface(INTERFACE))
                {
                    Ok(()) => return Ok(t),
                    Err(e) => last = e,
                }
            }
            Err(e) => last = e,
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(last)
}

/// Reads the firmware version and chip revision; fails on a version
/// the driver does not speak.
fn check_firmware(t: &mut dyn Transport) -> Result<(u8, u8)> {
    let v = t.control_in(ControlRequest::vendor(CMD_GET_FW_VERSION, 0, 0), 2, CONTROL_TIMEOUT)?;
    if v.len() < 2 {
        return Err(Error::device("short firmware version"));
    }
    if v[0] != REQUIRED_MAJOR {
        return Err(Error::device(format!(
            "expected firmware version {REQUIRED_MAJOR}.x, got {}.{}",
            v[0], v[1]
        )));
    }
    match t.control_in(ControlRequest::vendor(CMD_GET_REVID_VERSION, 0, 0), 1, CONTROL_TIMEOUT) {
        Ok(r) if !r.is_empty() => log::debug!(
            "Firmware {}.{}, REVID {}: a Cypress CY7C68013{}.",
            v[0],
            v[1],
            r[0],
            if r[0] == 1 { "A (FX2LP)" } else { " (FX2)" }
        ),
        _ => log::debug!("Firmware {}.{}, REVID unknown.", v[0], v[1]),
    }
    Ok((v[0], v[1]))
}

impl Device for Fx2lafwDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn classes(&self) -> &[DeviceClass] {
        &CLASSES
    }

    fn channels(&self) -> &[Channel] {
        &self.channels
    }

    fn channel_groups(&self) -> &[ChannelGroup] {
        &self.groups
    }

    fn enable_channel(&mut self, pos: usize, enabled: bool) -> Result<()> {
        let c = self.channels.get_mut(pos).ok_or_else(|| Error::arg("no such channel"))?;
        c.enabled = enabled;
        Ok(())
    }

    fn rename_channel(&mut self, pos: usize, name: &str) -> Result<()> {
        let c = self.channels.get_mut(pos).ok_or_else(|| Error::arg("no such channel"))?;
        name.clone_into(&mut c.name);
        Ok(())
    }

    fn options(&self, group: Option<&str>) -> Result<Vec<OptionDesc>> {
        match group {
            None => Ok(DEV_OPTIONS.to_vec()),
            Some(g) if g.eq_ignore_ascii_case("Logic") => Ok(Vec::new()),
            Some(g) => Err(Error::ChannelGroup(g.to_owned())),
        }
    }

    fn get(&self, key: &Key, group: Option<&str>) -> Result<Value> {
        if let Some(g) = group {
            return Err(Error::ChannelGroup(g.to_owned()));
        }
        Ok(match key {
            Key::Conn => Value::Str(self.usb.conn()),
            Key::Samplerate => Value::U64(self.settings.samplerate),
            Key::LimitSamples => Value::U64(self.settings.limit_samples),
            Key::LimitFrames => Value::U64(self.settings.limit_frames),
            Key::CaptureRatio => Value::U64(self.settings.capture_ratio),
            other => return Err(Error::UnknownKey(other.id().to_owned())),
        })
    }

    fn set(&mut self, key: &Key, value: &Value, group: Option<&str>) -> Result<()> {
        if let Some(g) = group {
            return Err(Error::ChannelGroup(g.to_owned()));
        }
        let bad = || Error::Arg(format!("invalid value for {}", key.id()));
        match key {
            Key::Samplerate => {
                let r = value.as_u64().ok_or_else(bad)?;
                if !SAMPLERATES.contains(&r) {
                    return Err(Error::Arg(format!("unsupported sample rate {r}")));
                }
                self.settings.samplerate = r;
            }
            Key::LimitSamples => self.settings.limit_samples = value.as_u64().ok_or_else(bad)?,
            Key::LimitFrames => self.settings.limit_frames = value.as_u64().ok_or_else(bad)?,
            Key::CaptureRatio => {
                let r = value.as_u64().ok_or_else(bad)?;
                if r > 100 {
                    return Err(Error::arg("capture ratio is a percentage"));
                }
                self.settings.capture_ratio = r;
            }
            other => return Err(Error::UnknownKey(other.id().to_owned())),
        }
        Ok(())
    }

    fn list(&self, key: &Key, group: Option<&str>) -> Result<ValueList> {
        if let Some(g) = group {
            return Err(Error::ChannelGroup(g.to_owned()));
        }
        Ok(match key {
            Key::Samplerate => ValueList::U64(SAMPLERATES.to_vec()),
            Key::TriggerMatch => ValueList::TriggerMatches(TRIGGER_MATCHES.to_vec()),
            other => return Err(Error::UnknownKey(other.id().to_owned())),
        })
    }

    fn open(&mut self) -> Result<()> {
        if self.transport.lock().expect("transport lock").is_some() {
            return Ok(());
        }
        // (transport, already configured and claimed)
        let (mut t, claimed) = if let Some(t) = self.pending.take() {
            (t, false)
        } else {
            let mut t = crate::usb::open(&self.usb)?;
            if has_firmware(&self.usb) {
                (t, false)
            } else {
                let image = crate::core::firmware::read(
                    self.profile.firmware,
                    "install the sigrok fx2lafw firmware, see docs/DEVICES.md",
                )?;
                log::info!("Uploading firmware {}.", self.profile.firmware);
                crate::ezusb::upload_firmware(t.as_mut(), &image)?;
                drop(t);
                let ids: Vec<UsbId> = PROFILES.iter().map(|p| p.id).collect();
                let new = crate::ezusb::wait_for_renumeration(
                    &ids,
                    &self.usb,
                    RENUM_TIMEOUT,
                    has_firmware,
                )?;
                self.info.conn = Some(new.conn());
                self.usb = new;
                // The OS may still be configuring the new device; this
                // retries and returns the transport claimed.
                (reopen_when_ready(&self.usb)?, true)
            }
        };
        if !claimed {
            t.set_configuration(CONFIGURATION)?;
            t.claim_interface(INTERFACE)?;
        }
        let (major, minor) = check_firmware(t.as_mut())?;
        self.info.version = format!("{major}.{minor}");
        *self.transport.lock().expect("transport lock") = Some(t);
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        self.stop()?;
        drop(self.transport.lock().expect("transport lock").take());
        Ok(())
    }

    fn trigger_features(&self) -> crate::core::TriggerFeatures {
        SoftTrigger::FEATURES
    }

    fn set_trigger(&mut self, trigger: Option<Trigger>) -> Result<()> {
        if let Some(t) = &trigger {
            SoftTrigger::check(t)?;
        }
        self.trigger = trigger;
        Ok(())
    }

    fn start(&mut self, sink: PacketSink) -> Result<()> {
        if self.worker.is_some() {
            return Err(Error::Busy);
        }
        let wide = self.wide();
        let rate = self.settings.samplerate;
        let max = if wide { MAX_16BIT_RATE } else { MAX_8BIT_RATE };
        if rate > max {
            return Err(Error::Arg(format!(
                "The maximum sample rate with {} channels is {} MHz.",
                if wide { 16 } else { 8 },
                max / 1_000_000
            )));
        }
        let command = start_command(rate, wide)?;
        let unit_size = if wide { 2 } else { 1 };
        let soft = self.trigger.as_ref().and_then(|t| SoftTrigger::new(t, unit_size));
        let acq = Acquisition {
            command,
            samplerate: rate,
            unit_size,
            limit_samples: self.settings.limit_samples,
            limit_frames: self.settings.limit_frames,
            pre_trigger: self.settings.limit_samples * self.settings.capture_ratio / 100,
            soft,
            trigger: self.trigger.clone(),
        };
        let transport = self.transport.lock().expect("transport lock").take();
        let Some(transport) = transport else {
            return Err(Error::device("device is not open"));
        };
        let slot = Arc::clone(&self.transport);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("fx2lafw acquisition".into())
            .spawn(move || {
                let mut t = transport;
                if let Err(e) = acq.run(t.as_mut(), &sink, &stop2) {
                    log::error!("Acquisition failed: {e}.");
                }
                sink.send(Packet::End);
                *slot.lock().expect("transport lock") = Some(t);
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
    command: [u8; 3],
    samplerate: u64,
    unit_size: usize,
    limit_samples: u64,
    /// Captures of `limit_samples` each; 0 means until stopped. Frame
    /// packets are sent only for more than one.
    limit_frames: u64,
    pre_trigger: u64,
    soft: Option<SoftTrigger>,
    trigger: Option<Trigger>,
}

impl Acquisition {
    fn run(mut self, t: &mut dyn Transport, sink: &PacketSink, stop: &AtomicBool) -> Result<()> {
        let frames = self.limit_frames != 1 && self.limit_samples > 0;
        let mut done = 0u64;
        let mut first = true;
        loop {
            if frames {
                sink.send(Packet::FrameBegin);
            }
            if let Some(trigger) = &self.trigger {
                self.soft = SoftTrigger::new(trigger, self.unit_size);
            }
            self.capture(t, sink, stop, first)?;
            first = false;
            if frames {
                sink.send(Packet::FrameEnd);
            }
            done += 1;
            if !frames
                || stop.load(Ordering::SeqCst)
                || (self.limit_frames > 0 && done >= self.limit_frames)
            {
                return Ok(());
            }
        }
    }

    /// One capture of `limit_samples` samples (or until stopped).
    fn capture(
        &mut self,
        t: &mut dyn Transport,
        sink: &PacketSink,
        stop: &AtomicBool,
        header: bool,
    ) -> Result<()> {
        let (size, depth, timeout) = transfer_plan(self.samplerate, self.unit_size);
        let mut stream = t.bulk_in_stream(ENDPOINT_IN, size, depth)?;
        t.control_out(ControlRequest::vendor(CMD_START, 0, 0), &self.command, CONTROL_TIMEOUT)?;
        log::debug!(
            "Acquisition started: flags 0x{:02x}, delay {}, {size} byte transfers, {depth} in flight.",
            self.command[0],
            u16::from_be_bytes([self.command[1], self.command[2]])
        );
        if header {
            sink.send(Packet::header());
        }
        let mut pre = PreTriggerBuffer::new(self.unit_size, self.pre_trigger as usize);
        let mut sent = 0u64;
        let mut empty = 0usize;
        let mut carry: Vec<u8> = Vec::new();
        while !stop.load(Ordering::SeqCst) {
            let mut data = match stream.next(timeout) {
                Ok(d) => d,
                Err(Error::Timeout) => Vec::new(),
                Err(e) => return Err(e),
            };
            if data.is_empty() {
                empty += 1;
                if empty > MAX_EMPTY_TRANSFERS {
                    log::warn!("Device stopped sending data.");
                    break;
                }
                continue;
            }
            empty = 0;
            // Keep whole samples; a transfer may split one.
            if !carry.is_empty() {
                carry.extend_from_slice(&data);
                data = std::mem::take(&mut carry);
            }
            let rem = data.len() % self.unit_size;
            if rem != 0 {
                carry = data.split_off(data.len() - rem);
            }
            if let Some(soft) = self.soft.as_mut() {
                match soft.feed(&data) {
                    None => {
                        pre.push(&data);
                        continue;
                    }
                    Some(i) => {
                        let at = i * self.unit_size;
                        // The pre-trigger window: the buffered samples
                        // and this transfer's head, cut to the window.
                        let mut before = pre.take();
                        before.extend_from_slice(&data[..at]);
                        let keep = self.pre_trigger as usize * self.unit_size;
                        if before.len() > keep {
                            before.drain(..before.len() - keep);
                        }
                        sent += self.emit(sink, before, sent);
                        sink.send(Packet::Trigger);
                        data.drain(..at);
                        self.soft = None;
                    }
                }
            }
            sent += self.emit(sink, data, sent);
            if self.limit_samples > 0 && sent >= self.limit_samples {
                break;
            }
        }
        log::debug!("Sent {sent} samples.");
        Ok(())
    }

    /// Sends `data`, cut at the sample limit. Returns the samples sent.
    fn emit(&self, sink: &PacketSink, mut data: Vec<u8>, sent: u64) -> u64 {
        if self.limit_samples > 0 {
            let room =
                (self.limit_samples - sent.min(self.limit_samples)) as usize * self.unit_size;
            data.truncate(room);
        }
        if data.is_empty() {
            return 0;
        }
        let n = (data.len() / self.unit_size) as u64;
        sink.send(Packet::logic(self.unit_size, data));
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_commands_match_the_firmware_clocks() {
        assert_eq!(start_command(24_000_000, false).unwrap(), [FLAG_CLK_48MHZ, 0, 1]);
        assert_eq!(start_command(48_000_000, false).unwrap(), [FLAG_CLK_48MHZ, 0, 0]);
        assert_eq!(start_command(12_000_000, true).unwrap(), [FLAG_CLK_48MHZ | FLAG_WIDE, 0, 3]);
        assert_eq!(start_command(3_000_000, false).unwrap(), [FLAG_CLK_48MHZ, 0, 15]);
        // 48 MHz / 20 kHz exceeds the delay range; 30 MHz / 20 kHz fits.
        assert_eq!(start_command(20_000, false).unwrap(), [0, 0x05, 0xdb]);
        assert_eq!(start_command(25_000, false).unwrap(), [0, 0x04, 0xaf]);
        assert_eq!(start_command(50_000, false).unwrap(), [FLAG_CLK_48MHZ, 0x03, 0xbf]);
        assert!(start_command(7_000_000, false).is_err());
        assert!(start_command(0, false).is_err());
    }

    #[test]
    fn transfer_plans() {
        assert_eq!(transfer_plan(24_000_000, 1).0, 240_128);
        assert_eq!(transfer_plan(24_000_000, 1).1, 32);
        assert_eq!(transfer_plan(20_000, 1), (512, 19, Duration::from_millis(486 + 121 + 100)));
        assert_eq!(
            transfer_plan(1_000_000, 2),
            (20_480, 32, Duration::from_millis(327 + 81 + 100))
        );
    }

    #[test]
    fn profiles_and_firmware_detection() {
        assert_eq!(find_profile(UsbId { vendor: 0x0925, product: 0x3881 }).unwrap().model, "Logic");
        let mut d = UsbDeviceInfo { manufacturer: Some("sigrok".into()), ..Default::default() };
        assert!(!has_firmware(&d));
        d.product = Some("fx2lafw".into());
        assert!(has_firmware(&d));
    }
}
