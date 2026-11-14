//! Difficulty modes, under the V2 rules only.
//!
//! A mode changes what a new run starts with, and nothing after that: the levers are applied
//! once, to the run's starting supplies, before the first turn. The turn rules, the event
//! draws and the order of RNG draws are the same in every mode. The Legacy rules have no modes
//! and are untouched.
//!
//! The lever values were chosen by measurement (`etv_simulate`, seeds 0 to 999, all five
//! scripts); the table is in the pull request that added this file. They are whole-number
//! percentages and the arithmetic is integer-only.

use serde::{Deserialize, Serialize};

use crate::model::RunState;

/// The save key a run's mode is written under (version 5 saves).
pub const DIFFICULTY_KEY: &str = "difficulty";

/// The three modes. `Normal` is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Hardcore,
    #[default]
    Normal,
    Easy,
}

/// What a mode changes: the food and the water a run starts with, as a percentage of the
/// standard amount (100 leaves it as it is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifficultyLevers {
    pub start_food_pct: i32,
    pub start_water_pct: i32,
}

impl DifficultyLevers {
    /// No change: the plain V2 start.
    pub const NONE: DifficultyLevers = DifficultyLevers {
        start_food_pct: 100,
        start_water_pct: 100,
    };
}

impl Default for DifficultyLevers {
    fn default() -> Self {
        Self::NONE
    }
}

impl Difficulty {
    /// Every mode, hardest first.
    pub const ALL: [Difficulty; 3] = [Difficulty::Hardcore, Difficulty::Normal, Difficulty::Easy];

    /// The name as it is typed and saved: `hardcore`, `normal`, `easy`.
    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Hardcore => "hardcore",
            Difficulty::Normal => "normal",
            Difficulty::Easy => "easy",
        }
    }

    /// The name as the player sees it.
    pub fn title(self) -> &'static str {
        match self {
            Difficulty::Hardcore => "Hardcore",
            Difficulty::Normal => "Normal",
            Difficulty::Easy => "Easy",
        }
    }

    /// One plain line saying what the mode does, for the screen.
    pub fn description(self) -> &'static str {
        match self {
            Difficulty::Hardcore => HARDCORE_DESCRIPTION,
            Difficulty::Normal => NORMAL_DESCRIPTION,
            Difficulty::Easy => EASY_DESCRIPTION,
        }
    }

    /// Read a mode from what the player typed (any case).
    pub fn parse(text: &str) -> Option<Difficulty> {
        match text.trim().to_ascii_lowercase().as_str() {
            "hardcore" => Some(Difficulty::Hardcore),
            "normal" => Some(Difficulty::Normal),
            "easy" => Some(Difficulty::Easy),
            _ => None,
        }
    }

    /// The mode's levers.
    pub fn levers(self) -> DifficultyLevers {
        match self {
            Difficulty::Hardcore => HARDCORE_LEVERS,
            Difficulty::Normal => NORMAL_LEVERS,
            Difficulty::Easy => EASY_LEVERS,
        }
    }
}

/// Hardcore: less food at the start.
pub const HARDCORE_LEVERS: DifficultyLevers = DifficultyLevers {
    start_food_pct: 100,
    start_water_pct: 100,
};

/// Normal: the V2 rules with one small lever.
pub const NORMAL_LEVERS: DifficultyLevers = DifficultyLevers {
    start_food_pct: 100,
    start_water_pct: 100,
};

/// Easy: more food at the start.
pub const EASY_LEVERS: DifficultyLevers = DifficultyLevers {
    start_food_pct: 100,
    start_water_pct: 100,
};

pub const HARDCORE_DESCRIPTION: &str = "The wagon sets out short of food.";
pub const NORMAL_DESCRIPTION: &str = "The standard journey.";
pub const EASY_DESCRIPTION: &str = "The wagon sets out with extra food.";

/// `amount` scaled to `pct` percent, rounded down, never below 1 for a supply the run starts
/// with. Integer arithmetic only.
pub fn scale_start_amount(amount: i32, pct: i32) -> i32 {
    if amount <= 0 {
        return amount;
    }
    let scaled = i64::from(amount) * i64::from(pct.max(0)) / 100;
    i32::try_from(scaled).unwrap_or(i32::MAX).max(1)
}

/// Apply levers to a run that has not started: its food and water are scaled. Call it once,
/// on a new run, before the first turn. It draws nothing from the RNG.
pub fn apply_starting_levers(state: &mut RunState, levers: DifficultyLevers) {
    let food = state.supplies.get("food");
    state
        .supplies
        .set("food", scale_start_amount(food, levers.start_food_pct));
    let water = state.supplies.get("water");
    state
        .supplies
        .set("water", scale_start_amount(water, levers.start_water_pct));
}
