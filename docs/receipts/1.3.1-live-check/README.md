# 1.3.1 live check on `9773b08`

This is the live check the Editor asked for on **`9773b08`** (PR #25, `release/1.3.1` → `main`). That commit
carries the player-facing fixes from the 1.3.0 audit. Items 1–6 are below. Item 7 (the local binary build) is
**held**: the Editor asked me to wait for the next head on PR #25, which adds the licence notices (#28).

- **Code:** `9773b08` (`trail 1.3.1`) in a Python 3.12 venv with the `[xrpl]` extra. Each item also ran on
  `053fdcf2` (`trail 1.3.0`), the commit before it, so the before and after sit side by side in the `base-1.3.0/`
  folders (`code.txt`).
- **Play:** GM off throughout.
  - **The classic CLI** was played through a pipe by `harness/cli_drive.py.txt`. At each prompt it chose
    Travel, the first event choice, and route A.
  - **The TUI** is the real `LedgerTrailApp` and `StepEngine`, built the way `trail tui --seed N --gm-off`
    builds them. Textual's Pilot drives it with the player's keys (`harness/tui_drive.py.txt`), and the engine
    state is read after every key (`steps.jsonl`).
  - **Same play on both commits.** 1.3.1 leaves the RNG draw order alone, so the same inputs walk the same path
    on both commits. The CLI run took 41 camp actions on each.
- **Testnet:** items 4 and 5 made real Testnet wallets and transactions. `5-backpack/account_tx.txt` reads every
  transaction back from rippled, outside the game (`harness/etv_ledger_probe.py.txt`). The txids are in
  `hashes.txt`.
- **Redaction:** no seeds or wallet files are here. Every `.trail/` folder (`run.json`, `secrets.json`) was left
  out. The item 4 listings name each `secrets.json` entry's fields as `<present>`, never their values. Paths read
  `<scratch>` or `<home>`.

## Verdict

**PASS on items 1–6.** Every fix in the brief shows in play, and on 1.3.0 the same play shows the old behaviour.
Item 7 is held. Off the brief I found three player-facing defects that 1.3.0 has too (D1–D3). None comes from
1.3.1.

## Item 1: the classic CLI (H-1, M-1) — PASS (`1-cli/`)

`trail new --seed 42 --gm-off`, travelling at every camp prompt. By day 15 the whole party had died of disease.

| Check | 1.3.0 `053fdcf2` | 1.3.1 `9773b08` |
|---|---|---|
| Outcome | game over, day 15, 0/4 alive | the same |
| `trail stats`: memory cards | `Memories: 0 cards` | **`Memories: 12 cards`** (`--json`: `"memory_cards": 12`) |
| `trail stats`: outcome | `Defeated: All party members have perished.` | **`Defeated: Disease`** |
| Trail Ledger (`trail postcard`) cause | `Cause: All party members have perished..` | **`Cause: Disease.`** |
| Trail Ledger death days | `Ruth -- died of disease` (no day, all four) | **Ruth day 15, Josiah day 12, Elias day 9, Agnes day 12** |

The 12 cards are 4 sickness cards, 4 death cards, 3 omens and the Fellgate arrival. The death days match the
death cards. All three commands exit 0.

## Item 2: Abandon Cargo in the TUI (H-2) — PASS (`2-tui-abandon/`)

Seed 10 opens Abandon Cargo after 62 steps of travel: day 15, wagon 22%, cooldown 0. In `press-e/` the player
answers the ledger nudge (it opens at the start town) with `N`, travels on, and presses `e` (E) Abandon Cargo).

| | 1.3.0 | 1.3.1 |
|---|---|---|
| Narration | `Dropped: nothing.` | **`Dropped: -4 salt, -1 cloth, -1 rope, -1 boots.`** |
| Memory card | `Day 15: Abandoned cargo to save the wagon.` | **`Day 15: Abandoned cargo to save the wagon (dropped: -4 salt, -1 cloth, -1 rope, -1 boots).`** |
| What was dropped (state) | salt 8→4, cloth 1→0, rope 2→1, boots 2→1; wagon 22→47 | the same |

Both lines read `Abandoned cargo to lighten the wagon. Wagon +25. … Morale fell.` `before-e.svg` and
`after-e.svg` are the screens. `press-5-nudge-open/` picks the same valve with `5` while the nudge is still open,
with the same result. `press-e-nudge-open/` is D1.

## Item 3: a fork during a maintenance window (H-3) — PASS (`3-tui-fork/`)

