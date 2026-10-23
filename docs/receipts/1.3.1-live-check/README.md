# 1.3.1 live check on `9773b08`, then `0d112bc`, `c6c30b5`, `7481d88`, `24926be` and the released npm package

This is the live check the Editor asked for on **`9773b08`** (PR #25, `release/1.3.1` → `main`). That commit
carries the player-facing fixes from the 1.3.0 audit. Items 1–6 ran on it. Item 7, the local binary build, ran on
the next head, **`0d112bc`**, which adds the licence notices (#28), as the Editor asked.

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

**PASS on items 1–6** on `9773b08`. Every fix in the brief shows in play, and on 1.3.0 the same play shows the
old behaviour. **Item 7 PASS** on `0d112bc` (#28): the locally built binary passes the smoke test, `self-check`,
a Testnet enable, and `licenses` in both frozen and pip modes. The notices content check waits for the final head.
The identity scan is `RESULT CLEAN` on the `.exe` but cannot see inside it; unpacked, it finds 28 third-party hits,
reported to the Publisher. **Item 8 PASS** on `c6c30b5`: a Linux build in WSL passes the new
`--verify-toc` step (exit 0, 22 distributions; a negative control exits 1) and the smoke test. Off the brief I
found three player-facing defects that 1.3.0 has too (D1–D3). None
comes from 1.3.1.

**Item 9, the final candidate `7481d88`:** a Linux build that follows the workflow exactly passes `--verify-toc`,
`--verify-bundle`, the smoke test and the notices content check. The vendored licence texts and the listed
distributions are right. There are two findings:
- **G1:** the binary's `_sysconfigdata` carries the build machine's home path (`/home/runner/…` on CI), and the
  gate reports it.
- **N1 (minor):** the game LICENSE header hash depends on line endings.

**G1 was open at `7481d88`.** The unpacked Linux check found the build machine's (on CI, the runner's) Python
install prefix in `_sysconfigdata`, and item 9's PASS doesn't cover it. The Publisher ruled to fix it, with no
exception.

`trail licenses` works from a pip install on Python 3.13.

**Item 10: G1 is fixed at `24926be`.** A Linux build that follows the workflow, with its new
`UV_PYTHON_INSTALL_DIR=/opt/uv-python` step, passes `--verify-toc`, `--verify-bundle` and the smoke test.
- The bundled `_sysconfigdata` holds `/opt/uv-python/…` and no home path.
- The unpacked identity scan is the 8 third-party hits alone: no `_sysconfigdata` hit and no build record.
- The new bundle check fails the xrpl-camp 1.5.0 release asset (it names both `direct_url.json` and
  `_sysconfigdata`) and fails the `7481d88` binary from item 9.

N1 is unchanged: the notices file is byte-identical to item 9's.

**Item 11: the released npm package, `@mcptoolshop/escape-the-valley@1.3.1`, passes on Linux. On Windows it passes
everything but the licences match (N2).**
- **Both platforms:** from fresh caches, `--version` prints `trail 1.3.1`; the help reads
  `Usage: escape-the-valley …`; and the downloaded binaries match `checksums-1.3.1.txt`.
- **Linux:** `licenses` is byte-identical to the release's `THIRD_PARTY_NOTICES-linux-x64.txt`, and
  `--verify-bundle` passes the release asset.
- **N2, Windows:** `licenses` isn't byte-identical to `THIRD_PARTY_NOTICES-win-x64.txt`. The release file has 291
  lines ending `\r\r\n`, from licence texts kept with their CRLF endings. The binary reads those back as extra blank
  lines.
- **pip:** `pip install escape-the-valley==1.3.1` on Python 3.13 runs `trail licenses` with exit 0.

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

## Item 7: the binary (M-6, #28) on `0d112bc` — PASS, notices content pending (`7-binary/`)

This ran on **`0d112bc`**, the #28 fixup on top of `9773b08`, in a clean checkout at that commit. It used the
Windows leg of `release-binaries.yml`, step by step:
1. `uv python install 3.12`, then `uv venv -p 3.12`, then `uv pip install ".[xrpl]" "pyinstaller>=6.9.0"`.
2. `python scripts/gen_third_party_notices.py THIRD_PARTY_NOTICES.txt`.
3. The workflow's `pyinstaller --onefile …` line with `DATA_SEP=;`, including the new
   `--add-data "THIRD_PARTY_NOTICES.txt;escape_the_valley"`.

The logs are `01-install-notices.log` and `02-pyinstaller.log`. The binary, `escape-the-valley-1.3.1-win-x64.exe`,
is **17,719,073 bytes**, sha256 `f796a08abd9367217a58f366fc78f4a750621a90e29ba6a76c9ed5c228751e30`. PyInstaller is
6.22.3 and Python 3.12.13.

| Check | Result |
|---|---|
| `scripts/smoke_test_binary.py` (5 checks) | **PASS**, exit 0: `--help`; 260 events (floor 200); `tui.tcss` loads; xrpl extra present; third-party licences present |
| `--version` | `trail 1.3.1` |
| `self-check` | exit 0: **`xrpl extra present`**, `Event library: 260 events`, `TUI stylesheet found: tui.tcss` |
| Backpack enable from the binary | `new --seed 7 --gm-off` (run `7902699b-68895840`), then `ledger enable`: **exit 0**, wallet `rweCbG8xuxLFQGdp9XYFK9SuwybzfZLsj2`. `ledger wallet`: trust lines Yes, balances AMO 20, FOD 50, MED 5, PRT 4, WTR 50 |
| `licenses` and `--licenses` (binary) | exit 0, 134,519 bytes each, **byte-identical to the bundled `THIRD_PARTY_NOTICES.txt`** (sha256 `850fac2e…`) |
| `licenses` and `--licenses` (pip install) | A normal, non-editable `uv pip install "<0d112bc>[xrpl]"`. No bundled file (`resolve_bundled_notices_path()` is `None`). Both exit 0 and **generate the text at run time**: 100,748 bytes, identical to each other, with every marker the smoke test checks |

- **The licences text is not committed.** `licenses-hashes.txt` has its hashes and sizes. The Editor asked for the
  content check to wait for the final head, because R&D's conditional pass adds CPython's licence and a certifi
  source line.
- **Why the binary's notices are larger than pip's:** the binary's list includes PyInstaller, whose bootloader
  ships inside the binary. The pip install doesn't have it.

**The identity scan on the binary:** `identity-scan.py` on the `.exe` gives **`RESULT CLEAN`**, exit 0
(`09-identity-scan-binary.txt`). Verbatim:

```
RESULT CLEAN
```

That result doesn't cover the bundled files. PyInstaller stores them, and the PYZ of Python modules,
zlib-compressed inside the `.exe`, and the scanner reads an `.exe` only as raw bytes. The positive control:
`Copyright (c) 2026 mcp-tool-shop`, which `licenses` prints, appears 0 times in the raw `.exe`.

So I unpacked it. `harness/unpack_pyinstaller.py.txt`, using PyInstaller's own readers, wrote 176 archive files and
1,520 modules, and the control string is there. The scan of that tree gives **`RESULT HIT 28`**
(`11-identity-scan-unpacked.txt`). `12-hit-sources.md` names each one:
- **22 home-path hits:**
  - 21 are the CPython, OpenSSL and libffi builder's own temp paths under its `Administrator` account, which come
    with uv's managed CPython;
  - 1 is a CPython docstring example.
- **6 gmail hits:** CPython, rich and pygments author lines, plus an httpx docstring example address.

None is this rig's: a direct search of the unpacked tree for the rig's username, paths and noreply id finds
nothing. None is on the allowlist either, so they go to the Publisher.

**The gate now unpacks it itself.** After this finding the Director ruled that the gate always unpacks packaged
programs and reports every hit, and that a release with third-party hits needs his written exception. The
Publisher then taught `identity-scan.py` to unpack PyInstaller bundles. Run directly on this `.exe` under the
rig's Python 3.10, it gives `RESULT HIT 28` (`13-identity-scan-binary-v2.txt`), labelled `<exe>!carchive/…` and
`<exe>!pyz/…`. That's the same 28 hits as my unpacker, by class, member and offset (a `diff` of the two lists is
empty), with no `unreadable-pyinstaller` and no hit on the raw `.exe` bytes. The allowlist holds inside the
bundle too: shellingham's allowlisted address is in the bundled notices and gives no hit.

**A first build on Python 3.13** (`build-py313/`). A bare `uv venv` picked a managed 3.13 here, while CI's runner
has only the 3.12 it installs. That build passed the same smoke test, `self-check` and enable (run
`7902699b-a31a371d`, wallet `rGZjmEC9j3t5UhrenYZUbnrnyVcgHZCvDu`). I rebuilt on 3.12 to match the workflow. The
notices file was identical across the two environments.

## Item 8: a Linux build with the TOC check, on `c6c30b5` — PASS (`8-linux-binary/`)

The Editor asked for this because two new steps in `release-binaries.yml` had never run. One copies the notices for
each OS; the other is `--verify-toc`. Both fire only on a release or a dispatched tag (review F3). This ran on
**`c6c30b5`** (fixup 2), in WSL: Ubuntu 26.04, glibc 2.43, uv 0.11.18, with uv's concurrency capped at 12 of 24
cores.

`harness/linux_build.sh` ran the linux-x64 leg's build job step by step: `uv python install 3.12`, the venv, the
install, the notices, the PyInstaller line with `DATA_SEP=:`, the rename and the per-OS notices copy, then
`--verify-toc` and the smoke test. The one change: `uv venv -p 3.12`, where the workflow has a bare `uv venv`
(this machine has other managed Pythons, as on Windows).

| Step | Result |
|---|---|
| Build | Python 3.12.13, PyInstaller 6.22.3. `escape-the-valley-1.3.1-linux-x64` is an ELF x86-64, 20,932,824 bytes, sha256 `c592db32…` (`dist-sha256.txt`) |
| Per-OS notices copy | `dist/THIRD_PARTY_NOTICES-linux-x64.txt`, 138,905 bytes, the same as the generated file |
| **`--verify-toc build/escape-the-valley THIRD_PARTY_NOTICES.txt`** | **exit 0**: `Verified 22 bundled site-packages distributions`: annotated-doc, anyio, base58, certifi, ecpy, h11, httpcore, httpx, idna, linkify-it-py, markdown-it-py, mdurl, platformdirs, pycryptodome, pygments, rich, shellingham, textual, typer, typing-extensions, websockets, xrpl-py |
| Negative control: the same check with `Name: rich` deleted from a copy of the notices | **exit 1**: `TOC VERIFICATION FAILED: … missing from …: rich` (`09-verify-toc-negative-rich.log`) |
| `scripts/smoke_test_binary.py` on the Linux binary | **PASS**, exit 0: `--help`; 260 events; `tui.tcss` loads; xrpl extra present; licences present |

**What it doesn't cover:** macOS (darwin-arm64) didn't run, so its leg stays unexercised until release. The notices
content and the unpacked-binary scan wait for the fixup 3 head (the Editor's F1/F2).

**Off the brief: the notices list three distributions the binary doesn't carry.** The notices have 27 `Name:`
entries (`notices-names.txt`): Python, PyInstaller, and 25 distributions. Three of those are installed in the build
environment but aren't in the bundle: `Deprecated`, `mdit-py-plugins` and `wrapt` have 0 entries in `PYZ-00.toc`
and 0 under `site-packages` in `Analysis-00.toc`. For comparison, rich, xrpl and certifi have 100, 274 and 2
(`10-toc-modules.txt`). The generator lists what is installed, and the check fails only on a bundled distribution
with no entry, so listing too many does no harm.

## Item 9: the final candidate, a Linux build on `7481d88` — PASS on the brief, one notices defect, one gate hit (G1, open) (`9-final-7481d88/`)

The Editor asked for this on **`7481d88`** (PR #25 head, fixup 4 plus its atlas map). It ran in WSL (Ubuntu 26.04,
uv 0.11.18, uv's concurrency capped at 12 of 24 cores, CPU only) in a fresh clone at that commit.
`harness/linux_build_final.sh` runs the linux-x64 leg of `release-binaries.yml` as written at `7481d88`, with no
changes this time, since the workflow now pins the Python:
1. `uv python install 3.12.13`, `uv venv --python 3.12.13`, `uv pip install ".[xrpl]" "pyinstaller>=6.9.0"`;
2. the `direct_url.json` removal step (`Removed direct_url.json from build venv.`);
3. `gen_third_party_notices.py --release` (251,748 bytes, 27 `Name:` entries);
4. the first `pyinstaller` call with the four `--exclude-module`s, then `--release --prune-toc` (240,186 bytes,
   22 distributions), then the second `pyinstaller --noconfirm` call;
5. the rename and the per-OS notices copy.

The binary, `escape-the-valley-1.3.1-linux-x64`, is an ELF x86-64, **20,964,960 bytes**, sha256
`70df0d60cfc9a1839ad7f832b3a6d51a9ab84588de915877750124fdc1e41a39`. Python 3.12.13, PyInstaller 6.22.3.

| Check | Result |
|---|---|
| `--verify-toc build/escape-the-valley THIRD_PARTY_NOTICES.txt` | **exit 0**: `[toc-check] Verified 22 bundled site-packages distributions and excluded extensions against THIRD_PARTY_NOTICES.txt` |
| `--verify-bundle dist/escape-the-valley-1.3.1-linux-x64` | **exit 0**: `[bundle-check] Verified 114 archive entries in dist/escape-the-valley-1.3.1-linux-x64: no direct_url.json present` |
| `scripts/smoke_test_binary.py` | **PASS**, exit 0: `--help`; 260 events (floor 200); `tui.tcss` loads; xrpl extra present; licences present |
| `identity-scan.py` on the binary | `RESULT HIT 9`, exit 1, **no `build-record`**. 8 hits are third party; **1 is the build machine's home path** (G1 below) |
| Notices content (`notices-check.txt`) | every check passes **except the game LICENSE header hash** (N1 below) |
| pip install into Python 3.13.13 (Windows), then `trail licenses` | **exit 0**, 71,252 bytes, nothing on stderr (`pip-py313/`) |

**The identity scan, verbatim** (`identity-scan-linux-binary.txt`; paths read `<scratch>`):

```
HIT gmail <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64!carchive/base_library.zip/io.pyc:@0x847
HIT home-path <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64!carchive/base_library.zip/ntpath.pyc:@0x1976
HIT home-path <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64!carchive/libpython3.12.so.1.0:@0xe73c8c
HIT gmail <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64!carchive/libpython3.12.so.1.0:@0xe63b77
HIT gmail <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64!pyz/PYZ.pyz/bz2.pyc-body:@0x1b9
HIT home-path <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64!pyz/PYZ.pyz/_sysconfigdata__linux_x86_64-linux-gnu.pyc-body:@0x2c42
HIT gmail <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64!pyz/PYZ.pyz/httpx/_urls.pyc-body:@0x320a
HIT gmail <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64!pyz/PYZ.pyz/pygments/lexers/gdscript.pyc-body:@0x111
HIT gmail <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64!pyz/PYZ.pyz/rich/_spinners.pyc-body:@0x448
RESULT HIT 9
```

`identity-scan-hit-sources.md` names each one. The third-party hits are CPython's `io` and `bz2` author lines and
its `splitroot()` docstring (in the stdlib zip and again in libpython), the pygments and rich author lines, and an
httpx docstring example.

**G1 (open, being fixed before release), the hit that isn't third party: `_sysconfigdata` carries the builder's
home path.** uv writes the
interpreter's install prefix into `_sysconfigdata__linux_x86_64-linux-gnu.py` when it installs Python, and
PyInstaller bundles that module. Here it is 20 strings under `<home>/.local/share/uv/python/cpython-3.12.13-…`, the
WSL account on this rig. The released xrpl-camp 1.5.0 Linux asset, built on CI, has the same 20 strings under
`/home/runner/work/_temp/uv-python-dir/…` (`sysconfigdata-home-paths.txt`). So a CI build of `7481d88` will carry
`/home/runner/…` there too, and the gate's bundle-only `/home/<account>` check will report it. The
`direct_url.json` removal doesn't touch this file. I haven't tested a fix.

**The notices content** (`harness/notices_check.py.txt`). It works from the repo's `_licenses/` and doesn't use
`notices.py`.
- **Three copies, one file:** the pruned `THIRD_PARTY_NOTICES.txt`, `dist/THIRD_PARTY_NOTICES-linux-x64.txt` and
  the copy bundled in the binary are byte-identical, sha256 `34390db4…` (`notices-three-copies-sha256.txt`).
- **Every vendored block matches `SOURCES.json`.** All 13 python-build-standalone files for
  `x86_64-unknown-linux-gnu` (cpython, bzip2, expat, libedit, libffi, liblzma, libuuid, mpdecimal, ncurses,
  openssl-1.1, openssl-3, sqlite, zlib) pass three checks each:
  - the vendored file's sha256 equals `SOURCES.json`;
  - the block's `SHA256:` line equals it;
  - the block text is the file's text verbatim.
- **The CPython sections:** the Incorporated Software section passes the same three checks. The `Doc/license.rst`
  hash is right and cited. The interpreter's `LICENSE.txt` block hash equals `install_license_sha256`.
- **OpenSSL:** the openssl-3 block carries the Apache-2.0 text from apache.org verbatim from the title through
  section 9 and `END OF TERMS AND CONDITIONS` (whitespace normalised; the licence URL is `https` here and `http` at
  apache.org). It leaves out apache.org's APPENDIX, the how-to for applying the licence to your own work, as
  OpenSSL's own `LICENSE.txt` does. The openssl-1.1 block (fixup 4's G4) is there as well.
- **bzip2 and mpdecimal:** both blocks carry their liability paragraphs (`IN NO EVENT SHALL THE AUTHOR BE LIABLE …`
  and `IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE …`).
- **The distributions match the bundle exactly.** `harness/bundled_dists.py.txt` maps every file each installed
  distribution put in the build venv (its `RECORD`) to the unpacked bundle. 22 distributions have files in it, and
  they are the 22 the notices list, with nothing missing and nothing extra (`bundled-dists.txt`). `Deprecated`,
  `mdit-py-plugins` and `wrapt` were in the generated file and are gone after `--prune-toc`.
- **A negative control on the checker:** one changed word in a copy's bzip2 paragraph gives `RESULT FAIL`, exit 1
  (`notices-check-negative.txt`).

**N1 (minor): the game LICENSE header hash depends on the checkout's line endings.** The notices header reads
`SHA256: 367e5141…`, but `SOURCES.json` `game_license` says `5ae6b3c8…`, the hash of `_licenses/LICENSE`. The text
is the same MIT licence. The difference is line endings:
- `_licenses/LICENSE` is committed with CRLF and `-text`, so it is `5ae6b3c8…` on every checkout.
- The generator reads the repo-root `LICENSE` first. That file is LF in the index (`text=auto`), so it is LF on a
  Linux checkout (`367e5141…`, which is the CRLF→LF form of the vendored file) and normally CRLF on Windows.
- The pip install reads the vendored copy and prints `5ae6b3c8…`.

So the Linux (and likely macOS) release notices cite a hash that matches no committed file byte for byte. Nothing
checks the header against `SOURCES.json`.

**pip install on a Python that isn't 3.12.13.** `uv venv -p 3.13` gave 3.13.13 on Windows, then
`uv pip install ./<7481d88>` (no `[xrpl]`). `trail licenses` exits 0 with 71,252 bytes: the game licence, the note
`CPython and python-build-standalone license texts apply to the standalone release binaries only.`, and the 19
installed distributions. So the release-only pins don't break the pip fallback (fixup 4's G1).

**What it doesn't cover:** macOS (darwin-arm64) and the Windows leg of this commit didn't run. On macOS,
`_sysconfigdata` would hold the macOS runner's home path, but that is untested. The binary itself isn't committed.

## Item 10: the G1 fix, a Linux build on `24926be` — PASS, G1 closed (`10-g1-24926be/`)

The Editor asked for this on **`24926be`**: `f4d5581` (the G1 fix) plus its atlas map, on top of the merged item 9
receipts (`990c3af`). The fix has two parts:
- a workflow step that sets `UV_PYTHON_INSTALL_DIR` to `/opt/uv-python` (Linux and macOS) or `C:\uv-python`
  (Windows) before `uv python install`;
- a `--verify-bundle` that also fails on a home path in the bundled `_sysconfigdata`.

It ran in WSL in a fresh clone at `24926be`, CPU only, with uv's concurrency capped at 12 of 24 cores.
`harness/linux_build_g1.sh` is item 9's script with one line added, `export UV_PYTHON_INSTALL_DIR=/opt/uv-python`,
which matches the new step. The rest of `release-binaries.yml` is unchanged since `7481d88`.

**About `/opt/uv-python` on this machine:** the folder already existed. It was writable by the build account and
held a `cpython-3.12.13` install made the evening before, so the workflow's `sudo mkdir` branch didn't run
(`00-uv-python-dir-before.txt`).
- `uv python install 3.12.13` found the install there and used it; the venv log reads `Using CPython 3.12.13`.
- Its own `_sysconfigdata` has 0 `/home/` strings.
- The install log has one harmless warning. uv couldn't link `python3.12` into `~/.local/bin`, because a file not
  managed by uv is already there. A runner has no such file.

The binary is an ELF x86-64, **20,967,824 bytes**, sha256
`36fb0530f9c1abcdbb761ac8faf100c91a651de8287207242f93f8c34d4a5dfc`. Python 3.12.13, PyInstaller 6.22.3. The notices
file is byte-identical to item 9's (`34390db4…`, 240,186 bytes).

| Check | Result |
|---|---|
| `--verify-toc build/escape-the-valley THIRD_PARTY_NOTICES.txt` | **exit 0**: `[toc-check] Verified 22 bundled site-packages distributions and excluded extensions against THIRD_PARTY_NOTICES.txt` |
| `--verify-bundle dist/escape-the-valley-1.3.1-linux-x64` | **exit 0**: `[bundle-check] Verified 114 archive entries in dist/escape-the-valley-1.3.1-linux-x64: no direct_url.json or _sysconfigdata home paths present` |
| `scripts/smoke_test_binary.py` | **PASS**, exit 0: `--help`; 260 events (floor 200); `tui.tcss` loads; xrpl extra present; licences present |
| The bundled `_sysconfigdata` (unpacked) | **0 `/home/` strings**, 20 under `/opt/uv-python/cpython-3.12.13-linux-x86_64-gnu`. The item 9 binary has 20 under `<home>/…`, and the camp 1.5.0 asset 20 under `/home/runner/…` (`sysconfigdata-prefix.txt`) |
| `identity-scan.py` on the binary | `RESULT HIT 8`, exit 1: the 8 third-party hits from item 9, at the same offsets, with **no `_sysconfigdata` hit and no `build-record`** |
| **Negative control:** `--verify-bundle` on the xrpl-camp v1.5.0 linux release asset (sha256 `12259e59…`) | **exit 1**, naming both: `BUNDLE VERIFICATION FAILED: Bundled PEP 610 direct_url.json build record found in …: xrpl_camp-1.5.0.dist-info/direct_url.json; Bundled _sysconfigdata home path(s) found in …: PYZ.pyz!_sysconfigdata__linux_x86_64-linux-gnu (12 distinct; first: /home/runner/work/_temp/uv-python-dir/cpython-3.12.15-linux-x86_64-gnu/bin)` |
| **Negative control:** `--verify-bundle` on item 9's `7481d88` binary (sha256 `70df0d60…`) | **exit 1**: `Bundled _sysconfigdata home path(s) found in …: PYZ.pyz!_sysconfigdata__linux_x86_64-linux-gnu (12 distinct; first: <home>/.local/share/uv/python/cpython-3.12.13-linux-x86_64-gnu/bin)` |

**The identity scan, verbatim** (`identity-scan-linux-binary.txt`; paths read `<scratch>`):

```
HIT gmail <scratch>/1.3.1-g1-24926be/linux/escape-the-valley-1.3.1-linux-x64!carchive/base_library.zip/io.pyc:@0x847
HIT home-path <scratch>/1.3.1-g1-24926be/linux/escape-the-valley-1.3.1-linux-x64!carchive/base_library.zip/ntpath.pyc:@0x1976
HIT home-path <scratch>/1.3.1-g1-24926be/linux/escape-the-valley-1.3.1-linux-x64!carchive/libpython3.12.so.1.0:@0xe73c8c
HIT gmail <scratch>/1.3.1-g1-24926be/linux/escape-the-valley-1.3.1-linux-x64!carchive/libpython3.12.so.1.0:@0xe63b77
HIT gmail <scratch>/1.3.1-g1-24926be/linux/escape-the-valley-1.3.1-linux-x64!pyz/PYZ.pyz/bz2.pyc-body:@0x1b9
HIT gmail <scratch>/1.3.1-g1-24926be/linux/escape-the-valley-1.3.1-linux-x64!pyz/PYZ.pyz/httpx/_urls.pyc-body:@0x320a
HIT gmail <scratch>/1.3.1-g1-24926be/linux/escape-the-valley-1.3.1-linux-x64!pyz/PYZ.pyz/pygments/lexers/gdscript.pyc-body:@0x111
HIT gmail <scratch>/1.3.1-g1-24926be/linux/escape-the-valley-1.3.1-linux-x64!pyz/PYZ.pyz/rich/_spinners.pyc-body:@0x448
RESULT HIT 8
```

These are the third-party hits that item 9's `identity-scan-hit-sources.md` names: CPython's `io` and `bz2` author
lines and `splitroot()` docstring, plus the httpx, pygments and rich lines. Under the Director's ruling, a release
with third-party hits still needs his written exception naming them.

**What it doesn't cover:**
- The macOS and Windows legs didn't run, so the `C:\uv-python` branch and the macOS `sudo mkdir` branch are
  untested.
- The workflow's `sudo mkdir` / `chown` path didn't run here, because the folder already existed.
- The binary isn't committed.

## Item 11: the released npm package, `@mcptoolshop/escape-the-valley@1.3.1` — Linux PASS, Windows PASS except the licences match (N2) (`11-npm-release/`)

1.3.1 is released: tag `v1.3.1` on `47b01a3`, npm, PyPI and the GitHub release assets. This is the npm check the
Editor asked for, the same as xrpl-camp's.
- **The command:** `npx --yes @mcptoolshop/escape-the-valley@1.3.1`, from an empty working folder with a new
  `npm_config_cache` and `npm_config_prefix` and a new launcher cache (`LOCALAPPDATA` on Windows, `XDG_CACHE_HOME`
  on Linux). So the first call downloads the binary from the v1.3.1 release and checks it against
  `checksums-1.3.1.txt`.
- **Node:** 22.22.3 on both platforms: native on Windows; in WSL, the official Node in `~/opt`.
- **Scripts:** `harness/npm_release_check_etv.sh` and `harness/_run_npm131_linux.sh`.

| Check | Windows | Linux |
|---|---|---|
| `--version` | exit 0, `trail 1.3.1` | exit 0, `trail 1.3.1` |
| `--help` | exit 0, `Usage: escape-the-valley [OPTIONS] COMMAND [ARGS]...` | the same |
| The cached binary | `escape-the-valley-1.3.1-win-x64.exe`, sha256 `d72cd44b…`, **equal to `checksums-1.3.1.txt`** | `escape-the-valley-1.3.1-linux-x64`, sha256 `4c2499f3…`, **equal to `checksums-1.3.1.txt`** |
| **argv[0] control:** the cached binary run directly | `Usage: escape-the-valley-1.3.1-win-x64.exe …` | `Usage: escape-the-valley-1.3.1-linux-x64 …` |
| `licenses` and `--licenses` | exit 0, identical to each other (254,441 bytes), **not byte-identical** to `THIRD_PARTY_NOTICES-win-x64.txt` (254,150 bytes, sha256 `b25c6be4…`) | exit 0, **byte-identical** to `THIRD_PARTY_NOTICES-linux-x64.txt` (240,186 bytes, sha256 `34390db4…`) |
| `--verify-bundle` (from the `v1.3.1` tag) on the release asset, and on the launcher's copy | — | **exit 0** on both: `[bundle-check] Verified 114 archive entries in …: no direct_url.json or _sysconfigdata home paths present` |

The direct runs show the file name, so the `escape-the-valley` in every launcher run comes from the launcher setting
argv[0]. The release's Linux notices are the same file as item 9's and item 10's local builds (`34390db4…`). So N1
is still there in the released Linux notices: the header reads `SHA256: 367e5141…`.

**N2 (minor): on Windows, `licenses` doesn't reproduce the release notices byte for byte.**
- **The file:** the copy bundled in the `.exe` is the release's `THIRD_PARTY_NOTICES-win-x64.txt`, the same
  sha256. Its lines end `\r\n`, except 291 that end `
` (`win/08-crcrlf-blocks.txt`). They are in five blocks
  whose source files are CRLF and are copied without normalising the line endings:
  - the game LICENSE: 20 lines;
  - CPython's `LICENSE.txt`: 138;
  - pycryptodome's `AUTHORS.rst`: 50;
  - pycryptodome's `LICENSE.rst`: 60;
  - websockets' `LICENSE`: 23.

  On the Windows build, writing that text in text mode turns each `\r\n` into `
`.
- **What `licenses` prints:** it reads the bundled file with universal newlines, so each `
` becomes two line
  breaks and those lines gain a blank line after them. It then prints with `\r\n`. The output is exactly that
  reading of the file (a Python `newline=None` read, re-joined with `\r\n`, equals `03-licenses.out`). It has
  5,305 lines; the release file has 5,014.
- **Why it's minor:** the text is all there, only with extra blank lines, and the release file itself carries the
  stray `\r`s.
- **Linux is unaffected:** it writes and reads `\n` only, so its output is byte-identical.

**pip install on a Python that isn't 3.12.13** (`pip-py313/`). `uv pip install escape-the-valley==1.3.1` came from
PyPI into Python 3.13.13 on Windows. `trail licenses` exits 0 with 71,252 bytes and nothing on stderr, and it prints
the note that the CPython and python-build-standalone texts apply to the release binaries only.

**What it doesn't cover:**
- macOS (darwin-arm64) didn't run.
- No game was played through the npm channel.
- The binaries and the release notices aren't committed. Their hashes are, and so is Windows' `licenses` output.

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
- `7-binary/`:
  - the build logs, the binary's sha256, the smoke test, and both scans with `12-hit-sources.md`;
  - `frozen/` (`--version`, `self-check`, the enable, and `licenses`) and `pip/` (the install and `licenses`), each
    command with `.cmd`, `.out` and `.exit`, apart from the licences text;
  - `licenses-hashes.txt`;
  - `build-py313/`, the first build's results.
- `8-linux-binary/`: each step's `.log` and `.exit`, the negative control, `notices-names.txt`,
  `10-toc-modules.txt`, and the binary's hash, `file` type and versions. The notices text isn't committed.
- `9-final-7481d88/`:
  - `linux-build/`: each step's `.log` and `.exit`, the notices before and after the prune, and the binary's hash,
    `file` type and versions;
  - the identity scan and `identity-scan-hit-sources.md`, plus `sysconfigdata-home-paths.txt`;
  - `notices-check.txt` and its negative control, `bundled-dists.txt` and `notices-three-copies-sha256.txt`;
  - `pip-py313/`: `trail licenses` on Python 3.13.
- `10-g1-24926be/`:
  - `linux-build/`: each step's `.log` and `.exit` (01–12), the two negative controls (13, 14) and their hashes,
    `00-uv-python-dir-before.txt`, and the binary's hash, `file` type and versions;
  - `identity-scan-linux-binary.txt` and `sysconfigdata-prefix.txt`.
- `11-npm-release/`:
  - `win/` and `linux/`: each `npx` call as `.cmd`, `.err` and `.exit`, with the `--version`, `--help` and
    direct-binary output;
  - the cache listing and sha256, and `06-licenses-vs-release.txt` (hashes, sizes, `cmp`);
  - `win/03-licenses.out` and `win/08-crcrlf-blocks.txt` (N2);
  - `linux/`: the tag head, the asset's sha256, and the two `--verify-bundle` runs (09, 10);
  - `checksums-1.3.1.txt`, and `pip-py313/`.
- `code.txt`, `hashes.txt`.
- `harness/`: `cli_drive`, `tui_drive`, `tui_key_probe` (D1), `seed_search` (how seed 10 was found),
  `etv_ledger_probe`, `redact_tree`, `unpack_pyinstaller` and `hit_context` (item 7). Each is a `.py.txt`, which
  keeps them out of the repo's lint. The item 8 shell scripts are `linux_build.sh`, `verify_toc_negative.sh` and
  `toc_has.sh`. Item 9 adds `linux_build_final.sh` and `_run_final.sh`, plus `notices_check`, `bundled_dists` and
  `hit_context_scan` as `.py.txt`. Item 10 adds `linux_build_g1.sh` and `_run_g1.sh`; item 11, `npm_release_check_etv.sh` and `_run_npm131_linux.sh`.
