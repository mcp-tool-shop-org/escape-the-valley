#!/usr/bin/env python3
"""Oracle vector generator for Escape the Valley 2.0 (Phase 0 Part 2 re-pin + Phase 2 targets).

Pinned legacy Python commit SHA:
    87104824d1f5b8bfda5b0dc5c14bc5a8e3721407

Run against the pinned CPython 3.12.13 (python-build-standalone 20260510) venv:

    uv run --no-sync python rust/oracle/generate_vectors.py

Produces 8 deterministic gzip-compressed vector files in `rust/oracle/vectors/`:
1. `rng_s1_vectors.json.gz`
2. `worldgen_200.json.gz`
3. `saves_20.json.gz`
4. `turns_1000_seeds.json.gz`
5. `turns_long_seeds.json.gz`
6. `resume_120_seeds.json.gz`
7. `ledger_reference.json.gz`
8. `notices_reference.json.gz`

Each vector file header (`metadata`) records `oracle_commit`, `generator_sha256`
(the full-file SHA-256 of `rust/oracle/generate_vectors.py`), `vector_sha256`
(the canonical JSON SHA-256 of that vector file's content excluding `metadata`),
and `vector_files_sha256` (all 8 vector files' content SHA-256s). Compressed
`.json.gz` file SHA-256s are recorded in `rust/oracle/README.md`.
"""

from __future__ import annotations

import gzip
import hashlib
import json
import logging
import random
import struct
import subprocess
import sys
import tempfile
import time
from collections import Counter
from pathlib import Path
from typing import Any

from escape_the_valley.adapter import (
    FrameState,
    build_ending_facts,
    build_run_stats,
    populate_end_data,
)
from escape_the_valley.backpack_models import (
    ParcelRecord,
    PermitRecord,
    SentParcelRecord,
    SettlementRecord,
    minted_snapshot_of,
    stamp_minted_snapshot,
)
from escape_the_valley.event_loader import load_json_events
from escape_the_valley.events import build_event_library
from escape_the_valley.gm import GMConfig
from escape_the_valley.intent import GamePhase, IntentAction, PlayerIntent
from escape_the_valley.ledger import (
    build_trail_ledger,
    build_xrpl_postcard,
)
from escape_the_valley.ledger_proof import (
    PlayerProofResult,
    proof_player_save,
    report_to_dict,
)
from escape_the_valley.memory import MemoryCard
from escape_the_valley.models import (
    Biome,
    Condition,
    Doctrine,
    GMProfile,
    JournalEntry,
    RunState,
    SeededRNG,
    Taboo,
)
from escape_the_valley.notices import (
    generate_third_party_notices,
    get_third_party_notices_text,
    verify_vendored_licenses_integrity,
)
from escape_the_valley.physics import (
    can_abandon_cargo,
    can_desperate_repair,
    can_hard_ration,
    compute_travel_distance,
)
from escape_the_valley.save import (
    SAVE_DIR,
    SAVE_FILE,
    _state_to_dict,
    load_game,
    save_game,
)
from escape_the_valley.step_engine import StepEngine, compute_ending
from escape_the_valley.worldgen import (
    create_new_run,
    generate_map,
    generate_party,
    generate_weather,
    pick_twists,
)


def sha256_hex(data: str) -> str:
    return hashlib.sha256(data.encode("utf-8")).hexdigest()


def f64_bits(val: float) -> int:
    """Return the exact 64-bit IEEE-754 unsigned integer representation of a float."""
    return struct.unpack(">Q", struct.pack(">d", val))[0]


def weighted_choice(rng: random.Random, items: list[Any], weights: list[float]) -> Any:
    """Exact copy of SeededRNG.weighted_choice from src/escape_the_valley/models.py."""
    total = sum(weights)
    r = rng.random() * total
    cumulative = 0.0
    for item, weight in zip(items, weights, strict=False):
        cumulative += weight
        if r <= cumulative:
            return item
    return items[-1]


def serialize_rng_state(state: tuple[Any, ...]) -> list[Any]:
    version, internal, gauss_next = state
    return [version, list(internal), gauss_next]


def rng_state_sha256_hex(state: tuple[Any, ...]) -> str:
    """Compact JSON SHA-256 of [3, [w0..w623, index], gauss_next]."""
    raw = json.dumps(serialize_rng_state(state), separators=(",", ":"))
    return sha256_hex(raw)


PINNED_ORACLE_COMMIT = "87104824d1f5b8bfda5b0dc5c14bc5a8e3721407"
PINNED_ORACLE_TREE = "e1f37816a4c25e2a4a796f4feef59ebfd489a997"


def get_oracle_metadata(repo_dir: Path | None = None) -> dict[str, Any]:
    if repo_dir is None:
        repo_dir = Path(__file__).resolve().parents[2]
    has_commit = (
        subprocess.run(
            [
                "git",
                "-C",
                str(repo_dir),
                "cat-file",
                "-e",
                f"{PINNED_ORACLE_COMMIT}^{{commit}}",
            ],
            capture_output=True,
            check=False,
        ).returncode
        == 0
    )
    diff_ref = PINNED_ORACLE_COMMIT if has_commit else "HEAD"
    head_tree = subprocess.check_output(
        ["git", "-C", str(repo_dir), "rev-parse", f"{diff_ref}:src/escape_the_valley"],
        text=True,
    ).strip()
    if head_tree != PINNED_ORACLE_TREE:
        raise RuntimeError(
            f"Refusing to generate oracle vectors: src/escape_the_valley differs from "
            f"pinned commit {PINNED_ORACLE_COMMIT} (tree {head_tree} != {PINNED_ORACLE_TREE})"
        )
    diff = subprocess.check_output(
        [
            "git",
            "-C",
            str(repo_dir),
            "diff",
            diff_ref,
            "--",
            "src/escape_the_valley",
        ],
        text=True,
    ).strip()
    if diff != "":
        raise RuntimeError(
            f"Refusing to generate oracle vectors: src/escape_the_valley differs from "
            f"pinned commit {PINNED_ORACLE_COMMIT}:\n{diff}"
        )
    status = subprocess.check_output(
        [
            "git",
            "-C",
            str(repo_dir),
            "status",
            "--porcelain",
            "--",
            "src/escape_the_valley",
        ],
        text=True,
    ).strip()
    if status != "":
        raise RuntimeError(
            f"Refusing to generate oracle vectors: dirty or untracked files in "
            f"src/escape_the_valley relative to {PINNED_ORACLE_COMMIT}:\n{status}"
        )
    generator_sha256 = hashlib.sha256(Path(__file__).resolve().read_bytes()).hexdigest()
    return {
        "python_version": sys.version.split()[0],
        "python_full": sys.version,
        "oracle_commit": PINNED_ORACLE_COMMIT,
        "generator_sha256": generator_sha256,
    }


# ── 1. S1 & Gameplay RNG Vectors (both `sample` branches + all call shapes) ──

S1_SEEDS: list[tuple[str, int]] = [
    ("zero", 0),
    ("one", 1),
    ("forty_two", 42),
    ("neg_one", -1),
    ("neg_forty_two", -42),
    ("u32_plus_7", (1 << 32) + 7),
    ("u64_plus_13", (1 << 64) + 13),
    ("bits_100", (1 << 99) + 0x123456789ABCDEF012345678),
    ("negative_large", -98765432101234567890),
    ("negative_bits_80", -(1 << 80) - 99),
]

GETRANDBITS_WIDTHS: list[int] = [
    1,
    2,
    7,
    16,
    31,
    32,
    33,
    48,
    53,
    63,
    64,
    65,
    96,
    100,
    128,
]

# Includes general bit-width ranges and all 18 `randint(a, b)` ranges used in the 1.3.1 engine:
# (1, 2) worldgen.py:204,220; (1, 8) worldgen.py:98; (2, 4) physics.py:393;
# (3, 8) physics.py:218,336; (3, 9) step_engine.py:1244 (engine.py:926); (3, 10) physics.py:327;
# (4, 6) worldgen.py:76; (5, 12) physics.py:279; (5, 15) physics.py:193,292,414,518;
# (7, 16) physics.py:277; (8, 16) physics.py:164; (8, 18) physics.py:275; (8, 20) physics.py:198;
# (8, 25) worldgen.py:107; (15, 35) worldgen.py:113; (80, 100) worldgen.py:208;
# (-3, 3) events.py:2136; (-5, 5) worldgen.py:92.
RANDINT_RANGES: list[tuple[int, int]] = [
    (0, 0),
    (0, 1),
    (0, 2),
    (1, 2),  # worldgen.py:204,220 generate_party num_traits / pick_twists count
    (1, 6),
    (1, 8),  # worldgen.py:98 generate_map node hazard
    (2, 4),  # physics.py:393 check_spoilage food loss
    (2, 6),
    (3, 8),  # physics.py:218,336 check_health_effects sickness damage / rest_day morale boost
    (3, 9),  # step_engine.py:1244 (engine.py:926) _arrive_at_next_node town trade food_offer
    (3, 10),  # physics.py:327 rest_day member heal
    (4, 5),
    (4, 6),  # worldgen.py:76 generate_map town spacing
    (4, 12),
    (5, 12),  # physics.py:279 attempt_hunt food yield (other biomes)
    (5, 15),  # physics.py:193,292,414,518 starvation / hunt & desperate_repair injury / night wagon
    (7, 16),  # physics.py:277 attempt_hunt food yield (Biome.PLAINS)
    (8, 16),  # physics.py:164 check_breakdown wagon damage
    (8, 18),  # physics.py:275 attempt_hunt food yield (Biome.FOREST)
    (8, 20),  # physics.py:198 check_health_effects dehydration damage
    (8, 25),  # worldgen.py:107 generate_map leg distance
    (10, 20),
    (12, 16),
    (15, 25),
    (15, 35),  # worldgen.py:113 generate_map fork branch_dist
    (70, 100),
    (80, 100),  # worldgen.py:208 generate_party initial member health
    (-3, 3),  # events.py:2136 resolve_event health_delta jitter
    (-5, 5),  # worldgen.py:92 generate_map node temperature jitter
    (0, 32),
    (0, 255),
    (0, 256),
    (0, 65535),
    (0, 65536),
    (-100000, 100000),
    (0, (1 << 32) - 1),
    (0, (1 << 32)),
    (-(1 << 40), (1 << 40)),
]

# Includes all gameplay `choice` sequence lengths (2, 3, 4, 5, 7) plus 1, 18, 60
CHOICE_LENGTHS: list[int] = [1, 2, 3, 4, 5, 7, 18, 60]
# Includes worldgen `shuffle` lengths 15 (TOWN_NAMES), 18 (FIRST_NAMES), 20 (LANDMARK_NAMES)
SHUFFLE_LENGTHS: list[int] = [0, 1, 2, 5, 12, 15, 18, 20, 25]

# Covers both CPython `sample` branches:
# - pool branch (`n <= setsize`, where `setsize = 21 + (4 ** ceil(log(k*3, 4)) if k > 5 else 0)`)
# - set branch (`n > setsize`)
# Includes gameplay `sample` shapes: (5, 1), (5, 2) in `pick_twists`, and (8, 1), (8, 2) in party.
SAMPLE_CASES: list[tuple[str, int, int]] = [
    ("pool_k0", 5, 0),
    ("pool_k1", 5, 1),
    ("pool_n5_k2", 5, 2),
    ("pool_n8_k1", 8, 1),
    ("pool_n8_k2", 8, 2),
    ("pool_n21_k5", 21, 5),
    ("set_n22_k5", 22, 5),
    ("pool_n25_k6", 25, 6),
    ("pool_n85_k6", 85, 6),
    ("set_n86_k6", 86, 6),
    ("set_n50_k3", 50, 3),
    ("pool_n100_k10", 100, 10),
    ("set_n300_k10", 300, 10),
    ("pool_n1000_k20", 1000, 20),
    ("set_n2000_k20", 2000, 20),
]

