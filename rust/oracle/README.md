# Escape the Valley — Python Reference Oracle (Frozen)

## Freeze Marker

The Python oracle (`rust/oracle/generate_vectors.py`) is permanently pinned to the final legacy Python commit (`87104824d1f5b8bfda5b0dc5c14bc5a8e3721407`). New behaviour introduced in Rust 2.0 after this point has no second implementation in Python; later Rust tests for new mechanics use hand-worked literal expected values rather than oracle comparison.

## Frozen Legacy Content Snapshot (`rust/oracle/content-legacy/`)

`rust/oracle/content-legacy/content_manifest.json` (`f84f98ea845bbfc9a0e0198ba9a29011104a2ed410d40867bb38a82dab342b9f`) and `rust/oracle/content-legacy/event_skeletons.json` (`3402c6e1a1508fd55ecc60e2bfe819f60c219638f10d10e2881ca2d237a44576`) are a permanently frozen, byte-identical snapshot of the legacy content pack (`content_hash` `7836724a01c9db8e8e97b234a817b4f3be44a923677292af96f8426d6540fe74` at `LOGIC_VERSION = "etv-rust-2.0-logic-v1"`). Tier 1 oracle parity (`rust/etv-core/tests/oracle_parity.rs`) always loads this frozen snapshot through `ContentPack` rather than the live embedded content in `rust/etv-core/content/`, so authoring `outcome_sentence` or other prose in live content never breaks Tier 1 mechanics parity, while Tier 2 (`rust/etv-core/src/snapshot.rs`) continues to read the live embedded content. This snapshot never changes; producing a new legacy snapshot would require a new oracle run, which is outside normal content authoring.

## Pin & Checksums

- **Pinned legacy Python commit SHA:** `87104824d1f5b8bfda5b0dc5c14bc5a8e3721407`
- **Python runtime:** CPython `3.12.13` (`python-build-standalone` build tag `20260510`)
- **`rust/oracle/generate_vectors.py` SHA-256 (`metadata.generator_sha256`):** `b632c11aadbdbaa110c99c0b4062fcfb32f2daa82d22b8c3d129276fd0243639`
- **`rust/oracle/content-legacy/content_manifest.json` SHA-256 (`metadata.content_legacy_manifest_sha256`):** `f84f98ea845bbfc9a0e0198ba9a29011104a2ed410d40867bb38a82dab342b9f`
- **`rust/oracle/content-legacy/event_skeletons.json` SHA-256 (`metadata.content_legacy_event_skeletons_sha256`):** `3402c6e1a1508fd55ecc60e2bfe819f60c219638f10d10e2881ca2d237a44576`

| Vector File | Bytes | `.json.gz` SHA-256 | Header `metadata.vector_sha256` | Target Phase |
|---|---:|---|---|---|
| `vectors/rng_s1_vectors.json.gz` | 127,398 | `b4cec10f901eb4febb930d273d825bffc4fee512d29cf1aacb73996e6b4c698b` | `1f891b7b21ac17c33c9a88d3e01091a58c3c0a5b38d014a58ea82d5842586605` | Phase 1a (RNG) |
| `vectors/worldgen_200.json.gz` | 125,739 | `c7a51b2c06b84172391f85beaa519d75074475946eb51d86db534c3aac48b768` | `179944a803e73791d27b8831cc1b9f20eebb3584a02cd0b40068f240e7804003` | Phase 1a (Worldgen & Event JSON) |
| `vectors/saves_20.json.gz` | 253,713 | `fe3f882d815a4c3a1aaa9d5185a999de12ac62f63b246d767b5efa8b4b47e1dc` | `cd5f10b46b5018ed3722b5c184087196549036223167e1ee3bc045476fcbfafa` | Phase 1a (Save/Load & Migrations) |
| `vectors/turns_1000_seeds.json.gz` | 7,526,397 | `89abab8e3829f4bbe2d4ce49ebd08908aea008d159dc82a733c5b988d229a47c` | `ce7feff96f7e7c22dd4a9925137c1fd10571afebd1bf9d50e1022c24fa375722` | Phase 1b (1,000 Seeds × 60 Turns) |
| `vectors/turns_long_seeds.json.gz` | 1,409,694 | `df8ce43cd842fec6a48908a00a45b405c64cdb9ba9b8ac782d1081b3e0e9a865` | `4c912f31e750ec3588db843ff5a54a593375e072033ab51ccec9cea16390237f` | Phase 1b (80 Long Seeds × 250 Turns) |
| `vectors/resume_120_seeds.json.gz` | 1,446,712 | `6af98bd0175d8bdf95e59d2151629b59d00c0fc7aa89e0b8dd50f47fe63ea112` | `76fa1228de387ecaf689e08725f603bb7e95cf4253a5623099207a45801b8caf` | Phase 1b (120 Save/Resume Runs) |
| `vectors/ledger_reference.json.gz` | 75,166 | `688cdc9491e03f5d44a50d792c836a1ed05cff99fede4b728e844716ad9bc743` | `bcbc4a6b30aed16fddbb66f301f1498b6a69e01424dd64cdacf9c7891773d964` | Phase 2 (Trail Ledger, Postcard & Proof) |
| `vectors/notices_reference.json.gz` | 186,734 | `4d75fa318f29677255578b547d051b0984048b500e4d9dab2b221b05e133a242` | `c56c3e2ee26d26dc3929624a36a81146face8546c605a85a7037dd1a8fcc59ee` | Phase 2 (`trail licenses` & `THIRD_PARTY_NOTICES.txt`) |
