"""Save/load system — JSON serialization of full game state."""

from __future__ import annotations

import json
from pathlib import Path

from .models import (
    Biome,
    Condition,
    GMProfile,
    JournalEntry,
    MapNode,
    Pace,
    PartyMember,
    PartyState,
    RunState,
    SuppliesState,
    TimeOfDay,
    Trait,
    TwistModifier,
    WagonState,
)

SAVE_DIR = ".trail"
SAVE_FILE = "run.json"


def _enum_to_str(obj):
    """Custom serializer for enums."""
    if hasattr(obj, "value"):
        return obj.value
    raise TypeError(f"Not serializable: {type(obj)}")


def save_game(state: RunState, base_path: Path | None = None) -> Path:
    """Save the current game state to disk."""
    base = base_path or Path(".")
    save_dir = base / SAVE_DIR
    save_dir.mkdir(parents=True, exist_ok=True)
    save_path = save_dir / SAVE_FILE

    data = _state_to_dict(state)
    save_path.write_text(json.dumps(data, indent=2, default=_enum_to_str), encoding="utf-8")
    return save_path


def load_game(base_path: Path | None = None) -> RunState | None:
    """Load a saved game state from disk."""
    base = base_path or Path(".")
    save_path = base / SAVE_DIR / SAVE_FILE

    if not save_path.exists():
        return None

    data = json.loads(save_path.read_text(encoding="utf-8"))
    return _dict_to_state(data)


def has_save(base_path: Path | None = None) -> bool:
    """Check if a save file exists."""
    base = base_path or Path(".")
    return (base / SAVE_DIR / SAVE_FILE).exists()


def _state_to_dict(state: RunState) -> dict:
    """Convert RunState to serializable dict."""
    return {
        "run_id": state.run_id,
        "seed": state.seed,
        "day": state.day,
        "time_of_day": state.time_of_day.value,
        "location_id": state.location_id,
        "destination_id": state.destination_id,
        "distance_remaining": state.distance_remaining,
        "total_distance": state.total_distance,
        "distance_traveled": state.distance_traveled,
        "mode": state.mode,
        "weirdness_level": state.weirdness_level,
        "uncanny_tokens": state.uncanny_tokens,
        "gm_profile": state.gm_profile.value,
        "twists": [t.value for t in state.twists],
        "game_over": state.game_over,
        "victory": state.victory,
        "cause_of_death": state.cause_of_death,
        "rng_counter": state.rng_counter,
        "party": {
            "members": [
                {
                    "name": m.name,
                    "health": m.health,
                    "condition": m.condition.value,
                    "traits": [t.value for t in m.traits],
                }
                for m in state.party.members
            ],
            "morale": state.party.morale,
        },
        "wagon": {
            "condition": state.wagon.condition,
            "animals_health": state.wagon.animals_health,
            "capacity": state.wagon.capacity,
            "pace": state.wagon.pace.value,
        },
        "supplies": state.supplies.to_dict(),
        "journal": [
            {
                "day": e.day,
                "location": e.location,
                "event_id": e.event_id,
                "scene_title": e.scene_title,
                "narration": e.narration,
                "choice_made": e.choice_made,
                "outcome": e.outcome,
                "deltas": e.deltas,
                "tags": e.tags,
            }
            for e in state.journal
        ],
        "map_nodes": [
            {
                "node_id": n.node_id,
                "name": n.name,
                "biome": n.biome.value,
                "hazard": n.hazard,
                "water_available": n.water_available,
                "temperature": n.temperature,
                "is_town": n.is_town,
                "connections": n.connections,
                "distance_to": n.distance_to,
            }
            for n in state.map_nodes
        ],
    }


def _dict_to_state(data: dict) -> RunState:
    """Reconstruct RunState from saved dict."""
    party_data = data["party"]
    members = [
        PartyMember(
            name=m["name"],
            health=m["health"],
            condition=Condition(m["condition"]),
            traits=[Trait(t) for t in m["traits"]],
        )
        for m in party_data["members"]
    ]

    wagon_data = data["wagon"]
    supplies_data = data["supplies"]
    map_data = data.get("map_nodes", [])
    journal_data = data.get("journal", [])

    return RunState(
        run_id=data["run_id"],
        seed=data["seed"],
        day=data["day"],
        time_of_day=TimeOfDay(data.get("time_of_day", "morning")),
        location_id=data["location_id"],
        destination_id=data.get("destination_id", ""),
        distance_remaining=data["distance_remaining"],
        total_distance=data.get("total_distance", 0),
        distance_traveled=data.get("distance_traveled", 0),
        mode=data.get("mode", "offline"),
        weirdness_level=data.get("weirdness_level", 2),
        uncanny_tokens=data.get("uncanny_tokens", 2),
        gm_profile=GMProfile(data.get("gm_profile", "fireside")),
        twists=[TwistModifier(t) for t in data.get("twists", [])],
        game_over=data.get("game_over", False),
        victory=data.get("victory", False),
        cause_of_death=data.get("cause_of_death", ""),
        rng_counter=data.get("rng_counter", 0),
        party=PartyState(members=members, morale=party_data.get("morale", 70)),
        wagon=WagonState(
            condition=wagon_data["condition"],
            animals_health=wagon_data.get("animals_health", 90),
            capacity=wagon_data.get("capacity", 200),
            pace=Pace(wagon_data.get("pace", "steady")),
        ),
        supplies=SuppliesState(**supplies_data),
        journal=[
            JournalEntry(
                day=e["day"],
                location=e["location"],
                event_id=e["event_id"],
                scene_title=e.get("scene_title", ""),
                narration=e.get("narration", ""),
                choice_made=e.get("choice_made", ""),
                outcome=e.get("outcome", ""),
                deltas=e.get("deltas", {}),
                tags=e.get("tags", []),
            )
            for e in journal_data
        ],
        map_nodes=[
            MapNode(
                node_id=n["node_id"],
                name=n["name"],
                biome=Biome(n["biome"]),
                hazard=n["hazard"],
                water_available=n.get("water_available", True),
                temperature=n.get("temperature", 15),
                is_town=n.get("is_town", False),
                connections=n.get("connections", []),
                distance_to=n.get("distance_to", {}),
            )
            for n in map_data
        ],
    )