WEIGHTED_CASES: list[tuple[str, list[str], list[float]]] = [
    ("uniform3", ["a", "b", "c"], [1.0, 1.0, 1.0]),
    ("skewed4", ["river", "bandit", "weather", "omen"], [0.1, 0.25, 0.05, 0.6]),
    ("trail5", ["alpha", "beta", "gamma", "delta", "epsilon"], [10.0, 0.5, 0.5, 3.0, 1.0]),
    (
        "weather_alpine7",
        ["clear", "overcast", "rain", "storm", "fog", "snow", "hot"],
        [0.25, 0.20, 0.10, 0.15, 0.10, 0.20, 0.0],
    ),
    (
        "events_cooldown10",
        [f"ev_{i}" for i in range(10)],
        [1.0, 0.25, 0.0625, 1.4, 2.0, 1.3, 0.3, 1.69, 0.5, 1.0],
    ),
]


def generate_s1_seed_vector(label: str, seed: int) -> dict[str, Any]:
    rng = random.Random(seed)
    initial_state = serialize_rng_state(rng.getstate())

    random_draws = []
    for _ in range(20):
        val = rng.random()
        random_draws.append(
            {
                "float": val,
                "bits": str(f64_bits(val)),
                "hex": val.hex(),
            }
        )

    getrandbits_draws = []
    for k in GETRANDBITS_WIDTHS:
        for _ in range(4):
            val = rng.getrandbits(k)
            getrandbits_draws.append(
                {
                    "k": k,
                    "value": str(val),
                }
            )

    randint_draws = []
    for a, b in RANDINT_RANGES:
        vals = [str(rng.randint(a, b)) for _ in range(16)]
        randint_draws.append(
            {
                "a": str(a),
                "b": str(b),
                "values": vals,
            }
        )

    choice_draws = []
    for length in CHOICE_LENGTHS:
        seq = [f"item_{i}" for i in range(length)]
        picks = [rng.choice(seq) for _ in range(10)]
        choice_draws.append(
            {
                "len": length,
                "picks": picks,
            }
        )

    shuffle_draws = []
    for length in SHUFFLE_LENGTHS:
        seq = list(range(length))
        rng.shuffle(seq)
        shuffle_draws.append(
            {
                "len": length,
                "result": seq,
            }
        )

    sample_draws = []
    for case_name, n, k in SAMPLE_CASES:
        pop = list(range(n))
        picked = rng.sample(pop, k)
        sample_draws.append(
            {
                "name": case_name,
                "n": n,
                "k": k,
                "result": picked,
            }
        )

    weighted_draws = []
    for case_name, items, weights in WEIGHTED_CASES:
        picks = [weighted_choice(rng, items, weights) for _ in range(15)]
        weighted_draws.append(
            {
                "name": case_name,
                "items": items,
                "weights": weights,
                "picks": picks,
            }
        )

    final_state = serialize_rng_state(rng.getstate())
    return {
        "label": label,
        "seed": str(seed),
        "initial_state": initial_state,
        "random_draws": random_draws,
        "getrandbits_draws": getrandbits_draws,
        "randint_draws": randint_draws,
        "choice_draws": choice_draws,
        "shuffle_draws": shuffle_draws,
        "sample_draws": sample_draws,
        "weighted_draws": weighted_draws,
        "final_state": final_state,
    }


def generate_s1_save_restore_vector() -> dict[str, Any]:
    rng = random.Random(42)
    for _ in range(17):
        rng.random()
    for _ in range(9):
        rng.randint(8, 25)
    names = ["Ruth", "Josiah", "Elias", "Agnes", "Martha", "Thomas", "Caleb", "Hannah"]
    rng.shuffle(names)
    _ = rng.sample(list(range(30)), 4)

    saved_json_str = json.dumps(rng.getstate())
    restored_list = json.loads(saved_json_str)

    restored_rng = random.Random(0)
    version, internal, gauss_next = restored_list
    restored_rng.setstate((version, tuple(internal), gauss_next))

    post_random = [str(f64_bits(restored_rng.random())) for _ in range(10)]
    post_randint = [str(restored_rng.randint(-50, 500)) for _ in range(10)]
    shuf = list(range(10))
    restored_rng.shuffle(shuf)
    post_sample_pool = restored_rng.sample(list(range(15)), 4)
    post_sample_set = restored_rng.sample(list(range(100)), 5)
    post_weighted = [
        weighted_choice(restored_rng, ["food", "water", "ammo", "parts"], [2.5, 3.0, 1.0, 0.5])
        for _ in range(10)
    ]

    return {
        "saved_rng_state": restored_list,
        "post_random_bits": post_random,
        "post_randint": post_randint,
        "post_shuffle": shuf,
        "post_sample_pool": post_sample_pool,
        "post_sample_set": post_sample_set,
        "post_weighted": post_weighted,
        "post_final_state": serialize_rng_state(restored_rng.getstate()),
    }


# ── 2. Worldgen 200 seeds + event_loader check ──────────────────────────────


def build_worldgen_seeds() -> list[int]:
    """200 seeds including 0, 1, 42, large ints, negative ints, and sequential/varied seeds."""
    special = [
        0,
        1,
        42,
        -1,
        -42,
        -999999999,
        -98765432101234567890,
        (1 << 31) - 1,
        1 << 31,
        (1 << 32) + 7,
        (1 << 62) + 1337,
        (1 << 99) + 0x123456789ABCDEF012345678,
    ]
    seeds = list(special)
    candidate = 2
    while len(seeds) < 200:
        if candidate not in seeds:
            seeds.append(candidate)
        candidate += 1
    assert len(seeds) == 200
    return seeds


def canonical_run_state_json(state: RunState) -> tuple[str, str]:
    """Return (run_id_prefix, canonical_json_str) with `run_id` normalized to its 8-hex prefix."""
    d = _state_to_dict(state)
    prefix = str(d["run_id"]).split("-")[0]
    d["run_id"] = prefix
    if not (-(1 << 63) <= state.seed < (1 << 63)):
        d["seed"] = str(state.seed)
    canonical = json.dumps(d, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return prefix, canonical


def serialize_event_skeleton(ev: Any) -> dict[str, Any]:
    return {
        "event_id": ev.event_id,
        "title": ev.title,
        "category": ev.category.value,
        "tags": list(ev.tags),
        "severity": ev.severity,
        "biome_filter": [b.value for b in ev.biome_filter] if ev.biome_filter else None,
        "costs_uncanny_token": ev.costs_uncanny_token,
        "folklore_type": ev.folklore_type.value if ev.folklore_type else None,
        "fallback_narration": ev.fallback_narration,
        "choices": [
            {
                "choice_id": c.choice_id,
                "label": c.label,
                "action": c.action,
                "style": c.style,
                "risk_hint": c.risk_hint,
                "cost_hint": c.cost_hint,
            }
            for c in ev.fallback_choices
        ],
        "outcomes": {
            cid: {
                "supplies_delta": out.supplies_delta,
                "supplies_delta_order": list(out.supplies_delta.keys()),
                "health_delta": out.health_delta,
                "wagon_delta": out.wagon_delta,
                "animals_health_delta": out.animals_health_delta,
                "morale_delta": out.morale_delta,
                "time_cost": out.time_cost,
                "distance_delta": out.distance_delta,
                "special_flags": list(out.special_flags),
            }
            for cid, out in ev.outcome_templates.items()
        },
    }


def generate_worldgen_vectors(meta: dict[str, str]) -> dict[str, Any]:
    seeds = build_worldgen_seeds()
    vectors = []
    for seed in seeds:
        rng = SeededRNG(seed)
        _ = generate_map(rng)
        _ = generate_party(rng)
        _ = pick_twists(rng)
        _ = rng.choice(list(Doctrine))
        _ = rng.choice(list(Taboo))
        rng_hash = rng_state_sha256_hex(rng.getstate())

        weather_picks = [
            generate_weather(rng, biome, day).value for day in (1, 25) for biome in Biome
        ]

        state = create_new_run(seed=seed, gm_profile=GMProfile.FIRESIDE, weirdness_level=2)
        assert state.rng_counter == rng.counter - 10
        prefix, canonical_json = canonical_run_state_json(state)

        vectors.append(
            {
                "seed": str(seed),
                "run_id_prefix": prefix,
                "rng_counter": state.rng_counter,
                "rng_state_sha256": rng_hash,
                "weather_picks": weather_picks,
                "canonical_json": canonical_json,
            }
        )

    json_events = load_json_events()
    event_records = [serialize_event_skeleton(ev) for ev in json_events]

    return {
        "schema": "etv-phase1a-worldgen-v1",
        "metadata": meta,
        "count": len(vectors),
        "vectors": vectors,
        "json_events_count": len(event_records),
        "json_events": event_records,
    }


# ── 3. Save Round Trip (20 saves), Migration (v1/v2 -> v3), RNG Restore ─────


def drive_steps(engine: StepEngine, actions: list[IntentAction], choice_seq: str = "ABCD") -> None:
    choice_idx = 0
    for act in actions:
        if engine.phase == GamePhase.GAME_OVER:
            break
        if engine.phase in (GamePhase.EVENT, GamePhase.ROUTE):
            letter = choice_seq[choice_idx % len(choice_seq)]
            choice_idx += 1
            engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id=letter))
            if engine.phase in (GamePhase.EVENT, GamePhase.ROUTE):
                engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
        if engine.phase == GamePhase.GAME_OVER:
            break
        engine.step(PlayerIntent(act))
        while engine.phase in (GamePhase.EVENT, GamePhase.ROUTE):
            letter = choice_seq[choice_idx % len(choice_seq)]
            choice_idx += 1
            engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id=letter))
            if engine.phase in (GamePhase.EVENT, GamePhase.ROUTE):
                engine.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))


def next_100_draws_from_save(seed: int, counter: int, rng_state: Any) -> list[dict[str, Any]]:
    rng = SeededRNG(seed, counter)
    if rng_state is not None:
        try:
            rng.setstate(rng_state)
        except (TypeError, ValueError):
            pass

    draws: list[dict[str, Any]] = []
    for i in range(100):
        kind = i % 5
        if kind == 0:
            val = rng.random()
            draws.append({"op": "random", "bits": str(f64_bits(val)), "counter": rng.counter})
        elif kind == 1:
            val = rng.randint(-20, 120)
            draws.append({"op": "randint", "val": val, "counter": rng.counter})
        elif kind == 2:
            seq = ["a", "b", "c", "d", "e", "f"]
            pick = rng.choice(seq)
            draws.append({"op": "choice", "pick": pick, "counter": rng.counter})
        elif kind == 3:
            pop = [0, 1, 2, 3, 4, 5, 6, 7]
            samp = rng.sample(pop, 3)
            draws.append({"op": "sample", "samp": samp, "counter": rng.counter})
        else:
            items = ["clear", "rain", "storm", "fog"]
            weights = [3.0, 1.5, 0.5, 1.0]
            pick = rng.weighted_choice(items, weights)
            draws.append({"op": "weighted", "pick": pick, "counter": rng.counter})
    return draws


