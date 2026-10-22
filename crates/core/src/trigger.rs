// Counterpart in sigrok:
//   libsigrok/src/trigger.c,
//   Copyright (C) 2014 Bert Vermeulen <bert@biot.com>,
//   with commits by Uwe Hermann, Daniel Elstner, Soeren Apel
//   (libsigrok libsigrok-unreleased-1855-g20baa77f).
// This file is an independent implementation written for oxrok from the
//   observed behaviour and the public interfaces.

//! Trigger definitions.
//!
//! The sigrok model (stages of level and edge conditions, fired in
//! order) plus what hardware triggers commonly add: an occurrence count
//! per stage, a duration, a value range over a group of channels, and
//! how two stages combine. A device says which of these it supports
//! with a [`TriggerFeatures`], and [`Trigger::check`] validates a
//! trigger against it. The command line grammar is in
//! [`crate::trigger_syntax`].

use crate::error::{Error, Result};

/// What a channel must do for a trigger stage to match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TriggerMatch {
    /// The level is low.
    Zero,
    /// The level is high.
    One,
    /// A low to high transition.
    Rising,
    /// A high to low transition.
    Falling,
    /// Any transition.
    Edge,
    /// An analog value above the threshold.
    Over,
    /// An analog value below the threshold.
    Under,
}

impl TriggerMatch {
    /// The one letter code used on the command line (`-t D0=r`).
    pub fn code(self) -> char {
        match self {
            TriggerMatch::Zero => '0',
            TriggerMatch::One => '1',
            TriggerMatch::Rising => 'r',
            TriggerMatch::Falling => 'f',
            TriggerMatch::Edge => 'e',
            TriggerMatch::Over => 'o',
            TriggerMatch::Under => 'u',
        }
    }

    /// The match for a one letter code.
    pub fn from_code(c: char) -> Option<Self> {
        Some(match c {
            '0' => TriggerMatch::Zero,
            '1' => TriggerMatch::One,
            'r' => TriggerMatch::Rising,
            'f' => TriggerMatch::Falling,
            'e' => TriggerMatch::Edge,
            'o' => TriggerMatch::Over,
            'u' => TriggerMatch::Under,
            _ => return None,
        })
    }

    /// True for the transition matches.
    pub fn is_edge(self) -> bool {
        matches!(self, TriggerMatch::Rising | TriggerMatch::Falling | TriggerMatch::Edge)
    }
}

/// One condition of a trigger stage.
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerCondition {
    /// The channel index, as the device numbers it.
    pub channel: usize,
    /// What the channel must do.
    pub matches: TriggerMatch,
    /// The threshold for analog matches, in the channel's unit.
    pub value: f32,
}

/// Fire on the Nth event of a stage instead of the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerCount {
    /// How many events, 1 or more.
    pub count: u32,
    /// Count only events in a row: an edge on one of the stage's edge
    /// channels while the stage's levels do not hold resets the count.
    pub consecutive: bool,
}

/// Fire when the stage's condition has held for a while.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerDuration {
    /// Held for more than this many samples: the trigger point is the
    /// sample that many after the condition became true.
    Longer(u64),
}

/// A value read from a group of channels, within or outside a range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggerRange {
    /// The channels, least significant first.
    pub channels: Vec<usize>,
    /// The lower bound, inclusive.
    pub low: u64,
    /// The upper bound, inclusive.
    pub high: u64,
    /// True to match inside the range, false outside.
    pub inside: bool,
}

/// How the stages of a two stage trigger combine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TriggerCombine {
    /// Each stage after the previous one.
    #[default]
    Sequence,
    /// Either stage.
    Or,
    /// Both stages, in any order.
    And,
}

/// One stage of a trigger: every condition must hold at the same sample.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriggerStage {
    /// The conditions.
    pub conditions: Vec<TriggerCondition>,
    /// Fire on the Nth event instead of the first.
    pub count: Option<TriggerCount>,
    /// Fire after the condition held for a while.
    pub duration: Option<TriggerDuration>,
    /// A value term on a group of channels.
    pub range: Option<TriggerRange>,
}

