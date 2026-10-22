// Counterpart in sigrok: libsigrok/src/hardware/intronix-logicport/protocol.c,
//   Copyright (C) 2026 Miguel Angel Ajo Pelayo <miguelangel@ajo.es>.
// This file is a port by the same author, relicensed.

//! The host protocol of the LogicPort.
//!
//! The LogicPort is an FT245BL USB FIFO bridge in front of an Altera
//! Cyclone FPGA. The FPGA is SRAM based and gets configured by the host
//! in FTDI bit-bang mode (passive serial). Once configured, the host
//! talks to it in FIFO mode with a small register protocol: every
//! command is a five byte header (opcode, 16 bit offset, 16 bit count
//! minus one), optionally followed by write data. The device answers
//! each command with a three byte ack (opcode, 16 bit sequence number)
//! followed by read data. A 32 bit register address is split into a
//! segment (upper 16 bits, selected by a separate command) and an
//! offset.

use crate::firmware::{self, Image};
use crate::readout::{DdrCapture, MEM_DEPTH, Rows, SdrCapture, ddr_window, sdr_window};
use oxrok_core::{Error, Result};
use oxrok_ftdi::{BitMode, Ftdi, Port};
use oxrok_usb::Transport;
use std::time::{Duration, Instant};

const CMD_WRITE: u8 = 0xc1;
const CMD_READ: u8 = 0xc2;
const CMD_SET_SEGMENT: u8 = 0xc3;
const CMD_RESYNC: u8 = 0x01;
const ACK_LEN: usize = 3;

const USB_TIMEOUT: Duration = Duration::from_secs(1);
const WRITE_ACK_TIMEOUT: Duration = Duration::from_secs(1);
const READ_TIMEOUT: Duration = Duration::from_secs(2);
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);
const LINK_TIMEOUT: Duration = Duration::from_secs(3);
const LATENCY_TIMER_MS: u8 = 4;
/// Sets the bit-bang clock for FPGA configuration, unused in FIFO mode.
const BITBANG_BAUDRATE: u32 = 460_800;

/// Control and status block.
const REG_STATUS: u32 = 0x0010_0000;
/// Write only, and only ever written with 0: it releases the FPGA's
/// configuration. Other values are documented as locking the device.
const REG_CONFIG_CTRL: u32 = 0x0010_0000;
const REG_ARM: u32 = 0x0010_0002;
const REG_RUN: u32 = 0x0010_0003;
const REG_PRE_DEPTH: u32 = 0x0010_0004;
const REG_POST_DEPTH: u32 = 0x0010_0006;
const REG_THRESHOLD: u32 = 0x0010_0008;
const REG_END_PREFILL: u32 = 0x0010_000a;
const REG_UNFILLED: u32 = 0x0010_000a;
const REG_END_POSTFILL: u32 = 0x0010_000b;
const REG_TRIGGER_COMBINE: u32 = 0x0010_000d;
const REG_IMAGE_ID: u32 = 0x0010_000f;

/// Trigger levels A and B share a layout.
const REG_TRIGGER_LEVEL_A: u32 = 0x0020_0000;
const REG_TRIGGER_LEVEL_B: u32 = 0x0040_0000;
const TRIG_PATTERN_MASK: u32 = 0x00;
const TRIG_PATTERN_VALUE: u32 = 0x05;
const TRIG_COUNT_HIGH: u32 = 0x0a;
const TRIG_COUNT_LOW: u32 = 0x0e;
const TRIG_CONDITION: u32 = 0x14;
const TRIG_COUNTER_SOURCE: u32 = 0x16;
const TRIG_COUNTER_MODE: u32 = 0x17;
const TRIG_CONSECUTIVE: u32 = 0x18;
const TRIG_PREQUALIFY: u32 = 0x19;
const TRIG_RISING_MASK: u32 = 0x29;
const TRIG_FALLING_MASK: u32 = 0x2e;

/// Sample clocking and channel configuration.
const REG_CLOCK_MODE: u32 = 0x0100_0000;
const REG_TRIGGER_POS: u32 = 0x0100_0000;
const REG_CHANNEL_CFG: u32 = 0x0100_0001;
const REG_COMPRESSION: u32 = 0x0100_0002;
const REG_COMPRESSION_MASK: u32 = 0x0100_0003;
const REG_INVERT_MASK: u32 = 0x0100_0008;

/// Sample memory.
const REG_WRITE_PTR: u32 = 0x0200_0000;
const REG_WRITE_PTR_B: u32 = 0x0200_0100;
const MEM_LANES_A: u32 = 0x0200_1000;
const MEM_LANES_B: u32 = 0x0200_5000;
const MEM_LANE_STRIDE: u32 = 0x1000;
const MEM_FLAGS: u32 = 0x0200_9000;

/// Timebase.
const REG_PRESCALER: u32 = 0x2000_0000;
const REG_STEP: u32 = 0x2000_0001;

const REG_INIT_COUNTER: u32 = 0x4000_0005;