def make_20_real_saves() -> list[dict[str, Any]]:
    T = IntentAction.TRAVEL  # noqa: N806
    R = IntentAction.REST  # noqa: N806
    H = IntentAction.HUNT  # noqa: N806
    P = IntentAction.REPAIR  # noqa: N806

    scenarios: list[tuple[str, int, GMProfile, list[IntentAction], str]] = [
        ("01_fresh_seed_0", 0, GMProfile.FIRESIDE, [], "fresh"),
        ("02_fresh_seed_42_chronicler", 42, GMProfile.CHRONICLER, [], "fresh"),
        ("03_fresh_seed_neg_lantern", -101, GMProfile.LANTERN, [], "fresh"),
        ("04_early_travel_seed_1", 1, GMProfile.FIRESIDE, [T, T, R], "mid"),
        ("05_early_hunt_seed_7", 7, GMProfile.FIRESIDE, [T, H, T, P], "mid"),
        ("06_mid_supplies_seed_13", 13, GMProfile.FIRESIDE, [T, T, T, H, R, T, T], "mid"),
        ("07_mid_events_seed_21", 21, GMProfile.CHRONICLER, [T] * 10, "events"),
        ("08_mid_events_seed_33", 33, GMProfile.LANTERN, [T, T, R, T, H, T, T, P, T], "events"),
        ("09_memory_cards_seed_42", 42, GMProfile.FIRESIDE, [T] * 14, "memories"),
        ("10_memory_cards_seed_55", 55, GMProfile.FIRESIDE, [T, H, T, R] * 4, "memories"),
        (
            "11_escape_valve_ration_seed_64",
            64,
            GMProfile.FIRESIDE,
            [T, T, IntentAction.HARD_RATION, T, T],
            "valve",
        ),
        (
            "12_escape_valve_cargo_seed_77",
            77,
            GMProfile.FIRESIDE,
            [T, IntentAction.ABANDON_CARGO, T, R, T],
            "valve",
        ),
        ("13_pace_hard_seed_88", 88, GMProfile.FIRESIDE, [T] * 12, "pace_hard"),
        ("14_pace_slow_seed_99", 99, GMProfile.FIRESIDE, [T, R, T, H] * 3, "pace_slow"),
        ("15_backpack_settled_seed_105", 105, GMProfile.FIRESIDE, [T] * 6, "backpack"),
        ("16_backpack_parcels_seed_120", 120, GMProfile.FIRESIDE, [T, H, T, T], "backpack_parcels"),
        ("17_late_run_seed_144", 144, GMProfile.FIRESIDE, [T] * 22, "late"),
        ("18_crisis_low_supplies_seed_160", 160, GMProfile.FIRESIDE, [T] * 18, "crisis"),
        ("19_game_over_starvation_seed_177", 177, GMProfile.FIRESIDE, [T] * 5, "game_over_death"),
        ("20_game_over_victory_seed_200", 200, GMProfile.FIRESIDE, [T] * 8, "game_over_victory"),
    ]

    saves_out: list[dict[str, Any]] = []

    with tempfile.TemporaryDirectory() as tmpdir:
        base = Path(tmpdir)
        for idx, (label, seed, profile, actions, tag) in enumerate(scenarios):
            run_dir = base / f"run_{idx:02d}"
            run_dir.mkdir(parents=True, exist_ok=True)

            state = create_new_run(seed=seed, gm_profile=profile)
            det_suffix = hashlib.sha256(f"etv-phase1a-save-{label}".encode()).hexdigest()[:8]
            state.run_id = f"{state.run_id.split('-')[0]}-{det_suffix}"

            engine = StepEngine(state, GMConfig(enabled=False), autosave=False)
            if tag == "pace_hard":
                engine.step(PlayerIntent(IntentAction.CHANGE_PACE, pace="hard"))
            elif tag == "pace_slow":
                engine.step(PlayerIntent(IntentAction.CHANGE_PACE, pace="slow"))

            drive_steps(engine, actions)

            if tag == "backpack":
                bp = engine.state.backpack
                bp.enabled = True
                bp.wallet_address = "rTestPlayerWallet111111111111"
                bp.issuer_address = "rTestIssuerWallet222222222222"
                bp.trust_lines_ready = True
                bp.last_settled_supplies = {
                    "food": 40,
                    "water": 38,
                    "meds": 5,
                    "ammo": 18,
                    "parts": 4,
                }
                bp.last_settlement_day = engine.state.day
                bp.nudge_shown = True
                stamp_minted_snapshot(
                    bp,
                    {"food": 50, "water": 50, "meds": 5, "ammo": 20, "parts": 4},
                )
                bp.settlements.append(
                    SettlementRecord(
                        day=engine.state.day,
                        location=engine.state.location_id,
                        deltas={"food": -10, "water": -12},
                        txids=["ABCDEF0123456789ABCDEF0123456789"],
                        status="settled",
                        memo=f"TRAIL|RUN:{engine.state.run_id}|DAY:{engine.state.day}|V:v1",
                        timestamp="2026-10-23T00:00:00+00:00",
                    )
                )
            elif tag == "backpack_parcels":
                bp = engine.state.backpack
                bp.enabled = True
                bp.wallet_address = "rParcelPlayerWallet3333333333"
                bp.issuer_address = "rParcelIssuerWallet4444444444"
                bp.trust_lines_ready = True
                bp.parcels.append(
                    ParcelRecord(
                        parcel_id="rSender999:FOD:6",
                        sender="rSender999",
                        contents={"food": 6},
                        txid="11223344556677889900AABBCCDDEEFF",
                        accepted=True,
                        day_received=engine.state.day,
                    )
                )
                bp.sent_parcels.append(
                    SentParcelRecord(
                        recipient="rFriend888",
                        supply="water",
                        amount=5,
                        txid="FFEEDDCCBBAA00998877665544332211",
                        day_sent=engine.state.day,
                        memo=f"PARCEL|RUN:{engine.state.run_id}|DAY:{engine.state.day}|water:5",
                    )
                )
                bp.permits.append(
                    PermitRecord(
                        permit_id="permit_rope_bridge",
                        txid="99887766554433221100FFEEDDCCBBAA",
                        used=False,
                        day_earned=engine.state.day,
                        day_used=0,
                    )
                )
            elif tag == "crisis":
                engine.state.supplies.set("food", 0)
                engine.state.supplies.set("water", 0)
                from escape_the_valley.memory_emitters import check_resource_crises

                check_resource_crises(engine.state)
            elif tag == "game_over_death":
                engine.state.supplies.set("food", 0)
                engine.state.supplies.set("water", 0)
                for m in engine.state.party.members:
                    m.health = 0
                    m.death_cause = "Starvation"
                engine.state.game_over = True
                engine.state.victory = False
                engine.state.cause_of_death = "Starvation"
            elif tag == "game_over_victory":
                engine.state.location_id = engine.state.map_nodes[-1].node_id
                engine.state.destination_id = engine.state.map_nodes[-1].node_id
                engine.state.distance_remaining = 0
                engine.state.game_over = True
                engine.state.victory = True

            engine.state.rng_counter = engine.rng.counter
            engine.state.rng_state = engine.rng.getstate()

            save_path = save_game(engine.state, run_dir)
            raw_json = save_path.read_text(encoding="utf-8")
            parsed_dict = json.loads(raw_json)
            canonical_json = json.dumps(
                parsed_dict, sort_keys=True, separators=(",", ":"), ensure_ascii=False
            )
            reloaded_for_snap = load_game(run_dir)
            assert reloaded_for_snap is not None
            raw_snap = minted_snapshot_of(reloaded_for_snap.backpack)
            snap_order = ("water", "ammo", "meds", "parts", "food")
            minted_snapshot = (
                {k: raw_snap[k] for k in snap_order if k in raw_snap} if raw_snap else raw_snap
            )

            draws_with_state = next_100_draws_from_save(
                engine.state.seed,
                engine.state.rng_counter,
                parsed_dict["rng_state"],
            )
            draws_counter_only = next_100_draws_from_save(
                engine.state.seed,
                engine.state.rng_counter,
                None,
            )

            saves_out.append(
                {
                    "label": label,
                    "raw_save_json": raw_json,
                    "canonical_save_json": canonical_json,
                    "minted_snapshot": minted_snapshot,
                    "next_100_draws_with_state": draws_with_state,
                    "next_100_draws_counter_only": draws_counter_only,
                }
            )

    return saves_out


def make_migration_vectors() -> list[dict[str, Any]]:
    cases: list[dict[str, Any]] = []

    with tempfile.TemporaryDirectory() as tmpdir:
        base = Path(tmpdir)

        s1 = create_new_run(seed=42)
        s1.run_id = "73475cb4"
        s1.rng_counter = 12
        d1 = _state_to_dict(s1)
        d1.pop("save_version", None)
        d1.pop("rng_state", None)
        d1.pop("doctrine", None)
        d1.pop("taboo", None)
        d1.pop("rationing_steps", None)
        d1.pop("escape_valve_cooldown", None)
        d1.pop("last_action", None)
        d1.pop("maintained_turns_remaining", None)
        d1.pop("last_spoilage_day", None)
        d1.pop("backpack", None)
        d1.pop("memory_cards", None)
        d1.pop("resource_crises_seen", None)
        d1.pop("callout_level", None)
        d1["supplies"] = {"food": 12, "water": 8, "meds": 1, "ammo": 4, "parts": 2}

        s2 = create_new_run(seed=19)
        s2.run_id = "73475cb4-00000002"
        d2 = _state_to_dict(s2)
        d2.pop("save_version", None)
        d2.pop("rng_state", None)
        d2["supplies"] = {"food": -5, "water": 30, "ammo": 9999, "gold": 7, "silver": -10}

        s3 = create_new_run(seed=7)
        s3.run_id = "73475cb4-00000003"
        s3.location_id = s3.map_nodes[3].node_id
        s3.distance_remaining = 5
        s3.rng_counter = 25
        d3 = _state_to_dict(s3)
        d3["destination_id"] = "does-not-exist-in-map-nodes"
        d3.pop("rng_state", None)
        d3.pop("last_spoilage_day", None)
        d3["backpack"].pop("sent_parcels", None)

        s4 = create_new_run(seed=11)
        s4.run_id = "73475cb4-00000004"
        s4.rng_counter = 4
        s4.backpack.enabled = True
        s4.backpack.wallet_address = "rLegacyWallet123"
        snap = {"food": 50, "water": 50, "meds": 10, "ammo": 20, "parts": 10}
        stamp_minted_snapshot(s4.backpack, snap)
        d4 = _state_to_dict(s4)
        d4["rng_state"] = [1, 2, 3]

        s5 = create_new_run(seed=99)
        s5.run_id = "73475cb4-00000005"
        s5.rng_counter = 9
        d5 = _state_to_dict(s5)
        d5["rng_state"] = "nope"
        for m in d5["party"]["members"]:
            m.pop("death_cause", None)
        d5["journal"] = [
            {
                "day": 2,
                "location": "Millford",
                "event_id": "legacy_ev",
            }
        ]

        raw_fixtures = [
            ("v1_minimal_five_supplies", d1),
            ("v1_negative_and_phantom_supplies", d2),
            ("v2_no_sent_parcels_dangling_dest", d3),
            ("v2_malformed_list_rng_state_with_shim", d4),
            ("v2_malformed_str_rng_state_minimal_journal", d5),
        ]

        for label, raw_dict in raw_fixtures:
            case_dir = base / label
            save_dir = case_dir / SAVE_DIR
            save_dir.mkdir(parents=True, exist_ok=True)
            input_json = json.dumps(raw_dict, indent=2)
            (save_dir / SAVE_FILE).write_text(input_json, encoding="utf-8")

            loaded = load_game(case_dir)
            assert loaded is not None, f"Failed to load migration fixture {label}"
            migrated_dict = _state_to_dict(loaded)
            canonical_migrated = json.dumps(
                migrated_dict, sort_keys=True, separators=(",", ":"), ensure_ascii=False
            )
            raw_snap = minted_snapshot_of(loaded.backpack)
            snap_order = ("water", "ammo", "meds", "parts", "food")
            minted_snapshot = (
                {k: raw_snap[k] for k in snap_order if k in raw_snap} if raw_snap else raw_snap
            )
            draws = next_100_draws_from_save(loaded.seed, loaded.rng_counter, loaded.rng_state)

            cases.append(
                {
                    "label": label,
                    "input_json": input_json,
                    "canonical_migrated_json": canonical_migrated,
                    "minted_snapshot": minted_snapshot,
                    "next_100_draws": draws,
                }
            )

    return cases


