"""Core game state models — the engine's source of truth."""

from __future__ import annotations

import hashlib
import random
from dataclasses import dataclass, field
from enum import StrEnum
from typing import Any

from .backpack_models import BackpackState


class Pace(StrEnum):
    SLOW = "slow"
    STEADY = "steady"
    HARD = "hard"


class GMProfile(StrEnum):
    CHRONICLER = "chronicler"
    FIRESIDE = "fireside"
    LANTERN = "lantern"


class Biome(StrEnum):
    PLAINS = "plains"
    FOREST = "forest"
    DESERT = "desert"
    SWAMP = "swamp"
    ALPINE = "alpine"


class Weather(StrEnum):
    CLEAR = "clear"
    OVERCAST = "overcast"
    RAIN = "rain"
    STORM = "storm"
    FOG = "fog"
    SNOW = "snow"
    HOT = "hot"


class TimeOfDay(StrEnum):
    MORNING = "morning"
    AFTERNOON = "afternoon"
    EVENING = "evening"
    NIGHT = "night"


class Condition(StrEnum):
    HEALTHY = "healthy"
    SICK = "sick"
    INJURED = "injured"
    EXHAUSTED = "exhausted"


class Trait(StrEnum):
    TOUGH = "tough"
    HEALER = "healer"
    TRACKER = "tracker"
    MECHANIC = "mechanic"
    LUCKY = "lucky"
    ANXIOUS = "anxious"
    STEADY = "steady"
    SHARP_EYE = "sharp_eye"


class TwistModifier(StrEnum):
    BANDIT_YEAR = "bandit_year"
    SICK_SEASON = "sick_season"
    FLOOD_YEAR = "flood_year"
    EARLY_WINTER = "early_winter"
    GOOD_HUNTING = "good_hunting"


class Doctrine(StrEnum):
    TRAVEL_LIGHT = "travel_light"
    CAREFUL_HANDS = "careful_hands"
    NO_DEBTS = "no_debts"


class Taboo(StrEnum):
    NEVER_NIGHT = "never_night"
    NEVER_RIVER = "never_river"
    LEAVE_NOTHING = "leave_nothing"


# Doctrine mechanical modifiers — keyed by Doctrine.value
DOCTRINE_MODIFIERS: dict[str, dict[str, float]] = {
    "travel_light": {
        "consumption_mult": 0.80,
        "breakdown_bonus": 0.05,
        "hunt_bonus": 0.10,
    },
    "careful_hands": {
        "repair_mult": 1.5,
        "distance_penalty": 1,
        "maintenance_bonus": 1,
    },
    "no_debts": {
        "morale_floor": 20,
        "capacity_mult": 0.85,
        "trade_bonus": 0.15,
    },
}


@dataclass
class PartyMember:
    name: str
    health: int = 100  # 0-100
    condition: Condition = Condition.HEALTHY
    traits: list[Trait] = field(default_factory=list)
    death_cause: str = ""  # canonical cause when health <= 0

    def is_alive(self) -> bool:
        return self.health > 0


@dataclass
class PartyState:
    members: list[PartyMember] = field(default_factory=list)
    morale: int = 70  # 0-100, local only

    @property
    def alive_count(self) -> int:
        return sum(1 for m in self.members if m.is_alive())

    @property
    def avg_health(self) -> int:
        alive = [m for m in self.members if m.is_alive()]
        if not alive:
            return 0
        return sum(m.health for m in alive) // len(alive)

    def has_trait(self, trait: Trait) -> bool:
        return any(trait in m.traits for m in self.members if m.is_alive())


@dataclass
class WagonState:
    condition: int = 80  # 0-100
    animals_health: int = 90  # 0-100
    capacity: int = 200  # weight units
    pace: Pace = Pace.STEADY


@dataclass
class SuppliesState:
    """Dict-backed supply storage with backward-compat properties."""

    items: dict[str, int] = field(default_factory=dict)

    def get(self, key: str) -> int:
        return self.items.get(key, 0)

    def set(self, key: str, val: int) -> None:
        self.items[key] = max(0, val)

    def apply_delta(self, delta: dict[str, int]) -> None:
        for key, val in delta.items():
            current = self.items.get(key, 0)
            self.items[key] = max(0, current + val)

    def to_dict(self) -> dict[str, int]:
        return dict(self.items)

    # ── Backward-compat properties for the original 5 resources ──

    @property
    def food(self) -> int:
        return self.items.get("food", 0)

    @food.setter
    def food(self, val: int) -> None:
        self.items["food"] = max(0, val)

    @property
    def water(self) -> int:
        return self.items.get("water", 0)

    @water.setter
    def water(self, val: int) -> None:
        self.items["water"] = max(0, val)

    @property
    def meds(self) -> int:
        return self.items.get("meds", 0)

    @meds.setter
    def meds(self, val: int) -> None:
        self.items["meds"] = max(0, val)

    @property
    def ammo(self) -> int:
        return self.items.get("ammo", 0)

    @ammo.setter
    def ammo(self, val: int) -> None:
        self.items["ammo"] = max(0, val)

    @property
    def parts(self) -> int:
        return self.items.get("parts", 0)

    @parts.setter
    def parts(self, val: int) -> None:
        self.items["parts"] = max(0, val)


