//! Trigger definitions.

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

/// One stage of a trigger: every condition must hold at the same sample.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriggerStage {
    /// The conditions.
    pub conditions: Vec<TriggerCondition>,
}

/// A trigger: the stages fire in order, each after the previous one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trigger {
    /// The stages, in order.
    pub stages: Vec<TriggerStage>,
}

impl Trigger {
    /// A trigger with no stages.
    pub fn new() -> Self {
        Trigger::default()
    }

    /// Adds a condition to stage `stage`, creating stages as needed.
    pub fn add(&mut self, stage: usize, channel: usize, matches: TriggerMatch) {
        while self.stages.len() <= stage {
            self.stages.push(TriggerStage::default());
        }
        self.stages[stage].conditions.push(TriggerCondition { channel, matches, value: 0.0 });
    }

    /// True when every stage has at least one condition.
    pub fn is_valid(&self) -> bool {
        !self.stages.is_empty() && self.stages.iter().all(|s| !s.conditions.is_empty())
    }
}
