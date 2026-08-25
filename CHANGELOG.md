# Changelog

All notable changes to Escape the Valley are documented here.

## [Unreleased]

### Added

- **Classic CLI (`trail new` / `trail play`) now runs graded endings, food
  spoilage, escape valves, the rest/repair maintenance window, and town
  ledger settle** — same helpers StepEngine already used. Not a merge.
- **Hand-authored events that advertise rope/parts/food/tools now gate or
  debit** instead of applying an empty `supplies_delta`.
- **TUI start/HUD shows seed, doctrine, twists, taboo, and morale 0–100.**
  `c` cycles pace via existing `CHANGE_PACE`.
- **`trail ledger proof`** proves the loaded save (PASS/FAIL/INCONCLUSIVE).
  Named in README Commands and the handbook.

### Fixed

- **Every ledger overlay paints its dismiss/action row at 80×24**
  (enable failure, send-parcel failure, menu, nudge, learn, wallet, parcel,
  progress). Proof is `render_line` / region ∩ screen, not `visual.plain`.
- **EventBar paints D) Repair at 80×24.** App.CSS fully specifies `#eventbar`
  so leftover `tui.tcss` `height: 9` + tall border cannot clip the dock.
  Proof is `render_line` strips, not `visual.plain`.
- **Ledger overlays at 80×24 paint Accept/Refuse, FOOD rows, and Esc.**
  Compact templates; proof is painted strips ∩ screen.
- **TUI HUD reflows at 80×24 and 120×30** so supplies and status stay on
  screen. Title is Escape the Valley, not the class name. CLI `(CRITICAL)`
  is the full word at 80 columns.
- **Wallet Info and the XRPL postcard print the full classic r-address.**
  Parcel `From:` uses first-4…last-4 so prefix-8 senders no longer collide.
  Overlay balances use FOOD not FOD.
- **Handbook dark tokens apply only under a dark media/class**, so Starlight’s
  light palette is not beaten.
- **EVENT/ROUTE retry copy names offered letters** (`A/B`), not `(1-4)` / `(1/2)`.
  CLI `Choose` is letters too. Keyboard and prompt are the same document.
- **Live death line names the cause:** `{name} has died ({cause}).` Both engines.
- **Hard-ration refusals split:** food not low / cooldown remaining / already min.
- **Ollama HTTP 404 is model-missing**, not a JSON reject. Player sees `ollama pull`
  and `--gm-off`.
- **Extra-missing ledger cannot stay ON.** Message names
  `pip install "escape-the-valley[xrpl]"`.
- **TUI help matches live keys:** 1–7 and Shift+J. `--voice` mount toasts ON or
  an honest fail, never silence then OFF.
- **Handbook beginners keys match that TUI.**
- **`enable()` `_get_client()` lives inside the EnableResult try.** Every
  `_get_client()` / `Wallet.from_seed` in `backpack.py` degrades to a result
  object. Sweep leftover: found none.
- **README Security sentence matches SECURITY.md:** GM on by default
  (`--gm-off` to disable); XRPL off until `trail ledger enable`; voice opt-in.
- **Loaded supplies go through `SuppliesState.set` clamps.** A hostile or
  legacy save with negative stacks no longer loads illegal values.
- **Malformed `rng_state` degrades to counter-replay.** Load no longer
  bricks `trail play` on `SeededRNG.setstate`.
- **GameEngine gained StepEngine's arrival/travel extras** (water refill,
  supply cache, town trade, night-oil danger). Pairwise port, not a merge.
  Same seed on the fixed CLI engine still reproduces.
- **GM journal tags and scene narration no longer TypeError** on None /
  non-list tags or a list-shaped narration. GM-JSON still never bricks a run.
- **Ledger settle wraps `Wallet.from_seed` / memo build** into
  `SettlementResult` failure. Testnet only. No invented txid-skipping.
