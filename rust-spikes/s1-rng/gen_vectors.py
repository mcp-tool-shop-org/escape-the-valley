#!/usr/bin/env python3
"""Generate bit-exact CPython MT19937 / SeededRNG test vectors for Spike S1."""

from __future__ import annotations

import json
import random
import struct
import sys
from pathlib import Path
from typing import Any


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


SEEDS: list[tuple[str, int]] = [
    ("zero", 0),
    ("one", 1),
    ("forty_two", 42),
    ("u32_plus_7", (1 << 32) + 7),
    ("bits_100", (1 << 99) + 0x123456789ABCDEF012345678),
    ("negative_large", -98765432101234567890),
]

GETRANDBITS_WIDTHS: list[int] = [1, 2, 7, 16, 31, 32, 33, 48, 53, 63, 64, 65, 96, 100, 128]

RANDINT_RANGES: list[tuple[int, int]] = [
    (0, 0),
    (0, 1),
    (0, 2),          # n=3 (just above 2^1, triggers _randbelow rejection loop)
    (1, 6),          # n=6
    (-5, 5),         # n=11, signed
    (8, 25),         # n=18 (just above 2^4=16, high rejection rate)
    (0, 32),         # n=33 (just above 2^5=32, almost 50% rejection rate)
    (0, 255),        # n=256 (exact 2^8)
    (0, 256),        # n=257 (just above 2^8)
    (0, 65535),      # n=65536 (exact 2^16)
    (0, 65536),      # n=65537 (just above 2^16)
    (-100000, 100000),
    (0, (1 << 32) - 1),
    (0, (1 << 32)),  # n=2^32 + 1 (33-bit _randbelow rejection loop)
    (-(1 << 40), (1 << 40)),
]

CHOICE_LENGTHS: list[int] = [1, 3, 5, 18, 60]

SHUFFLE_LENGTHS: list[int] = [0, 1, 2, 5, 12, 25]

# CPython Random.sample threshold:
# setsize = 21 + (4 ** ceil(log(k * 3, 4)) if k > 5 else 0)
# Pool branch runs when n <= setsize; Set branch runs when n > setsize.
SAMPLE_CASES: list[tuple[str, int, int]] = [
    ("pool_k0", 5, 0),
    ("pool_k1", 5, 1),
    ("pool_n8_k2", 8, 2),        # setsize=21, 8 <= 21 (pool)
    ("pool_n21_k5", 21, 5),      # setsize=21, 21 <= 21 (pool boundary)
    ("set_n22_k5", 22, 5),       # setsize=21, 22 > 21 (set boundary)
    ("pool_n25_k6", 25, 6),      # k=6 -> setsize=21+64=85, 25 <= 85 (pool)
    ("pool_n85_k6", 85, 6),      # setsize=85, 85 <= 85 (pool boundary)
    ("set_n86_k6", 86, 6),       # setsize=85, 86 > 85 (set boundary)
    ("set_n50_k3", 50, 3),       # setsize=21, 50 > 21 (set)
    ("pool_n100_k10", 100, 10),  # k=10 -> setsize=21+256=277, 100 <= 277 (pool)
    ("set_n300_k10", 300, 10),   # setsize=277, 300 > 277 (set)
    ("pool_n1000_k20", 1000, 20), # setsize=1045, 1000 <= 1045 (large pool)
]

WEIGHTED_CASES: list[tuple[str, list[str], list[float]]] = [
    ("uniform3", ["a", "b", "c"], [1.0, 1.0, 1.0]),
    ("skewed4", ["river", "bandit", "weather", "omen"], [0.1, 0.25, 0.05, 0.6]),
    ("trail5", ["alpha", "beta", "gamma", "delta", "epsilon"], [10.0, 0.5, 0.5, 3.0, 1.0]),
]


def serialize_state(state: tuple[Any, ...]) -> list[Any]:
    """Serialize random.getstate() the same way .trail/run.json does via json.dumps."""
    version, internal, gauss_next = state
    return [version, list(internal), gauss_next]


def generate_seed_vectors(label: str, seed: int) -> dict[str, Any]:
    rng = random.Random(seed)
    initial_state = serialize_state(rng.getstate())

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

    final_state = serialize_state(rng.getstate())

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


def generate_save_restore_vector() -> dict[str, Any]:
    """Simulate a mid-run .trail/run.json rng_state save and restore."""
    rng = random.Random(42)
    # Advance through a realistic mix of worldgen-style calls
    for _ in range(17):
        rng.random()
    for _ in range(9):
        rng.randint(8, 25)
    names = ["Ruth", "Josiah", "Elias", "Agnes", "Martha", "Thomas", "Caleb", "Hannah"]
    rng.shuffle(names)
    _ = rng.sample(list(range(30)), 4)

    # Round-trip through JSON just like save.py -> .trail/run.json
    saved_json_str = json.dumps(rng.getstate())
    restored_list = json.loads(saved_json_str)

    # Continue on original rng AND on a newly restored rng
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
        "post_final_state": serialize_state(restored_rng.getstate()),
    }


def main() -> None:
    out_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("vectors.json")
    payload = {
        "schema": "etv-s1-rng-v1",
        "seed_vectors": [generate_seed_vectors(label, seed) for label, seed in SEEDS],
        "save_restore_vector": generate_save_restore_vector(),
    }
    out_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {out_path} (Python {sys.version.split()[0]})")


if __name__ == "__main__":
    main()