/// Status register bits. It cannot tell idle from pre-filling, and a
/// previous capture's done bit survives into later sessions, so the
/// driver only looks at it after having started the acquisition.
pub const STATUS_DONE: u8 = 1 << 0;
/// The sample memory wrapped around at least once.
pub const STATUS_WRAPPED: u8 = 1 << 1;
/// The trigger fired.
pub const STATUS_TRIGGERED: u8 = 1 << 5;
/// The pre-trigger part of the memory is full.
pub const STATUS_PREFILLED: u8 = 1 << 6;
const FLAG_COUNT: u8 = 1 << 3;

const CLOCK_MODE_CLK1: u8 = 1 << 2;
const CLOCK_MODE_CLK2: u8 = 1 << 1;
const CLOCK_MODE_FALLING: u8 = 1 << 0;
const CLOCK_MODE_NO_QUAL: u8 = 1 << 4;

const COND_PATTERN_TRUE: u8 = 1;
const COUNTER_SRC_EDGE: u8 = 3;
const COUNTER_MODE_OCCURRENCE: u8 = 1;

/// The threshold DAC output settles with a time constant of about 30 ms
/// (measured); a full scale step needs about 165 ms to get within 50 mV.
const THRESHOLD_SETTLE: Duration = Duration::from_millis(200);

/// Bit-bang pins used for passive serial FPGA configuration.
const BB_DATA0: u8 = 1 << 0;
const BB_DCLK: u8 = 1 << 1;
const BB_NCONFIG: u8 = 1 << 2;
const BB_NSTATUS: u8 = 1 << 3;
const BB_CONF_DONE: u8 = 1 << 4;
const BB_OUTPUTS: u8 = BB_DATA0 | BB_DCLK | BB_NCONFIG;
const BB_CHUNK_BYTES: usize = 10000;
const BB_EXTRA_CLOCKS: usize = 32;

const RESYNC_RETRIES: usize = 5;
const CONFIG_RETRIES: usize = 3;

/// A sample rate table entry: prescaler and 1-2-5 step registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rate {
    /// Samples per second.
    pub samplerate: u64,
    /// True for the 500 MHz double data rate mode.
    pub ddr: bool,
    prescaler: u8,
    step: u8,
}

/// The sample rates the hardware offers.
pub const RATES: [Rate; 19] = [
    Rate { samplerate: 1_000, ddr: false, prescaler: 0x61, step: 0x00 },
    Rate { samplerate: 2_000, ddr: false, prescaler: 0x51, step: 0x44 },
    Rate { samplerate: 5_000, ddr: false, prescaler: 0x51, step: 0x11 },
    Rate { samplerate: 10_000, ddr: false, prescaler: 0x51, step: 0x00 },
    Rate { samplerate: 20_000, ddr: false, prescaler: 0x41, step: 0x44 },
    Rate { samplerate: 50_000, ddr: false, prescaler: 0x41, step: 0x11 },
    Rate { samplerate: 100_000, ddr: false, prescaler: 0x41, step: 0x00 },
    Rate { samplerate: 200_000, ddr: false, prescaler: 0x31, step: 0x44 },
    Rate { samplerate: 500_000, ddr: false, prescaler: 0x31, step: 0x11 },
    Rate { samplerate: 1_000_000, ddr: false, prescaler: 0x31, step: 0x00 },
    Rate { samplerate: 2_000_000, ddr: false, prescaler: 0x21, step: 0x44 },
    Rate { samplerate: 5_000_000, ddr: false, prescaler: 0x21, step: 0x11 },
    Rate { samplerate: 10_000_000, ddr: false, prescaler: 0x21, step: 0x00 },
    Rate { samplerate: 20_000_000, ddr: false, prescaler: 0x11, step: 0x44 },
    Rate { samplerate: 50_000_000, ddr: false, prescaler: 0x11, step: 0x11 },
    Rate { samplerate: 100_000_000, ddr: false, prescaler: 0x11, step: 0x00 },
    Rate { samplerate: 200_000_000, ddr: false, prescaler: 0x01, step: 0x00 },
    Rate { samplerate: 250_000_000, ddr: false, prescaler: 0x00, step: 0x00 },
    Rate { samplerate: 500_000_000, ddr: true, prescaler: 0x00, step: 0x00 },
];

/// The table entry for `samplerate`.
pub fn find_rate(samplerate: u64) -> Option<&'static Rate> {
    RATES.iter().find(|r| r.samplerate == samplerate)
}

/// One of the two hardware trigger levels (A, B). Masks have one bit
/// per channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TriggerLevel {
    /// Channels taking part in the pattern.
    pub pattern_mask: u64,
    /// Their required levels.
    pub pattern_value: u64,
    /// Channels whose rising edge counts.
    pub rising_mask: u64,
    /// Channels whose falling edge counts.
    pub falling_mask: u64,
    /// Pattern true / false / unused.
    pub condition: u8,
    /// Event counted, e.g. edges.
    pub counter_source: u8,
    /// How the count is evaluated.
    pub counter_mode: u8,
}