@dataclass
class MapNode:
    node_id: str
    name: str
    biome: Biome
    hazard: int  # 0-10
    water_available: bool
    temperature: int  # rough celsius
    is_town: bool = False
    connections: list[str] = field(default_factory=list)  # node_ids
    distance_to: dict[str, int] = field(default_factory=dict)  # node_id -> distance
    cache_supplies: dict[str, int] | None = None  # one-time supply pickup


@dataclass
class JournalEntry:
    day: int
    location: str
    event_id: str
    scene_title: str
    narration: str
    choice_made: str
    outcome: str
    deltas: dict[str, int] = field(default_factory=dict)
    tags: list[str] = field(default_factory=list)


@dataclass
class MemoryCard:
    """Structured memory for GM narrative continuity."""

    id: str
    kind: str  # npc|omen|place|rumor|event_callback|wound|crisis|landmark|promise
    title: str
    text: str
    tags: list[str] = field(default_factory=list)
    day_created: int = 1
    day_last_seen: int = 1
    entities: list[str] = field(default_factory=list)
    salience: float = 0.5  # 0-1 (engine=0.7, gm=0.5)
    cooldown_until: int = 0
    source: str = "engine"  # "engine" | "gm"


@dataclass
class RunState:
    run_id: str = ""
    seed: int = 0
    day: int = 1
    time_of_day: TimeOfDay = TimeOfDay.MORNING
    location_id: str = ""
    destination_id: str = ""
    distance_remaining: int = 0
    total_distance: int = 0
    distance_traveled: int = 0
    mode: str = "offline"
    weirdness_level: int = 2
    uncanny_tokens: int = 2
    gm_profile: GMProfile = GMProfile.FIRESIDE
    twists: list[TwistModifier] = field(default_factory=list)
    game_over: bool = False
    victory: bool = False
    cause_of_death: str = ""

    party: PartyState = field(default_factory=PartyState)
    wagon: WagonState = field(default_factory=WagonState)
    supplies: SuppliesState = field(default_factory=SuppliesState)
    journal: list[JournalEntry] = field(default_factory=list)
    map_nodes: list[MapNode] = field(default_factory=list)

    # RNG state tracking for determinism
    rng_counter: int = 0

    # Variety guard — last N event tag families (for cooldown)
    recent_event_tags: list[str] = field(default_factory=list)

    # GM memory system
    memory_cards: list[MemoryCard] = field(default_factory=list)
    resource_crises_seen: list[str] = field(default_factory=list)

    # Run identity — doctrine + taboo
    doctrine: str = ""
    taboo: str = ""

    # Escape valve state
    rationing_steps: int = 0
    escape_valve_cooldown: int = 0

    # Maintenance window (Phase 4 Balance)
    last_action: str = ""
    maintained_turns_remaining: int = 0

    # Ledger Backpack (Phase 2 — optional XRPL inventory)
    backpack: BackpackState = field(default_factory=BackpackState)

    # Warning callout presentation level
    callout_level: str = "verbose"  # "verbose" or "minimal"

    @staticmethod
    def generate_run_id(seed: int) -> str:
        return hashlib.sha256(str(seed).encode()).hexdigest()[:8]


class SeededRNG:
    """Deterministic RNG wrapper for reproducible runs."""

    def __init__(self, seed: int, counter: int = 0):
        self.seed = seed
        self.counter = counter
        self._rng = random.Random(seed)
        # Advance to the right position
        for _ in range(counter):
            self._rng.random()

    def random(self) -> float:
        self.counter += 1
        return self._rng.random()

    def randint(self, a: int, b: int) -> int:
        self.counter += 1
        return self._rng.randint(a, b)

    def choice(self, seq: list[Any]) -> Any:
        self.counter += 1
        return self._rng.choice(seq)

    def sample(self, population: list[Any], k: int) -> list[Any]:
        self.counter += k
        return self._rng.sample(population, k)

    def shuffle(self, seq: list[Any]) -> None:
        self.counter += len(seq)
        self._rng.shuffle(seq)

    def weighted_choice(self, items: list[Any], weights: list[float]) -> Any:
        self.counter += 1
        total = sum(weights)
        r = self._rng.random() * total
        cumulative = 0.0
        for item, weight in zip(items, weights, strict=False):
            cumulative += weight
            if r <= cumulative:
                return item
        return items[-1]
