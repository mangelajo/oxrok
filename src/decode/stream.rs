//! The sample stream a decoder reads through `wait`.
//!
//! The writer side pushes chunks of logic samples and ends the stream;
//! the reader side scans them for conditions. Consumed chunks are
//! dropped, so memory stays bounded by what the writer is ahead.

use crate::core::{Error, Result};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

/// What a condition asks of a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CondKind {
    /// Level 0.
    Low,
    /// Level 1.
    High,
    /// 0 then 1.
    Rising,
    /// 1 then 0.
    Falling,
    /// Any change.
    Edge,
    /// No change.
    Stable,
}

/// One condition: a channel and what it must do, or a sample count to
/// skip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    /// `channel` must meet `kind`.
    Pin(usize, CondKind),
    /// Match after this many samples.
    Skip(u64),
}

impl Cond {
    /// A level or edge condition.
    pub fn pin(channel: usize, kind: CondKind) -> Cond {
        Cond::Pin(channel, kind)
    }
}

/// Pin levels at a sample: 0, 1, or 0xff for an unassigned channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pins(pub Vec<u8>);

impl Pins {
    /// The level of a channel as bool, false when unassigned.
    pub fn get(&self, channel: usize) -> bool {
        self.0.get(channel).is_some_and(|&v| v == 1)
    }
}

struct Shared {
    chunks: VecDeque<Vec<u8>>,
    ended: bool,
}

/// The writer side of a [`Stream`].
#[derive(Clone)]
pub struct StreamWriter {
    shared: Arc<(Mutex<Shared>, Condvar)>,
}

impl StreamWriter {
    /// Appends samples.
    pub fn push(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let (m, cv) = &*self.shared;
        m.lock().expect("stream lock").chunks.push_back(data.to_vec());
        cv.notify_all();
    }

    /// Ends the stream.
    pub fn end(&self) {
        let (m, cv) = &*self.shared;
        m.lock().expect("stream lock").ended = true;
        cv.notify_all();
    }
}

/// The reader side: the current position and the scanner.
pub struct Stream {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    unit: usize,
    /// Decoder channel to sample bit.
    map: Vec<Option<usize>>,
    /// The chunk being read and the sample offset of its start.
    cur: Vec<u8>,
    cur_base: u64,
    /// Current sample number; the sample at `samplenum` has been
    /// examined once `started`.
    samplenum: u64,
    started: bool,
    /// The previous sample, for edges across chunks.
    prev: Vec<u8>,
}

impl Stream {
    /// A stream of `unit` byte samples; `map` assigns decoder channels
    /// to logic channel indices.
    pub fn new(unit: usize, map: Vec<Option<usize>>) -> (Stream, StreamWriter) {
        let shared = Arc::new((
            Mutex::new(Shared { chunks: VecDeque::new(), ended: false }),
            Condvar::new(),
        ));
        let writer = StreamWriter { shared: Arc::clone(&shared) };
        let stream = Stream {
            shared,
            unit: unit.max(1),
            map,
            cur: Vec::new(),
            cur_base: 0,
            samplenum: 0,
            started: false,
            prev: Vec::new(),
        };
        (stream, writer)
    }

    /// The current sample number.
    pub fn samplenum(&self) -> u64 {
        self.samplenum
    }

    /// True when the decoder channel is assigned.
    pub fn has_channel(&self, channel: usize) -> bool {
        self.map.get(channel).copied().flatten().is_some()
    }

    /// Makes sure the sample `n` is in the current chunk; false at EOF.
    fn load(&mut self, n: u64) -> bool {
        loop {
            let count = (self.cur.len() / self.unit) as u64;
            if n >= self.cur_base && n < self.cur_base + count {
                return true;
            }
            let (m, cv) = &*self.shared;
            let mut g = m.lock().expect("stream lock");
            loop {
                if let Some(chunk) = g.chunks.pop_front() {
                    drop(g);
                    if !self.cur.is_empty() {
                        let last = self.cur.len() - self.unit;
                        self.prev = self.cur[last..].to_vec();
                    }
                    self.cur_base += count;
                    self.cur = chunk;
                    break;
                }
                if g.ended {
                    return false;
                }
                g = cv.wait(g).expect("stream lock");
            }
        }
    }