# ── 4 & 5. Phase 1b Golden Harness: 1,000 Seeds × 60 Turns + 120 Save/Resume ─


def build_1000_seeds() -> list[int]:
    """1,000 distinct seeds including 0, 1, 42, negative ints, and ints above u32/u64::MAX."""
    special = [
        0,
        1,
        42,
        -1,
        -42,
        -999999999,
        -98765432101234567890,
        (1 << 31) - 1,
        1 << 31,
        (1 << 32) + 7,
        (1 << 62) + 1337,
        (1 << 64) + 13,
        (1 << 99) + 0x123456789ABCDEF012345678,
        -(1 << 80) - 99,
    ]
    seeds = list(special)
    cand = 2
    while len(seeds) < 1000:
        val = -cand if (cand % 9 == 0) else cand
        if val not in seeds:
            seeds.append(val)
        cand += 1
    assert len(seeds) == 1000
    return seeds


def make_policy_rng(seed: int) -> random.Random:
    """Create a separate `random.Random` for the action policy from the run seed by a fixed rule."""
    return random.Random((seed * 0x9E3779B97F4A7C15) ^ 0x5A5A5A5A5A5A5A5A)


def normalize_state_dict(d: dict[str, Any]) -> dict[str, Any]:
    out = dict(d)
    run_id = str(out.get("run_id", ""))
    if "-" in run_id:
        out["run_id"] = run_id.split("-", 1)[0] + "-00000000"
    if isinstance(out.get("seed"), int) and not (-(1 << 63) <= out["seed"] < (1 << 63)):
        out["seed"] = str(out["seed"])
    return out