- **Missing `event_skeletons.json` still returns `[]` but logs ERROR.**
- **`trail self-check` probes event-library count, voice, and xrpl extra.**
  No third `__main__.py` env hook.
- **SECURITY.md matches the live CLI:** GM on by default (local Ollama;
  `--gm-off` to disable); XRPL off until enable.
- **TUI markup splices neutralize leftover `[`.** `textual.markup.escape()`
  leaves an unmatched `[` intact, so a GM choice label like `Look [ west`
  crashed EventBar on chrome `[/i]`. Same class as the ledger overlay fix;
  the helper is duplicated in `tui_app.py` (ledger-owned, not imported).
  Chrome `[b]`/`[i]`/`[dim]` stays markup.
- **Wallet overlay `settlements`/`pending` go through `_escape_dynamic`.**
  Production values are ints from `len()`; a hostile dict could still
  unbalance chrome.
- **Ledger overlay dynamic fragments all go through `_escape_dynamic`.**
  `show_failure` still used `escape()`, which does not neutralize a leftover
  `[` after truncation (`rSender[...`) — a reachable player-typed send-parcel
  path. `EnableFlowOverlay.show_success` interpolated the address raw. Chrome
  `[b]` stays markup.
- **GM-offered event letters are capped to templates that have an outcome.**
  A 4-choice GM scene on a 2-template event now offers A/B, not A/B/C/D that
  `resolve_event` cannot honor. Both engines. Fallback choices were already
  honest. The events visible-miss path stays as a backstop. The cap does not
  draw RNG.
- **GM card tags/entities drop non-string list elements.** A proposal like
  `tags: ["river", null]` no longer persists a `None` that later crashes
  `build_gm_brief`. Field-level null was already guarded; this is the
  list-element sibling. GM-JSON failure still must not brick a run, including
  one event later.
- **Ledger overlay leftover sinks escape dynamic fragments.** Parcel
  sender/contents, send-success message, send-form supplies text, and wallet
  address/issuer no longer crash `Static.update` on an orphan `[/tag]`. Chrome
  `[b]` stays markup. Not a uniform `markup=False`.
- **Shipped binaries actually run.** `src/escape_the_valley/__main__.py` used a
  package-relative import, so every PyInstaller binary since v1.1.0 crashed on
  `--help` with `ImportError: attempted relative import with no known parent
  package`. The entrypoint is now an absolute import. The release workflow
  also bundles `tui.tcss` and `data/event_skeletons.json` (`--add-data`, with
  the Windows `;` / Unix `:` separator PyInstaller actually parses). A
  post-build smoke (`scripts/smoke_test_binary.py`) asserts `--help` *and* a
  loaded event-library count of at least 200 — a `--help`-only check still
  passes on the 60-event quarter-game you get if the JSON is missing.
- **Spoilage fires once per qualifying day**, not once per TRAVEL action on
  that day. `last_spoilage_day` is persisted in the save. This changes the
  RNG draw count on any run that crosses a `day % 3 == 0` day, so a seed
  recorded on v1.1.1 will not replay identically on this build. Same seed on
  this build still reproduces, including across save/load.
- **Half-day hunt/repair consumption** no longer charges a full day when the
  daily cost is odd (`-1 // 2 == -1`). Shared `halve_consumption()` rounds
  toward zero.
- **Route choice** no longer maps an unknown letter to index 0, and a fork
  whose connections don't resolve no longer opens an empty ROUTE gate.
- **Uncanny tokens now spend on JSON-loaded events**, not just the 5
  hand-authored ones. Weirdness is therefore actually scarce (budget still 2
  per run, never regenerates). Token budget is not retuned here; that belongs
  with the deferred balance pass.
- **GM `memory_proposals: null` no longer crashes** a turn. Card ids are
  always engine-computed — a model-supplied `id` cannot collide with and
  suppress an engine-authored memory card.
- **TUI workers fail safe.** An exception in a `@work(thread=True)` worker
  used to leave `_in_flight` set forever ("the storyteller is thinking...").
  Workers now catch, clear the flag, and notify. Choice keys that aren't on
  the current prompt are ignored.
