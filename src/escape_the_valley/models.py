"""Core game state models — the engine's source of truth."""

from __future__ import annotations

import hashlib
import random
from dataclasses import dataclass, field
from enum import StrEnum
from typing import Any


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


@dataclass
class PartyMember:
    name: str
    health: int = 100  # 0-100
    condition: Condition = Condition.HEALTHY
    traits: list[Trait] = field(default_factory=list)

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
    food: int = 50
    water: int = 50
    meds: int = 5
    ammo: int = 20
    parts: int = 3

    def apply_delta(self, delta: dict[str, int]) -> None:
        for key, val in delta.items():
            current = getattr(self, key, 0)
            setattr(self, key, max(0, current + val))

    def to_dict(self) -> dict[str, int]:
        return {
            "food": self.food,
            "water": self.water,
            "meds": self.meds,
            "ammo": self.ammo,
            "parts": self.parts,
        }


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