`trail tui --seed 42 --gm-off`. Seed 42's first fork is Sunken Meadow (Ashen Bluffs or Devil's Bend), reached on
day 6. At the fork: `r` (rest), then `p` (repair), which opens the maintenance window, then `t`, `1` (route A)
and `t`.

| Key | What happens | `maintained_turns_remaining` 1.3.0 | 1.3.1 |
|---|---|---|---|
| `p` | "Maintenance window: the wagon rides steady. (-3 water)" | 2 | 2 |
| `t` | "The trail forks ahead. Choose your path." (route prompt) | **1** | **2** |
| `1` | "Heading toward Ashen Bluffs (11 miles)." | 1 | **2** |
| `t` | "Traveled 5 miles. 6 miles remaining." | 0 | **1** |

On 1.3.1 the counter holds through the route prompt and drops by 1 on the travel that follows. The rest is the
same on both commits: the miles, the event after the travel, and the day.

**In the CLI** the route prompt and the travel happen inside one action, so the counter dropped once either way;
there the change only moves the decrement after the prompt. I didn't run a CLI fork.

## Item 4: same-seed runs (M-5) — PASS (`4-same-seed/`)

In one folder: `trail new --seed 42 --gm-off`, then `trail ledger enable`. Then `trail new --seed 42 --gm-off`
again (answering `y` to replace the save), then `trail ledger enable` again.

| | Run A | Run B |
|---|---|---|
| `run_id` | `73475cb4-2c853b9b` | `73475cb4-93d7d481` |
| `ledger enable` | exit 0, wallet `rHJgLAiK…UKUV` | exit 0, wallet `rwmL1Cmj…Maok` |
| `.trail/secrets.json` after the enable | 1 entry: A (wallet and issuer secrets present) | **2 entries:** A and B, both present |

The seed prefix `73475cb4` is the same, and the salt differs. On 1.3.0 every seed-42 run here got the bare
`73475cb4` (`1-cli/base-1.3.0/`, and the `base-1.3.0/` TUI runs). The sidecar is keyed by `run_id`, so on 1.3.0 a
second enable would have replaced the first run's entry. I didn't spend faucet grants to show that on 1.3.0.

## Item 5: the backpack on Testnet — PASS (`5-backpack/`)

Runs A and B are the two runs from item 4, each copied into its own folder.

1. **Run A** travels to Fellgate, the first town past the start, on day 11. The CLI settles there:
   `Checkpoint settled. Receipt: E07719F552F1...`.
2. **`trail ledger proof`** on run A: **`# Ledger Reconciliation Proof — PASS`**, exit 0. It shows 1 settlement,
   none pending, and on-chain and local memo integrity `ok`. Engine and ledger agree on all five resources: AMO 13,
   FOD 26, MED 5, PRT 6, WTR 50. `ledger status` and `ledger wallet` agree too.
3. **The parcel:** `trail parcel send rwmL1CmjeA71ALAJWjKtizTJTgBCJdMaok food 5` from run A: exit 0,
   `Sent 5 food to rwmL...Maok. Receipt: 440A2E758EAD...`.
4. **Run B** travels to Fellgate and settles. The game prints `A parcel arrived from rHJgLAiK...: 5 food`.
5. **Run B accepts it:** `trail parcel accept 440A2E75…` (the full id) gives exit 0 and `Parcel accepted: +5 food`.
   Run B's proof then gives **PASS**, exit 0.

**On chain** (`account_tx.txt`): all 29 transactions on the two wallets are `tesSUCCESS` and validated. That
covers the faucet funding, 5 trust lines and 5 opening mints for each run, 3 settlement payments each, and the
parcel. The parcel is a 12-drop XRP payment with the memo `PARCEL|RUN:73475cb4-2c853b9b|DAY:12|food:5`. Each
run's settlement memos carry its own salted run id (`TRAIL|RUN:73475cb4-2c853b9b|…` and
`TRAIL|RUN:73475cb4-93d7d481|…`), so same-seed runs no longer share a memo id (M-5).

**A note on the proof:** its Engine column is the last settled supplies, not the current ones. So the parcel's −5
and +5 food reach the proof at the next settlement. That's by design: the proof checks what has been settled.

## Item 6: #19, a cp1252 pipe on Windows — PASS (`6-cp1252/`)

`printf 'Q\n' | PYTHONIOENCODING=cp1252 trail new --seed 42 --gm-off`