- **Ledger parcels** reject non-positive amounts at decode; `settle()` /
  `enable()` no longer re-submit a Payment that already confirmed when a later
  resource in the same batch fails; `check_parcels()` paginates via `marker`.
- **`yaml_to_json.py` refuses to overwrite** `event_skeletons.json` with an
  empty list when its `.txt` sources are missing.
- Release workflow routes the tag through `env:` rather than interpolating
  `workflow_dispatch` input into `run:`, and publishes the GitHub Release
  before `npm publish` so a failed release step cannot leave npm ahead of
  PyPI/binaries with no compensator.

## [1.1.1] - 2026-06-15

### Fixed

- **Voice extra now installs.** The `voice` extra pinned `voice-soundboard>=2.5`,
  which was never published to PyPI — so `pip install "escape-the-valley[voice]"`
  (and the binary build, which resolved the extra) failed. `voice-soundboard`
  2.5.2 is now on PyPI; the extra pins `voice-soundboard[kokoro]>=2.5` so the
  configured Kokoro voices synthesize out of the box.
- **Binary build no longer resolves optional extras.** `release-binaries.yml`
  now uses `uv run --no-sync`, so the PyInstaller binary builds from the base
  game only — voice's heavy backends (onnxruntime/kokoro) never bloat it. Voice
  stays pip-install-only via `escape-the-valley[voice]`.

## [1.1.0] - 2026-06-15

### Added

- **Streaming GM narration:** the storyteller now writes token-by-token, visibly composing each beat as Ollama generates it, threaded through the async TUI so the narration appears live instead of arriving in one block after a pause.
- **Graded endings + epilogue:** a run now ends with a graded `EndingResult` — **triumphant**, **weathered**, **pyrrhic**, or **lost** — read from survivors, days-versus-par, taboo, and uncanny, and narrated as a proper epilogue rather than a bare cause-of-death line.
- **Real stakes:** data-driven events can now wound or kill the party. A new event health path plus roughly 30 curated bodily-danger events mean a bad choice can cost a life, with attributed death causes (an event-wound death reads as *Injury*, not a generic "The trail").
- **Run artifacts:** an end-of-run screen surfaces the XRPL postcard, the run's stats, and an export/share path, so a finished run leaves something you can keep and pass along.
- **Cross-family GM panel simulator** (`scripts/sim_gm_panel.py`): validates GM behavior across model families, so the narrator can be checked for tone and safety on more than one backend.
- **Multiplayer parcel trading** over XRPL: `trail parcel send <addr> <supply> <amount>`, `trail parcel list`, `trail parcel accept <id>`, `trail parcel sent`, and `trail wallet share` — send supply tokens to another traveler via a Testnet micropayment with an attached memo, and accept incoming parcels into your backpack.
- **Ledger reconciliation proof harness** (audit mode): an on-ledger reconciliation proof that replays settlement receipts and verifies them against the XRPL Testnet, so a run's supply history can be independently audited.
- `--gm-profile` and `--weirdness` options on `trail tui`, matching the classic `trail new` CLI, so the GM voice and weirdness band can be set when launching the TUI.

### Changed

- **Event time-cost is real:** `WAIT` / `DETOUR` / `REST` event choices now spend clock time, so the risk-versus-time tradeoff actually bites — playing it safe costs days, and days are not free.

### Changed (Stage-C humanization)