    fn sample(&self, n: u64) -> &[u8] {
        let off = ((n - self.cur_base) as usize) * self.unit;
        &self.cur[off..off + self.unit]
    }

    fn level(&self, sample: &[u8], channel: usize) -> Option<bool> {
        let bit = self.map.get(channel).copied().flatten()?;
        sample.get(bit / 8).map(|b| (b >> (bit % 8)) & 1 == 1)
    }

    /// The pins at the current sample.
    fn pins(&self) -> Pins {
        let s = self.sample(self.samplenum);
        Pins((0..self.map.len()).map(|c| self.level(s, c).map_or(0xff, u8::from)).collect())
    }

    /// Scans for the conditions. Returns the pins and which groups
    /// matched, or `Eof`.
    pub fn wait(&mut self, conds: &[&[Cond]]) -> Result<(Pins, Vec<bool>)> {
        let targets: Vec<Option<u64>> = conds
            .iter()
            .map(|g| {
                g.iter().find_map(|c| match c {
                    Cond::Skip(n) => {
                        Some(if self.started { self.samplenum + n } else { n.saturating_sub(1) })
                    }
                    Cond::Pin(..) => None,
                })
            })
            .collect();
        if self.started {
            self.samplenum += 1;
        } else {
            self.started = true;
        }
        loop {
            if !self.load(self.samplenum) {
                return Err(Error::Eof);
            }
            if conds.is_empty() {
                return Ok((self.pins(), Vec::new()));
            }
            let n = self.samplenum;
            let s = self.sample(n);
            let prev: Option<&[u8]> = if n > self.cur_base {
                Some(self.sample(n - 1))
            } else if n > 0 && !self.prev.is_empty() {
                Some(&self.prev)
            } else {
                None
            };
            let mut matched = vec![false; conds.len()];
            let mut any = false;
            for (gi, g) in conds.iter().enumerate() {
                let ok = g.iter().all(|c| match *c {
                    Cond::Skip(_) => targets[gi].is_some_and(|t| n >= t),
                    Cond::Pin(ch, kind) => {
                        let now = self.level(s, ch).unwrap_or(false);
                        // Before the first sample the previous level is
                        // taken to be the same, as libsigrokdecode does.
                        let before = prev.and_then(|p| self.level(p, ch)).unwrap_or(now);
                        match kind {
                            CondKind::Low => !now,
                            CondKind::High => now,
                            CondKind::Rising => !before && now,
                            CondKind::Falling => before && !now,
                            CondKind::Edge => before != now,
                            CondKind::Stable => before == now,
                        }
                    }
                });
                matched[gi] = ok;
                any |= ok;
            }
            if any {
                return Ok((self.pins(), matched));
            }
            self.samplenum += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_skips_and_eof_across_chunks() {
        let (mut s, w) = Stream::new(1, vec![Some(0), None]);
        w.push(&[0, 0, 1]);
        w.push(&[1, 0, 0, 1]);
        w.end();
        let (pins, m) = s.wait(&[&[Cond::pin(0, CondKind::Rising)]]).unwrap();
        assert_eq!((s.samplenum(), pins.get(0), m), (2, true, vec![true]));
        assert_eq!(pins.0[1], 0xff);
        // Falling across the chunk boundary at sample 4.
        let (_, m) = s.wait(&[&[Cond::pin(0, CondKind::Falling)], &[Cond::Skip(10)]]).unwrap();
        assert_eq!((s.samplenum(), m), (4, vec![true, false]));
        // Skip lands exactly two samples later.
        s.wait(&[&[Cond::Skip(2)]]).unwrap();
        assert_eq!(s.samplenum(), 6);
        assert!(matches!(s.wait(&[&[Cond::Skip(1)]]), Err(Error::Eof)));
    }

    #[test]
    fn first_sample_is_examined_in_place() {
        let (mut s, w) = Stream::new(1, vec![Some(0)]);
        w.push(&[1, 1]);
        w.end();
        // No edge is possible on the first sample; a level matches it.
        let (_, m) = s.wait(&[&[Cond::pin(0, CondKind::High)]]).unwrap();
        assert_eq!((s.samplenum(), m), (0, vec![true]));
        assert!(s.wait(&[&[Cond::pin(0, CondKind::Edge)]]).is_err());
    }
}