impl TriggerLevel {
    /// Fills the derived fields once the masks are set.
    pub fn finish(&mut self) {
        if self.pattern_mask != 0 {
            self.condition = COND_PATTERN_TRUE;
        }
        if self.rising_mask != 0 || self.falling_mask != 0 {
            self.counter_source = COUNTER_SRC_EDGE;
        }
        if self.condition != 0 || self.counter_source != 0 {
            self.counter_mode = COUNTER_MODE_OCCURRENCE;
        }
    }
}

/// Everything `program` writes to the device.
#[derive(Clone, Debug)]
pub struct Program {
    /// The sample rate, used in timing mode.
    pub rate: Rate,
    /// External clock (state mode) instead of the timebase.
    pub external_clock: bool,
    /// 0: CLK1, 1: CLK2.
    pub clock_source: usize,
    /// 0: rising, 1: falling.
    pub clock_edge: usize,
    /// Run length compression.
    pub rle: bool,
    /// Channels whose changes start a new RLE entry.
    pub compression_mask: u64,
    /// Pre-trigger percentage.
    pub capture_ratio: u64,
    /// Input threshold in volts.
    pub threshold: f64,
    /// The two trigger levels.
    pub trigger: [TriggerLevel; 2],
    /// 0: none, 1: level A, 5: A then B.
    pub trigger_combine: u8,
}

impl Program {
    /// The FPGA image the settings need.
    pub fn image(&self) -> Image {
        if self.external_clock {
            Image::StateOn
        } else if self.rate.ddr {
            Image::TimingDdr
        } else {
            Image::TimingSdr
        }
    }

    /// True when DDR readout applies.
    pub fn ddr(&self) -> bool {
        !self.external_clock && self.rate.ddr
    }
}

/// The DAC code for a threshold voltage, with the unit's calibration.
pub fn threshold_dac(volts: f64, cal: i32) -> u16 {
    // Input divider and DAC transfer function.
    let v = (volts - 1.315) * -0.402_038_109_862_497_4 + 1.315;
    let dac = ((4.559 - v) / 5.875 * 1023.0).round() as i64 + i64::from(cal);
    dac.clamp(0, 1023) as u16
}

/// Halves, rounding to even on ties.
fn half_even(x: i64) -> i64 {
    if x & 1 == 0 {
        return x / 2;
    }
    if ((x + 1) / 2) % 2 != 0 { (x - 1) / 2 } else { (x + 1) / 2 }
}

/// The pre and post trigger depths for a capture ratio.
pub fn buffer_split(ratio: u64, ddr: bool) -> (u16, u16) {
    let depth = MEM_DEPTH as i64;
    let ratio = ratio.min(100) as i64;
    let mut pre = ratio * depth / 100 + 1;
    let mut post = (100 - ratio) * depth / 100 - 1;
    if ddr {
        pre = half_even(pre + 0x23);
        post = half_even((post - 0x1e).max(0));
    } else {
        pre += 7;
        post = (post - 7).max(0);
    }
    (pre as u16, post as u16)
}

/// The register writes of `program`, in the order the vendor software
/// uses: address, width in bytes, value.
pub fn program_regs(p: &Program, cal: i32) -> Vec<(u32, usize, u64)> {
    let mut q: Vec<(u32, usize, u64)> = Vec::with_capacity(48);
    let mode = if p.external_clock {
        CLOCK_MODE_NO_QUAL
            | if p.clock_source == 1 { CLOCK_MODE_CLK2 } else { CLOCK_MODE_CLK1 }
            | if p.clock_edge == 1 { CLOCK_MODE_FALLING } else { 0 }
    } else {
        q.push((REG_PRESCALER, 1, u64::from(p.rate.prescaler)));
        q.push((REG_STEP, 1, u64::from(p.rate.step)));
        CLOCK_MODE_CLK1 | CLOCK_MODE_NO_QUAL
    };
    q.push((REG_CLOCK_MODE, 1, u64::from(mode)));
    q.push((REG_COMPRESSION, 1, 0));
    // All channels are always captured, this only steers compression:
    // changes on channels outside the mask don't start a new RLE entry,
    // so disabled (e.g. floating) inputs don't fill the sample memory.
    q.push((REG_COMPRESSION_MASK, 5, p.compression_mask));
    q.push((REG_INVERT_MASK, 5, 0));
    q.push((REG_COMPRESSION, 1, u64::from(p.rle)));
    q.push((REG_CHANNEL_CFG, 1, 0));
    q.push((REG_RUN, 1, 0));
    q.push((REG_ARM, 1, 1));
    for (base, level) in
        [(REG_TRIGGER_LEVEL_A, &p.trigger[0]), (REG_TRIGGER_LEVEL_B, &p.trigger[1])]
    {
        q.push((base + TRIG_PATTERN_MASK, 5, level.pattern_mask));
        q.push((base + TRIG_PATTERN_VALUE, 5, level.pattern_value));
        q.push((base + TRIG_RISING_MASK, 5, level.rising_mask));
        q.push((base + TRIG_FALLING_MASK, 5, level.falling_mask));
        q.push((base + TRIG_CONSECUTIVE, 1, 0));
        q.push((base + TRIG_PREQUALIFY, 1, 0));
        q.push((base + TRIG_COUNT_LOW, 4, 0));
        // Occurrence count minus one: fire on the first event.
        q.push((base + TRIG_COUNT_HIGH, 4, 0));
        q.push((base + TRIG_CONDITION, 1, u64::from(level.condition)));
        q.push((base + TRIG_COUNTER_SOURCE, 1, u64::from(level.counter_source)));
        q.push((base + TRIG_COUNTER_MODE, 1, u64::from(level.counter_mode)));
    }
    q.push((REG_TRIGGER_COMBINE, 1, u64::from(p.trigger_combine)));
    q.push((REG_THRESHOLD, 2, u64::from(threshold_dac(p.threshold, cal))));
    let (pre, post) = buffer_split(p.capture_ratio, p.ddr());
    q.push((REG_PRE_DEPTH, 2, u64::from(pre)));
    q.push((REG_POST_DEPTH, 2, u64::from(post)));
    q
}

