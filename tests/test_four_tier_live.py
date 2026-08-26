"""WAVE_7 live four-tier gate.

GM-off skilled policy, natural GAME_OVER. Pins from the WAVE_6 300-set.
Forced-state weathered tests elsewhere do not satisfy this gate.
"""
from __future__ import annotations

import sys
from pathlib import Path

import pytest

_ROOT = Path(__file__).resolve().parents[1]
if str(_ROOT) not in sys.path:
    sys.path.insert(0, str(_ROOT))

from agents.skilled import choose_intent, reset_step_counter  # noqa: E402
from escape_the_valley.gm import GMConfig  # noqa: E402
from escape_the_valley.intent import GamePhase  # noqa: E402
from escape_the_valley.models import GMProfile  # noqa: E402
from escape_the_valley.step_engine import StepEngine  # noqa: E402
from escape_the_valley.worldgen import create_new_run  # noqa: E402

MAX_STEPS = 3000

PINS = [
    (1207, "triumphant"),
    (1188, "weathered"),
    (1006, "pyrrhic"),
    (1000, "lost"),
]


@pytest.mark.parametrize(
    "seed,expected",
    PINS,
    ids=[f"{seed}-{tier}" for seed, tier in PINS],
)
def test_live_four_tier_gate(seed: int, expected: str) -> None:
    reset_step_counter()
    state = create_new_run(
        seed=seed,
        gm_profile=GMProfile.FIRESIDE,
        weirdness_level=2,
    )
    engine = StepEngine(state, gm_config=GMConfig(enabled=False))
    for _ in range(MAX_STEPS):
        if engine.phase == GamePhase.GAME_OVER:
            break
        engine.step(choose_intent(state, engine))
    ending = engine.finalize_run(reason="complete")
    assert engine.phase == GamePhase.GAME_OVER
    assert state.ending is not None
    assert ending.tier == expected
    assert state.ending.tier == expected
