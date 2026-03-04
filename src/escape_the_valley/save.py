"""Save/load system — JSON serialization of full game state."""

from __future__ import annotations

import json
from pathlib import Path

from .backpack_models import (
    BackpackState,
    ParcelRecord,
    PermitRecord,
    SettlementRecord,
)
from .models import (
    Biome,
    Condition,
    GMProfile,
    JournalEntry,
    MapNode,
    MemoryCard,
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
from .resources import DEFAULT_SUPPLIES

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
        "recent_event_tags": state.recent_event_tags,
        "party": {
            "members": [
                {
                    "name": m.name,
                    "health": m.health,
                    "condition": m.condition.value,
                    "traits": [t.value for t in m.traits],
                    "death_cause": m.death_cause,
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
                "cache_supplies": n.cache_supplies,
            }
            for n in state.map_nodes
        ],
        "memory_cards": [
            {
                "id": c.id,
                "kind": c.kind,
                "title": c.title,
                "text": c.text,
                "tags": c.tags,
                "day_created": c.day_created,
                "day_last_seen": c.day_last_seen,
                "entities": c.entities,
                "salience": c.salience,
                "cooldown_until": c.cooldown_until,
                "source": c.source,
            }
            for c in state.memory_cards
        ],
        "resource_crises_seen": state.resource_crises_seen,
        "doctrine": state.doctrine,
        "taboo": state.taboo,
        "rationing_steps": state.rationing_steps,
        "escape_valve_cooldown": state.escape_valve_cooldown,
        "last_action": state.last_action,
        "maintained_turns_remaining": state.maintained_turns_remaining,
        "backpack": _backpack_to_dict(state.backpack),
        "callout_level": state.callout_level,
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
            death_cause=m.get("death_cause", ""),
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
        recent_event_tags=data.get("recent_event_tags", []),
        party=PartyState(members=members, morale=party_data.get("morale", 70)),
        wagon=WagonState(
            condition=wagon_data["condition"],
            animals_health=wagon_data.get("animals_health", 90),
            capacity=wagon_data.get("capacity", 200),
            pace=Pace(wagon_data.get("pace", "steady")),
        ),
        supplies=_load_supplies(supplies_data),
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
                cache_supplies=n.get("cache_supplies"),
            )
            for n in map_data
        ],
        memory_cards=[
            MemoryCard(
                id=mc["id"],
                kind=mc["kind"],
                title=mc["title"],
                text=mc["text"],
                tags=mc.get("tags", []),
                day_created=mc.get("day_created", 1),
                day_last_seen=mc.get("day_last_seen", 1),
                entities=mc.get("entities", []),
                salience=mc.get("salience", 0.5),
                cooldown_until=mc.get("cooldown_until", 0),
                source=mc.get("source", "engine"),
            )
            for mc in data.get("memory_cards", [])
        ],
        resource_crises_seen=data.get("resource_crises_seen", []),
        doctrine=data.get("doctrine", ""),
        taboo=data.get("taboo", ""),
        rationing_steps=data.get("rationing_steps", 0),
        escape_valve_cooldown=data.get("escape_valve_cooldown", 0),
        last_action=data.get("last_action", ""),
        maintained_turns_remaining=data.get(
            "maintained_turns_remaining", 0,
        ),
        backpack=_load_backpack(data.get("backpack", {})),
        callout_level=data.get("callout_level", "verbose"),
    )


def _load_supplies(data: dict) -> SuppliesState:
    """Load supplies with backward compat — old saves have only 5 keys."""
    # Start with full defaults, then overlay saved values
    items = dict(DEFAULT_SUPPLIES)
    items.update(data)
    return SuppliesState(items=items)


def _backpack_to_dict(bp: BackpackState) -> dict:
    """Serialize BackpackState to dict."""
    return {
        "enabled": bp.enabled,
        "wallet_address": bp.wallet_address,
        "wallet_secret": bp.wallet_secret,
        "issuer_address": bp.issuer_address,
        "issuer_secret": bp.issuer_secret,
        "trust_lines_ready": bp.trust_lines_ready,
        "last_settled_supplies": bp.last_settled_supplies,
        "last_settlement_day": bp.last_settlement_day,
        "settlements": [
            {
                "day": s.day,
                "location": s.location,
                "deltas": s.deltas,
                "txids": s.txids,
                "status": s.status,
                "memo": s.memo,
                "timestamp": s.timestamp,
            }
            for s in bp.settlements
        ],
        "pending_settlements": [
            {
                "day": s.day,
                "location": s.location,
                "deltas": s.deltas,
                "txids": s.txids,
                "status": s.status,
                "memo": s.memo,
                "timestamp": s.timestamp,
            }
            for s in bp.pending_settlements
        ],
        "parcels": [
            {
                "parcel_id": p.parcel_id,
                "sender": p.sender,
                "contents": p.contents,
                "txid": p.txid,
                "accepted": p.accepted,
                "day_received": p.day_received,
            }
            for p in bp.parcels
        ],
        "permits": [
            {
                "permit_id": p.permit_id,
                "txid": p.txid,
                "used": p.used,
                "day_earned": p.day_earned,
                "day_used": p.day_used,
            }
            for p in bp.permits
        ],
        "nudge_shown": bp.nudge_shown,
        "nudge_dismissed": bp.nudge_dismissed,
    }


def _load_backpack(data: dict) -> BackpackState:
    """Load BackpackState with full backward compat."""
    if not data:
        return BackpackState()

    return BackpackState(
        enabled=data.get("enabled", False),
        wallet_address=data.get("wallet_address", ""),
        wallet_secret=data.get("wallet_secret", ""),
        issuer_address=data.get("issuer_address", ""),
        issuer_secret=data.get("issuer_secret", ""),
        trust_lines_ready=data.get("trust_lines_ready", False),
        last_settled_supplies=data.get("last_settled_supplies", {}),
        last_settlement_day=data.get("last_settlement_day", 0),
        settlements=[
            SettlementRecord(
                day=s.get("day", 0),
                location=s.get("location", ""),
                deltas=s.get("deltas", {}),
                txids=s.get("txids", []),
                status=s.get("status", "pending"),
                memo=s.get("memo", ""),
                timestamp=s.get("timestamp", ""),
            )
            for s in data.get("settlements", [])
        ],
        pending_settlements=[
            SettlementRecord(
                day=s.get("day", 0),
                location=s.get("location", ""),
                deltas=s.get("deltas", {}),
                txids=s.get("txids", []),
                status=s.get("status", "pending"),
                memo=s.get("memo", ""),
                timestamp=s.get("timestamp", ""),
            )
            for s in data.get("pending_settlements", [])
        ],
        parcels=[
            ParcelRecord(
                parcel_id=p.get("parcel_id", ""),
                sender=p.get("sender", ""),
                contents=p.get("contents", {}),
                txid=p.get("txid", ""),
                accepted=p.get("accepted", False),
                day_received=p.get("day_received", 0),
            )
            for p in data.get("parcels", [])
        ],
        permits=[
            PermitRecord(
                permit_id=p.get("permit_id", ""),
                txid=p.get("txid", ""),
                used=p.get("used", False),
                day_earned=p.get("day_earned", 0),
                day_used=p.get("day_used", 0),
            )
            for p in data.get("permits", [])
        ],
        nudge_shown=data.get("nudge_shown", False),
        nudge_dismissed=data.get("nudge_dismissed", False),
    )