| | 1.3.0 | 1.3.1 |
|---|---|---|
| Exit | **1**, `UnicodeEncodeError: 'charmap' codec can't encode characters…` while drawing the status panel | **0** |
| Reaches the camp prompt | no | yes: `What do you do?`, then `Game saved.` |
| `.trail/` written | no | yes |

The CLI driver's own runs show the same thing. With no `PYTHONIOENCODING`, a Windows pipe defaults to cp1252.
1.3.1 played through, and 1.3.0 crashed before the first prompt
(`1-cli/base-1.3.0/00-new-seed42-piped-cp1252-default.out`).

## Item 7: the binary (M-6) — HELD

Held at the Editor's request until PR #25's next head (the #28 licence notices, a `licenses` command and a
release-workflow change). A short re-check will cover the local PyInstaller build, `self-check`
("xrpl extra present"), an enable from the frozen binary, and the `licenses` command together.

## Off the brief (all three are on 1.3.0 too; none from 1.3.1)

- **D1 (minor, UX): in the TUI, `e` on the open ledger nudge starts a Testnet enable instead of Abandon Cargo**
  (`2-tui-abandon/press-e-nudge-open/`, `base-1.3.0/press-5-nudge-open/`).
  - **When it happens:** the "Ledger Backpack available / E) Enable now N) Not now" nudge opens once, on a town.
    On seed 10 that is the start town, Copperhill, after the first key. It stays open while `t`, `r`, `h`, `p`
    and `1`–`7` keep playing. So it was still on screen 62 steps later, when the wagon fell to 22% and the camp
    menu offered `E) Abandon Cargo`. A player who presses `e` there gets the enable flow, which makes faucet
    wallets, and the valve doesn't run.
  - **The probe:** with the network blocked, `e` gave `show_enable_flow=True`,
    `Backpack enable failed: All connection attempts failed`, and wagon 22→22. `5` picks the valve with the nudge
    open (`press-5-nudge-open/`).
  - **Why it's minor:** the help says "e/f/g pick E/F/G when no overlay is open", and the nudge does say
    `E) Enable now`. The problem is that the nudge outlives the town while the game carries on behind it.
  - **The cost to this check:** my first three probes hit it without the network blocked and made 3 pairs of
    Testnet wallets (`hashes.txt`).
  - `tui_app.py` is unchanged between 1.3.0 and 1.3.1.
- **D2 (cosmetic): `trail parcel list` drops the status label.** Before and after the accept, the line reads
  `   From rHJgLAiK...: 5 food (day 11)`. The code prints `[pending]` / `[accepted]` / `[refused]` through Rich,
  which reads them as markup tags and drops them (`5-backpack/runB/02-parcel-list.out`, `06-parcel-list.out`).
- **D3 (minor, UX): the CLI never shows the parcel id that `trail parcel accept` needs.** The arrival line, `parcel
  list` and `parcel accept --help` don't show it, and a prefix is refused (`04-parcel-accept-prefix.out`:
  `Parcel not found or already accepted.`, exit 1). I took the full id from the save. The TUI accepts with `A` on
  the parcel overlay, so this is CLI-only.

## Files

- `1-cli/`: the seed 42 run (`01-new-seed42.out`, the whole transcript with each answer marked `<<kind -> answer>>`),
  then `stats`, `stats --json` and `postcard`, each as `.cmd`, `.out`, `.err` and `.exit`. `base-1.3.0/` holds
  the same, plus the default-encoding crash.
- `2-tui-abandon/`: `press-e/`, `press-5-nudge-open/` and `press-e-nudge-open/` (D1), plus `base-1.3.0/`. Each
  holds `drive.*`, `steps.jsonl`, the narration before and after, `abandon-card.json` and screens (`.svg`).
- `3-tui-fork/`: `drive.*`, `steps.jsonl`, the narration after each key, and screens at the fork and after each
  `t`. `base-1.3.0/` holds the same.
- `4-same-seed/`: the two `new` and `enable` runs, and `03-after-A.txt` / `06-after-B.txt` (the save's run id and
  wallet, and the `secrets.json` entries with values withheld).
- `5-backpack/`: `runA/`, `runB/` and `account_tx.txt`.
- `6-cp1252/`: the pipe test, plus `base-1.3.0/`.
- `code.txt`, `hashes.txt`.
- `harness/`: `cli_drive`, `tui_drive`, `tui_key_probe` (D1), `seed_search` (how seed 10 was found),
  `etv_ledger_probe` and `redact_tree`. Each is a `.py.txt`, which keeps them out of the repo's lint.
