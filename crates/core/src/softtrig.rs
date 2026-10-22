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
//! fires on the first sample where all of its conditions hold, or on the
//! Nth with a count, or once they held for a duration. Then the next
//! stage is armed on the samples that follow. The trigger point is the
//! sample that completed the last stage. Value ranges and the or/and
//! combines are not implemented here; [`SoftTrigger::FEATURES`] says so
//! and [`Trigger::check`] rejects them.

use crate::error::Result;
use crate::trigger::{Trigger, TriggerDuration, TriggerFeatures, TriggerMatch};

/// One stage, flattened.
#[derive(Clone, Debug)]
struct Stage {
    conds: Vec<(usize, TriggerMatch)>,
    count: u32,
    consecutive: bool,
    /// Samples the condition must hold, 0 for none.
    duration: u64,
    /// Events seen so far.
    hits: u32,
    /// Samples in a row the condition held.
    run: u64,
    /// Whether the condition held on the previous sample.
    held: bool,
}

/// The matcher state.
#[derive(Clone, Debug)]
pub struct SoftTrigger {
    stages: Vec<Stage>,
    unit_size: usize,
    stage: usize,
    prev: Option<Vec<u8>>,
}

impl SoftTrigger {
    /// What this matcher supports.
    pub const FEATURES: TriggerFeatures = TriggerFeatures {
        count: true,
        consecutive: true,
        duration: true,
        range: false,
        or: false,
        and: false,
        max_stages: 0,
    };

    /// Checks `trigger` against [`SoftTrigger::FEATURES`], for a driver's
    /// `set_trigger`.
    pub fn check(trigger: &Trigger) -> Result<()> {
        trigger.check(&Self::FEATURES)
    }

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
            .map(|s| Stage {
                conds: s.conditions.iter().map(|c| (c.channel, c.matches)).collect(),
                count: s.count.map_or(1, |c| c.count.max(1)),
                consecutive: s.count.is_some_and(|c| c.consecutive),
                duration: match s.duration {
                    Some(TriggerDuration::Longer(n)) => n,
                    None => 0,
                },
                hits: 0,
                run: 0,
                held: false,
            })
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

    fn matches_one(&self, sample: &[u8], ch: usize, m: TriggerMatch) -> bool {
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
    }

    fn matches(&self, sample: &[u8]) -> bool {
        self.stages[self.stage].conds.iter().all(|&(ch, m)| self.matches_one(sample, ch, m))
    }

    /// Whether the stage's edge conditions all match, and whether its
    /// level conditions all hold, on this sample.
    fn edges_and_levels(&self, sample: &[u8]) -> (bool, bool) {
        let conds = &self.stages[self.stage].conds;
        let edges = conds
            .iter()
            .filter(|(_, m)| m.is_edge())
            .all(|&(ch, m)| self.matches_one(sample, ch, m));
        let levels = conds
            .iter()
            .filter(|(_, m)| !m.is_edge())
            .all(|&(ch, m)| self.matches_one(sample, ch, m));
        (edges, levels)
    }

    /// Advances the current stage by one sample; true when it fired.
    fn step(&mut self, sample: &[u8]) -> bool {
        let matched = self.matches(sample);
        let idx = self.stage;
        let (duration, consecutive, count, held, run) = {
            let s = &self.stages[idx];
            (s.duration, s.consecutive, s.count, s.held, s.run)
        };
        let has_edge = self.stages[idx].conds.iter().any(|(_, m)| m.is_edge());
        if duration > 0 {
            let run = if matched { run + 1 } else { 0 };
            self.stages[idx].run = run;
            return run == duration + 1;
        }
        // A level-only stage counts the times it becomes true. A
        // consecutive count starts over when the counted edge comes
        // while the levels do not hold.
        let event = if has_edge { matched } else { matched && !held };
        let (edges, levels) = self.edges_and_levels(sample);
        let reset = consecutive && has_edge && edges && !levels;
        let st = &mut self.stages[idx];
        st.held = matched;
        if reset {
            st.hits = 0;
        }
        if event {
            st.hits += 1;
            if st.hits >= count {
                return true;
            }
        }
        false
    }

    /// Runs the samples in `data` through the matcher. Returns the
    /// index of the sample that completed the trigger, if it did here.
    /// After that, calls return `None`.
    pub fn feed(&mut self, data: &[u8]) -> Option<usize> {
        if self.fired() {
            return None;
        }
        for (i, sample) in data.chunks_exact(self.unit_size).enumerate() {
            if self.step(sample) {
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
    use crate::trigger::TriggerCount;

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
    fn counts_and_consecutive_counts() {
        // Third rising edge of D0: pulses at 1, 3, 5.
        let mut t = trig(&[&[(0, TriggerMatch::Rising)]]);
        t.set_count(0, TriggerCount { count: 3, consecutive: false });
        let mut s = SoftTrigger::new(&t, 1).unwrap();
        assert_eq!(s.feed(&[0, 1, 0, 1, 0]), None);
        assert_eq!(s.feed(&[1, 0]), Some(0));
        // Third rising edge of D0 while D1 is high; an edge with D1 low
        // resets a consecutive count but not a cumulative one.
        let data = [0b10, 0b11, 0b10, 0b11, 0b00, 0b01, 0b10, 0b11, 0b10, 0b11, 0b10, 0b11];
        let mut t = trig(&[&[(0, TriggerMatch::Rising), (1, TriggerMatch::One)]]);
        t.set_count(0, TriggerCount { count: 3, consecutive: false });
        assert_eq!(SoftTrigger::new(&t, 1).unwrap().feed(&data), Some(7));
        t.set_count(0, TriggerCount { count: 3, consecutive: true });
        assert_eq!(SoftTrigger::new(&t, 1).unwrap().feed(&data), Some(11));
        // A level-only stage counts the times the level is entered.
        let mut t = trig(&[&[(0, TriggerMatch::One)]]);
        t.set_count(0, TriggerCount { count: 2, consecutive: false });
        assert_eq!(SoftTrigger::new(&t, 1).unwrap().feed(&[1, 1, 0, 1, 1]), Some(3));
    }

    #[test]
    fn durations_fire_after_the_hold_time() {
        let mut t = trig(&[&[(0, TriggerMatch::One)]]);
        t.set_duration(0, TriggerDuration::Longer(3));
        let mut s = SoftTrigger::new(&t, 1).unwrap();
        // High for two samples is not enough; the long pulse starts at 4
        // and fires three samples later, across a chunk boundary.
        assert_eq!(s.feed(&[1, 1, 0, 0, 1, 1]), None);
        assert_eq!(s.feed(&[1, 1, 1]), Some(1));
        assert!(SoftTrigger::check(&t).is_ok());
        let mut r = Trigger::new();
        r.set_range(
            0,
            crate::trigger::TriggerRange { channels: vec![0, 1], low: 1, high: 2, inside: true },
        );
        assert!(SoftTrigger::check(&r).is_err());
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
