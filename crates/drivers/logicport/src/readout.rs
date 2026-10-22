// Counterpart in sigrok:
//   libsigrok/src/hardware/intronix-logicport/protocol.c (the readout),
//   Copyright (C) 2026 Miguel Angel Ajo Pelayo <miguelangel@ajo.es>
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is a port by the same author, relicensed.

//! Decoding the sample memory into samples.
//!
//! Sample memory has 2048 entries of 40 bits: four lanes holding D0-D31
//! and a flags lane holding CLK1/CLK2. With compression, an entry with
//! the count flag set is a count record instead: the lanes and the low
//! flag bits form a 35 bit number of sample periods without a
//! transition. At 500 MHz two banks of 1024 rows hold the even and odd
//! samples.

/// Entries in the sample memory.
pub const MEM_DEPTH: usize = 2048;
/// Bytes per sample in the data feed: 34 channels.
pub const UNIT_SIZE: usize = 5;
/// Channel index of CLK1.
pub const CH_CLK1: usize = 32;
/// Channel index of CLK2.
pub const CH_CLK2: usize = 33;

const FLAG_CLK1: u8 = 1 << 1;
const FLAG_CLK2: u8 = 1 << 2;
const FLAG_COUNT: u8 = 1 << 3;
const FLAG_DDR_B_SHIFT: u32 = 4;
/// Trigger pipeline delays, in samples.
const TRIGGER_DELAY_SDR: u64 = 7;
const TRIGGER_DELAY_DDR: i64 = 21;

/// Raw rows read from the memory: four data lanes and the flags lane.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rows {
    /// `lanes[k][row]` for k in 0..4, then the flags.
    pub lanes: [Vec<u8>; 5],
}

impl Rows {
    /// Rows from one 40 bit value per entry (bits 0..32 data, 32..40
    /// flags), for tests and fixtures.
    pub fn from_values(values: &[u64]) -> Rows {
        let mut r = Rows::default();
        for &v in values {
            for (k, lane) in r.lanes.iter_mut().enumerate() {
                lane.push((v >> (8 * k)) as u8);
            }
        }
        r
    }

    fn len(&self) -> usize {
        self.lanes[4].len()
    }

    fn data_record(&self, row: usize, clk_shift: u32) -> u64 {
        let mut value = u64::from(self.lanes[0][row])
            | u64::from(self.lanes[1][row]) << 8
            | u64::from(self.lanes[2][row]) << 16
            | u64::from(self.lanes[3][row]) << 24;
        let flags = self.lanes[4][row] >> clk_shift;
        if flags & FLAG_CLK1 != 0 {
            value |= 1 << CH_CLK1;
        }
        if flags & FLAG_CLK2 != 0 {
            value |= 1 << CH_CLK2;
        }
        value
    }

    fn is_count(&self, row: usize) -> bool {
        self.lanes[4][row] & FLAG_COUNT != 0
    }

    fn count_record(&self, row: usize) -> u64 {
        u64::from(self.lanes[4][row] & 0x07) << 32
            | u64::from(self.lanes[3][row]) << 24
            | u64::from(self.lanes[2][row]) << 16
            | u64::from(self.lanes[1][row]) << 8
            | u64::from(self.lanes[0][row])
    }

    fn total_samples(&self) -> u64 {
        let n = self.len();
        (1..n).filter(|&i| self.is_count(i)).map(|i| self.count_record(i)).sum::<u64>() + n as u64
    }
}

/// Where decoded samples go.
pub trait SampleOut {
    /// `count` repetitions of `sample`.
    fn samples(&mut self, sample: u64, count: u64);
    /// The trigger point: the next sample is the first after it.
    fn trigger(&mut self);
}

/// Applies the sample limit and the capture ratio around the trigger,
/// then feeds samples out.
pub struct Sink<'a> {
    out: &'a mut dyn SampleOut,
    skip: u64,
    /// Samples sent so far.
    pub sent: u64,
    limit: u64,
    ratio: u64,
    trigger: Option<u64>,
}

impl<'a> Sink<'a> {
    /// A sink with `limit` samples (0: none) and the pre-trigger
    /// `ratio` in percent.
    pub fn new(out: &'a mut dyn SampleOut, limit: u64, ratio: u64) -> Self {
        Sink { out, skip: 0, sent: 0, limit, ratio, trigger: None }
    }