impl TriggerStage {
    /// True when some condition is an edge.
    pub fn has_edge(&self) -> bool {
        self.conditions.iter().any(|c| c.matches.is_edge())
    }

    /// True when some condition is a level.
    pub fn has_level(&self) -> bool {
        self.conditions.iter().any(|c| !c.matches.is_edge())
    }
}

/// A trigger: the stages fire in order, each after the previous one,
/// unless `combine` says otherwise.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trigger {
    /// The stages, in order.
    pub stages: Vec<TriggerStage>,
    /// How two stages combine.
    pub combine: TriggerCombine,
}

/// What a device's trigger can do beyond level and edge stages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct TriggerFeatures {
    /// Occurrence counts.
    pub count: bool,
    /// Consecutive counting.
    pub consecutive: bool,
    /// Durations.
    pub duration: bool,
    /// Value ranges.
    pub range: bool,
    /// Two stages combined with or.
    pub or: bool,
    /// Two stages combined with and.
    pub and: bool,
    /// The most stages, 0 for any number.
    pub max_stages: usize,
}

impl Trigger {
    /// A trigger with no stages.
    pub fn new() -> Self {
        Trigger::default()
    }

    fn stage_mut(&mut self, stage: usize) -> &mut TriggerStage {
        while self.stages.len() <= stage {
            self.stages.push(TriggerStage::default());
        }
        &mut self.stages[stage]
    }

    /// Adds a condition to stage `stage`, creating stages as needed.
    pub fn add(&mut self, stage: usize, channel: usize, matches: TriggerMatch) {
        self.stage_mut(stage).conditions.push(TriggerCondition { channel, matches, value: 0.0 });
    }

    /// Sets the occurrence count of a stage.
    pub fn set_count(&mut self, stage: usize, count: TriggerCount) {
        self.stage_mut(stage).count = Some(count);
    }

    /// Sets the duration of a stage.
    pub fn set_duration(&mut self, stage: usize, duration: TriggerDuration) {
        self.stage_mut(stage).duration = Some(duration);
    }

    /// Sets the value range of a stage.
    pub fn set_range(&mut self, stage: usize, range: TriggerRange) {
        self.stage_mut(stage).range = Some(range);
    }

    /// True when every stage has a condition or a range.
    pub fn is_valid(&self) -> bool {
        !self.stages.is_empty()
            && self.stages.iter().all(|s| !s.conditions.is_empty() || s.range.is_some())
    }