/// An open link to the FPGA through the FTDI chip.
pub struct Link {
    ftdi: Ftdi,
    /// Expected sequence number of the next ack.
    seq: u16,
    /// Selected address segment.
    segment: Option<u16>,
    /// Link out of step, cleared by a resync.
    io_error: bool,
    /// The loaded image, when known.
    pub fpga_image: Option<Image>,
    /// DAC offset from the EEPROM.
    pub threshold_cal: i32,
    /// Last DAC value written.
    threshold_dac: Option<u16>,
}

impl Link {
    /// Claims the FTDI port and sets it up.
    pub fn open(transport: Box<dyn Transport>) -> Result<Link> {
        let mut ftdi = Ftdi::open(transport, Port::A)?;
        Self::setup(&mut ftdi)?;
        Ok(Link {
            ftdi,
            seq: 0,
            segment: None,
            io_error: false,
            fpga_image: None,
            threshold_cal: 0,
            threshold_dac: None,
        })
    }

    fn setup(ftdi: &mut Ftdi) -> Result<()> {
        ftdi.control_timeout = USB_TIMEOUT;
        ftdi.read_timeout = USB_TIMEOUT;
        ftdi.write_timeout = USB_TIMEOUT;
        ftdi.reset()?;
        ftdi.set_latency_timer(LATENCY_TIMER_MS)?;
        ftdi.set_baudrate(BITBANG_BAUDRATE)?;
        Ok(())
    }

    /// Gives the transport back.
    pub fn into_transport(self) -> Box<dyn Transport> {
        self.ftdi.into_transport()
    }

    /// The equivalent of closing and reopening the FTDI device: the
    /// port is reset and set up again.
    fn reopen(&mut self) -> Result<()> {
        self.ftdi.transport().release_interface(0)?;
        std::thread::sleep(Duration::from_millis(50));
        self.ftdi.transport().claim_interface(0)?;
        Self::setup(&mut self.ftdi)
    }

    fn purge_both(&mut self) -> Result<()> {
        self.ftdi.purge_read()?;
        self.ftdi.purge_write()
    }

