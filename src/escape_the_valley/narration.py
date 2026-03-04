"""Narration Bus — extracts voice-worthy moments from StepMessages.

The engine never imports this module. The TUI calls extract_narration()
after each step, and if voice is enabled, feeds events to the VoiceBridge.
"""

from __future__ import annotations

import re
import time
from dataclasses import dataclass
from enum import StrEnum
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .step_engine import StepMessages


class NarrationType(StrEnum):
    """Categories of narration moments."""

    SCENE_OPEN = "scene_open"
    OUTCOME = "outcome"
    WARNING = "warning"
    ARRIVAL = "arrival"
    GAME_OVER = "game_over"


@dataclass(frozen=True)
class NarrationEvent:
    """A single voice-worthy moment."""

    type: NarrationType
    voice_text: str
    priority: int = 1
    pause_before_ms: int = 0


# ── Guardrails ─────────────────────────────────────────────────────

MAX_UTTERANCE_CHARS = 240
MIN_INTERVAL_SECONDS = 3.0

_SECRETS_PATTERN = re.compile(
    r"[A-Za-z]:\\|/home/|/tmp/|\.env|api[_-]?key|token|secret|password",
    re.IGNORECASE,
)

_last_warning_time: float = 0.0


def _sanitize(text: str) -> str:
    """Strip secrets/paths and truncate to max length."""
    text = _SECRETS_PATTERN.sub("", text).strip()
    if len(text) > MAX_UTTERANCE_CHARS:
        truncated = text[:MAX_UTTERANCE_CHARS]
        last_period = truncated.rfind(".")
        if last_period > MAX_UTTERANCE_CHARS // 2:
            text = truncated[: last_period + 1]
        else:
            text = truncated[: MAX_UTTERANCE_CHARS - 3].rstrip() + "..."
    return text


def _warning_rate_limited() -> bool:
    """Check if warnings should be suppressed (rate limit)."""
    now = time.monotonic()
    return now - _last_warning_time < MIN_INTERVAL_SECONDS


def _mark_warning_narrated() -> None:
    """Record that we just narrated a warning."""
    global _last_warning_time  # noqa: PLW0603
    _last_warning_time = time.monotonic()


# ── Voice Script Condensation ──────────────────────────────────────

def _condense(title: str, narration: str) -> str:
    """Title + first 2 sentences of narration, capped at 240 chars."""
    sentences = re.split(r"(?<=[.!?])\s+", narration.strip())
    voice_text = " ".join(sentences[:2])
    return _sanitize(f"{title}. {voice_text}")


_CLIFF_KEYWORDS = ("one day", "one more break", "barely", "no spare parts")


# ── Main Extraction ────────────────────────────────────────────────


def extract_narration(
    msgs: StepMessages,
    phase_str: str,
    warnings: list[str] | None = None,
) -> list[NarrationEvent]:
    """Extract voice-worthy narration events from a StepMessages.

    Returns 0-3 NarrationEvents. The TUI decides whether to send
    them to the VoiceBridge.

    SPEAK: scene title + opening, outcome title + recap, cliff warnings
    SKIP:  inventory, journal, travel/status, choices
    """
    events: list[NarrationEvent] = []

    # 1. Scene opening
    if msgs.event_title and msgs.event_narration:
        text = _condense(msgs.event_title, msgs.event_narration)
        if text:
            events.append(NarrationEvent(
                type=NarrationType.SCENE_OPEN,
                voice_text=text,
                priority=2,
            ))

    # 2. Outcome (only when event phase is resolved)
    if msgs.outcome_title and msgs.outcome_narration and phase_str != "event":
        text = _condense(msgs.outcome_title, msgs.outcome_narration)
        if text:
            events.append(NarrationEvent(
                type=NarrationType.OUTCOME,
                voice_text=text,
                priority=2,
                pause_before_ms=500,
            ))

    # 3. Critical cliff-edge warnings (rate-limited)
    if warnings and not _warning_rate_limited():
        critical = [
            w for w in warnings
            if any(kw in w.lower() for kw in _CLIFF_KEYWORDS)
        ]
        if critical:
            text = _sanitize(critical[0])
            if text:
                events.append(NarrationEvent(
                    type=NarrationType.WARNING,
                    voice_text=text,
                    priority=3,
                ))
                _mark_warning_narrated()

    # 4. Arrival
    arrival = next(
        (line for line in msgs.lines if "arrived at" in line.lower()),
        None,
    )
    if arrival:
        events.append(NarrationEvent(
            type=NarrationType.ARRIVAL,
            voice_text=_sanitize(arrival),
            priority=1,
        ))

    # 5. Game over
    game_over = next(
        (line for line in msgs.lines
         if "victory" in line.lower() or "journey ends" in line.lower()),
        None,
    )
    if game_over:
        events.append(NarrationEvent(
            type=NarrationType.GAME_OVER,
            voice_text=_sanitize(game_over),
            priority=3,
        ))

    return events