    /// Checks the trigger against what a device supports and against
    /// its own rules, with an error message for the user.
    pub fn check(&self, f: &TriggerFeatures) -> Result<()> {
        if !self.is_valid() {
            return Err(Error::arg("Invalid trigger."));
        }
        if f.max_stages > 0 && self.stages.len() > f.max_stages {
            return Err(Error::Arg(format!("Only {} trigger stages are supported.", f.max_stages)));
        }
        match self.combine {
            TriggerCombine::Sequence => {}
            TriggerCombine::Or if !f.or => {
                return Err(Error::arg(
                    "Trigger stages combined with or are not supported by device.",
                ));
            }
            TriggerCombine::And if !f.and => {
                return Err(Error::arg(
                    "Trigger stages combined with and are not supported by device.",
                ));
            }
            _ if self.stages.len() != 2 => {
                return Err(Error::arg("Or and and combine exactly two trigger stages."));
            }
            _ => {}
        }
        for (i, s) in self.stages.iter().enumerate() {
            let n = i + 1;
            if let Some(c) = &s.count {
                if !f.count {
                    return Err(Error::arg("Trigger counts not supported by device."));
                }
                if c.count == 0 {
                    return Err(Error::arg("Invalid trigger count '0'."));
                }
                if c.consecutive && !f.consecutive {
                    return Err(Error::arg("Consecutive trigger counts not supported by device."));
                }
                if c.consecutive && !s.has_edge() {
                    return Err(Error::Arg(format!(
                        "Consecutive counting needs an edge condition in trigger stage {n}."
                    )));
                }
            }
            if let Some(d) = &s.duration {
                if !f.duration {
                    return Err(Error::arg("Trigger durations not supported by device."));
                }
                let TriggerDuration::Longer(len) = d;
                if *len == 0 {
                    return Err(Error::arg("Invalid trigger duration '0'."));
                }
                if s.has_edge() {
                    return Err(Error::Arg(format!(
                        "A duration needs level conditions, not edges, in trigger stage {n}."
                    )));
                }
                if !s.has_level() && s.range.is_none() {
                    return Err(Error::Arg(format!(
                        "A duration needs a condition in trigger stage {n}."
                    )));
                }
                if s.count.is_some() {
                    return Err(Error::Arg(format!(
                        "Trigger stage {n} has both a count and a duration."
                    )));
                }
            }
            if let Some(r) = &s.range {
                if !f.range {
                    return Err(Error::arg("Trigger value ranges not supported by device."));
                }
                let w = r.channels.len();
                if w == 0 || w > 63 {
                    return Err(Error::Arg(format!("Invalid channel range in trigger stage {n}.")));
                }
                let mut sorted = r.channels.clone();
                sorted.sort_unstable();
                sorted.dedup();
                if sorted.len() != w {
                    return Err(Error::Arg(format!("Repeated channel in trigger stage {n}.")));
                }
                let max = (1u64 << w) - 1;
                if r.low > r.high {
                    return Err(Error::Arg(format!(
                        "Trigger value range {}..{} is reversed.",
                        r.low, r.high
                    )));
                }
                if r.high > max {
                    return Err(Error::Arg(format!(
                        "Trigger value {} does not fit {w} channels.",
                        r.high
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple() -> Trigger {
        let mut t = Trigger::new();
        t.add(0, 0, TriggerMatch::Rising);
        t
    }

    #[test]
    fn checks_against_features() {
        let none = TriggerFeatures::default();
        let all = TriggerFeatures {
            count: true,
            consecutive: true,
            duration: true,
            range: true,
            or: true,
            and: true,
            max_stages: 2,
        };
        assert!(simple().check(&none).is_ok());
        assert!(Trigger::new().check(&all).is_err());
        let mut t = simple();
        t.set_count(0, TriggerCount { count: 5, consecutive: true });
        assert!(t.check(&none).unwrap_err().to_string().contains("counts not supported"));
        assert!(t.check(&all).is_ok());
        let mut t = Trigger::new();
        t.add(0, 1, TriggerMatch::One);
        t.set_count(0, TriggerCount { count: 2, consecutive: true });
        assert!(t.check(&all).unwrap_err().to_string().contains("needs an edge"));
        t.stages[0].count = None;
        t.set_duration(0, TriggerDuration::Longer(100));
        assert!(t.check(&all).is_ok());
        t.add(0, 2, TriggerMatch::Falling);
        assert!(t.check(&all).unwrap_err().to_string().contains("not edges"));
        let mut t = Trigger::new();
        t.set_range(0, TriggerRange { channels: vec![0, 1, 2], low: 3, high: 9, inside: true });
        assert!(t.check(&all).unwrap_err().to_string().contains("does not fit 3 channels"));
        t.stages[0].range.as_mut().unwrap().high = 5;
        assert!(t.check(&all).is_ok());
        assert!(t.check(&none).unwrap_err().to_string().contains("value ranges"));
        let mut t = simple();
        t.add(1, 1, TriggerMatch::Rising);
        t.combine = TriggerCombine::Or;
        assert!(t.check(&all).is_ok());
        assert!(t.check(&none).unwrap_err().to_string().contains("with or"));
        t.add(2, 2, TriggerMatch::Rising);
        assert!(t.check(&all).unwrap_err().to_string().contains("Only 2"));
    }
}
