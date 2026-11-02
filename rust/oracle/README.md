# Escape the Valley — Python Reference Oracle (Frozen)

## Freeze Marker

The Python oracle (`rust/oracle/generate_vectors.py`) is permanently pinned to the final legacy Python commit (`87104824d1f5b8bfda5b0dc5c14bc5a8e3721407`). New behaviour introduced in Rust 2.0 after this point has no second implementation in Python; later Rust tests for new mechanics use hand-worked literal expected values rather than oracle comparison.

## Pin & Checksums

- **Pinned legacy Python commit SHA:** `87104824d1f5b8bfda5b0dc5c14bc5a8e3721407`
- **Python runtime:** CPython `3.12.13` (`python-build-standalone` build tag `20260510`)
- **`rust/oracle/generate_vectors.py` SHA-256 (`metadata.generator_sha256`):** `3917d0889f28f949f63c07f832c8cab9c04593dc0a49009f1dc3b17be19627a8`

| Vector File | Bytes | `.json.gz` SHA-256 | Header `metadata.vector_sha256` | Target Phase |
|---|---:|---|---|---|
| `vectors/rng_s1_vectors.json.gz` | 127,249 | `0c4204946dac928343569b324742e1944605fc6cecae3ac5b9ba739ce470b7cc` | `1f891b7b21ac17c33c9a88d3e01091a58c3c0a5b38d014a58ea82d5842586605` | Phase 1a (RNG) |
| `vectors/worldgen_200.json.gz` | 125,590 | `125802c245e785acdcc19764eb313c4c47192072f965db4c4bdddc0f317ce527` | `179944a803e73791d27b8831cc1b9f20eebb3584a02cd0b40068f240e7804003` | Phase 1a (Worldgen & Event JSON) |
| `vectors/saves_20.json.gz` | 253,580 | `5d2136bd93d2d5f7e9500b01247e98a0fa1a73251a022e10c8cab1607007cfab` | `cd5f10b46b5018ed3722b5c184087196549036223167e1ee3bc045476fcbfafa` | Phase 1a (Save/Load & Migrations) |
| `vectors/turns_1000_seeds.json.gz` | 7,526,207 | `146a87f029ec6993d5848ef1f04cc1e47391a6b871bd31b3bb164bc613d0ff0a` | `ce7feff96f7e7c22dd4a9925137c1fd10571afebd1bf9d50e1022c24fa375722` | Phase 1b (1,000 Seeds × 60 Turns) |
| `vectors/turns_long_seeds.json.gz` | 1,409,525 | `48fa1e5575ea1c34a0d4351de971e7cf9df47f00e17572e813f481cc00480f30` | `4c912f31e750ec3588db843ff5a54a593375e072033ab51ccec9cea16390237f` | Phase 1b (80 Long Seeds × 250 Turns) |
| `vectors/resume_120_seeds.json.gz` | 1,446,569 | `5691dd6fff8d6730a0dff2b75b52ae222d156117bc0a7d12eec568f8b74ecbbc` | `76fa1228de387ecaf689e08725f603bb7e95cf4253a5623099207a45801b8caf` | Phase 1b (120 Save/Resume Runs) |
| `vectors/ledger_reference.json.gz` | 75,011 | `ffe4b12dabeed12ec751469bb642bef353f5305c8da0d30d64d08d30756fd766` | `bcbc4a6b30aed16fddbb66f301f1498b6a69e01424dd64cdacf9c7891773d964` | Phase 2 (Trail Ledger, Postcard & Proof) |
| `vectors/notices_reference.json.gz` | 186,596 | `be8eec30247e4a9cd95dc4f5b40ea1e7776bbdb6a86634b0b786f835b21075ca` | `c56c3e2ee26d26dc3929624a36a81146face8546c605a85a7037dd1a8fcc59ee` | Phase 2 (`trail licenses` & `THIRD_PARTY_NOTICES.txt`) |
