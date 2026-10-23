#!/usr/bin/env python3
"""Oracle vector generator for Escape the Valley 2.0 (Phase 1a + Phase 1b).

Run against the pinned CPython 3.12.13 venv in the detached v1.3.1 (47b01a3)
oracle worktree:

    uv run --project E:/AI/_worktrees/etv-oracle-131 --python 3.12.13 python rust/oracle/generate_vectors.py

Produces gzip-compressed vector files in `rust/oracle/vectors/`:
1. `rng_s1_vectors.json.gz`    — S1 & gameplay RNG call-shape vectors (both `sample` branches, large/negative seeds)
2. `worldgen_200.json.gz`      — 200 seeds: canonical RunState JSON + RNG state SHA-256 + weather + 200 JSON events
3. `saves_20.json.gz`          — 20 real Python saves across play stages + 5 migrations + next 100 RNG draws
4. `turns_1000_seeds.json.gz`  — 1,000 seeds × 60 turns golden harness + 260 full event library + 8 ending vectors
5. `resume_120_seeds.json.gz`  — 120 save-and-resume runs saved at fixed turn 20
"""

from __future__ import annotations

import gzip
import hashlib
import json
import random
import struct
import subprocess
import sys
import tempfile
import time
from collections import Counter
from pathlib import Path
from typing import Any

