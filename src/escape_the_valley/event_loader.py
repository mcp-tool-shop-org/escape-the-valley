"""Load event skeletons from JSON data file."""
from __future__ import annotations

import json
from pathlib import Path

from .events import (
    ChoiceTemplate,
    EventCategory,
    EventOutcome,
    EventSkeleton,
    FolkloreType,
)

_DATA_DIR = Path(__file__).parent / "data"

# Keys in engine_effect_profile that map to EventOutcome fields (not supplies)
_META_KEYS = {"time_days", "distance", "morale", "animals_health", "wagon_condition"}

# Resource key aliases (YAML name → catalog name)
_RESOURCE_ALIASES: dict[str, str] = {
    "oil": "lantern_oil",
}


def _classify_category(tags: list[str]) -> EventCategory:
    """Derive EventCategory from tags."""
    if "folklore" in tags:
        return EventCategory.FOLKLORE
    if "human" in tags:
        return EventCategory.HUMAN
    return EventCategory.SURVIVAL


def _classify_weirdness(band: int) -> tuple[bool, FolkloreType | None]:
    """Convert weirdness_band to (costs_uncanny_token, folklore_type)."""
    if band >= 3:
        return True, FolkloreType.UNCANNY
    if band == 2:
        return False, FolkloreType.NATURAL_ODDITY
    if band == 1:
        return False, None  # slightly eerie but not folkloric
    return False, None


def _convert_profile(profile: dict) -> EventOutcome:
    """Convert engine_effect_profile dict to EventOutcome."""
    supplies: dict[str, int] = {}
    health = 0
    wagon = 0
    morale = 0
    time_cost = 0
    distance = 0

    for key, val in profile.items():
        if key == "time_days":
            time_cost = int(val)
        elif key == "distance":
            distance = int(val)
        elif key == "morale":
            morale = int(val)
        elif key == "animals_health":
            health = int(val)
        elif key == "wagon_condition":
            wagon = int(val)
        else:
            # It's a supply resource
            res_key = _RESOURCE_ALIASES.get(key, key)
            supplies[res_key] = int(val)

    return EventOutcome(
        supplies_delta=supplies,
        health_delta=health,
        wagon_delta=wagon,
        morale_delta=morale,
        time_cost=time_cost,
        distance_delta=distance,
    )


def _infer_style(action: str) -> str:
    """Infer choice style from intent action."""
    bold = {"FORD", "TRAVEL", "HUNT"}
    cautious = {"WAIT", "REST", "DETOUR"}
    if action in bold:
        return "BOLD"
    if action in cautious:
        return "CAUTIOUS"
    return "NEUTRAL"


def _convert_event(raw: dict) -> EventSkeleton:
    """Convert a raw JSON event dict to an EventSkeleton."""
    tags = raw.get("tags", [])
    band = raw.get("weirdness_band", 0)
    costs_token, folklore_type = _classify_weirdness(band)
    category = _classify_category(tags)

    # If we classified as folklore based on weirdness but tags don't say it,
    # override category if weirdness_band >= 2
    if band >= 2 and category != EventCategory.FOLKLORE:
        category = EventCategory.FOLKLORE

    choices: list[ChoiceTemplate] = []
    outcomes: dict[str, EventOutcome] = {}

    for i, ch in enumerate(raw.get("choices", [])):
        cid = chr(65 + i)  # A, B, C
        action = ch.get("intent_action", "INVESTIGATE")
        style = _infer_style(action)
        profile = ch.get("engine_effect_profile", {})
        outcome = _convert_profile(profile)

        choices.append(ChoiceTemplate(
            choice_id=cid,
            label=ch.get("label", ""),
            action=action,
            style=style,
            risk_hint="",
            cost_hint="",
        ))
        outcomes[cid] = outcome

    return EventSkeleton(
        event_id=raw["id"],
        title=raw.get("title", ""),
        category=category,
        tags=tags,
        severity="medium",
        costs_uncanny_token=costs_token,
        folklore_type=folklore_type,
        fallback_narration=raw.get("narration_seed", ""),
        fallback_choices=choices,
        outcome_templates=outcomes,
    )


def load_json_events() -> list[EventSkeleton]:
    """Load all events from the JSON data file."""
    path = _DATA_DIR / "event_skeletons.json"
    if not path.exists():
        return []
    raw_list = json.loads(path.read_text(encoding="utf-8"))
    return [_convert_event(raw) for raw in raw_list]
