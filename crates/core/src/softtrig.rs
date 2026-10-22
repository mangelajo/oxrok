// Counterpart in sigrok:
//   libsigrok/src/soft-trigger.c,
//   Copyright (C) 2014 Bert Vermeulen <bert@biot.com>,
//   with commits by Uwe Hermann, Daniel Elstner, Aurelien Jacobs,
//   Soeren Apel, luftek
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is an independent implementation written for oxrok from the
//   observed behaviour and the public interfaces.

//! A software trigger for streaming devices: matches a [`Trigger`]
//! against logic samples on the host, one stage after another.
//!
//! Level conditions (`0`, `1`) must all hold on one sample; edge
//! conditions (`r`, `f`, `e`) compare with the previous sample. A stage
//! fires on the first sample where all of its conditions hold, then the
//! next stage is armed on the samples that follow. The trigger point is
//! the sample that completed the last stage.

use crate::trigger::{Trigger, TriggerMatch};

/// The matcher state.
#[derive(Clone, Debug)]
pub struct SoftTrigger {
    stages: Vec<Vec<(usize, TriggerMatch)>>,
    unit_size: usize,
    stage: usize,
    prev: Option<Vec<u8>>,
}

impl SoftTrigger {
    /// A matcher for `trigger` over samples of `unit_size` bytes.
    /// Conditions on channels beyond the sample width never match.
    /// Returns `None` for a trigger without stages.
    pub fn new(trigger: &Trigger, unit_size: usize) -> Option<Self> {
        if trigger.stages.is_empty() {
            return None;
        }
        let stages = trigger
            .stages
            .iter()
            .map(|s| s.conditions.iter().map(|c| (c.channel, c.matches)).collect())
            .collect();
        Some(SoftTrigger { stages, unit_size, stage: 0, prev: None })
    }

    /// Bytes per sample.
    pub fn unit_size(&self) -> usize {
        self.unit_size
    }

    /// True once every stage has fired.
    pub fn fired(&self) -> bool {
        self.stage >= self.stages.len()
    }

    fn bit(sample: &[u8], channel: usize) -> Option<bool> {
        sample.get(channel / 8).map(|b| (b >> (channel % 8)) & 1 != 0)
    }

    fn matches(&self, sample: &[u8]) -> bool {
        self.stages[self.stage].iter().all(|&(ch, m)| {
            let Some(now) = Self::bit(sample, ch) else { return false };
            let before = self.prev.as_ref().and_then(|p| Self::bit(p, ch));
            match m {
                TriggerMatch::Zero => !now,
                TriggerMatch::One => now,
                TriggerMatch::Rising => before == Some(false) && now,
                TriggerMatch::Falling => before == Some(true) && !now,
                TriggerMatch::Edge => before.is_some_and(|b| b != now),
                TriggerMatch::Over | TriggerMatch::Under => false,
            }
        })
    }

    /// Runs the samples in `data` through the matcher. Returns the
    /// index of the sample that completed the trigger, if it did here.
    /// After that, calls return `None`.
    pub fn feed(&mut self, data: &[u8]) -> Option<usize> {
        if self.fired() {
            return None;
        }
        for (i, sample) in data.chunks_exact(self.unit_size).enumerate() {
            if self.matches(sample) {
                self.stage += 1;
                if self.fired() {
                    self.prev = None;
                    return Some(i);
                }
            }
            self.prev = Some(sample.to_vec());
        }
        None
    }
}

/// Keeps the last `limit` samples seen before the trigger.
#[derive(Clone, Debug)]
pub struct PreTriggerBuffer {
    unit_size: usize,
    limit: usize,
    data: Vec<u8>,
}

impl PreTriggerBuffer {
    /// A buffer for `limit` samples of `unit_size` bytes.
    pub fn new(unit_size: usize, limit: usize) -> Self {
        PreTriggerBuffer { unit_size, limit, data: Vec::new() }
    }

    /// Appends `data`, dropping the oldest samples beyond the limit.
    pub fn push(&mut self, data: &[u8]) {
        let max = self.limit * self.unit_size;
        if data.len() >= max {
            self.data.clear();
            self.data.extend_from_slice(&data[data.len() - max..]);
            return;
        }
        self.data.extend_from_slice(data);
        if self.data.len() > max {
            let excess = self.data.len() - max;
            self.data.drain(..excess);
        }
    }

    /// Takes the buffered samples.
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trig(stages: &[&[(usize, TriggerMatch)]]) -> Trigger {
        let mut t = Trigger::new();
        for (i, s) in stages.iter().enumerate() {
            for &(ch, m) in *s {
                t.add(i, ch, m);
            }
        }
        t
    }

    #[test]
    fn levels_and_edges() {
        let t = trig(&[&[(0, TriggerMatch::Rising), (1, TriggerMatch::One)]]);
        let mut s = SoftTrigger::new(&t, 1).unwrap();
        // D0 rises at index 1 with D1 low, at index 4 with D1 high.
        assert_eq!(s.feed(&[0b00, 0b01, 0b00, 0b10, 0b11, 0b11]), Some(4));
        assert!(s.fired());
        assert_eq!(s.feed(&[0, 1]), None);
    }

    #[test]
    fn edges_across_chunks_and_stages() {
        let t = trig(&[&[(0, TriggerMatch::Rising)], &[(1, TriggerMatch::Falling)]]);
        let mut s = SoftTrigger::new(&t, 2).unwrap();
        assert_eq!(s.feed(&[0, 0]), None);
        // D0 rises on the first sample of this chunk, D1 falls two later.
        assert_eq!(s.feed(&[1, 0, 3, 0, 1, 0]), Some(2));
        let t = trig(&[&[(0, TriggerMatch::Edge)]]);
        let mut s = SoftTrigger::new(&t, 1).unwrap();
        // The first sample has no predecessor, so it cannot be an edge.
        assert_eq!(s.feed(&[1, 1, 0]), Some(2));
        let t = trig(&[&[(9, TriggerMatch::One)]]);
        let mut s = SoftTrigger::new(&t, 1).unwrap();
        assert_eq!(s.feed(&[0xff; 4]), None);
        assert!(SoftTrigger::new(&Trigger::new(), 1).is_none());
    }

    #[test]
    fn pre_trigger_buffer_keeps_the_tail() {
        let mut b = PreTriggerBuffer::new(2, 3);
        b.push(&[1, 1, 2, 2]);
        b.push(&[3, 3, 4, 4]);
        assert_eq!(b.take(), [2, 2, 3, 3, 4, 4]);
        b.push(&[0; 20]);
        assert_eq!(b.take().len(), 6);
    }
}