    /// With a sample limit below what was captured, keeps the window
    /// around the trigger such that the capture ratio is honoured.
    fn set_trigger(&mut self, trigger: Option<u64>, available: u64) {
        self.trigger = trigger;
        let Some(t) = trigger else { return };
        if self.limit == 0 || available <= self.limit {
            return;
        }
        let pre = self.limit * self.ratio / 100;
        let skip = t.saturating_sub(pre).min(available - self.limit);
        self.skip = skip;
        self.trigger = Some(t - skip);
    }

    fn put(&mut self, value: u64, mut count: u64) {
        let chunk = count.min(self.skip);
        self.skip -= chunk;
        count -= chunk;
        while count > 0 && (self.limit == 0 || self.sent < self.limit) {
            if self.trigger == Some(self.sent) {
                self.out.trigger();
                self.trigger = None;
            }
            let mut chunk = count;
            if let Some(t) = self.trigger {
                chunk = chunk.min(t - self.sent);
            }
            if self.limit > 0 {
                chunk = chunk.min(self.limit - self.sent);
            }
            self.out.samples(value, chunk);
            self.sent += chunk;
            count -= chunk;
        }
    }
}

/// A capture read in SDR mode (up to 250 MHz, and state mode).
#[derive(Clone, Debug)]
pub struct SdrCapture {
    /// The rows in time order, starting at the oldest.
    pub rows: Rows,
    /// The trigger position register.
    pub trigger_pos: u16,
}

impl SdrCapture {
    /// Decodes the capture into `sink`. `triggered` says whether a
    /// trigger was armed, else the trigger position is meaningless.
    pub fn decode(&self, sink: &mut Sink<'_>, triggered: bool) {
        let rows = &self.rows;
        let n = rows.len();
        let trigger_pos = usize::from(self.trigger_pos).min(MEM_DEPTH - 1);
        // When the trigger fires, the FPGA writes a record (with
        // compression the only data record without a transition)
        // TRIGGER_DELAY_SDR samples after the match. The trigger position
        // points just past that record.
        let mut trigger_idx = (n + 1 - trigger_pos.min(n + 1)) % (n + 1);
        trigger_idx = if trigger_idx > 0 { trigger_idx.min(n) - 1 } else { 0 };
        let mut ts = 0u64;
        for i in 1..=trigger_idx {
            ts += 1;
            if rows.is_count(i) {
                ts += rows.count_record(i);
            }
        }
        let trigger_ts = ts.saturating_sub(TRIGGER_DELAY_SDR);
        sink.set_trigger(triggered.then_some(trigger_ts), rows.total_samples());

        let mut prev = 0u64;
        let mut prev_ts = 0u64;
        for i in 0..n {
            let ts;
            if rows.is_count(i) {
                // Repeat the last value, including this entry's slot.
                ts = prev_ts + 1 + rows.count_record(i);
                sink.put(prev, ts - prev_ts);
            } else {
                let value = rows.data_record(i, 0);
                ts = if i > 0 { prev_ts + 1 } else { 0 };
                sink.put(value, 1);
                prev = value;
            }
            prev_ts = ts;
        }
    }
}

/// A capture read in 500 MHz DDR mode: bank A holds the even samples,
/// bank B the odd ones.
#[derive(Clone, Debug)]
pub struct DdrCapture {
    /// Bank A rows, oldest first.
    pub bank_a: Rows,
    /// Bank B rows, oldest first.
    pub bank_b: Rows,
    /// Number of samples to decode.
    pub n: usize,
    /// The trigger position register.
    pub trigger_pos: u16,
}

impl DdrCapture {
    /// Decodes the capture into `sink`.
    pub fn decode(&self, sink: &mut Sink<'_>, triggered: bool) {
        let n = self.n as i64;
        // Empirical pipeline delay, accurate to about two samples.
        let mut trigger_idx = (n + 1 - i64::from(self.trigger_pos)) % (n + 1);
        if trigger_idx < 0 {
            trigger_idx += n + 1;
        }
        let trigger_idx = (trigger_idx - TRIGGER_DELAY_DDR).clamp(0, n - 1);
        sink.set_trigger(triggered.then_some(trigger_idx as u64), self.n as u64);
        for i in 0..self.n {
            let value = if i & 1 == 1 {
                self.bank_b.data_record(i / 2, FLAG_DDR_B_SHIFT)
            } else {
                self.bank_a.data_record(i / 2, 0)
            };
            sink.put(value, 1);
        }
    }
}

