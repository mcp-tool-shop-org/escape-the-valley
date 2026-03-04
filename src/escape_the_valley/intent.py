"""Player intents and game phases — the UI-to-engine contract."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum


class GamePhase(StrEnum):
    """Current state of the game loop."""

    CAMP = "camp"        # Player picks t/r/h/p
    EVENT = "event"      # Player picks A/B/C/D from event choices
    ROUTE = "route"      # Player picks which fork to take
    GAME_OVER = "over"   # Run ended (victory or death)


class IntentAction(StrEnum):
    """What the player wants to do."""

    TRAVEL = "TRAVEL"
    REST = "REST"
    HUNT = "HUNT"
    REPAIR = "REPAIR"
    CHOOSE = "CHOOSE"          # Pick A/B/C/D during EVENT or ROUTE
    CHANGE_PACE = "CHANGE_PACE"


@dataclass(frozen=True)
class PlayerIntent:
    """Single player action submitted from the UI."""

    action: IntentAction
    choice_id: str = ""   # A/B/C/D for CHOOSE
    pace: str = ""        # slow/steady/hard for CHANGE_PACE
