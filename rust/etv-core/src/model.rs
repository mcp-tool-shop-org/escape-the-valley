//! Core game state models, resource catalog, player intents, and backpack data (`models.py`,
//! `resources.py`, `intent.py`, `backpack_models.py`).
//!
//! Serde field names and shapes match `.trail/run.json` directly with one deliberate flattened
//! exception: `SuppliesState` wraps `items: BTreeMap<String, i32>` with `#[serde(transparent)]`
//! because Python's `save.py:335` writes `"supplies": state.supplies.to_dict()` (a flat JSON
//! object) rather than `{"items": ...}`.

use crate::rng::RngState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

// ── Enums (models.py) ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Pace {
    Slow,
    #[default]
    Steady,
    Hard,
}

impl Pace {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Slow => "slow",
            Self::Steady => "steady",
            Self::Hard => "hard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GMProfile {
    Chronicler,
    #[default]
    Fireside,
    Lantern,
}

impl GMProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Chronicler => "chronicler",
            Self::Fireside => "fireside",
            Self::Lantern => "lantern",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Biome {
    Plains,
    Forest,
    Desert,
    Swamp,
    Alpine,
}

impl Biome {
    pub const ALL: [Self; 5] = [
        Self::Plains,
        Self::Forest,
        Self::Desert,
        Self::Swamp,
        Self::Alpine,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Plains => "plains",
            Self::Forest => "forest",
            Self::Desert => "desert",
            Self::Swamp => "swamp",
            Self::Alpine => "alpine",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weather {
    Clear,
    Overcast,
    Rain,
    Storm,
    Fog,
    Snow,
    Hot,
}

impl Weather {
    pub const ALL: [Self; 7] = [
        Self::Clear,
        Self::Overcast,
        Self::Rain,
        Self::Storm,
        Self::Fog,
        Self::Snow,
        Self::Hot,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Overcast => "overcast",
            Self::Rain => "rain",
            Self::Storm => "storm",
            Self::Fog => "fog",
            Self::Snow => "snow",
            Self::Hot => "hot",
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum TimeOfDay {
    #[default]
    Morning,
    Afternoon,
    Evening,
    Night,
}

impl TimeOfDay {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Morning => "morning",
            Self::Afternoon => "afternoon",
            Self::Evening => "evening",
            Self::Night => "night",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    #[default]
    Healthy,
    Sick,
    Injured,
    Exhausted,
}

impl Condition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Sick => "sick",
            Self::Injured => "injured",
            Self::Exhausted => "exhausted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trait {
    Tough,
    Healer,
    Tracker,
    Mechanic,
    Lucky,
    Anxious,
    Steady,
    SharpEye,
}

impl Trait {
    pub const ALL: [Self; 8] = [
        Self::Tough,
        Self::Healer,
        Self::Tracker,
        Self::Mechanic,
        Self::Lucky,
        Self::Anxious,
        Self::Steady,
        Self::SharpEye,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tough => "tough",
            Self::Healer => "healer",
            Self::Tracker => "tracker",
            Self::Mechanic => "mechanic",
            Self::Lucky => "lucky",
            Self::Anxious => "anxious",
            Self::Steady => "steady",
            Self::SharpEye => "sharp_eye",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TwistModifier {
    BanditYear,
    SickSeason,
    FloodYear,
    EarlyWinter,
    GoodHunting,
}

impl TwistModifier {
    pub const ALL: [Self; 5] = [
        Self::BanditYear,
        Self::SickSeason,
        Self::FloodYear,
        Self::EarlyWinter,
        Self::GoodHunting,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BanditYear => "bandit_year",
            Self::SickSeason => "sick_season",
            Self::FloodYear => "flood_year",
            Self::EarlyWinter => "early_winter",
            Self::GoodHunting => "good_hunting",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Doctrine {
    TravelLight,
    CarefulHands,
    NoDebts,
}

impl Doctrine {
    pub const ALL: [Self; 3] = [Self::TravelLight, Self::CarefulHands, Self::NoDebts];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TravelLight => "travel_light",
            Self::CarefulHands => "careful_hands",
            Self::NoDebts => "no_debts",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Taboo {
    NeverNight,
    NeverRiver,
    LeaveNothing,
}

impl Taboo {
    pub const ALL: [Self; 3] = [Self::NeverNight, Self::NeverRiver, Self::LeaveNothing];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NeverNight => "never_night",
            Self::NeverRiver => "never_river",
            Self::LeaveNothing => "leave_nothing",
        }
    }
}

// ── Doctrine mechanical modifiers (models.py:91-107) ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DoctrineModifiers {
    pub consumption_mult: Option<f64>,
    pub breakdown_bonus: Option<f64>,
    pub hunt_bonus: Option<f64>,
    pub repair_mult: Option<f64>,
    pub distance_penalty: Option<i32>,
    pub maintenance_bonus: Option<i32>,
    pub morale_floor: Option<i32>,
    pub capacity_mult: Option<f64>,
    pub trade_bonus: Option<f64>,
}

pub fn doctrine_modifiers(doctrine: &str) -> DoctrineModifiers {
    match doctrine {
        "travel_light" => DoctrineModifiers {
            consumption_mult: Some(0.80),
            breakdown_bonus: Some(0.05),
            hunt_bonus: Some(0.10),
            repair_mult: None,
            distance_penalty: None,
            maintenance_bonus: None,
            morale_floor: None,
            capacity_mult: None,
            trade_bonus: None,
        },
        "careful_hands" => DoctrineModifiers {
            consumption_mult: None,
            breakdown_bonus: None,
            hunt_bonus: None,
            repair_mult: Some(1.5),
            distance_penalty: Some(1),
            maintenance_bonus: Some(1),
            morale_floor: None,
            capacity_mult: None,
            trade_bonus: None,
        },
        "no_debts" => DoctrineModifiers {
            consumption_mult: None,
            breakdown_bonus: None,
            hunt_bonus: None,
            repair_mult: None,
            distance_penalty: None,
            maintenance_bonus: None,
            morale_floor: Some(20),
            capacity_mult: Some(0.85),
            trade_bonus: Some(0.15),
        },
        _ => DoctrineModifiers {
            consumption_mult: None,
            breakdown_bonus: None,
            hunt_bonus: None,
            repair_mult: None,
            distance_penalty: None,
            maintenance_bonus: None,
            morale_floor: None,
            capacity_mult: None,
            trade_bonus: None,
        },
    }
}

// ── Resources (resources.py) ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceCategory {
    Consumable,
    Gear,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceDef {
    pub key: &'static str,
    pub display: &'static str,
    pub category: ResourceCategory,
    pub default: i32,
    pub max_stack: i32,
    pub daily_decay: f64,
    pub warning_low: i32,
    pub description: &'static str,
}

pub const RESOURCE_CATALOG: [ResourceDef; 12] = [
    ResourceDef {
        key: "food",
        display: "FOOD",
        category: ResourceCategory::Consumable,
        default: 50,
        max_stack: 200,
        daily_decay: 0.0,
        warning_low: 10,
        description: "Daily rations. 2 per person per day, modified by pace.",
    },
    ResourceDef {
        key: "water",
        display: "WATR",
        category: ResourceCategory::Consumable,
        default: 50,
        max_stack: 200,
        daily_decay: 0.0,
        warning_low: 10,
        description: "Water supply. 2 per person per day. Desert increases need.",
    },
    ResourceDef {
        key: "firewood",
        display: "FIRE",
        category: ResourceCategory::Consumable,
        default: 15,
        max_stack: 50,
        daily_decay: 0.0,
        warning_low: 3,
        description: "Camp fuel. 1 per night. Cold weather uses more.",
    },
    ResourceDef {
        key: "meds",
        display: "MEDS",
        category: ResourceCategory::Consumable,
        default: 5,
        max_stack: 20,
        daily_decay: 0.0,
        warning_low: 1,
        description: "Medicine. Treats illness and injury.",
    },
    ResourceDef {
        key: "salt",
        display: "SALT",
        category: ResourceCategory::Consumable,
        default: 8,
        max_stack: 20,
        daily_decay: 0.0,
        warning_low: 2,
        description: "Preserves food. Without it, spoilage events increase.",
    },
    ResourceDef {
        key: "ammo",
        display: "AMMO",
        category: ResourceCategory::Consumable,
        default: 20,
        max_stack: 100,
        daily_decay: 0.0,
        warning_low: 5,
        description: "Ammunition. Required for hunting and defense events.",
    },
    ResourceDef {
        key: "parts",
        display: "PART",
        category: ResourceCategory::Gear,
        default: 4,
        max_stack: 10,
        daily_decay: 0.0,
        warning_low: 1,
        description: "Wagon parts. Required for repairs.",
    },
    ResourceDef {
        key: "rope",
        display: "ROPE",
        category: ResourceCategory::Gear,
        default: 2,
        max_stack: 5,
        daily_decay: 0.0,
        warning_low: 0,
        description: "Rope. Helps with river crossings and rescue events.",
    },
    ResourceDef {
        key: "tools",
        display: "TOOL",
        category: ResourceCategory::Gear,
        default: 1,
        max_stack: 3,
        daily_decay: 0.0,
        warning_low: 0,
        description: "Hand tools. Improves repair success.",
    },
    ResourceDef {
        key: "lantern_oil",
        display: "LAMP",
        category: ResourceCategory::Consumable,
        default: 6,
        max_stack: 20,
        daily_decay: 0.0,
        warning_low: 2,
        description: "Lantern fuel. 1 per night travel. Without it, danger increases.",
    },
    ResourceDef {
        key: "cloth",
        display: "CLTH",
        category: ResourceCategory::Consumable,
        default: 5,
        max_stack: 20,
        daily_decay: 0.0,
        warning_low: 1,
        description: "Fabric for repairs and trade. Patches boots, wagon cover, harness.",
    },
    ResourceDef {
        key: "boots",
        display: "BOOT",
        category: ResourceCategory::Gear,
        default: 2,
        max_stack: 5,
        daily_decay: 0.0,
        warning_low: 1,
        description: "Footwear. Worn out by rough terrain. Repair with cloth.",
    },
];

pub const LEGACY_KEYS: [&str; 5] = ["food", "water", "meds", "ammo", "parts"];

pub fn is_valid_resource_key(key: &str) -> bool {
    RESOURCE_CATALOG.iter().any(|r| r.key == key)
}

pub fn default_supplies_map() -> BTreeMap<String, i32> {
    let mut map = BTreeMap::new();
    for rdef in &RESOURCE_CATALOG {
        map.insert(rdef.key.to_string(), rdef.default);
    }
    map
}

// ── Intents & Phases (intent.py) ────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GamePhase {
    #[serde(rename = "camp")]
    Camp,
    #[serde(rename = "event")]
    Event,
    #[serde(rename = "route")]
    Route,
    #[serde(rename = "over")]
    GameOver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntentAction {
    Travel,
    Rest,
    Hunt,
    Repair,
    Choose,
    ChangePace,
    AbandonCargo,
    DesperateRepair,
    HardRation,
    LedgerEnable,
    LedgerDisable,
    LedgerSettle,
    ParcelAccept,
    ParcelRefuse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerIntent {
    pub action: IntentAction,
    #[serde(default)]
    pub choice_id: String,
    #[serde(default)]
    pub pace: String,
}

// ── Party, Wagon, Supplies (models.py) ──────────────────────────────────────

fn default_health_100() -> i32 {
    100
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyMember {
    pub name: String,
    #[serde(default = "default_health_100")]
    pub health: i32,
    #[serde(default)]
    pub condition: Condition,
    #[serde(default)]
    pub traits: Vec<Trait>,
    #[serde(default)]
    pub death_cause: String,
}

impl PartyMember {
    pub fn is_alive(&self) -> bool {
        self.health > 0
    }
}

fn default_morale_70() -> i32 {
    70
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyState {
    #[serde(default)]
    pub members: Vec<PartyMember>,
    #[serde(default = "default_morale_70")]
    pub morale: i32,
}

impl Default for PartyState {
    fn default() -> Self {
        Self {
            members: Vec::new(),
            morale: 70,
        }
    }
}

impl PartyState {
    pub fn alive_count(&self) -> usize {
        self.members.iter().filter(|m| m.is_alive()).count()
    }

    pub fn avg_health(&self) -> i32 {
        let alive: Vec<&PartyMember> = self.members.iter().filter(|m| m.is_alive()).collect();
        if alive.is_empty() {
            return 0;
        }
        let sum: i32 = alive.iter().map(|m| m.health).sum();
        sum / (alive.len() as i32)
    }

    pub fn has_trait(&self, trait_kind: Trait) -> bool {
        self.members
            .iter()
            .any(|m| m.is_alive() && m.traits.contains(&trait_kind))
    }
}

fn default_wagon_condition() -> i32 {
    80
}
fn default_animals_health() -> i32 {
    90
}
fn default_wagon_capacity() -> i32 {
    200
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WagonState {
    #[serde(default = "default_wagon_condition")]
    pub condition: i32,
    #[serde(default = "default_animals_health")]
    pub animals_health: i32,
    #[serde(default = "default_wagon_capacity")]
    pub capacity: i32,
    #[serde(default)]
    pub pace: Pace,
}

impl Default for WagonState {
    fn default() -> Self {
        Self {
            condition: 80,
            animals_health: 90,
            capacity: 200,
            pace: Pace::Steady,
        }
    }
}

/// Dict-backed supply storage matching `models.py::SuppliesState`.
/// Serialized as a flat JSON map `{"food": 50, ...}` to match `.trail/run.json`,
/// and deserialized with catalog filtering and `>= 0` clamping (`save.py::_load_supplies`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(transparent)]
pub struct SuppliesState {
    pub items: BTreeMap<String, i32>,
}

impl<'de> Deserialize<'de> for SuppliesState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let val = Value::deserialize(deserializer)?;
        Ok(crate::save::load_supplies(&val))
    }
}

impl SuppliesState {
    pub fn with_defaults() -> Self {
        Self {
            items: default_supplies_map(),
        }
    }

    pub fn get(&self, key: &str) -> i32 {
        self.items.get(key).copied().unwrap_or(0)
    }

    /// Clamps value to `>= 0` (`models.py:159-160`).
    pub fn set(&mut self, key: &str, val: i32) {
        self.items.insert(key.to_string(), val.max(0));
    }

    pub fn apply_delta(&mut self, delta: &BTreeMap<String, i32>) {
        for (key, val) in delta {
            let current = self.get(key);
            self.items.insert(key.clone(), (current + val).max(0));
        }
    }

    pub fn to_dict(&self) -> BTreeMap<String, i32> {
        self.items.clone()
    }

    pub fn food(&self) -> i32 {
        self.get("food")
    }
    pub fn set_food(&mut self, val: i32) {
        self.set("food", val);
    }
    pub fn water(&self) -> i32 {
        self.get("water")
    }
    pub fn set_water(&mut self, val: i32) {
        self.set("water", val);
    }
    pub fn meds(&self) -> i32 {
        self.get("meds")
    }
    pub fn set_meds(&mut self, val: i32) {
        self.set("meds", val);
    }
    pub fn ammo(&self) -> i32 {
        self.get("ammo")
    }
    pub fn set_ammo(&mut self, val: i32) {
        self.set("ammo", val);
    }
    pub fn parts(&self) -> i32 {
        self.get("parts")
    }
    pub fn set_parts(&mut self, val: i32) {
        self.set("parts", val);
    }
}

fn default_true() -> bool {
    true
}
fn default_temp_15() -> i32 {
    15
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapNode {
    pub node_id: String,
    pub name: String,
    pub biome: Biome,
    pub hazard: i32,
    #[serde(default = "default_true")]
    pub water_available: bool,
    #[serde(default = "default_temp_15")]
    pub temperature: i32,
    #[serde(default)]
    pub is_town: bool,
    #[serde(default)]
    pub connections: Vec<String>,
    #[serde(default)]
    pub distance_to: BTreeMap<String, i32>,
    #[serde(default)]
    pub cache_supplies: Option<BTreeMap<String, i32>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub day: i32,
    pub location: String,
    pub event_id: String,
    #[serde(default)]
    pub scene_title: String,
    #[serde(default)]
    pub narration: String,
    #[serde(default)]
    pub choice_made: String,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub deltas: BTreeMap<String, i32>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndingResult {
    pub tier: String,
    #[serde(default)]
    pub facts: BTreeMap<String, Value>,
    #[serde(default)]
    pub headline: String,
}

fn default_day_1() -> i32 {
    1
}
fn default_salience() -> f64 {
    0.5
}
fn default_engine_source() -> String {
    "engine".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryCard {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_day_1")]
    pub day_created: i32,
    #[serde(default = "default_day_1")]
    pub day_last_seen: i32,
    #[serde(default)]
    pub entities: Vec<String>,
    #[serde(default = "default_salience")]
    pub salience: f64,
    #[serde(default)]
    pub cooldown_until: i32,
    #[serde(default = "default_engine_source")]
    pub source: String,
}

// ── Backpack data models (backpack_models.py) ───────────────────────────────

pub const XRPL_RESOURCES: [&str; 5] = ["ammo", "food", "meds", "parts", "water"];
pub const PARCEL_ACCEPT_CAP: i32 = 20;
pub const MEMO_SCHEMA_VERSION: &str = "v1";
pub const MINTED_SNAPSHOT_PERMIT_ID: &str = "__minted_initial__";

fn default_pending_status() -> String {
    "pending".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementRecord {
    #[serde(default)]
    pub day: i32,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub deltas: BTreeMap<String, i32>,
    #[serde(default)]
    pub txids: Vec<String>,
    #[serde(default = "default_pending_status")]
    pub status: String,
    #[serde(default)]
    pub memo: String,
    #[serde(default)]
    pub timestamp: String,
}

impl Default for SettlementRecord {
    fn default() -> Self {
        Self {
            day: 0,
            location: String::new(),
            deltas: BTreeMap::new(),
            txids: Vec::new(),
            status: "pending".to_string(),
            memo: String::new(),
            timestamp: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ParcelRecord {
    #[serde(default)]
    pub parcel_id: String,
    #[serde(default)]
    pub sender: String,
    #[serde(default)]
    pub contents: BTreeMap<String, i32>,
    #[serde(default)]
    pub txid: String,
    #[serde(default)]
    pub accepted: bool,
    #[serde(default)]
    pub day_received: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SentParcelRecord {
    #[serde(default)]
    pub recipient: String,
    #[serde(default)]
    pub supply: String,
    #[serde(default)]
    pub amount: i32,
    #[serde(default)]
    pub txid: String,
    #[serde(default)]
    pub day_sent: i32,
    #[serde(default)]
    pub memo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PermitRecord {
    #[serde(default)]
    pub permit_id: String,
    #[serde(default)]
    pub txid: String,
    #[serde(default)]
    pub used: bool,
    #[serde(default)]
    pub day_earned: i32,
    #[serde(default)]
    pub day_used: i32,
}

/// Full state for the optional XRPL Ledger Backpack (`backpack_models.py::BackpackState`).
///
/// Note on audit L-2: `minted_initial` and `last_settle_failed` are persisted directly in
/// Rust saves (unless default/empty when comparing against Python saves that lack them).
/// Secrets (`wallet_secret`, `issuer_secret`) are never serialized to `run.json`; they
/// live exclusively in `.trail/secrets.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BackpackState {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub wallet_address: String,
    #[serde(skip)]
    pub wallet_secret: String,
    #[serde(default)]
    pub issuer_address: String,
    #[serde(skip)]
    pub issuer_secret: String,
    #[serde(default)]
    pub trust_lines_ready: bool,
    #[serde(default)]
    pub last_settled_supplies: BTreeMap<String, i32>,
    #[serde(default)]
    pub last_settlement_day: i32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub minted_initial: BTreeMap<String, i32>,
    #[serde(default)]
    pub settlements: Vec<SettlementRecord>,
    #[serde(default)]
    pub pending_settlements: Vec<SettlementRecord>,
    #[serde(default)]
    pub parcels: Vec<ParcelRecord>,
    #[serde(default)]
    pub sent_parcels: Vec<SentParcelRecord>,
    #[serde(default)]
    pub permits: Vec<PermitRecord>,
    #[serde(default)]
    pub nudge_shown: bool,
    #[serde(default)]
    pub nudge_dismissed: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub last_settle_failed: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl BackpackState {
    /// Return the enable-time mint snapshot, hydrating lazily from the `__minted_initial__`
    /// save shim (`backpack_models.py::minted_snapshot_of`).
    ///
    /// Prefers the dedicated `minted_initial` field when all 5 XRPL resources are present
    /// (filtering out any non-XRPL keys). Falls back to the reserved `__minted_initial__`
    /// `PermitRecord`, and sets `self.minted_initial` when the shim contains a complete
    /// 5-resource snapshot. Returns an empty `BTreeMap` when no complete snapshot exists.
    pub fn minted_snapshot_of(&mut self) -> BTreeMap<String, i32> {
        if XRPL_RESOURCES
            .iter()
            .all(|k| self.minted_initial.contains_key(*k))
        {
            let mut out = BTreeMap::new();
            for &k in &XRPL_RESOURCES {
                if let Some(&v) = self.minted_initial.get(k) {
                    out.insert(k.to_string(), v);
                }
            }
            return out;
        }
        for permit in &self.permits {
            if permit.permit_id != MINTED_SNAPSHOT_PERMIT_ID {
                continue;
            }
            let parsed = parse_minted_txid(&permit.txid);
            if XRPL_RESOURCES.iter().all(|k| parsed.contains_key(*k)) {
                let mut complete = BTreeMap::new();
                for &k in &XRPL_RESOURCES {
                    if let Some(&v) = parsed.get(k) {
                        complete.insert(k.to_string(), v);
                    }
                }
                self.minted_initial = complete.clone();
                return complete;
            }
        }
        BTreeMap::new()
    }

    /// Alias for [`BackpackState::minted_snapshot_of`].
    pub fn hydrate_minted_from_permit_shim(&mut self) -> BTreeMap<String, i32> {
        self.minted_snapshot_of()
    }
}

pub fn parse_minted_txid(txid: &str) -> BTreeMap<String, i32> {
    let mut out = BTreeMap::new();
    if txid.is_empty() {
        return out;
    }
    for part in txid.split(',') {
        if let Some((k, raw)) = part.split_once('=') {
            let key = k.trim();
            if XRPL_RESOURCES.contains(&key) {
                if let Ok(val) = raw.trim().parse::<i32>() {
                    out.insert(key.to_string(), val);
                }
            }
        }
    }
    out
}

// ── RunState (models.py) ────────────────────────────────────────────────────

fn default_mode() -> String {
    "offline".to_string()
}
fn default_two() -> i32 {
    2
}
fn default_callout_level() -> String {
    "verbose".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunState {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub seed: i128,
    #[serde(default = "default_day_1")]
    pub day: i32,
    #[serde(default)]
    pub time_of_day: TimeOfDay,
    #[serde(default)]
    pub location_id: String,
    #[serde(default)]
    pub destination_id: String,
    #[serde(default)]
    pub distance_remaining: i32,
    #[serde(default)]
    pub total_distance: i32,
    #[serde(default)]
    pub distance_traveled: i32,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_two")]
    pub weirdness_level: i32,
    #[serde(default = "default_two")]
    pub uncanny_tokens: i32,
    #[serde(default)]
    pub gm_profile: GMProfile,
    #[serde(default)]
    pub twists: Vec<TwistModifier>,
    #[serde(default)]
    pub game_over: bool,
    #[serde(default)]
    pub victory: bool,
    #[serde(default)]
    pub cause_of_death: String,
    #[serde(skip)]
    pub ending: Option<EndingResult>,
    #[serde(default)]
    pub party: PartyState,
    #[serde(default)]
    pub wagon: WagonState,
    #[serde(default)]
    pub supplies: SuppliesState,
    #[serde(default)]
    pub journal: Vec<JournalEntry>,
    #[serde(default)]
    pub map_nodes: Vec<MapNode>,
    #[serde(default)]
    pub rng_counter: u64,
    #[serde(
        default,
        deserialize_with = "crate::rng::deserialize_lenient_rng_state"
    )]
    pub rng_state: Option<RngState>,
    #[serde(default)]
    pub recent_event_tags: Vec<String>,
    #[serde(default)]
    pub memory_cards: Vec<MemoryCard>,
    #[serde(default)]
    pub resource_crises_seen: Vec<String>,
    #[serde(default)]
    pub doctrine: String,
    #[serde(default)]
    pub taboo: String,
    #[serde(default)]
    pub rationing_steps: i32,
    #[serde(default)]
    pub escape_valve_cooldown: i32,
    #[serde(default)]
    pub last_action: String,
    #[serde(default)]
    pub maintained_turns_remaining: i32,
    #[serde(default)]
    pub last_spoilage_day: i32,
    #[serde(default)]
    pub backpack: BackpackState,
    #[serde(default = "default_callout_level")]
    pub callout_level: String,
}

impl Default for RunState {
    fn default() -> Self {
        Self {
            run_id: String::new(),
            seed: 0,
            day: 1,
            time_of_day: TimeOfDay::Morning,
            location_id: String::new(),
            destination_id: String::new(),
            distance_remaining: 0,
            total_distance: 0,
            distance_traveled: 0,
            mode: "offline".to_string(),
            weirdness_level: 2,
            uncanny_tokens: 2,
            gm_profile: GMProfile::Fireside,
            twists: Vec::new(),
            game_over: false,
            victory: false,
            cause_of_death: String::new(),
            ending: None,
            party: PartyState::default(),
            wagon: WagonState::default(),
            supplies: SuppliesState::default(),
            journal: Vec::new(),
            map_nodes: Vec::new(),
            rng_counter: 0,
            rng_state: None,
            recent_event_tags: Vec::new(),
            memory_cards: Vec::new(),
            resource_crises_seen: Vec::new(),
            doctrine: String::new(),
            taboo: String::new(),
            rationing_steps: 0,
            escape_valve_cooldown: 0,
            last_action: String::new(),
            maintained_turns_remaining: 0,
            last_spoilage_day: 0,
            backpack: BackpackState::default(),
            callout_level: "verbose".to_string(),
        }
    }
}

impl RunState {
    /// Compute the deterministic 8-hex seed prefix of `run_id` (`sha256(str(seed))[:8]`).
    pub fn seed_prefix(seed: i128) -> String {
        let digest = Sha256::digest(seed.to_string().as_bytes());
        crate::rng::hex_encode(&digest[..4])
    }

    /// Generate a 1.3.1 salted `run_id`: `<8-hex seed hash>-<8 random hex chars from OS>`,
    /// matching `RunState.generate_run_id` (`models.py:356-358`). Never touches the game RNG.
    pub fn generate_run_id(seed: i128) -> String {
        let prefix = Self::seed_prefix(seed);
        let mut salt = [0u8; 4];
        getrandom::fill(&mut salt).expect("OS CSPRNG available");
        let suffix = crate::rng::hex_encode(&salt);
        format!("{prefix}-{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fails on b888cb8 because BackpackState::minted_snapshot_of did not exist and
    // hydrate_minted_from_permit_shim returned () instead of the snapshot BTreeMap.
    #[test]
    fn test_minted_snapshot_of_lazy_hydration_and_partial_rejection() {
        let mut bp = BackpackState::default();
        assert!(bp.minted_snapshot_of().is_empty());
        assert!(bp.minted_initial.is_empty());

        // Partial (< 5 resources) permit shim does not hydrate
        bp.permits.push(PermitRecord {
            permit_id: MINTED_SNAPSHOT_PERMIT_ID.to_string(),
            txid: "food=50,water=50,meds=5".to_string(),
            used: true,
            day_earned: 1,
            day_used: 0,
        });
        assert!(bp.minted_snapshot_of().is_empty());
        assert!(bp.minted_initial.is_empty());

        // Complete 5-resource permit shim hydrates lazily and populates minted_initial
        bp.permits.clear();
        bp.permits.push(PermitRecord {
            permit_id: MINTED_SNAPSHOT_PERMIT_ID.to_string(),
            txid: "ammo=20,food=50,meds=5,parts=4,water=50,unknown=99".to_string(),
            used: true,
            day_earned: 1,
            day_used: 0,
        });
        let snap = bp.minted_snapshot_of();
        assert_eq!(snap.len(), 5);
        assert_eq!(snap.get("food"), Some(&50));
        assert_eq!(snap.get("water"), Some(&50));
        assert_eq!(snap.get("meds"), Some(&5));
        assert_eq!(snap.get("ammo"), Some(&20));
        assert_eq!(snap.get("parts"), Some(&4));
        assert_eq!(bp.minted_initial, snap);

        // Subsequent calls prefer minted_initial over permits
        bp.permits.clear();
        bp.minted_initial.insert("food".to_string(), 75);
        let snap2 = bp.minted_snapshot_of();
        assert_eq!(snap2.get("food"), Some(&75));
    }
}
