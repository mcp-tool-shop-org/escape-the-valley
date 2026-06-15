"""Load event skeletons from JSON data file."""
from __future__ import annotations

import json
import logging
from pathlib import Path

from .events import (
    ChoiceTemplate,
    EventCategory,
    EventOutcome,
    EventSkeleton,
    FolkloreType,
)
from .resources import RESOURCE_CATALOG

log = logging.getLogger(__name__)

_DATA_DIR = Path(__file__).parent / "data"

# Keys in engine_effect_profile that map to EventOutcome fields (not supplies).
# EC-02: 'health' joins the meta keys so data events can wound or heal the party
# (apply_outcome already applies health_delta and attributes event-caused deaths
# via _proximate_death_cause). 'condition' is parsed as a benign no-op flag today
# — it is accepted so curated data can label *why* a wound landed (e.g. "injured",
# "sick") without becoming a phantom supply, and a future slice can map it onto
# PartyMember.condition. Both are kept out of supplies so they never enter the
# resource conservation ledger.
_META_KEYS = {
    "time_days", "distance", "morale", "animals_health", "wagon_condition",
    "health", "condition",
}

# Resource key aliases (YAML name → catalog name)
_RESOURCE_ALIASES: dict[str, str] = {
    "oil": "lantern_oil",
}

# Most choices map to A-D; the engine offers at most four labelled choices.
_MAX_CHOICES = 4


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


def _convert_profile(profile: dict, *, event_id: str = "") -> EventOutcome:
    """Convert engine_effect_profile dict to EventOutcome.

    ENG-B-07: supply keys are validated against RESOURCE_CATALOG (after alias
    resolution). An unknown key (e.g. a typo'd "gold") is dropped with a warning
    rather than silently entering the supplies dict — otherwise it would become a
    permanent phantom supply that nothing in the game ever consumes or caps.
    """
    supplies: dict[str, int] = {}
    health = 0
    wagon = 0
    animals_health = 0
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
        elif key == "health":
            # EC-02: party-health delta. apply_outcome clamps to 0..100 and
            # attributes any resulting death via _proximate_death_cause, so a
            # negative value here wounds (and can kill) the party.
            health = int(val)
        elif key == "condition":
            # EC-02: accepted but not yet mechanically applied (parsed no-op so
            # curated data can annotate the wound kind without leaking a phantom
            # supply). A future slice maps this onto PartyMember.condition.
            continue
        elif key == "animals_health":
            # ENG-A-03: this is the wagon's draft team, not party health.
            animals_health = int(val)
        elif key == "wagon_condition":
            wagon = int(val)
        else:
            # It's a supply resource — validate against the catalog.
            res_key = _RESOURCE_ALIASES.get(key, key)
            if res_key not in RESOURCE_CATALOG:
                log.warning(
                    "event %s: dropping unknown resource key %r "
                    "(not in RESOURCE_CATALOG)",
                    event_id or "<unknown>", key,
                )
                continue
            supplies[res_key] = int(val)

    return EventOutcome(
        supplies_delta=supplies,
        health_delta=health,
        wagon_delta=wagon,
        animals_health_delta=animals_health,
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
    """Convert a raw JSON event dict to an EventSkeleton.

    ENG-B-03: validates raw['id'] before use (a missing/blank id raises so the
    caller can skip+log this one entry instead of crashing the whole library)
    and caps offered choices at four (A-D), warning on overflow.
    """
    event_id = raw.get("id")
    if not event_id or not isinstance(event_id, str):
        raise ValueError(f"event entry missing a valid 'id': {raw!r:.120}")

    tags = raw.get("tags", [])
    band = raw.get("weirdness_band", 0)
    costs_token, folklore_type = _classify_weirdness(band)
    category = _classify_category(tags)

    # If we classified as folklore based on weirdness but tags don't say it,
    # override category if weirdness_band >= 2
    if band >= 2 and category != EventCategory.FOLKLORE:
        category = EventCategory.FOLKLORE

    raw_choices = raw.get("choices", [])
    if len(raw_choices) > _MAX_CHOICES:
        log.warning(
            "event %s: %d choices exceed the %d-choice cap (A-D); "
            "extra choices dropped",
            event_id, len(raw_choices), _MAX_CHOICES,
        )
        raw_choices = raw_choices[:_MAX_CHOICES]

    choices: list[ChoiceTemplate] = []
    outcomes: dict[str, EventOutcome] = {}

    for i, ch in enumerate(raw_choices):
        cid = chr(65 + i)  # A, B, C, D
        action = ch.get("intent_action", "INVESTIGATE")
        style = _infer_style(action)
        profile = ch.get("engine_effect_profile", {})
        outcome = _convert_profile(profile, event_id=event_id)

        choices.append(ChoiceTemplate(
            choice_id=cid,
            label=ch.get("label", ""),
            action=action,
            style=style,
            risk_hint="",
            cost_hint="",
        ))
        outcomes[cid] = outcome

    # EC-02: honor an optional explicit severity from the data (defaulting to the
    # historical "medium"). Curated bodily-danger events set "high" so the
    # severity curve in select_event weights them up late-game, and the engine's
    # events_high_sev diagnostic counts them. Unknown values fall back to medium.
    severity = raw.get("severity", "medium")
    if severity not in ("low", "medium", "high"):
        log.warning(
            "event %s: unknown severity %r; defaulting to 'medium'",
            event_id, severity,
        )
        severity = "medium"

    return EventSkeleton(
        event_id=event_id,
        title=raw.get("title", ""),
        category=category,
        tags=tags,
        severity=severity,
        costs_uncanny_token=costs_token,
        folklore_type=folklore_type,
        fallback_narration=raw.get("narration_seed", ""),
        fallback_choices=choices,
        outcome_templates=outcomes,
    )


def load_json_events() -> list[EventSkeleton]:
    """Load all events from the JSON data file.

    ENG-B-03: a corrupt data file (or a top-level shape that is not a list)
    logs an error and yields an empty library instead of crashing
    build_event_library()/StepEngine.__init__. One malformed entry is skipped
    and logged so a single bad event never takes down the whole game.
    """
    path = _DATA_DIR / "event_skeletons.json"
    if not path.exists():
        return []

    try:
        raw_list = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, UnicodeDecodeError, OSError) as e:
        log.error("event_skeletons.json could not be loaded: %s", e)
        return []

    if not isinstance(raw_list, list):
        log.error(
            "event_skeletons.json: expected a top-level list, got %s",
            type(raw_list).__name__,
        )
        return []

    events: list[EventSkeleton] = []
    for i, raw in enumerate(raw_list):
        try:
            events.append(_convert_event(raw))
        except Exception as e:  # one bad entry must not break the whole load
            log.warning("skipping malformed event at index %d: %s", i, e)
    return events
