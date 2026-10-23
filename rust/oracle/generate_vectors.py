#!/usr/bin/env python3
"""Oracle vector generator for Escape the Valley 2.0 Phase 1a.

Run against the pinned CPython 3.12.13 venv in the detached v1.3.1 (47b01a3)
oracle worktree:

    E:/AI/_worktrees/etv-oracle-131/.venv/Scripts/python.exe rust/oracle/generate_vectors.py

Produces gzip-compressed vector files in `rust/oracle/vectors/`:
1. `rng_s1_vectors.json.gz`   — S1 RNG call-shape vectors (with `pool_n1000_k20` label fix)
2. `worldgen_200.json.gz`     — 200 seeds: canonical RunState JSON + RNG state SHA-256 + weather/events
3. `saves_20.json.gz`         — 20 real Python saves across play stages + next 100 RNG draws + secrets
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
from pathlib import Path
from typing import Any

import escape_the_valley
from escape_the_valley.backpack_models import (
    ParcelRecord,
    PermitRecord,
    SentParcelRecord,
    SettlementRecord,
    stamp_minted_snapshot,
)
from escape_the_valley.event_loader import load_json_events
from escape_the_valley.gm import GMConfig
from escape_the_valley.intent import GamePhase, IntentAction, PlayerIntent
from escape_the_valley.models import Biome, GMProfile, RunState, SeededRNG
from escape_the_valley.save import (
    SAVE_DIR,
    SAVE_FILE,
    _state_to_dict,
    load_game,
    save_game,
)
from escape_the_valley.step_engine import StepEngine
from escape_the_valley.worldgen import (
    create_new_run,
    generate_map,
    generate_party,
    generate_weather,
    pick_twists,
)


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
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


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


# ── 1. S1 RNG Vectors (with pool_n1000_k20 renamed) ─────────────────────────

S1_SEEDS: list[tuple[str, int]] = [
    ("zero", 0),
    ("one", 1),
    ("forty_two", 42),
    ("u32_plus_7", (1 << 32) + 7),
    ("bits_100", (1 << 99) + 0x123456789ABCDEF012345678),
    ("negative_large", -98765432101234567890),
]

GETRANDBITS_WIDTHS: list[int] = [
    1, 2, 7, 16, 31, 32, 33, 48, 53, 63, 64, 65, 96, 100, 128,
]

RANDINT_RANGES: list[tuple[int, int]] = [
    (0, 0),
    (0, 1),
    (0, 2),
    (1, 6),
    (-5, 5),
    (8, 25),
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

CHOICE_LENGTHS: list[int] = [1, 3, 5, 18, 60]
SHUFFLE_LENGTHS: list[int] = [0, 1, 2, 5, 12, 25]

SAMPLE_CASES: list[tuple[str, int, int]] = [
    ("pool_k0", 5, 0),
    ("pool_k1", 5, 1),
    ("pool_n8_k2", 8, 2),
    ("pool_n21_k5", 21, 5),
    ("set_n22_k5", 22, 5),
    ("pool_n25_k6", 25, 6),
    ("pool_n85_k6", 85, 6),
    ("set_n86_k6", 86, 6),
    ("set_n50_k3", 50, 3),
    ("pool_n100_k10", 100, 10),
    ("set_n300_k10", 300, 10),
    ("pool_n1000_k20", 1000, 20),  # Renamed from set_n1000_k20 per Phase 1a brief
]

WEIGHTED_CASES: list[tuple[str, list[str], list[float]]] = [
    ("uniform3", ["a", "b", "c"], [1.0, 1.0, 1.0]),
    ("skewed4", ["river", "bandit", "weather", "omen"], [0.1, 0.25, 0.05, 0.6]),
    ("trail5", ["alpha", "beta", "gamma", "delta", "epsilon"], [10.0, 0.5, 0.5, 3.0, 1.0]),
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
    # For large >64-bit seeds that don't fit in i64, normalize seed in dict to string if outside i64
    if not (-(1 << 63) <= state.seed < (1 << 63)):
        d["seed"] = str(state.seed)
    canonical = json.dumps(d, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return prefix, canonical


def generate_worldgen_vectors(meta: dict[str, str]) -> dict[str, Any]:
    seeds = build_worldgen_seeds()
    vectors = []
    for seed in seeds:
        # Reproduce exact RNG state after create_new_run(seed)
        rng = SeededRNG(seed)
        _ = generate_map(rng)
        _ = generate_party(rng)
        _ = pick_twists(rng)
        from escape_the_valley.models import Doctrine, Taboo
        _ = rng.choice(list(Doctrine))
        _ = rng.choice(list(Taboo))
        rng_hash = rng_state_sha256_hex(rng.getstate())

        # Weather sample after worldgen for day 1 & day 25 across all 5 biomes
        weather_picks = [
            generate_weather(rng, biome, day).value
            for day in (1, 25)
            for biome in Biome
        ]

        state = create_new_run(seed=seed, gm_profile=GMProfile.FIRESIDE, weirdness_level=2)
        assert state.rng_counter == rng.counter - 10  # before the 10 weather_picks
        prefix, canonical_json = canonical_run_state_json(state)

        vectors.append({
            "seed": str(seed),
            "run_id_prefix": prefix,
            "rng_counter": state.rng_counter,
            "rng_state_sha256": rng_hash,
            "weather_picks": weather_picks,
            "canonical_json": canonical_json,
        })

    # Also include compiled-in JSON event skeletons summary for parity verification
    json_events = load_json_events()
    event_records = []
    for ev in json_events:
        event_records.append({
            "event_id": ev.event_id,
            "title": ev.title,
            "category": ev.category.value,
            "tags": ev.tags,
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
                }
                for c in ev.fallback_choices
            ],
            "outcomes": {
                cid: {
                    "supplies_delta": out.supplies_delta,
                    "health_delta": out.health_delta,
                    "wagon_delta": out.wagon_delta,
                    "animals_health_delta": out.animals_health_delta,
                    "morale_delta": out.morale_delta,
                    "time_cost": out.time_cost,
                    "distance_delta": out.distance_delta,
                    "special_flags": out.special_flags,
                }
                for cid, out in ev.outcome_templates.items()
            },
        })

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
    """Reproduce GameEngine/StepEngine RNG restore and record the next 100 mixed draws."""
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
    """Create 20 real Python saves across diverse points of play."""
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
            # Make run_id deterministic in committed test vectors so regenerating vectors is reproducible
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

            # Sync RNG state before saving, just as StepEngine._save() does
            engine.state.rng_counter = engine.rng.counter
            engine.state.rng_state = engine.rng.getstate()

            save_path = save_game(engine.state, run_dir)
            raw_json = save_path.read_text(encoding="utf-8")
            parsed_dict = json.loads(raw_json)
            canonical_json = json.dumps(parsed_dict, sort_keys=True, separators=(",", ":"), ensure_ascii=False)

            # Compute next 100 RNG draws from this exact saved state
            draws_with_state = next_100_draws_from_save(
                engine.state.seed,
                engine.state.rng_counter,
                parsed_dict["rng_state"],
            )
            # Also compute next 100 RNG draws when rng_state is None (counter-replay fallback)
            draws_counter_only = next_100_draws_from_save(
                engine.state.seed,
                engine.state.rng_counter,
                None,
            )

            saves_out.append({
                "label": label,
                "raw_save_json": raw_json,
                "canonical_save_json": canonical_json,
                "next_100_draws_with_state": draws_with_state,
                "next_100_draws_counter_only": draws_counter_only,
            })

    return saves_out


def make_migration_vectors() -> list[dict[str, Any]]:
    """Create legacy v1 and v2 save fixtures (using test_save.py / test_backpack_save.py patterns)
    and record the migrated v3 state produced by Python's load_game -> _state_to_dict."""
    cases: list[dict[str, Any]] = []

    with tempfile.TemporaryDirectory() as tmpdir:
        base = Path(tmpdir)

        # Case 1: v1 minimal save (pre-save_version, only 5 legacy supply keys, no rng_state,
        # no doctrine/taboo/rationing_steps/last_spoilage_day/backpack/memory_cards, unsalted run_id)
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

        # Case 2: v1 save with hostile/negative and phantom supplies (F-cfcd07e0 test fixture)
        s2 = create_new_run(seed=19)
        s2.run_id = "73475cb4-00000002"
        d2 = _state_to_dict(s2)
        d2.pop("save_version", None)
        d2.pop("rng_state", None)
        d2["supplies"] = {"food": -5, "water": 30, "ammo": 9999, "gold": 7, "silver": -10}

        # Case 3: v2 mid-evolution save (has save_version=1, backpack without sent_parcels,
        # no rng_state, no last_spoilage_day, dangling destination_id per F-e86c2e71)
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

        # Case 4: v2 save with malformed rng_state ([1, 2, 3]) and reserved permit shim
        s4 = create_new_run(seed=11)
        s4.run_id = "73475cb4-00000004"
        s4.rng_counter = 4
        s4.backpack.enabled = True
        s4.backpack.wallet_address = "rLegacyWallet123"
        snap = {"food": 50, "water": 50, "meds": 10, "ammo": 20, "parts": 10}
        stamp_minted_snapshot(s4.backpack, snap)
        d4 = _state_to_dict(s4)
        d4["rng_state"] = [1, 2, 3]

        # Case 5: v2 save with string rng_state ("nope") and missing optional member/journal fields
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
            draws = next_100_draws_from_save(loaded.seed, loaded.rng_counter, loaded.rng_state)

            cases.append({
                "label": label,
                "input_json": input_json,
                "canonical_migrated_json": canonical_migrated,
                "next_100_draws": draws,
            })

    return cases


def main() -> None:
    out_dir = Path(__file__).resolve().parent / "vectors"
    out_dir.mkdir(parents=True, exist_ok=True)

    meta = get_oracle_metadata()
    print(f"Oracle: Python {meta['python_version']} at {meta['oracle_commit']} ({meta['oracle_tag']})")

    # 1. S1 RNG vectors
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


if __name__ == "__main__":
    main()