/// The memory window to read in SDR mode, from the write pointer, the
/// wrapped flag and the unfilled count: start row and row count.
pub fn sdr_window(write_ptr: u16, wrapped: bool, unfilled: Option<u16>) -> (usize, usize) {
    let wp = usize::from(write_ptr).min(MEM_DEPTH - 1);
    let x = match (wrapped, unfilled) {
        (false, Some(u)) => (usize::from(u) + 1).min(MEM_DEPTH - 1),
        _ => 0,
    };
    ((wp + 1 + x) % MEM_DEPTH, MEM_DEPTH - x)
}

/// The window in DDR mode: start rows of bank A and B, and the sample
/// count.
pub fn ddr_window(
    write_ptr_a: u16,
    write_ptr_b: u16,
    wrapped: bool,
    unfilled: Option<u16>,
) -> (usize, usize, usize) {
    let bank = MEM_DEPTH / 2;
    let wp_a = usize::from(write_ptr_a) % bank;
    let wp_b = usize::from(write_ptr_b) % bank;
    let mut x = 0;
    if let (false, Some(u)) = (wrapped, unfilled) {
        let u = usize::from(u);
        if u + 1 > bank {
            x = 2 * (u + 1 - bank);
        }
    }
    let n = if x < MEM_DEPTH - 2 { MEM_DEPTH - 1 - x } else { 1 };
    ((wp_a + 1 + x) % bank, (wp_b + 1 + x) % bank, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Collect {
        samples: Vec<u64>,
        trigger: Option<usize>,
    }

    impl SampleOut for Collect {
        fn samples(&mut self, sample: u64, count: u64) {
            self.samples.extend(std::iter::repeat_n(sample, count as usize));
        }

        fn trigger(&mut self) {
            self.trigger = Some(self.samples.len());
        }
    }

    const COUNT: u64 = (FLAG_COUNT as u64) << 32;

    #[test]
    fn sdr_expands_count_records() {
        // value 1, then 3 more periods of it, then value 2 with CLK1.
        let rows = Rows::from_values(&[1, COUNT | 2, 2 | (u64::from(FLAG_CLK1) << 32)]);
        let cap = SdrCapture { rows, trigger_pos: 0 };
        let mut c = Collect::default();
        let mut sink = Sink::new(&mut c, 0, 50);
        cap.decode(&mut sink, false);
        assert_eq!(c.samples, [1, 1, 1, 1, 2 | 1 << 32]);
        assert_eq!(c.trigger, None);
    }

    #[test]
    fn sdr_trigger_and_limit_window() {
        let values: Vec<u64> = (0..100).collect();
        let cap = SdrCapture { rows: Rows::from_values(&values), trigger_pos: 30 };
        // trigger_idx = (101 - 30) % 101 = 71 -> 70; ts 70 - 7 = 63.
        let mut c = Collect::default();
        let mut sink = Sink::new(&mut c, 0, 50);
        cap.decode(&mut sink, true);
        assert_eq!(c.samples.len(), 100);
        assert_eq!(c.trigger, Some(63));
        // A 20 sample limit at 25 % keeps 5 samples before the trigger.
        let mut c = Collect::default();
        let mut sink = Sink::new(&mut c, 20, 25);
        cap.decode(&mut sink, true);
        assert_eq!(c.samples.len(), 20);
        assert_eq!(c.trigger, Some(5));
        assert_eq!(c.samples[5], 63);
    }

    #[test]
    fn ddr_interleaves_banks() {
        let a = Rows::from_values(&[0x10, 0x30, 0x50]);
        let b = Rows::from_values(&[
            0x20,
            0x40 | (u64::from(FLAG_CLK1) << (32 + FLAG_DDR_B_SHIFT)),
            0x60,
        ]);
        let cap = DdrCapture { bank_a: a, bank_b: b, n: 5, trigger_pos: 0 };
        let mut c = Collect::default();
        let mut sink = Sink::new(&mut c, 0, 50);
        cap.decode(&mut sink, true);
        assert_eq!(c.samples, [0x10, 0x20, 0x30, 0x40 | 1 << 32, 0x50]);
        // trigger_idx = (6 - 0) % 6 = 0 -> clamp(0 - 21) = 0.
        assert_eq!(c.trigger, Some(0));
    }

    #[test]
    fn windows() {
        assert_eq!(sdr_window(100, true, None), (101, MEM_DEPTH));
        assert_eq!(sdr_window(100, false, Some(9)), (111, MEM_DEPTH - 10));
        assert_eq!(sdr_window(2047, true, None), (0, MEM_DEPTH));
        assert_eq!(ddr_window(10, 10, true, None), (11, 11, MEM_DEPTH - 1));
        assert_eq!(ddr_window(10, 10, false, Some(1024 + 4)), (21, 21, MEM_DEPTH - 11));
    }
}