    /// Raw transfer errors are expected while the FPGA is unconfigured
    /// (the FIFO isn't drained), so they are only logged at debug level.
    fn read_raw(&mut self, len: usize, timeout: Duration) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.ftdi.read_exact(&mut buf, timeout).map_err(|e| {
            log::debug!("Failed to read FTDI data: {e}.");
            e
        })?;
        Ok(buf)
    }

    fn write_raw(&mut self, data: &[u8]) -> Result<()> {
        self.ftdi.write(data).map_err(|e| {
            log::debug!("Failed to write FTDI data: {e}.");
            e
        })
    }

    /// Sends one command and checks its ack; read data follows the ack.
    fn send_cmd(
        &mut self,
        cmd: u8,
        arg0: u16,
        arg1: u16,
        wdata: &[u8],
        rlen: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        // Fail fast until the next resync once the link is out of step.
        if self.io_error {
            return Err(Error::device("link out of step"));
        }
        let mut buf = Vec::with_capacity(5 + wdata.len());
        buf.push(cmd);
        buf.extend_from_slice(&arg0.to_le_bytes());
        buf.extend_from_slice(&arg1.to_le_bytes());
        buf.extend_from_slice(wdata);
        let r =
            self.write_raw(&buf).and_then(|()| self.read_raw(ACK_LEN, timeout)).and_then(|ack| {
                if ack[0] != cmd || u16::from_be_bytes([ack[1], ack[2]]) != self.seq {
                    log::debug!(
                        "Bad ack {:02x} {:02x} {:02x} for command 0x{cmd:02x}, seq {}.",
                        ack[0],
                        ack[1],
                        ack[2],
                        self.seq
                    );
                    return Err(Error::device("bad ack"));
                }
                if rlen > 0 { self.read_raw(rlen, timeout) } else { Ok(Vec::new()) }
            });
        match r {
            Ok(data) => {
                self.seq = self.seq.wrapping_add(1);
                Ok(data)
            }
            Err(e) => {
                self.segment = None;
                self.io_error = true;
                Err(e)
            }
        }
    }

    fn set_segment(&mut self, addr: u32) -> Result<()> {
        let segment = (addr >> 16) as u16;
        if self.segment == Some(segment) {
            return Ok(());
        }
        self.send_cmd(CMD_SET_SEGMENT, segment, 0, &[], 0, WRITE_ACK_TIMEOUT)?;
        self.segment = Some(segment);
        Ok(())
    }

    /// Writes `data` at `addr`. Register 0x100000 only ever takes 0.
    pub fn reg_write(&mut self, addr: u32, data: &[u8]) -> Result<()> {
        if addr == REG_CONFIG_CTRL && data != [0] {
            return Err(Error::device(format!(
                "refusing to write {data:02x?} to register 0x{REG_CONFIG_CTRL:08x}"
            )));
        }
        if data.is_empty() || data.len() > 8 {
            return Err(Error::arg("register write of 1 to 8 bytes"));
        }
        self.set_segment(addr)?;
        self.send_cmd(CMD_WRITE, addr as u16, (data.len() - 1) as u16, data, 0, WRITE_ACK_TIMEOUT)?;
        Ok(())
    }

    fn reg_write_u8(&mut self, addr: u32, value: u8) -> Result<()> {
        self.reg_write(addr, &[value])
    }

    fn reg_read_timeout(&mut self, addr: u32, len: usize, timeout: Duration) -> Result<Vec<u8>> {
        self.set_segment(addr)?;
        self.send_cmd(CMD_READ, addr as u16, (len - 1) as u16, &[], len, timeout)
    }

    /// Reads `len` bytes at `addr`.
    pub fn reg_read(&mut self, addr: u32, len: usize) -> Result<Vec<u8>> {
        self.reg_read_timeout(addr, len, READ_TIMEOUT)
    }

    fn reg_read_u8(&mut self, addr: u32) -> Result<u8> {
        Ok(self.reg_read(addr, 1)?[0])
    }

    fn reg_read_u16(&mut self, addr: u32) -> Result<u16> {
        let d = self.reg_read(addr, 2)?;
        Ok(u16::from_le_bytes([d[0], d[1]]))
    }

    /// Resynchronizes with the FPGA's command parser. The device keeps
    /// its sequence counter across host sessions, so this is needed on
    /// every open. The flood of zeros drains any partially received
    /// command of maximum length, the final 0x01 resets the parser and
    /// the counter.
    fn resync_once(&mut self, reopen: bool) -> Result<()> {
        self.segment = None;
        if reopen {
            self.reopen()?;
        }
        self.purge_both()?;
        let zeros = vec![0u8; 65535];
        self.write_raw(&zeros)?;
        self.write_raw(&zeros[..30000])?;
        self.write_raw(&[CMD_RESYNC])?;
        std::thread::sleep(Duration::from_millis(50));
        self.purge_both()?;
        self.seq = 0;
        self.io_error = false;
        Ok(())
    }

    /// Resyncs, up to `attempts` times, reopening the port from the
    /// second attempt (or the first with `reopen`).
    pub fn resync(&mut self, reopen: bool, attempts: usize) -> Result<()> {
        let mut last = Error::device("resync failed");
        for i in 0..attempts {
            match self.resync_once(reopen || i > 0) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    log::debug!("Resync attempt {} failed.", i + 1);
                    last = e;
                }
            }
            if i + 1 < attempts {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        log::debug!("Failed to resynchronize with the device.");
        Err(last)
    }

    /// Finds out which FPGA image is loaded. An unconfigured FPGA won't
    /// reply.
    pub fn probe_image(&mut self) -> Result<Image> {
        self.fpga_image = None;
        let id = match self.reg_read_timeout(REG_IMAGE_ID, 1, PROBE_TIMEOUT) {
            Ok(d) => d[0],
            Err(e) => {
                log::debug!("No reply from the FPGA, assuming it is unconfigured.");
                return Err(e);
            }
        };
        let image = if id & 0xf0 == 0x10 { Image::from_index(id & 0x0f) } else { None };
        let Some(image) = image else {
            log::debug!("Unexpected FPGA image ID 0x{id:02x}.");
            return Err(Error::device(format!("unexpected FPGA image id 0x{id:02x}")));
        };
        self.fpga_image = Some(image);
        log::debug!("FPGA image {} is loaded.", image.index());
        Ok(image)
    }

    /// The threshold calibration offset is kept as a signed decimal
    /// string (e.g. "+003") at the start of the EEPROM user area, which
    /// follows the string descriptors and the four byte PnP block. The
    /// EEPROM is never written.
    pub fn read_calibration(&mut self) -> Result<i32> {
        self.threshold_cal = 0;
        let buf = self.ftdi.read_eeprom(64).map_err(|e| {
            log::warn!("Failed to read EEPROM: {e}.");
            e
        })?;
        let cal = parse_calibration(&buf).ok_or_else(|| {
            log::warn!("No threshold calibration found in EEPROM.");
            Error::device("no threshold calibration in EEPROM")
        })?;
        log::debug!("Threshold calibration offset: {cal}.");
        self.threshold_cal = cal;
        Ok(cal)
    }

    fn wait_pins(&mut self, mask: u8) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let pins = self.ftdi.read_pins()?;
            if pins & mask != 0 {
                return Ok(());
            }
            if Instant::now() > deadline {
                log::debug!("Timeout waiting for pins 0x{mask:02x} (pins 0x{pins:02x}).");
                return Err(Error::Timeout);
            }
        }
    }

    /// Sends a bit-bang stream: two bytes per bit, DCLK low then high.
    fn send_bitstream(&mut self, stream: &[u8]) -> Result<()> {
        for chunk in stream.chunks(BB_CHUNK_BYTES) {
            self.write_raw(chunk)?;
            // The bit-bang echo is of no use, don't let it pile up.
            self.ftdi.purge_read()?;
        }
        Ok(())
    }

    fn bitbang_configure(&mut self, stream: &[u8]) -> Result<()> {
        self.ftdi.purge_read()?;
        self.ftdi.set_bitmode(0, BitMode::Reset)?;
        self.ftdi.set_bitmode(BB_OUTPUTS, BitMode::BitBang)?;
        self.write_raw(&[0])?;
        std::thread::sleep(Duration::from_millis(10));
        self.write_raw(&[BB_NCONFIG])?;
        if self.wait_pins(BB_NSTATUS).is_err() {
            log::error!("FPGA did not enter configuration mode.");
            return Err(Error::Timeout);
        }
        self.send_bitstream(stream)?;
        if self.wait_pins(BB_CONF_DONE).is_err() {
            log::error!("FPGA did not accept the configuration.");
            return Err(Error::Timeout);
        }
        // Cyclone devices need extra DCLK edges to start initialization.
        let mut extra = Vec::with_capacity(2 * BB_EXTRA_CLOCKS);
        for _ in 0..BB_EXTRA_CLOCKS {
            extra.push(BB_NCONFIG);
            extra.push(BB_NCONFIG | BB_DCLK);
        }
        self.write_raw(&extra)?;
        self.ftdi.set_bitmode(0, BitMode::Reset)?;
        Ok(())
    }

    /// After a power-on configuration the FPGA was seen to ignore
    /// commands for more than a second, while a reconfiguration answers
    /// right away. Poll with short timeouts, resyncing in between.
    fn wait_link(&mut self) -> Result<()> {
        let deadline = Instant::now() + LINK_TIMEOUT;
        loop {
            if self.reg_read_timeout(REG_INIT_COUNTER, 1, PROBE_TIMEOUT).is_ok() {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(Error::Timeout);
            }
            log::debug!("FPGA not answering yet, resyncing.");
            std::thread::sleep(Duration::from_millis(100));
            let _ = self.resync(true, 1);
        }
    }

    /// After configuration, the FPGA runs an init sequence of about
    /// 250 ms during which a counter increments. Acquisitions don't
    /// start before it has finished. Wait for the counter to settle, its
    /// final value varies. Stability is counted in reads, not time, so a
    /// replayed recording takes the same path as the live run.
    fn wait_init_done(&mut self) -> Result<()> {
        const STABLE_READS: u32 = 10;
        let start = Instant::now();
        let mut last: Option<u8> = None;
        let mut stable = 0;
        loop {
            let count = self.reg_read_u8(REG_INIT_COUNTER).map_err(|e| {
                log::debug!("Failed to read the FPGA init counter: {e}.");
                e
            })?;
            if last == Some(count) {
                stable += 1;
            } else {
                last = Some(count);
                stable = 0;
            }
            if count != 0 && stable >= STABLE_READS {
                log::debug!("FPGA init done after {} ms.", start.elapsed().as_millis());
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(3) {
                log::error!("FPGA init did not finish (counter {count}).");
                return Err(Error::Timeout);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Loads `image` into the FPGA unless it is loaded already.
    pub fn configure_fpga(&mut self, image: Image) -> Result<()> {
        if self.fpga_image == Some(image) {
            return Ok(());
        }
        let stream = firmware::load_image(image)?;
        log::info!("Configuring FPGA with image '{}'.", image.name());
        self.threshold_dac = None;
        let mut result = Err(Error::device("FPGA configuration failed"));
        let mut step = "none";
        for attempt in 0..CONFIG_RETRIES {
            if attempt > 0 {
                log::debug!("Configuration attempt {attempt} failed ({step}).");
                std::thread::sleep(Duration::from_millis(500));
                // The image may have loaded, only the link failed.
                if self.resync(true, 1).is_ok()
                    && self.probe_image().is_ok()
                    && self.fpga_image == Some(image)
                    && self.wait_init_done().is_ok()
                {
                    result = Ok(());
                    break;
                }
            }
            // A configured FPGA ignores nCONFIG until it is told to
            // release its configuration. This stalls the FIFO, so it has
            // to be followed by bit-bang configuration right away. An
            // unconfigured FPGA won't ack, which is fine.
            if self.fpga_image.is_some() && self.reg_write_u8(REG_CONFIG_CTRL, 0).is_err() {
                log::debug!("No ack for 'enable configure'.");
            }
            self.fpga_image = None;
            step = "bit-bang";
            result = self.bitbang_configure(&stream);
            if result.is_err() {
                continue;
            }
            // A close/reopen is needed before the resync here.
            step = "resync";
            result = self.resync(true, RESYNC_RETRIES);
            if result.is_err() {
                continue;
            }
            step = "link";
            result = self.wait_link();
            if result.is_err() {
                continue;
            }
            step = "init";
            result = self.wait_init_done();
            if result.is_err() {
                continue;
            }
            step = "image check";
            result = match self.probe_image() {
                Ok(i) if i == image => Ok(()),
                Ok(i) => Err(Error::device(format!("image {} loaded instead", i.index()))),
                Err(e) => Err(e),
            };
            if result.is_ok() {
                break;
            }
        }
        if result.is_err() {
            log::error!("Failed to configure the FPGA.");
        }
        result
    }

    /// Writes all acquisition settings.
    pub fn program(&mut self, p: &Program) -> Result<()> {
        let regs = program_regs(p, self.threshold_cal);
        let mut dac = None;
        for (addr, width, value) in regs {
            if addr == REG_THRESHOLD {
                dac = Some(value as u16);
            }
            self.reg_write(addr, &value.to_le_bytes()[..width])?;
        }
        if dac.is_some() && dac != self.threshold_dac {
            log::debug!("Waiting for the threshold DAC ({}) to settle.", dac.unwrap_or(0));
            std::thread::sleep(THRESHOLD_SETTLE);
            self.threshold_dac = dac;
        }
        Ok(())
    }

    /// Arms the capture.
    pub fn start(&mut self) -> Result<()> {
        self.reg_write_u8(REG_RUN, 1)?;
        self.reg_write_u8(REG_ARM, 1)
    }

    /// Stops the capture.
    pub fn halt(&mut self) -> Result<()> {
        self.reg_write_u8(REG_RUN, 0)
    }

    /// The status register.
    pub fn status(&mut self) -> Result<u8> {
        self.reg_read_u8(REG_STATUS)
    }

    /// Ends the pre-fill phase of a compressed capture.
    pub fn end_prefill(&mut self) -> Result<()> {
        self.reg_write_u8(REG_END_PREFILL, 1)
    }

    /// Ends the post-fill phase of a compressed capture.
    pub fn end_postfill(&mut self) -> Result<()> {
        self.reg_write_u8(REG_END_POSTFILL, 1)
    }

    fn read_lanes(&mut self, lanes: u32, start: usize, count: usize) -> Result<Rows> {
        let mut rows = Rows::default();
        for k in 0..4 {
            rows.lanes[k] =
                self.reg_read(lanes + k as u32 * MEM_LANE_STRIDE + start as u32, count)?;
        }
        rows.lanes[4] = self.reg_read(MEM_FLAGS + start as u32, count)?;
        Ok(rows)
    }

    fn unfilled(&mut self, status: u8) -> Result<Option<u16>> {
        if status & STATUS_WRAPPED != 0 {
            return Ok(None);
        }
        Ok(Some(self.reg_read_u16(REG_UNFILLED)?))
    }

    /// Reads the sample memory after an SDR capture. The memory is a
    /// ring; the trigger position counts entries after it.
    pub fn read_sdr(&mut self, status: u8) -> Result<SdrCapture> {
        let wp = self.reg_read_u16(REG_WRITE_PTR)?;
        let unfilled = self.unfilled(status)?;
        let (mut start, mut n) = sdr_window(wp, status & STATUS_WRAPPED != 0, unfilled);
        // Drop a leading count record, it has no data to repeat.
        let flags = self.reg_read_u8(MEM_FLAGS + start as u32)?;
        if flags & FLAG_COUNT != 0 && n > 1 {
            start = (start + 1) % MEM_DEPTH;
            n -= 1;
        }
        let rows = self.read_lanes(MEM_LANES_A, start, n)?;
        let trigger_pos = self.reg_read_u16(REG_TRIGGER_POS)?;
        log::debug!("SDR readout: wp {wp}, start {start}, n {n}, trigger {trigger_pos}.");
        Ok(SdrCapture { rows, trigger_pos })
    }

    /// Reads the sample memory after a 500 MHz capture.
    pub fn read_ddr(&mut self, status: u8) -> Result<DdrCapture> {
        let wp_a = self.reg_read_u16(REG_WRITE_PTR)?;
        let wp_b = self.reg_read_u16(REG_WRITE_PTR_B)?;
        let unfilled = self.unfilled(status)?;
        let (start_a, start_b, n) = ddr_window(wp_a, wp_b, status & STATUS_WRAPPED != 0, unfilled);
        let rows = n.div_ceil(2);
        let bank_a = self.read_lanes(MEM_LANES_A, start_a, rows)?;
        let bank_b = self.read_lanes(MEM_LANES_B, start_b, rows)?;
        let trigger_pos = self.reg_read_u16(REG_TRIGGER_POS)?;
        log::debug!("DDR readout: wp {wp_a}/{wp_b}, n {n}, trigger {trigger_pos}.");
        Ok(DdrCapture { bank_a, bank_b, n, trigger_pos })
    }
}

/// Finds the threshold calibration string in an EEPROM image.
pub fn parse_calibration(buf: &[u8]) -> Option<i32> {
    const PNP: [u8; 4] = [0x02, 0x03, 0x01, 0x00];
    if buf.len() < 0x14 {
        return None;
    }
    // Serial number descriptor: byte offset (bit 7 set) and length.
    let mut pos = usize::from(buf[0x12] & 0x7f) + usize::from(buf[0x13]);
    if buf.get(pos..pos + 4) == Some(&PNP) {
        pos += 4;
    }
    let s = buf.get(pos..pos + 4)?;
    if !(s[0] == b'+' || s[0] == b'-') || !s[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(s).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dac_codes() {
        assert_eq!(threshold_dac(1.5, 0), 578);
        assert_eq!(threshold_dac(1.5, 3), 581);
        assert_eq!(threshold_dac(-6.0, 0), 53);
        assert_eq!(threshold_dac(6.0, 0), 893);
        assert_eq!(threshold_dac(100.0, 0), 1023);
    }

    #[test]
    fn buffer_splits() {
        assert_eq!(buffer_split(50, false), (1032, 1016));
        assert_eq!(buffer_split(0, false), (8, 2040));
        assert_eq!(buffer_split(100, false), (2056, 0));
        assert_eq!(half_even(5), 2);
        assert_eq!(half_even(7), 4);
        assert_eq!(buffer_split(50, true), (530, 496));
    }

    #[test]
    fn calibration_parsing() {
        let mut eeprom = vec![0u8; 128];
        eeprom[0x12] = 0x80 | 0x66;
        eeprom[0x13] = 0x12;
        let pos = 0x66 + 0x12;
        eeprom[pos..pos + 4].copy_from_slice(&[0x02, 0x03, 0x01, 0x00]);
        eeprom[pos + 4..pos + 8].copy_from_slice(b"-004");
        assert_eq!(parse_calibration(&eeprom), Some(-4));
        eeprom[pos + 4] = b'x';
        assert_eq!(parse_calibration(&eeprom), None);
    }

    #[test]
    fn program_order_and_values() {
        let mut level = TriggerLevel { pattern_mask: 1, pattern_value: 1, ..Default::default() };
        level.finish();
        let p = Program {
            rate: *find_rate(1_000_000).unwrap(),
            external_clock: false,
            clock_source: 0,
            clock_edge: 0,
            rle: true,
            compression_mask: 0x3_ffff_ffff,
            capture_ratio: 50,
            threshold: 1.5,
            trigger: [level, TriggerLevel::default()],
            trigger_combine: 1,
        };
        let regs = program_regs(&p, 0);
        assert_eq!(regs[0], (REG_PRESCALER, 1, 0x31));
        assert_eq!(regs[1], (REG_STEP, 1, 0x00));
        assert_eq!(regs[2], (REG_CLOCK_MODE, 1, 0x14));
        assert_eq!(regs[6], (REG_COMPRESSION, 1, 1));
        assert_eq!(regs[10], (REG_TRIGGER_LEVEL_A, 5, 1));
        assert_eq!(regs[18], (REG_TRIGGER_LEVEL_A + TRIG_CONDITION, 1, 1));
        assert_eq!(regs[20], (REG_TRIGGER_LEVEL_A + TRIG_COUNTER_MODE, 1, 1));
        let n = regs.len();
        assert_eq!(regs[n - 4], (REG_TRIGGER_COMBINE, 1, 1));
        assert_eq!(regs[n - 3], (REG_THRESHOLD, 2, 578));
        assert_eq!(regs[n - 2], (REG_PRE_DEPTH, 2, 1032));
        assert_eq!(regs[n - 1], (REG_POST_DEPTH, 2, 1016));
        assert_eq!(p.image(), Image::TimingSdr);
        assert!(!regs.iter().any(|r| r.0 == REG_CONFIG_CTRL && r.2 != 0));
    }
}