import escape_the_valley
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
from escape_the_valley.models import (
    Biome,
    Doctrine,
    GMProfile,
    JournalEntry,
    RunState,
    SeededRNG,
    Taboo,
)
from escape_the_valley.physics import (
    can_abandon_cargo,
    can_desperate_repair,
    can_hard_ration,
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


def get_oracle_metadata() -> dict[str, str]:
    repo_dir = Path(escape_the_valley.__file__).resolve().parents[2]
    commit = subprocess.check_output(
        ["git", "-C", str(repo_dir), "rev-parse", "HEAD"],
        text=True,
    ).strip()
    describe = subprocess.check_output(
        ["git", "-C", str(repo_dir), "describe", "--tags", "--always"],
        text=True,
    ).strip()
    return {
        "python_version": sys.version.split()[0],
        "python_full": sys.version,
        "oracle_commit": commit,
        "oracle_tag": describe,
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
    1, 2, 7, 16, 31, 32, 33, 48, 53, 63, 64, 65, 96, 100, 128,
]

# Includes general bit-width ranges and every gameplay `randint(a, b)` call shape
RANDINT_RANGES: list[tuple[int, int]] = [
    (0, 0),
    (0, 1),
    (0, 2),
    (1, 6),
    (2, 6),       # physics.py:224 spoilage
    (3, 9),       # step_engine.py:1349 town trade food offer
    (4, 5),       # worldgen.py:303 party size
    (4, 12),      # physics.py:395 hunt food gain
    (5, 15),      # physics.py:296,345,372,402 night/sickness/heal/hunt-injury
    (8, 25),      # worldgen.py:259 leg distance & physics.py:257 breakdown damage
    (10, 20),     # physics.py:319,495 starvation & desperate repair injury
    (12, 16),     # worldgen.py:216 node count
    (15, 25),     # physics.py:331,486 dehydration & desperate repair wagon gain
    (70, 100),    # worldgen.py:310 initial member health
    (-5, 5),
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
SHUFFLE_LENGTHS: list[int] = [0, 1, 2, 5, 12, 25]

# Covers both CPython `sample` branches:
# - pool branch (`n <= setsize`, where `setsize = 21 + (4 ** ceil(log(k*3, 4)) if k > 5 else 0)`)
# - set branch (`n > setsize`)
# Includes gameplay `sample` shapes: (5, 1), (5, 2) in `pick_twists`, and (8, 1), (8, 2) in `generate_party`.
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
        random_draws.append({
            "float": val,
            "bits": str(f64_bits(val)),
            "hex": val.hex(),
        })

    getrandbits_draws = []
    for k in GETRANDBITS_WIDTHS:
        for _ in range(4):
            val = rng.getrandbits(k)
            getrandbits_draws.append({
                "k": k,
                "value": str(val),
            })

    randint_draws = []
    for a, b in RANDINT_RANGES:
        vals = [str(rng.randint(a, b)) for _ in range(16)]
        randint_draws.append({
            "a": str(a),
            "b": str(b),
            "values": vals,
        })

    choice_draws = []
    for length in CHOICE_LENGTHS:
        seq = [f"item_{i}" for i in range(length)]
        picks = [rng.choice(seq) for _ in range(10)]
        choice_draws.append({
            "len": length,
            "picks": picks,
        })

    shuffle_draws = []
    for length in SHUFFLE_LENGTHS:
        seq = list(range(length))
        rng.shuffle(seq)
        shuffle_draws.append({
            "len": length,
            "result": seq,
        })

    sample_draws = []
    for case_name, n, k in SAMPLE_CASES:
        pop = list(range(n))
        picked = rng.sample(pop, k)
        sample_draws.append({
            "name": case_name,
            "n": n,
            "k": k,
            "result": picked,
        })

    weighted_draws = []
    for case_name, items, weights in WEIGHTED_CASES:
        picks = [weighted_choice(rng, items, weights) for _ in range(15)]
        weighted_draws.append({
            "name": case_name,
            "items": items,
            "weights": weights,
            "picks": picks,
        })

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
    """Return (run_id_prefix, canonical_json_str) with `run_id` normalized to its 8-hex seed prefix."""
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
            generate_weather(rng, biome, day).value
            for day in (1, 25)
            for biome in Biome
        ]

        state = create_new_run(seed=seed, gm_profile=GMProfile.FIRESIDE, weirdness_level=2)
        assert state.rng_counter == rng.counter - 10
        prefix, canonical_json = canonical_run_state_json(state)

        vectors.append({
            "seed": str(seed),
            "run_id_prefix": prefix,
            "rng_counter": state.rng_counter,
            "rng_state_sha256": rng_hash,
            "weather_picks": weather_picks,
            "canonical_json": canonical_json,
        })

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
    T = IntentAction.TRAVEL
    R = IntentAction.REST
    H = IntentAction.HUNT
    P = IntentAction.REPAIR

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
        ("11_escape_valve_ration_seed_64", 64, GMProfile.FIRESIDE, [T, T, IntentAction.HARD_RATION, T, T], "valve"),
        ("12_escape_valve_cargo_seed_77", 77, GMProfile.FIRESIDE, [T, IntentAction.ABANDON_CARGO, T, R, T], "valve"),
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
                bp.last_settled_supplies = {"food": 40, "water": 38, "meds": 5, "ammo": 18, "parts": 4}
                bp.last_settlement_day = engine.state.day
                bp.nudge_shown = True
                stamp_minted_snapshot(
                    bp,
                    {"food": 50, "water": 50, "meds": 5, "ammo": 20, "parts": 4},
                )
                bp.settlements.append(SettlementRecord(
                    day=engine.state.day,
                    location=engine.state.location_id,
                    deltas={"food": -10, "water": -12},
                    txids=["ABCDEF0123456789ABCDEF0123456789"],
                    status="settled",
                    memo=f"TRAIL|RUN:{engine.state.run_id}|DAY:{engine.state.day}|V:v1",
                    timestamp="2026-10-23T00:00:00+00:00",
                ))
            elif tag == "backpack_parcels":
                bp = engine.state.backpack
                bp.enabled = True
                bp.wallet_address = "rParcelPlayerWallet3333333333"
                bp.issuer_address = "rParcelIssuerWallet4444444444"
                bp.trust_lines_ready = True
                bp.parcels.append(ParcelRecord(
                    parcel_id="rSender999:FOD:6",
                    sender="rSender999",
                    contents={"food": 6},
                    txid="11223344556677889900AABBCCDDEEFF",
                    accepted=True,
                    day_received=engine.state.day,
                ))
                bp.sent_parcels.append(SentParcelRecord(
                    recipient="rFriend888",
                    supply="water",
                    amount=5,
                    txid="FFEEDDCCBBAA00998877665544332211",
                    day_sent=engine.state.day,
                    memo=f"PARCEL|RUN:{engine.state.run_id}|DAY:{engine.state.day}|water:5",
                ))
                bp.permits.append(PermitRecord(
                    permit_id="permit_rope_bridge",
                    txid="99887766554433221100FFEEDDCCBBAA",
                    used=False,
                    day_earned=engine.state.day,
                    day_used=0,
                ))
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
            canonical_json = json.dumps(parsed_dict, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
            reloaded_for_snap = load_game(run_dir)
            assert reloaded_for_snap is not None
            minted_snapshot = minted_snapshot_of(reloaded_for_snap.backpack)

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

            saves_out.append({
                "label": label,
                "raw_save_json": raw_json,
                "canonical_save_json": canonical_json,
                "minted_snapshot": minted_snapshot,
                "next_100_draws_with_state": draws_with_state,
                "next_100_draws_counter_only": draws_counter_only,
            })

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
        d5["journal"] = [{
            "day": 2,
            "location": "Millford",
            "event_id": "legacy_ev",
        }]

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
            canonical_migrated = json.dumps(migrated_dict, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
            minted_snapshot = minted_snapshot_of(loaded.backpack)
            draws = next_100_draws_from_save(loaded.seed, loaded.rng_counter, loaded.rng_state)

            cases.append({
                "label": label,
                "input_json": input_json,
                "canonical_migrated_json": canonical_migrated,
                "minted_snapshot": minted_snapshot,
                "next_100_draws": draws,
            })

    return cases


# ── 4 & 5. Phase 1b Golden Harness: 1,000 Seeds × 60 Turns + 120 Save/Resume ─

def build_1000_seeds() -> list[int]:
    """1,000 distinct seeds including 0, 1, 42, negative ints, and ints above u32::MAX and u64::MAX."""
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
        return PlayerIntent(IntentAction.CHOOSE, choice_id=prng.choice(offered))
    if engine.phase == GamePhase.ROUTE:
        offered = ["A", "B", "C", "D"][: len(engine._pending_routes)]
        r = prng.random()
        if r < 0.02:
            return PlayerIntent(IntentAction.TRAVEL)
        if r < 0.04:
            return PlayerIntent(IntentAction.CHOOSE, choice_id="D")
        return PlayerIntent(IntentAction.CHOOSE, choice_id=prng.choice(offered))

    # Every 6th seed pushes hard to exercise breakdowns, starvation, dehydration, and deaths
    if seed_idx % 6 == 0:
        if turn == 1:
            return PlayerIntent(IntentAction.CHANGE_PACE, pace="hard")
        # Subset of hard-push seeds hunts repeatedly at camp first so food stays at 50 while water drains -> Dehydration deaths
        if seed_idx % 24 == 6 and turn <= 12 and engine.state.supplies.ammo > 0:
            return PlayerIntent(IntentAction.HUNT)
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
    """Generate 8 ending/finalization vectors covering all `compute_ending` tiers and `finalize_run`."""
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
    s2.journal.append(JournalEntry(
        day=5,
        location="Crossing",
        event_id="river_crossing",
        scene_title="Deep River",
        narration="",
        choice_made="A: Ford carefully",
        outcome="",
        deltas={},
        tags=["river", "ford"],
    ))
    cases.append(("triumphant_vow_broken_river", s2, False))

    # 3. Triumphant (vow broken: never_night + night tag)
    s3 = create_new_run(seed=42)
    s3.victory = True
    s3.game_over = True
    s3.day = 12
    s3.distance_traveled = s3.total_distance
    s3.taboo = "never_night"
    s3.journal.append(JournalEntry(
        day=4,
        location="Camp",
        event_id="wolf_pack",
        scene_title="Wolves",
        narration="",
        choice_made="A: Stand guard",
        outcome="",
        deltas={},
        tags=["danger", "night"],
    ))
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

    out = []
    for label, st, use_finalize in cases:
        raw_input = json.dumps(_state_to_dict(st), sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        if use_finalize:
            eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
            ending = eng.finalize_run("stalled")
            final_cod = eng.state.cause_of_death
            final_over = eng.state.game_over
        else:
            ending = compute_ending(st)
            final_cod = st.cause_of_death
            final_over = st.game_over
        out.append({
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
        })
    return out


def generate_golden_harness_vectors(meta: dict[str, str]) -> tuple[dict[str, Any], dict[str, Any]]:
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
            saved_at_20_raw: str | None = None

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

                turns.append({
                    "t": t,
                    "act": intent.action.value,
                    "cid": intent.choice_id,
                    "pace": intent.pace,
                    "cnt": eng.state.rng_counter,
                    "rng_h": rng_state_sha256_hex(eng.state.rng_state),
                    "st_h": state_hash,
                    "st_d": state_digest,
                    "ev": ev_dict,
                })

                if (
                    t == 20
                    and len(resume_runs) < 120
                    and (
                        (eng.phase == GamePhase.CAMP and eng.state.distance_remaining > 0)
                        or eng.phase == GamePhase.ROUTE
                    )
                ):
                    save_dir = base / f"resume_{len(resume_runs):03d}"
                    save_dir.mkdir(parents=True, exist_ok=True)
                    sp = save_game(eng.state, save_dir)
                    saved_at_20_raw = sp.read_text(encoding="utf-8")

                if eng.phase == GamePhase.GAME_OVER:
                    if eng.state.cause_of_death:
                        death_causes[eng.state.cause_of_death] += 1
                    break

            runs.append({
                "seed": str(seed),
                "turns": turns,
            })

            if saved_at_20_raw is not None and len(turns) > 20:
                resume_runs.append({
                    "seed": str(seed),
                    "seed_index": idx,
                    "save_turn": 20,
                    "raw_save_json": saved_at_20_raw,
                })

    full_library = [serialize_event_skeleton(ev) for ev in build_event_library()]
    assert len(full_library) == 260
    assert len(runs) == 1000
    assert len(resume_runs) == 120

    ending_vectors = generate_ending_vectors()

    print(
        f"Golden harness summary: seeds={len(runs)}, total_turns={total_turns}, "
        f"resume_runs={len(resume_runs)}, distinct_events={len(distinct_events)}/260"
    )
    print(f"  Actions: {dict(action_counts)}")
    print(f"  Phases:  {dict(phase_counts)}")
    print(f"  Deaths:  {dict(death_causes)}")

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
    resume_payload = {
        "schema": "etv-phase1b-resume-v1",
        "metadata": meta,
        "count": len(resume_runs),
        "save_turn": 20,
        "resume_runs": resume_runs,
    }
    return turns_payload, resume_payload


def main() -> None:
    t0 = time.time()
    out_dir = Path(__file__).resolve().parent / "vectors"
    out_dir.mkdir(parents=True, exist_ok=True)

    meta = get_oracle_metadata()
    print(f"Oracle: Python {meta['python_version']} at {meta['oracle_commit']} ({meta['oracle_tag']})")

    # 1. S1 & Gameplay RNG vectors
    s1_payload = {
        "schema": "etv-s1-rng-v2",
        "metadata": meta,
        "seed_vectors": [generate_s1_seed_vector(label, seed) for label, seed in S1_SEEDS],
        "save_restore_vector": generate_s1_save_restore_vector(),
    }
    s1_path = out_dir / "rng_s1_vectors.json.gz"
    with gzip.open(s1_path, "wt", encoding="utf-8", compresslevel=9) as f:
        json.dump(s1_payload, f, indent=2)
        f.write("\n")
    print(f"Wrote {s1_path} ({s1_path.stat().st_size} bytes)")

    # 2. Worldgen 200 seeds + 200 JSON events
    wg_payload = generate_worldgen_vectors(meta)
    wg_path = out_dir / "worldgen_200.json.gz"
    with gzip.open(wg_path, "wt", encoding="utf-8", compresslevel=9) as f:
        json.dump(wg_payload, f, indent=2)
        f.write("\n")
    print(f"Wrote {wg_path} ({wg_path.stat().st_size} bytes)")

    # 3. Saves 20 + Migrations + RNG restore
    saves_payload = {
        "schema": "etv-phase1a-saves-v1",
        "metadata": meta,
        "saves_20": make_20_real_saves(),
        "migrations": make_migration_vectors(),
    }
    saves_path = out_dir / "saves_20.json.gz"
    with gzip.open(saves_path, "wt", encoding="utf-8", compresslevel=9) as f:
        json.dump(saves_payload, f, indent=2)
        f.write("\n")
    print(f"Wrote {saves_path} ({saves_path.stat().st_size} bytes)")

    # 4 & 5. Golden harness: 1,000 seeds × 60 turns + 120 save-and-resume runs at turn 20
    turns_payload, resume_payload = generate_golden_harness_vectors(meta)
    turns_path = out_dir / "turns_1000_seeds.json.gz"
    with gzip.open(turns_path, "wt", encoding="utf-8", compresslevel=9) as f:
        json.dump(turns_payload, f, separators=(",", ":"), ensure_ascii=False)
        f.write("\n")
    print(f"Wrote {turns_path} ({turns_path.stat().st_size} bytes)")

    resume_path = out_dir / "resume_120_seeds.json.gz"
    with gzip.open(resume_path, "wt", encoding="utf-8", compresslevel=9) as f:
        json.dump(resume_payload, f, separators=(",", ":"), ensure_ascii=False)
        f.write("\n")
    print(f"Wrote {resume_path} ({resume_path.stat().st_size} bytes)")

    total_bytes = sum(p.stat().st_size for p in out_dir.glob("*.json.gz"))
    print(f"Total committed vector size: {total_bytes} bytes ({total_bytes / (1024 * 1024):.2f} MB) in {time.time() - t0:.1f}s")


if __name__ == "__main__":
    main()
