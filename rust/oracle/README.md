# Escape the Valley — Python Reference Oracle (Frozen)

## Freeze Marker

The Python oracle (`rust/oracle/generate_vectors.py`) is permanently pinned to the final legacy Python commit (`87104824d1f5b8bfda5b0dc5c14bc5a8e3721407`). New behaviour introduced in Rust 2.0 after this point has no second implementation in Python; later Rust tests for new mechanics use hand-worked literal expected values rather than oracle comparison.

## Pin & Checksums

- **Pinned legacy Python commit SHA:** `87104824d1f5b8bfda5b0dc5c14bc5a8e3721407`
- **Python runtime:** CPython `3.12.13` (`python-build-standalone` build tag `20260510`)
- **`rust/oracle/generate_vectors.py` SHA-256:** `86aeb3f08f9bdc05680d16ef5ba01efb93590688520dddfc9d98479fb5b263d1`
- **`rust/oracle/generate_vectors.py` post-docstring code SHA-256:** `bc2ca5f8276599ddc32908e5063a78a93726342903c961c6bb526c68eb87ccd0`

| Vector File | Bytes | SHA-256 | Target Phase |
|---|---:|---|---|
| `vectors/rng_s1_vectors.json.gz` | 126,685 | `3502a1a025af60ba887dc0bd4f3d28a0efe54de5f07007c51b329a90bcb200af` | Phase 1a (RNG) |
| `vectors/worldgen_200.json.gz` | 125,068 | `8d0687472f3d8bd747c65ea10160a6ff593c9fdd19a1e95d1e6bfa6330c9a054` | Phase 1a (Worldgen & Event JSON) |
| `vectors/saves_20.json.gz` | 253,097 | `29cb378837c3771bf7e82f3878488f5e2c45c2cfcd7458ffdd8d248e0e3ce027` | Phase 1a (Save/Load & Migrations) |
| `vectors/turns_1000_seeds.json.gz` | 7,525,427 | `61525662348f3c7874fe74cb9c71ce5de2f6f6553e83a65aef19f8d19196b418` | Phase 1b (1,000 Seeds × 60 Turns) |
| `vectors/turns_long_seeds.json.gz` | 1,408,800 | `94278582e7be5eac3e5c0ba7f608d8a832e800bddaebc9c1ae67c7c46e570435` | Phase 1b (80 Long Seeds × 250 Turns) |
| `vectors/resume_120_seeds.json.gz` | 1,446,080 | `f40d1adb6d7eba495732cf3fe13f0144b708f10d89d8c405f46dcb4c4769e4ac` | Phase 1b (120 Save/Resume Runs) |
| `vectors/ledger_reference.json.gz` | 74,422 | `5d04b91a438c5a27487573a7b2443a784260721c1ccdf780cf5538a878465605` | Phase 2 (Trail Ledger, Postcard & Proof) |
| `vectors/notices_reference.json.gz` | 186,064 | `d8edd4d272c827b5e44ad379dc2262b7380b5d8a22be5b4855b401e282a48c6f` | Phase 2 (`trail licenses` & `THIRD_PARTY_NOTICES.txt`) |