def hash_run_state_and_digest(state: RunState) -> tuple[str, dict[str, Any]]:
    d = normalize_state_dict(_state_to_dict(state))
    raw = json.dumps(d, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    digest = {
        "day": d["day"],
        "time": d["time_of_day"],
        "loc": d["location_id"],
        "dest": d["destination_id"],
        "rem": d["distance_remaining"],
        "dist": d["distance_traveled"],
        "wagon": d["wagon"]["condition"],
        "pace": d["wagon"]["pace"],
        "morale": d["party"]["morale"],
        "hp": [m["health"] for m in d["party"]["members"]],
        "sup": d["supplies"],
        "j_len": len(d["journal"]),
        "mc_len": len(d["memory_cards"]),
        "over": d["game_over"],
        "vic": d["victory"],
        "cod": d["cause_of_death"],
    }
    return sha256_hex(raw), digest


def serialize_events_emitted(engine: StepEngine, msgs: Any) -> dict[str, Any]:
    ending_dict = None
    if msgs.ending is not None:
        ending_dict = {
            "tier": msgs.ending.tier,
            "headline": msgs.ending.headline,
            "facts": msgs.ending.facts,
        }
    return {
        "phase": engine.phase.value,
        "lines": list(msgs.lines),
        "event_id": engine._pending_event.event_id if engine._pending_event else "",
        "event_title": msgs.event_title,
        "event_choices": [
            {
                "id": c.id,
                "label": c.label,
                "risk_hint": c.risk_hint,
                "cost_hint": c.cost_hint,
            }
            for c in msgs.event_choices
        ],
        "route_options": [
            {"node_id": r.node_id, "name": r.name, "distance": r.distance}
            for r in msgs.route_options
        ],
        "outcome_title": msgs.outcome_title,
        "outcome_narration": msgs.outcome_narration,
        "outcome_deltas": dict(msgs.outcome_deltas),
        "ending": ending_dict,
    }


def pick_intent(engine: StepEngine, prng: random.Random, seed_idx: int, turn: int) -> PlayerIntent:
    """Action policy drawing from a separate `random.Random` (never the game RNG)."""
    if engine.phase == GamePhase.EVENT:
        offered = [c.id for c in engine._pending_event_choices]
        r = prng.random()
        if r < 0.02:
            return PlayerIntent(IntentAction.TRAVEL)
        if r < 0.04:
            return PlayerIntent(IntentAction.CHOOSE, choice_id="Z")
        if engine._pending_event is not None:
            templates = engine._pending_event.outcome_templates
            # On weathered-victory seeds (seed_idx % 24 == 13), prefer safe time_cost > 0 choices
            if seed_idx % 24 == 13:
                best_tc = max(
                    offered,
                    key=lambda c: (
                        templates[c].time_cost
                        if templates[c].health_delta >= 0 and templates[c].wagon_delta >= -5
                        else -1
                    ),
                )
                if templates[best_tc].time_cost > 0 and templates[best_tc].health_delta >= 0:
                    return PlayerIntent(IntentAction.CHOOSE, choice_id=best_tc)
            # On spoilage seeds (seed_idx % 6 == 5), pick the most negative wagon_delta choice
            if seed_idx % 6 == 5:
                best_wag = min(
                    offered,
                    key=lambda c: (
                        templates[c].wagon_delta if templates[c].health_delta >= -10 else 99
                    ),
                )
                if templates[best_wag].wagon_delta < 0:
                    return PlayerIntent(IntentAction.CHOOSE, choice_id=best_wag)
            # On low-wagon / desperate_repair seeds (seed_idx % 6 == 0), prefer wagon/parts < 0
            if seed_idx % 6 == 0:
                for cid in offered:
                    out = templates.get(cid)
                    if out and (out.wagon_delta < 0 or out.supplies_delta.get("parts", 0) < 0):
                        return PlayerIntent(IntentAction.CHOOSE, choice_id=cid)
            # On dehydration seeds (seed_idx % 24 == 3), prefer water_delta < 0 and health_delta < 0
            if seed_idx % 24 == 3:
                best_w = min(
                    offered,
                    key=lambda c: (
                        templates[c].supplies_delta.get("water", 0),
                        templates[c].health_delta,
                    ),
                )
                return PlayerIntent(IntentAction.CHOOSE, choice_id=best_w)
            # On event-death / exhaustion / injury seeds (seed_idx % 24 in (9, 15)), prefer
            # health_delta < 0 while keeping food > 2 and water > 2
            if seed_idx % 24 in (9, 15):
                best_h = min(
                    offered,
                    key=lambda c: (
                        templates[c].health_delta
                        if (
                            engine.state.supplies.food + templates[c].supplies_delta.get("food", 0)
                            > 2
                            and engine.state.supplies.water
                            + templates[c].supplies_delta.get("water", 0)
                            > 2
                        )
                        else 99
                    ),
                )
                return PlayerIntent(IntentAction.CHOOSE, choice_id=best_h)
        return PlayerIntent(IntentAction.CHOOSE, choice_id=prng.choice(offered))
    if engine.phase == GamePhase.ROUTE:
        offered = ["A", "B", "C", "D"][: len(engine._pending_routes)]
        r = prng.random()
        if r < 0.02:
            return PlayerIntent(IntentAction.TRAVEL)
        if r < 0.04:
            return PlayerIntent(IntentAction.CHOOSE, choice_id="D")
        return PlayerIntent(IntentAction.CHOOSE, choice_id=prng.choice(offered))

    # Dehydration branch (seed_idx % 24 == 3):
    # Sets slow pace, drains water via REST on non-arrival legs until water <= alive_count,
    # then steps TRAVEL so daily consumption drops water to 0 before check_health_effects runs.
    if seed_idx % 24 == 3:
        if turn == 1:
            return PlayerIntent(IntentAction.CHANGE_PACE, pace="slow")
        alive = engine.state.party.alive_count
        if (
            engine.state.supplies.water > alive
            and engine.state.distance_remaining > compute_travel_distance(engine.state)
        ):
            return PlayerIntent(IntentAction.REST)
        return PlayerIntent(IntentAction.TRAVEL)

    # Out-of-ammo hunt refusal & ammo crisis branch (seed_idx % 24 == 21):
    # Hunts in camp until ammo == 0, then triggers one out-of-ammo HUNT refusal before traveling.
    if seed_idx % 24 == 21:
        if engine.state.supplies.ammo > 0:
            return PlayerIntent(IntentAction.HUNT)
        if engine.state.last_action == "HUNT" and turn <= 22:
            return PlayerIntent(IntentAction.HUNT)
        return PlayerIntent(IntentAction.TRAVEL)

    # Event-wound / Exhaustion / Injury branch (seed_idx % 24 in (9, 15)):
    # Travels on slow pace while keeping food > 0, water > 0, and wagon repaired so event
    # health_delta < 0 kills members with Condition.EXHAUSTED or Condition.HEALTHY/INJURED.
    if seed_idx % 24 in (9, 15):
        if turn == 1:
            return PlayerIntent(IntentAction.CHANGE_PACE, pace="slow")
        if engine.state.supplies.food < 16 and engine.state.supplies.ammo > 0:
            return PlayerIntent(IntentAction.HUNT)
        if engine.state.wagon.condition < 50 and engine.state.supplies.parts > 0:
            return PlayerIntent(IntentAction.REPAIR)
        return PlayerIntent(IntentAction.TRAVEL)

    # Spoilage & Repeated ABANDON_CARGO branch (seed_idx % 6 == 5):
    # Pushes hard without repairing so wagon drops < 30, then repeatedly calls ABANDON_CARGO
    # (never DESPERATE_REPAIR) so salt drops 8->4->2->1->0 and check_spoilage fires on day % 3 == 0.
    if seed_idx % 6 == 5:
        if turn == 1:
            return PlayerIntent(IntentAction.CHANGE_PACE, pace="hard")
        if can_abandon_cargo(engine.state):
            return PlayerIntent(IntentAction.ABANDON_CARGO)
        if (
            engine.state.supplies.get("salt") == 0
            and engine.state.supplies.food <= 8
            and engine.state.supplies.ammo > 0
        ):
            return PlayerIntent(IntentAction.HUNT)
        if engine.state.supplies.food < 6 and engine.state.supplies.ammo > 0:
            return PlayerIntent(IntentAction.HUNT)
        return PlayerIntent(IntentAction.TRAVEL)

    # Low-wagon / DESPERATE_REPAIR / ABANDON_CARGO branch (seed_idx % 6 == 0):
    if seed_idx % 6 == 0:
        if turn == 1:
            return PlayerIntent(IntentAction.CHANGE_PACE, pace="hard")
        if can_desperate_repair(engine.state):
            return PlayerIntent(IntentAction.DESPERATE_REPAIR)
        if can_abandon_cargo(engine.state):
            return PlayerIntent(IntentAction.ABANDON_CARGO)
        if can_hard_ration(engine.state) and prng.random() < 0.5:
            return PlayerIntent(IntentAction.HARD_RATION)
        return PlayerIntent(IntentAction.TRAVEL)

    # Skilled / valley-bound branch (seed_idx % 6 == 1):
    # Travels briskly while maintaining food, wagon, and health so long runs reach the valley
    # across triumphant, weathered (seed_idx % 24 == 13), and pyrrhic endings.
    if seed_idx % 6 == 1:
        if turn == 1:
            pace = "hard" if (seed_idx % 12 == 1) else "steady"
            return PlayerIntent(IntentAction.CHANGE_PACE, pace=pace)
        if can_desperate_repair(engine.state):
            return PlayerIntent(IntentAction.DESPERATE_REPAIR)
        if can_abandon_cargo(engine.state):
            return PlayerIntent(IntentAction.ABANDON_CARGO)
        if engine.state.wagon.condition < 65 and engine.state.supplies.parts > 0:
            return PlayerIntent(IntentAction.REPAIR)
        # On seed_idx % 24 == 13, once on the final approach with all members alive, rest in camp
        # past par_days before the final step so the run grades as a `weathered` victory.
        if (
            seed_idx % 24 == 13
            and engine.state.map_nodes
            and engine.state.destination_id == engine.state.map_nodes[-1].node_id
            and engine.state.distance_remaining <= compute_travel_distance(engine.state)
            and all(m.is_alive() for m in engine.state.party.members)
        ):
            par_days = max(8, -(-engine.state.total_distance // 8))
            if engine.state.day <= par_days:
                return PlayerIntent(IntentAction.REST)
            return PlayerIntent(IntentAction.TRAVEL)
        if can_hard_ration(engine.state):
            return PlayerIntent(IntentAction.HARD_RATION)
        if engine.state.supplies.food < 16 and engine.state.supplies.ammo > 0:
            return PlayerIntent(IntentAction.HUNT)
        if any(m.is_alive() and m.health < 50 for m in engine.state.party.members):
            return PlayerIntent(IntentAction.REST)
        return PlayerIntent(IntentAction.TRAVEL)

    if can_desperate_repair(engine.state) and prng.random() < 0.6:
        return PlayerIntent(IntentAction.DESPERATE_REPAIR)
    if can_abandon_cargo(engine.state) and prng.random() < 0.5:
        return PlayerIntent(IntentAction.ABANDON_CARGO)
    if can_hard_ration(engine.state) and prng.random() < 0.5:
        return PlayerIntent(IntentAction.HARD_RATION)
    if engine.state.last_action == "REPAIR" and prng.random() < 0.5:
        return PlayerIntent(IntentAction.REST)
    if (
        engine.state.last_action == "REST"
        and engine.state.wagon.condition < 90
        and engine.state.supplies.parts > 0
        and prng.random() < 0.5
    ):
        return PlayerIntent(IntentAction.REPAIR)

    r = prng.random()
    if r < 0.65:
        return PlayerIntent(IntentAction.TRAVEL)
    if r < 0.74:
        return PlayerIntent(IntentAction.REST)
    if r < 0.83:
        return PlayerIntent(IntentAction.HUNT)
    if r < 0.91:
        return PlayerIntent(IntentAction.REPAIR)
    if r < 0.96:
        p = prng.choice(["slow", "steady", "hard", "steady", "hard", "badpace"])
        return PlayerIntent(IntentAction.CHANGE_PACE, pace=p)
    if r < 0.98:
        return PlayerIntent(IntentAction.ABANDON_CARGO)
    if r < 0.99:
        return PlayerIntent(IntentAction.DESPERATE_REPAIR)
    return PlayerIntent(IntentAction.HARD_RATION)


def generate_ending_vectors() -> list[dict[str, Any]]:
    """Generate 8 ending/finalization vectors covering all `compute_ending` tiers."""
    cases = []

    # 1. Triumphant (vow kept)
    s1 = create_new_run(seed=42)
    s1.victory = True
    s1.game_over = True
    s1.day = 15
    s1.distance_traveled = s1.total_distance
    s1.taboo = "never_river"
    cases.append(("triumphant_vow_kept", s1, False))

    # 2. Triumphant (vow broken: never_river + river ford journal entry)
    s2 = create_new_run(seed=42)
    s2.victory = True
    s2.game_over = True
    s2.day = 15
    s2.distance_traveled = s2.total_distance
    s2.taboo = "never_river"
    s2.journal.append(
        JournalEntry(
            day=5,
            location="Crossing",
            event_id="river_crossing",
            scene_title="Deep River",
            narration="",
            choice_made="A: Ford carefully",
            outcome="",
            deltas={},
            tags=["river", "ford"],
        )
    )
    cases.append(("triumphant_vow_broken_river", s2, False))

    # 3. Triumphant (vow broken: never_night + night tag)
    s3 = create_new_run(seed=42)
    s3.victory = True
    s3.game_over = True
    s3.day = 12
    s3.distance_traveled = s3.total_distance
    s3.taboo = "never_night"
    s3.journal.append(
        JournalEntry(
            day=4,
            location="Camp",
            event_id="wolf_pack",
            scene_title="Wolves",
            narration="",
            choice_made="A: Stand guard",
            outcome="",
            deltas={},
            tags=["danger", "night"],
        )
    )
    cases.append(("triumphant_vow_broken_night", s3, False))

    # 4. Weathered (all alive, days > par_days)
    s4 = create_new_run(seed=42)
    s4.victory = True
    s4.game_over = True
    s4.day = 99
    s4.distance_traveled = s4.total_distance
    cases.append(("weathered_late", s4, False))

    # 5. Pyrrhic (victory with fallen member)
    s5 = create_new_run(seed=42)
    s5.victory = True
    s5.game_over = True
    s5.day = 18
    s5.distance_traveled = s5.total_distance
    s5.taboo = "leave_nothing"
    s5.party.members[0].health = 0
    s5.party.members[0].death_cause = "Disease"
    cases.append(("pyrrhic_one_lost", s5, False))

    # 6. Lost (all party members dead)
    s6 = create_new_run(seed=42)
    s6.victory = False
    s6.game_over = True
    s6.day = 14
    s6.cause_of_death = "Starvation"
    for idx, m in enumerate(s6.party.members):
        m.health = 0
        m.death_cause = "Starvation" if idx % 2 == 0 else "Dehydration"
    cases.append(("lost_all_dead", s6, False))

    # 7. Lost (partial survivors when wagon failed)
    s7 = create_new_run(seed=42)
    s7.victory = False
    s7.game_over = True
    s7.day = 16
    s7.cause_of_death = "Wagon failure"
    s7.party.members[0].health = 0
    s7.party.members[0].death_cause = "Injury"
    cases.append(("lost_partial_survivors", s7, False))

    # 8. finalize_run on a stalled non-game-over run with broken wagon and 0 parts
    s8 = create_new_run(seed=42)
    s8.wagon.condition = 0
    s8.supplies.set("parts", 0)
    cases.append(("finalize_run_wagon_failure", s8, True))

    deterministic_ending_run_ids = (
        "73475cb4-9e6bf2c9",
        "73475cb4-263b7ef6",
        "73475cb4-cf225297",
        "73475cb4-751a2382",
        "73475cb4-45c8750a",
        "73475cb4-838f5a0e",
        "73475cb4-7197ce98",
        "73475cb4-799913ba",
    )
    out = []
    for (label, st, use_finalize), det_run_id in zip(
        cases, deterministic_ending_run_ids, strict=True
    ):
        st.run_id = det_run_id
        raw_input = json.dumps(
            _state_to_dict(st), sort_keys=True, separators=(",", ":"), ensure_ascii=False
        )
        if use_finalize:
            eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
            ending = eng.finalize_run("stalled")
            final_cod = eng.state.cause_of_death
            final_over = eng.state.game_over
        else:
            ending = compute_ending(st)
            final_cod = st.cause_of_death
            final_over = st.game_over
        out.append(
            {
                "label": label,
                "use_finalize": use_finalize,
                "input_state_json": raw_input,
                "expected_cause_of_death": final_cod,
                "expected_game_over": final_over,
                "expected_ending": {
                    "tier": ending.tier,
                    "headline": ending.headline,
                    "facts": ending.facts,
                },
            }
        )
    return out


def generate_golden_harness_vectors(
    meta: dict[str, str],
) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    seeds = build_1000_seeds()
    runs: list[dict[str, Any]] = []
    resume_runs: list[dict[str, Any]] = []

    action_counts: Counter[str] = Counter()
    phase_counts: Counter[str] = Counter()
    death_causes: Counter[str] = Counter()
    distinct_events: set[str] = set()
    total_turns = 0

    with tempfile.TemporaryDirectory() as tmpdir:
        base = Path(tmpdir)
        for idx, seed in enumerate(seeds):
            st = create_new_run(seed=seed)
            det_suffix = hashlib.sha256(f"etv-phase1b-seed-{seed}".encode()).hexdigest()[:8]
            st.run_id = f"{st.run_id.split('-')[0]}-{det_suffix}"

            eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
            prng = make_policy_rng(seed)
            turns: list[dict[str, Any]] = []
            saved_at_20_info: tuple[Path, str, str, tuple[Any, ...]] | None = None

            for t in range(1, 61):
                intent = pick_intent(eng, prng, idx, t)
                action_counts[intent.action.value] += 1
                msgs = eng.step(intent)
                total_turns += 1
                phase_counts[eng.phase.value] += 1

                if eng._pending_event is not None:
                    distinct_events.add(eng._pending_event.event_id)

                state_hash, state_digest = hash_run_state_and_digest(eng.state)
                ev_dict = serialize_events_emitted(eng, msgs)

                turns.append(
                    {
                        "t": t,
                        "act": intent.action.value,
                        "cid": intent.choice_id,
                        "pace": intent.pace,
                        "cnt": eng.state.rng_counter,
                        "rng_h": rng_state_sha256_hex(eng.state.rng_state),
                        "st_h": state_hash,
                        "st_d": state_digest,
                        "ev": ev_dict,
                    }
                )

                # Why GamePhase.EVENT is excluded at turn 20:
                # In Python 1.3.1 (`step_engine.py:224-228` and `save.py:288-358`),
                # `StepEngine._pending_event` and `_pending_event_choices` are transient
                # engine fields not serialized into `RunState` (`run.json`). Reloading a save
                # taken during `GamePhase.EVENT` resets `StepEngine.phase` to `GamePhase.CAMP`.
                #
                # All `GamePhase.CAMP` (both `distance_remaining > 0` and `distance_remaining <= 0`
                # at a fork) and `GamePhase.ROUTE` saves at turn 20 are included. When a save is
                # taken in `GamePhase.CAMP` at a fork (`distance_remaining <= 0`), reloading in
                # Python 1.3.1 calls `StepEngine._check_initial_phase()` (`step_engine.py:238-245`),
                # which transitions to `GamePhase.ROUTE` on load (`escape-the-valley#41`),
                # diverging from the uninterrupted trace (which stays in `GamePhase.CAMP` until the
                # next `TRAVEL` step). We therefore reload the turn-20 save in Python, step turns
                # 21..=60 with the restored policy RNG state, and record `reloaded_turns`.
                if (
                    t == 20
                    and len(resume_runs) < 120
                    and eng.phase in (GamePhase.CAMP, GamePhase.ROUTE)
                ):
                    if eng.phase == GamePhase.ROUTE:
                        phase_at_save = "route"
                    elif eng.state.distance_remaining > 0:
                        phase_at_save = "camp"
                    else:
                        phase_at_save = "camp_at_fork"
                    save_dir = base / f"resume_{len(resume_runs):03d}"
                    save_dir.mkdir(parents=True, exist_ok=True)
                    sp = save_game(eng.state, save_dir)
                    saved_at_20_raw = sp.read_text(encoding="utf-8")
                    saved_at_20_info = (save_dir, saved_at_20_raw, phase_at_save, prng.getstate())

                if eng.phase == GamePhase.GAME_OVER:
                    if eng.state.cause_of_death:
                        death_causes[eng.state.cause_of_death] += 1
                    break

            runs.append(
                {
                    "seed": str(seed),
                    "turns": turns,
                }
            )

            if saved_at_20_info is not None and len(turns) > 20:
                save_dir, saved_at_20_raw, phase_at_save, prng_state_at_20 = saved_at_20_info
                loaded_st = load_game(save_dir)
                assert loaded_st is not None
                eng_rel = StepEngine(loaded_st, GMConfig(enabled=False), autosave=False)
                prng_rel = make_policy_rng(seed)
                prng_rel.setstate(prng_state_at_20)
                reloaded_turns: list[dict[str, Any]] = []
                for t_rel in range(21, 61):
                    intent_rel = pick_intent(eng_rel, prng_rel, idx, t_rel)
                    msgs_rel = eng_rel.step(intent_rel)
                    st_h_rel, st_d_rel = hash_run_state_and_digest(eng_rel.state)
                    ev_rel = serialize_events_emitted(eng_rel, msgs_rel)
                    reloaded_turns.append(
                        {
                            "t": t_rel,
                            "act": intent_rel.action.value,
                            "cid": intent_rel.choice_id,
                            "pace": intent_rel.pace,
                            "cnt": eng_rel.state.rng_counter,
                            "rng_h": rng_state_sha256_hex(eng_rel.state.rng_state),
                            "st_h": st_h_rel,
                            "st_d": st_d_rel,
                            "ev": ev_rel,
                        }
                    )
                    if eng_rel.phase == GamePhase.GAME_OVER:
                        break
                differs = reloaded_turns != turns[20:]
                resume_runs.append(
                    {
                        "seed": str(seed),
                        "seed_index": idx,
                        "save_turn": 20,
                        "phase_at_save": phase_at_save,
                        "differs_from_uninterrupted": differs,
                        "raw_save_json": saved_at_20_raw,
                        "reloaded_turns": reloaded_turns,
                    }
                )

    # Second long-run set: first 80 seeds × up to 250 turns so valley-bound seeds finish the trail
    # across all 4 `EndingResult` tiers (`triumphant`, `weathered`, `pyrrhic`, `lost`), all 6
    # reachable `cause_of_death` values in 1.3.1 play (`Starvation`, `Dehydration`, `Disease`,
    # `Wagon failure`, `Exhaustion`, `Injury`), and active `check_spoilage` after `ABANDON_CARGO`.
    long_seeds = seeds[:80]
    long_runs: list[dict[str, Any]] = []
    long_total_turns = 0
    long_endings: Counter[str] = Counter()
    long_deaths: Counter[str] = Counter()

    for idx, seed in enumerate(long_seeds):
        st = create_new_run(seed=seed)
        det_suffix = hashlib.sha256(f"etv-phase1b-long-seed-{seed}".encode()).hexdigest()[:8]
        st.run_id = f"{st.run_id.split('-')[0]}-{det_suffix}"

        eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
        prng = make_policy_rng(seed)
        turns_long: list[dict[str, Any]] = []

        for t in range(1, 251):
            intent = pick_intent(eng, prng, idx, t)
            msgs = eng.step(intent)
            long_total_turns += 1
            state_hash, state_digest = hash_run_state_and_digest(eng.state)
            ev_dict = serialize_events_emitted(eng, msgs)
            turns_long.append(
                {
                    "t": t,
                    "act": intent.action.value,
                    "cid": intent.choice_id,
                    "pace": intent.pace,
                    "cnt": eng.state.rng_counter,
                    "rng_h": rng_state_sha256_hex(eng.state.rng_state),
                    "st_h": state_hash,
                    "st_d": state_digest,
                    "ev": ev_dict,
                }
            )
            if eng.phase == GamePhase.GAME_OVER:
                if eng.state.cause_of_death:
                    long_deaths[eng.state.cause_of_death] += 1
                if msgs.ending is not None:
                    long_endings[msgs.ending.tier] += 1
                break

        long_runs.append(
            {
                "seed": str(seed),
                "turns": turns_long,
            }
        )

    full_library = [serialize_event_skeleton(ev) for ev in build_event_library()]
    assert len(full_library) == 260
    assert len(runs) == 1000
    assert len(resume_runs) == 120
    assert len(long_runs) == 80

    ending_vectors = generate_ending_vectors()

    print(
        f"Golden harness summary (1000x60): seeds={len(runs)}, total_turns={total_turns}, "
        f"resume_runs={len(resume_runs)}, distinct_events={len(distinct_events)}/260"
    )
    print(f"  Actions: {dict(action_counts)}")
    print(f"  Phases:  {dict(phase_counts)}")
    print(f"  Deaths:  {dict(death_causes)}")
    print(
        f"Golden harness summary (80x250): seeds={len(long_runs)}, total_turns={long_total_turns}, "
        f"endings={dict(long_endings)}, deaths={dict(long_deaths)}"
    )

    turns_payload = {
        "schema": "etv-phase1b-turns-v1",
        "metadata": meta,
        "seed_count": len(runs),
        "max_turns_per_seed": 60,
        "total_turns": total_turns,
        "event_library_count": len(full_library),
        "event_library": full_library,
        "ending_vectors": ending_vectors,
        "runs": runs,
    }
    long_turns_payload = {
        "schema": "etv-phase1b-turns-long-v1",
        "metadata": meta,
        "seed_count": len(long_runs),
        "max_turns_per_seed": 250,
        "total_turns": long_total_turns,
        "event_library_count": len(full_library),
        "event_library": full_library,
        "ending_vectors": ending_vectors,
        "runs": long_runs,
    }
    resume_payload = {
        "schema": "etv-phase1b-resume-v2",
        "metadata": meta,
        "count": len(resume_runs),
        "save_turn": 20,
        "resume_runs": resume_runs,
    }
    return turns_payload, long_turns_payload, resume_payload


# ── 6. Ledger & Proof Reference Vectors (Phase 2 target, network/XRPL off) ──


def _new_deterministic_run(seed: int) -> RunState:
    st = create_new_run(seed=seed)
    det_suffix = hashlib.sha256(f"etv-phase2-ledger-seed-{seed}".encode()).hexdigest()[:8]
    st.run_id = f"{st.run_id.split('-')[0]}-{det_suffix}"
    return st


def _snapshot_ledger_case(
    case_id: str,
    state: RunState,
    diagnostics: dict[str, int] | None = None,
) -> dict[str, Any]:
    trail_ledger_lines = build_trail_ledger(state)
    xrpl_postcard_lines = build_xrpl_postcard(state)
    frame = FrameState()
    populate_end_data(frame, state)
    return {
        "case_id": case_id,
        "seed": state.seed,
        "state": _state_to_dict(state),
        "diagnostics": diagnostics,
        "trail_ledger_lines": trail_ledger_lines,
        "trail_ledger_text": "\n".join(trail_ledger_lines),
        "trail_ledger_sha256": sha256_hex("\n".join(trail_ledger_lines)),
        "xrpl_postcard_lines": xrpl_postcard_lines,
        "xrpl_postcard_text": "\n".join(xrpl_postcard_lines),
        "xrpl_postcard_sha256": sha256_hex("\n".join(xrpl_postcard_lines)),
        "ending_facts": [list(row) for row in build_ending_facts(state)],
        "run_stats": [list(row) for row in build_run_stats(state)],
        "end_frame": {
            "game_over": frame.game_over,
            "victory": frame.victory,
            "ending_tier": frame.ending_tier,
            "ending_headline": frame.ending_headline,
            "ending_facts": [list(row) for row in frame.ending_facts],
            "run_stats": [list(row) for row in frame.run_stats],
            "is_postcard": frame.is_postcard,
            "postcard_lines": list(frame.postcard_lines),
        },
    }


def generate_ledger_reference_vectors(meta: dict[str, str]) -> dict[str, Any]:
    """Generate deterministic `ledger.py` + `ledger_proof.py` reference vectors.

    All XRPL/network operations remain completely disabled (zero network calls,
    zero wallet keys).
    """
    ledger_cases: list[dict[str, Any]] = []

    # A. 6 real stepped runs from fixed seeds [1, 2, 7, 19, 42, 77]
    stepped_specs = [
        ("stepped_seed_1_turn_60", 1, 0, 60),
        ("stepped_seed_2_turn_60", 2, 1, 60),
        ("stepped_seed_7_turn_220_victory", 7, 6, 220),
        ("stepped_seed_19_turn_220_defeat", 19, 18, 220),
        ("stepped_seed_42_turn_80", 42, 41, 80),
        ("stepped_seed_77_turn_160", 77, 76, 160),
    ]
    for case_id, seed, idx, max_turns in stepped_specs:
        st = _new_deterministic_run(seed)
        eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
        prng = make_policy_rng(seed)
        for t in range(1, max_turns + 1):
            if eng.phase == GamePhase.GAME_OVER:
                break
            intent = pick_intent(eng, prng, idx, t)
            eng.step(intent)
        ledger_cases.append(_snapshot_ledger_case(case_id, eng.state, dict(eng.diagnostics)))

    # B. 6 curated runs exercising every branch of `ledger.py` and `build_xrpl_postcard`
    # 1. Victory with XRPL backpack enabled, settlements, parcels, permits, kept promise, omen
    c1 = _new_deterministic_run(101)
    c1.victory = True
    c1.game_over = True
    c1.day = 14
    c1.distance_traveled = c1.total_distance
    c1.doctrine = Doctrine.TRAVEL_LIGHT.value
    c1.taboo = Taboo.NEVER_RIVER.value
    c1.ending = compute_ending(c1)
    c1.backpack.enabled = True
    c1.backpack.wallet_address = "rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh"
    c1.backpack.issuer_address = "rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe"
    c1.backpack.trust_lines_ready = True
    stamp_minted_snapshot(
        c1.backpack, {"food": 50, "water": 50, "meds": 5, "ammo": 20, "parts": 3}
    )
    c1.backpack.last_settled_supplies = {
        "food": 32,
        "water": 28,
        "meds": 4,
        "ammo": 14,
        "parts": 2,
    }
    c1.backpack.last_settlement_day = 14
    c1.backpack.settlements = [
        SettlementRecord(
            day=5,
            location="Oak Crossing",
            deltas={"food": -10, "water": -12, "ammo": -4},
            txids=["TX_SETTLE_D5_001", "TX_SETTLE_D5_002"],
            status="confirmed",
        ),
        SettlementRecord(
            day=14,
            location="Salt Flats",
            deltas={"food": -8, "water": -10, "meds": -1, "ammo": -2, "parts": -1},
            txids=["TX_SETTLE_D14_001"],
            status="confirmed",
        ),
    ]
    c1.backpack.parcels = [
        ParcelRecord(
            parcel_id="parcel_d7",
            sender="Trail Cache",
            contents={"food": 6, "meds": 1},
            txid="TX_PARCEL_IN_001",
            accepted=True,
            day_received=7,
        ),
        ParcelRecord(
            parcel_id="parcel_d9_refused",
            sender="Stranger",
            contents={"ammo": 4},
            txid="",
            accepted=False,
            day_received=9,
        ),
    ]
    c1.backpack.sent_parcels = [
        SentParcelRecord(
            recipient="rN7n7otQDd6FczFgLdSqtcsAUxDkw6fzRH",
            supply="food",
            amount=4,
            txid="TX_PARCEL_OUT_001",
            day_sent=11,
            memo=f"TRAIL|RUN:{c1.run_id}|DAY:11|PARCEL:FOD+4",
        )
    ]
    c1.backpack.permits = [
        PermitRecord(
            permit_id="ferry_permit_1",
            txid="TX_PERMIT_001",
            used=True,
            day_earned=4,
            day_used=6,
        ),
        PermitRecord(
            permit_id="ridge_permit_2",
            txid="TX_PERMIT_002",
            used=False,
            day_earned=10,
            day_used=0,
        ),
    ]
    c1.journal.extend([
        JournalEntry(
            day=5,
            location="Oak Crossing",
            event_id="storm_sudden",
            scene_title="Sudden Storm",
            narration="Rain lashed the canvas.",
            choice_made="A: Hunker down",
            outcome="wagon -4, -3 food",
            deltas={"food": -3, "water": 2},
            tags=["weather", "storm"],
        ),
        JournalEntry(
            day=9,
            location="Broken Ridge",
            event_id="bandit_toll",
            scene_title="Toll at the Ridge",
            narration="Armed riders blocked the pass.",
            choice_made="B: Pay the toll",
            outcome="-6 food, -4 ammo, -2 meds",
            deltas={"food": -6, "ammo": -4, "meds": -2},
            tags=["human", "bandits"],
        ),
    ])
    c1.memory_cards.extend([
        MemoryCard(
            id="gm_promise_3_aabbccddeeff",
            kind="promise",
            title="Deliver the Salt Box",
            text="Promised the ferryman to carry the sealed box to the valley.",
            tags=["promise"],
            day_created=3,
            day_last_seen=12,
            entities=["Ferryman"],
            salience=0.8,
            cooldown_until=0,
            source="gm",
        ),
        MemoryCard(
            id="eng_event_whispering_tree_d8",
            kind="omen",
            title="Whispering Ash",
            text=(
                "The ash tree at the bend spoke three names in the wind before "
                "dawn, and every lantern in camp flickered blue until sunrise "
                "over the ridge."
            ),
            tags=["folklore", "omen"],
            day_created=8,
            day_last_seen=8,
            entities=[],
            salience=0.7,
            cooldown_until=0,
            source="engine",
        ),
    ])
    ledger_cases.append(
        _snapshot_ledger_case(
            "curated_victory_xrpl_promise_omen",
            c1,
            {
                "wagon_breakdowns": 1,
                "events_high_sev": 2,
                "events_total": 9,
                "maintenance_windows": 2,
                "caches_found": 1,
                "escape_valves_used": 0,
                "gm_calls": 0,
                "gm_fallbacks": 0,
                "memory_card_failures": 0,
            },
        )
    )

    # 2. Pyrrhic victory with fallen party member (death_day from journal + fallback 0),
    #    Breakdown Without Parts closest call, and unresolved promise
    c2 = _new_deterministic_run(202)
    c2.victory = True
    c2.game_over = True
    c2.day = 26
    c2.distance_traveled = c2.total_distance
    c2.taboo = Taboo.LEAVE_NOTHING.value
    c2.party.members[0].health = 0
    c2.party.members[0].death_cause = "Disease"
    c2.party.members[1].health = 0
    c2.party.members[1].death_cause = "Starvation"
    c2.ending = compute_ending(c2)
    c2.journal.append(
        JournalEntry(
            day=11,
            location="Black Marsh",
            event_id="death:disease",
            scene_title=f"Death: {c2.party.members[0].name}",
            narration="",
            choice_made="",
            outcome=f"{c2.party.members[0].name} died of disease on day 11 near Black Marsh.",
            deltas={},
            tags=["death", "disease"],
        )
    )
    c2.memory_cards.extend([
        MemoryCard(
            id="eng_wagon_noparts_d14",
            kind="event_callback",
            title="Breakdown Without Parts",
            text="The wagon took 14 damage on day 14 with no parts to repair it.",
            tags=["wagon", "breakdown", "crisis"],
            day_created=14,
            day_last_seen=14,
            entities=[],
            salience=0.7,
            cooldown_until=0,
            source="engine",
        ),
        MemoryCard(
            id="gm_promise_5_112233445566",
            kind="promise",
            title="Keep Every Soul Together",
            text="Swore around the fire that no one would be left behind.",
            tags=["promise"],
            day_created=5,
            day_last_seen=5,
            entities=[],
            salience=0.6,
            cooldown_until=0,
            source="gm",
        ),
    ])
    ledger_cases.append(_snapshot_ledger_case("curated_pyrrhic_fallen_and_noparts", c2, None))

    # 3. Defeat with Hard Rationing closest call and costliest day tie-breaking
    c3 = _new_deterministic_run(303)
    c3.victory = False
    c3.game_over = True
    c3.day = 18
    c3.cause_of_death = "Starvation"
    for m in c3.party.members:
        m.health = 0
        m.death_cause = "Starvation"
    c3.ending = compute_ending(c3)
    c3.journal.extend([
        JournalEntry(
            day=7,
            location="Dry Gulch",
            event_id="spoilage",
            scene_title="Spoiled Stores",
            narration="",
            choice_made="A: Discard",
            outcome="-5 food",
            deltas={"food": -5},
            tags=["survival"],
        ),
        JournalEntry(
            day=12,
            location="Windy Pass",
            event_id="ambush",
            scene_title="Canyon Ambush",
            narration="",
            choice_made="A: Fight",
            outcome="-3 food, -2 ammo",
            deltas={"food": -3, "ammo": -2},
            tags=["human"],
        ),
    ])
    c3.memory_cards.append(
        MemoryCard(
            id="eng_valve_hard_ration_d15",
            kind="crisis",
            title="Hard Rationing Imposed",
            text="Day 15: Hard rationing imposed — half rations for 2 days.",
            tags=["escape_valve", "hard_ration"],
            day_created=15,
            day_last_seen=15,
            entities=[],
            salience=0.7,
            cooldown_until=0,
            source="engine",
        )
    )
    ledger_cases.append(_snapshot_ledger_case("curated_defeat_hard_ration_tie_day", c3, None))

    # 4. Closest call fallback to resource crisis ("food") and no negative deltas
    c4 = _new_deterministic_run(404)
    c4.day = 6
    c4.resource_crises_seen = ["food", "ammo"]
    c4.memory_cards.append(
        MemoryCard(
            id="eng_crisis_food_d6",
            kind="crisis",
            title="Starvation Begins",
            text="The party ran out of food on day 6.",
            tags=["food", "crisis"],
            day_created=6,
            day_last_seen=6,
            entities=[],
            salience=0.7,
            cooldown_until=0,
            source="engine",
        )
    )
    ledger_cases.append(_snapshot_ledger_case("curated_crisis_fallback_no_neg_deltas", c4, None))

    # 5. Closest call fallback to low member health (< 40) when no crisis cards exist
    c5 = _new_deterministic_run(505)
    c5.day = 4
    c5.party.members[2].health = 22
    c5.party.members[2].condition = Condition.INJURED
    ledger_cases.append(_snapshot_ledger_case("curated_low_health_closest_call", c5, None))

    # 6. Clean run with all members >= 40 HP and no crises -> closest_call is None
    c6 = _new_deterministic_run(606)
    c6.day = 2
    ledger_cases.append(_snapshot_ledger_case("curated_clean_no_closest_call", c6, None))

    # C. 8 deterministic local `ledger_proof.py` reconciliation & player-save proof cases
    #    (network/XRPL submission disabled; uses pure `reconcile` and `proof_player_save`
    #    with a local mock manager — zero network calls, zero keys).
    class _OfflineMockManager:
        def __init__(
            self,
            *,
            available: bool = True,
            balances: dict[str, int] | None = None,
            balances_error: str | None = None,
            onchain_memos: dict[str, str] | None = None,
        ) -> None:
            self.available = available
            self._balances = balances or {}
            self._balances_error = balances_error
            self._onchain_memos = onchain_memos

        def wallet_info(self, _state: RunState) -> dict[str, Any]:
            return {
                "balances": dict(self._balances),
                "balances_error": self._balances_error,
            }

        def fetch_onchain_memos(self, _state: RunState) -> dict[str, str] | None:
            return None if self._onchain_memos is None else dict(self._onchain_memos)

        def close(self) -> None:
            pass

    def _make_proof_base_state(seed: int) -> RunState:
        st = _new_deterministic_run(seed)
        st.day = 12
        st.supplies.food = 34
        st.supplies.water = 34
        st.supplies.meds = 3
        st.supplies.ammo = 15
        st.supplies.parts = 2
        st.backpack.enabled = True
        st.backpack.wallet_address = "rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh"
        st.backpack.issuer_address = "rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe"
        st.backpack.trust_lines_ready = True
        stamp_minted_snapshot(
            st.backpack, {"food": 50, "water": 50, "meds": 5, "ammo": 20, "parts": 3}
        )
        st.backpack.last_settled_supplies = {
            "food": 34,
            "water": 34,
            "meds": 3,
            "ammo": 15,
            "parts": 2,
        }
        st.backpack.last_settlement_day = 12
        st.backpack.settlements = [
            SettlementRecord(
                day=6,
                location="Red Ford",
                deltas={"food": -10, "water": -12, "ammo": -3},
                txids=["TX_PROOF_D6_1", "TX_PROOF_D6_2"],
                status="confirmed",
                memo=f"TRAIL|RUN:{st.run_id}|DAY:6|DELTA:FOD-10,WTR-12,AMO-3",
            ),
            SettlementRecord(
                day=12,
                location="Pine Camp",
                deltas={"food": -6, "water": -4, "meds": -2, "ammo": -2, "parts": -1},
                txids=["TX_PROOF_D12_1"],
                status="confirmed",
                memo=f"TRAIL|RUN:{st.run_id}|DAY:12|DELTA:FOD-6,WTR-4,MED-2,AMO-2,PRT-1",
            ),
        ]
        return st

    # Expected XRPL currency balances (minted + sum_deltas == last_settled_supplies):
    # FOD (food):  50 - 16 = 34
    # WTR (water): 50 - 16 = 34
    # MED (meds):   5 -  2 =  3
    # AMO (ammo):  20 -  5 = 15
    # PRT (parts):  3 -  1 =  2
    matching_chain = {"FOD": 34, "WTR": 34, "MED": 3, "AMO": 15, "PRT": 2}
    drifted_chain = {"FOD": 31, "WTR": 34, "MED": 3, "AMO": 19, "PRT": 2}

    def _valid_memos_for(st: RunState) -> dict[str, str]:
        return {
            "TX_PROOF_D6_1": f"TRAIL|RUN:{st.run_id}|DAY:6|DELTA:FOD-10",
            "TX_PROOF_D6_2": f"TRAIL|RUN:{st.run_id}|DAY:6|DELTA:WTR-12,AMO-3",
            "TX_PROOF_D12_1": f"TRAIL|RUN:{st.run_id}|DAY:12|DELTA:FOD-6,WTR-4,MED-2,AMO-2,PRT-1",
        }

    proof_specs: list[tuple[str, RunState, PlayerProofResult]] = []

    # 1. PASS: exact balance + conservation + external on-chain memo match
    p1_st = _make_proof_base_state(701)
    p1_res = proof_player_save(
        p1_st,
        manager=_OfflineMockManager(
            balances=matching_chain, onchain_memos=_valid_memos_for(p1_st)
        ),
    )
    assert p1_res.verdict == "PASS"
    proof_specs.append(("pass_exact_match", p1_st, p1_res))

    # 2. PASS: legacy `__minted_initial__` permit shim hydration
    p2_st = _make_proof_base_state(702)
    p2_st.backpack.minted_initial = {}
    p2_res = proof_player_save(
        p2_st,
        manager=_OfflineMockManager(
            balances=matching_chain, onchain_memos=_valid_memos_for(p2_st)
        ),
    )
    assert p2_res.verdict == "PASS"
    proof_specs.append(("pass_legacy_permit_shim", p2_st, p2_res))

    # 3. FAIL: on-ledger balance drift on FOD and AMO
    p3_st = _make_proof_base_state(703)
    p3_res = proof_player_save(
        p3_st,
        manager=_OfflineMockManager(
            balances=drifted_chain, onchain_memos=_valid_memos_for(p3_st)
        ),
    )
    assert p3_res.verdict == "FAIL"
    proof_specs.append(("fail_balance_drift", p3_st, p3_res))

    # 4. FAIL: pending settlement + balance drift (FAIL takes precedence over INCONCLUSIVE)
    p4_st = _make_proof_base_state(704)
    p4_st.backpack.pending_settlements.append(
        SettlementRecord(
            day=12,
            location="Pine Camp",
            deltas={"food": -2},
            txids=[],
            status="pending",
            memo=f"TRAIL|RUN:{p4_st.run_id}|DAY:12",
        )
    )
    p4_res = proof_player_save(
        p4_st,
        manager=_OfflineMockManager(
            balances=drifted_chain, onchain_memos=_valid_memos_for(p4_st)
        ),
    )
    assert p4_res.verdict == "FAIL"
    proof_specs.append(("fail_pending_and_drift", p4_st, p4_res))

    # 5. INCONCLUSIVE: balances + memos match, sole blocker is pending settlement
    p5_st = _make_proof_base_state(705)
    p5_st.backpack.pending_settlements.append(
        SettlementRecord(
            day=12,
            location="Pine Camp",
            deltas={"water": -3},
            txids=[],
            status="pending",
            memo=f"TRAIL|RUN:{p5_st.run_id}|DAY:12",
        )
    )
    p5_res = proof_player_save(
        p5_st,
        manager=_OfflineMockManager(
            balances=matching_chain, onchain_memos=_valid_memos_for(p5_st)
        ),
    )
    assert p5_res.verdict == "INCONCLUSIVE"
    proof_specs.append(("inconclusive_pending_settlements", p5_st, p5_res))

    # 6. INCONCLUSIVE: network / AccountLines unreachable (`balances_error`)
    p6_st = _make_proof_base_state(706)
    p6_res = proof_player_save(
        p6_st,
        manager=_OfflineMockManager(
            balances={}, balances_error="timeout", onchain_memos=None
        ),
    )
    assert p6_res.verdict == "INCONCLUSIVE"
    proof_specs.append(("inconclusive_network_unreachable", p6_st, p6_res))

    # 7. INCONCLUSIVE: missing enable-time mint snapshot (legacy save without shim)
    p7_st = _make_proof_base_state(707)
    p7_st.backpack.minted_initial = {}
    p7_st.backpack.permits = []
    p7_res = proof_player_save(
        p7_st,
        manager=_OfflineMockManager(
            balances=matching_chain, onchain_memos=_valid_memos_for(p7_st)
        ),
    )
    assert p7_res.verdict == "INCONCLUSIVE"
    proof_specs.append(("inconclusive_missing_mint_snapshot", p7_st, p7_res))

    # 8. FAIL: external on-chain memo tampering (wrong RUN header on TX_PROOF_D6_1)
    p8_st = _make_proof_base_state(708)
    bad_memos = _valid_memos_for(p8_st)
    bad_memos["TX_PROOF_D6_1"] = "TRAIL|RUN:tampered-00000000|DAY:6|DELTA:FOD-10"
    p8_res = proof_player_save(
        p8_st,
        manager=_OfflineMockManager(balances=matching_chain, onchain_memos=bad_memos),
    )
    assert p8_res.verdict == "FAIL"
    proof_specs.append(("fail_onchain_memo_mismatch", p8_st, p8_res))

    # 9. INCONCLUSIVE: backpack disabled on save (`report is None`)
    p9_st = _make_proof_base_state(709)
    p9_st.backpack.enabled = False
    p9_res = proof_player_save(p9_st, manager=_OfflineMockManager())
    assert p9_res.verdict == "INCONCLUSIVE"
    assert p9_res.report is None
    proof_specs.append(("inconclusive_backpack_disabled", p9_st, p9_res))

    proof_cases: list[dict[str, Any]] = []
    for case_id, st, pres in proof_specs:
        rep_dict = report_to_dict(pres.report) if pres.report is not None else None
        proof_cases.append(
            {
                "case_id": case_id,
                "seed": st.seed,
                "verdict": pres.verdict,
                "passed": pres.passed,
                "report_dict": rep_dict,
                "markdown": pres.markdown,
                "markdown_sha256": sha256_hex(pres.markdown),
                "notes": list(pres.notes),
                "overlay_dict": pres.to_overlay_dict(),
            }
        )

    return {
        "schema": "etv-phase2-ledger-v1",
        "metadata": meta,
        "ledger_case_count": len(ledger_cases),
        "proof_case_count": len(proof_cases),
        "ledger_cases": ledger_cases,
        "proof_cases": proof_cases,
    }


# ── 7. Notices Reference Vectors (Phase 2 target) ───────────────────────────


def generate_notices_reference_vectors(meta: dict[str, str]) -> dict[str, Any]:
    """Export `notices.py` (`trail licenses` / `THIRD_PARTY_NOTICES.txt`) reference vectors."""
    sources = verify_vendored_licenses_integrity()
    pip_runtime_text = generate_third_party_notices(release=False)
    cli_runtime_text = get_third_party_notices_text()
    assert pip_runtime_text == cli_runtime_text

    outputs: list[dict[str, Any]] = []
    pip_bytes = pip_runtime_text.encode("utf-8")
    outputs.append(
        {
            "output_id": "pip_runtime",
            "release": False,
            "target": None,
            "byte_length": len(pip_bytes),
            "sha256": hashlib.sha256(pip_bytes).hexdigest(),
            "text": pip_runtime_text,
        }
    )

    for target in sorted(sources["targets"]):
        rel_text = generate_third_party_notices(target=target, release=True)
        rel_bytes = rel_text.encode("utf-8")
        outputs.append(
            {
                "output_id": f"release_{target}",
                "release": True,
                "target": target,
                "byte_length": len(rel_bytes),
                "sha256": hashlib.sha256(rel_bytes).hexdigest(),
                "text": rel_text,
            }
        )

    return {
        "schema": "etv-phase2-notices-v1",
        "metadata": meta,
        "python_version": sources["python_version"],
        "pbs_build_tag": sources["pbs_build_tag"],
        "targets": sorted(sources["targets"]),
        "sources_manifest": sources,
        "outputs": outputs,
    }


def compute_vector_content_sha256(payload: dict[str, Any]) -> str:
    """Compute the deterministic SHA-256 of a vector payload excluding `metadata`."""
    body = {k: v for k, v in payload.items() if k != "metadata"}
    canonical = json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return sha256_hex(canonical)


def write_gzip_json(path: Path, payload: dict[str, Any], *, compact: bool = False) -> str:
    """Write deterministic gzip-compressed JSON (`mtime=0`, `filename=''`) and return SHA-256."""
    if compact:
        raw_text = json.dumps(payload, separators=(",", ":"), ensure_ascii=False) + "\n"
    else:
        raw_text = json.dumps(payload, indent=2, ensure_ascii=False) + "\n"
    raw_bytes = raw_text.encode("utf-8")
    with path.open("wb") as raw_file:
        with gzip.GzipFile(
            filename="", mode="wb", fileobj=raw_file, compresslevel=9, mtime=0
        ) as gz:
            gz.write(raw_bytes)
    file_sha = hashlib.sha256(path.read_bytes()).hexdigest()
    print(f"Wrote {path.name} ({path.stat().st_size} bytes, sha256={file_sha})")
    return file_sha


def main() -> None:
    logging.disable(logging.CRITICAL)
    t0 = time.time()
    out_dir = Path(__file__).resolve().parent / "vectors"
    out_dir.mkdir(parents=True, exist_ok=True)

    meta = get_oracle_metadata()
    print(f"Oracle: Python {meta['python_version']} at {meta['oracle_commit']}")

    # 1. S1 & Gameplay RNG vectors
    s1_payload: dict[str, Any] = {
        "schema": "etv-s1-rng-v2",
        "metadata": dict(meta),
        "seed_vectors": [generate_s1_seed_vector(label, seed) for label, seed in S1_SEEDS],
        "save_restore_vector": generate_s1_save_restore_vector(),
    }

    # 2. Worldgen 200 seeds + 200 JSON events
    wg_payload = generate_worldgen_vectors(dict(meta))

    # 3. Saves 20 + Migrations + RNG restore
    saves_payload: dict[str, Any] = {
        "schema": "etv-phase1a-saves-v1",
        "metadata": dict(meta),
        "saves_20": make_20_real_saves(),
        "migrations": make_migration_vectors(),
    }

    # 4, 5, 6. Golden harness: 1,000x60 + 80x250 + 120 save-and-resume runs at turn 20
    turns_payload, long_turns_payload, resume_payload = generate_golden_harness_vectors(dict(meta))

    # 7. Ledger & proof reference vectors (Phase 2 target)
    ledger_payload = generate_ledger_reference_vectors(dict(meta))

    # 8. Notices reference vectors (Phase 2 target)
    notices_payload = generate_notices_reference_vectors(dict(meta))

    vector_specs: list[tuple[str, dict[str, Any], bool]] = [
        ("rng_s1_vectors.json.gz", s1_payload, False),
        ("worldgen_200.json.gz", wg_payload, False),
        ("saves_20.json.gz", saves_payload, False),
        ("turns_1000_seeds.json.gz", turns_payload, True),
        ("turns_long_seeds.json.gz", long_turns_payload, True),
        ("resume_120_seeds.json.gz", resume_payload, True),
        ("ledger_reference.json.gz", ledger_payload, False),
        ("notices_reference.json.gz", notices_payload, False),
    ]

    vector_files_sha256: dict[str, str] = {
        filename: compute_vector_content_sha256(payload) for filename, payload, _ in vector_specs
    }

    for filename, payload, compact in vector_specs:
        payload["metadata"] = {
            **meta,
            "vector_sha256": vector_files_sha256[filename],
            "vector_files_sha256": dict(vector_files_sha256),
        }
        write_gzip_json(out_dir / filename, payload, compact=compact)

    total_bytes = sum(p.stat().st_size for p in out_dir.glob("*.json.gz"))
    mb = total_bytes / (1024 * 1024)
    elapsed = time.time() - t0
    print(f"Total committed vector size: {total_bytes} bytes ({mb:.2f} MB) in {elapsed:.1f}s")


if __name__ == "__main__":
    main()