- **Async, non-freezing GM and ledger calls:** narration and XRPL round-trips run off the UI thread, so a slow Ollama warm-up or a flaky Testnet no longer freezes the TUI. The first narrated turn loads the model and can take 10-30s by design — this is now documented as expected, not a hang.
- **Reachable escape valves:** hard ration, desperate repair, and abandon cargo are reachable from the TUI as genuine last resorts, with cooldowns and side effects intact.
- **Degradation signals surfaced to the UI:** `StepMessages` carries `gm_degraded` / `gm_degraded_reason`, the engine tracks `gm_calls` / `gm_fallbacks`, `GMClient` exposes a `.stats` dict, `VoiceBridge` exposes availability/last-error status, and `BackpackState` carries `last_settle_failed` — so the player can see when the GM has fallen back, when voice is unavailable, and when a settlement failed, instead of guessing.
- **Corrupt-save backup:** on a corrupt or unreadable `run.json`, the engine renames it to `run.json.corrupt-<timestamp>` before refusing it, so the next save can't clobber the only evidence and the file is recoverable.
- **Robust event loading:** the event loader tolerates malformed event entries and validates resource keys, so a single bad event definition degrades to a skip instead of crashing the run.
- **Testnet-only guard:** ledger operations assert the XRPL Testnet endpoint with a request timeout, keeping the game off mainnet and resilient to a stalled network.
- **Troubleshooting docs:** a new Troubleshooting handbook page (plus a README section) keyed to the three real failure modes — no narration ⇒ start Ollama or use `--gm-off`; ledger pending ⇒ `trail ledger reconcile`; save won't resume ⇒ recover from the `run.json.corrupt-*` backup. `trail self-check` is documented as the first thing to run.

### Fixed (Stage-A hardening)

- **Determinism across save/load:** the full PRNG state is now persisted, so resuming a saved game reproduces the same world and event stream as an uninterrupted run.
- **Secrets sidecar:** wallet and issuer seeds are split into a local `.trail/secrets.json` sidecar instead of the main save file, keeping private key material out of shareable saves.
- **Weirdness `>= 2` gate:** the uncanny is only available once a run crosses into `weirdness_level >= 2` (with tokens remaining); weirdness 0-1 stays grounded survival with no uncanny-token spend.
- **Ledger idempotency:** `ledger enable` and `accept_parcel` are now idempotent — repeating them does not double-enable, double-credit, or corrupt balances; balances are parsed exactly without binary-float drift.
- **External memo verification:** settlement memo verification reads the memo back from the ledger rather than trusting local state, making the receipt check genuinely external.
- **TUI persistence:** the TUI persists ledger and parcel state across actions, and guards outcome rendering against non-numeric deltas and missing labels.
- **Tone-lint on outcomes:** outcome narration is tone-linted so the engine's plain consequences read in the GM's period voice; event-caused deaths are attributed to their cause instead of a generic "The trail."
- **CI action pinning:** all GitHub Actions are pinned to full commit SHAs (publish, release-binaries), and CI installs the `xrpl` extra so the mock-based XRPL tests run instead of silently skipping.

## [1.0.1] - 2026-03-25

### Added
- `--version` / `-V` flag on root CLI (Typer callback)
- `trail stats` command — run summary with `--json` output (party, distance, wagon, journal, outcome)
- 7 new tests (347 total)

## [1.0.0] - 2026-03-04

### Added

- Full-screen Textual TUI with camp actions, event choices, and route forks
- Three GM profiles: Chronicler (spare), Fireside (default), Lantern-Bearer (weird)
- Voice narration via local audio synthesis (optional)
- XRPL Testnet ledger backpack: supply tokens, settlement receipts, parcels (optional)
- Procedural world generation with seeded RNG for reproducible runs
- Party traits, twist deck, and doctrine/taboo system
- Maintenance windows (rest+repair combo) for breakdown resistance
- Escape valves: hard ration, desperate repair, abandon cargo
- Warning callouts toggle (`--callouts verbose|minimal`)
- Oregon Trail-style death causes in trail ledger
- Memory card system for GM narrative continuity
- Supply cache discovery at nodes
- Spoilage mechanic (food decays without salt)
- Night travel danger (needs lantern oil)
- Classic CLI mode (`trail new`, `trail play`)
- Save/load with full backward compatibility
- 316 tests, lint clean
- Survival guide (`docs/survival-guide.md`)
