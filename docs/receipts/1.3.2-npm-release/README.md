# The released npm package, `@mcptoolshop/escape-the-valley@1.3.2`

This is the check the Editor asked for after the release (tag `v1.3.2` on `67f162d`, npm 1.3.2). It has the same
shape as 1.3.1's item 11 (`docs/receipts/1.3.1-live-check/11-npm-release/`), with three additions:
- `self-check`;
- a short run with the GM off;
- the Publisher's question: does Windows `licenses` still show N2's extra blank lines, both piped to a file and in a
  real console?

macOS was not run.

## Verdict

**PASS on Windows and on Linux (WSL). N2 is fixed.**

On Windows, `trail licenses` piped to a file is **byte-identical** to the release's
`THIRD_PARTY_NOTICES-win-x64.txt` (sha256 `83ec143d…7f9907`). Both have 0 CRs, 5,014 lines and 886 blank lines.
In a real console, the 5,014 rows on screen equal the notices' lines one for one, with no extra blank rows. The
1.3.1 package, run the same way as a control, puts 5,305 rows on screen: N2's 291 extra blank rows.

## How

Each platform starts from fresh state:
- a fresh npm cache and prefix (`npm_config_cache`, `npm_config_prefix`);
- a fresh launcher cache (`LOCALAPPDATA` on Windows, `XDG_CACHE_HOME` on Linux);
- Node v22.22.3 with npm 10.9.8.

The scripts are in `harness/`:
- `npm_release_check.sh` is item 11's script, unchanged.
- `_run_npm132_win.sh` and `_run_npm132_linux.sh` add the new checks.
- `console_capture.py.txt` runs a command in a real Windows console and reads the console back.
- `line_endings.py.txt` counts line endings and blank lines.
- `cli_drive.py.txt` plays the classic CLI through a pipe.

## Results

| Check | Windows (`win/`) | Linux (`linux/`) |
|---|---|---|
| `npx --yes @mcptoolshop/escape-the-valley@1.3.2 --version` | exit 0: `trail 1.3.2` | exit 0: `trail 1.3.2` |
| `--help` | exit 0: `Usage: escape-the-valley …` | the same |
| **argv[0] control:** the cached binary run directly | `Usage: escape-the-valley-1.3.2-win-x64.exe …`, so the name comes from the launcher | `Usage: escape-the-valley-1.3.2-linux-x64 …` |
| Cached binary vs `checksums-1.3.2.txt` | `3e33acce…edf0f`: **matches** | `1d59d770…bebe`: **matches** |
| `self-check` | exit 0, `trail 1.3.2` | exit 0, `trail 1.3.2` |
| A short GM-off run: `new --seed 42 --gm-off`, played by `cli_drive.py` (Travel each turn, then quit) | exit 0. 13 camp actions, saved on day 4, `Game saved.` | exit 0. The same 13 actions, day 4 |
| `licenses` and `--licenses` piped vs the release notices | **byte-identical**, 248,845 bytes | **byte-identical**: 240,186 bytes, sha256 `ac68c119…5fc5f2` |
| `--verify-bundle` from the v1.3.2 tag (`08-tag-head.txt` = `67f162d…`) | — | **exit 0** on the release asset and on the launcher's copy. 114 archive entries, no `direct_url.json` or `_sysconfigdata` home path |

The `.trail/` save folders from the GM-off runs are not committed.

## N2: Windows `licenses`, piped and in a console

**Piped to a file** (`win/12-line-endings.txt`):

| | Bytes | CR | LF | CR CR LF | Lines | Blank (LF split) | Blank (universal newlines) |
|---|---|---|---|---|---|---|---|
| Release `THIRD_PARTY_NOTICES-win-x64.txt` (1.3.2) | 248,845 | 0 | 5,014 | 0 | 5,014 | 886 | 886 |
| `licenses` output (1.3.2) | 248,845 | 0 | 5,014 | 0 | 5,014 | 886 | 886 |
| *Control:* release notices (1.3.1) | 254,150 | 5,305 | 5,014 | 291 | 5,014 | 0 | 1,177 |
| *Control:* `licenses` output (1.3.1) | 254,441 | 5,305 | 5,305 | 0 | 5,305 | 0 | 1,177 |

The 1.3.1 rows come from item 11's files, and `win/13-line-endings-131-control.txt` counts them.

**In a real console:** `harness/console_capture.py.txt` starts `npx.cmd --yes … licenses` in its own hidden Windows
console.
- The screen buffer is set to 800 columns × 32,000 rows before anything is written, so no line wraps. The longest
  notices line is 755 characters.
- When the command exits, the script reads every row of the console back. Markers that cmd prints before and after
  the command bound its output.
- The rows are compared with the notices' lines, after the same changes a console makes: tabs expanded to 8-column
  stops, CRs removed, and trailing spaces trimmed.

| Console run | Rows on screen | Blank rows | Equal to the notices' lines? |
|---|---|---|---|
| 1.3.2 (`win/10-console-licenses.*`) | 5,014 | 889 | **yes, row for row** |
| *Control:* 1.3.1 (`win/11-console-licenses-131-control.meta.txt`) | 5,305 | 1,180 | no. The first extra blank row is row 9, inside the game licence |

The console's 889 blank rows include 3 lines that hold only spaces. Trimmed, they are blank, so the notices' count
is 886 by bytes and 889 by rows.

## Committed and left out

- **Large text files:** to keep the folder small, only two are committed: `win/03-licenses.out` (the piped output)
  and `win/10-console-licenses.rows.txt` (the console capture). The release notices files, `--licenses` outputs,
  Linux `licenses` output and the 1.3.1 control's rows are byte-identical copies, or were already committed in item
  11. Their sha256 values and counts are in `06-licenses-vs-release.txt` and the line-ending files.
- **Paths** read `<scratch>` or `<home>`.
- **No binaries, caches or `.trail/` folders** are committed.
- **The identity scan** of this folder is `RESULT CLEAN`.
