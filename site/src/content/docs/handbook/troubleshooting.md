---
title: Troubleshooting
description: The three things that go wrong, and the one command to run first.
sidebar:
  order: 7
---

Most problems fall into three buckets. **When anything looks wrong, run `trail self-check` first** -- it tells you whether Ollama is reachable, whether your save loads, and which model is installed, so you don't have to guess.

```bash
trail self-check
```

## 1. The narration is generic, or there's no story at all

**Symptom:** turns are described in flat fallback text ("You travel. -3 food, -2 water.") with no atmosphere, or the GM never says anything beyond the bare numbers. In the TUI you may see a *GM degraded* note.

**Cause:** the AI Game Master needs [Ollama](https://ollama.com/) running locally. If Ollama isn't running, isn't installed, or doesn't have the configured model pulled, the game falls back to plain deterministic narration. **This is by design -- a missing GM never bricks the game, it just removes the story layer.**

**Fix -- pick one:**

- **Start Ollama** and try again: `ollama serve` (then in another terminal, confirm the model is pulled, e.g. `ollama pull llama3.2`).
- **Run `trail self-check`** to see exactly what's wrong -- it reports whether Ollama is reachable at `OLLAMA_HOST` (default `http://localhost:11434`) and whether your `--model` is installed, with the exact `ollama pull` command if it isn't.
- **Play without the GM:** add `--gm-off`. The game plays identically; you just see the deterministic fallback text instead of narration. Good for speed runs or machines without Ollama.

The first narrated turn loads the model and can take 10-30 seconds -- see [What to expect from AI narration](/escape-the-valley/handbook/beginners/#what-to-expect-from-ai-narration). A slow first turn is normal, not a hang.

## 2. The ledger says "pending" or a settlement failed

**Symptom:** a town checkpoint shows as pending, `trail ledger status` lists unsettled checkpoints, or the status line reports a failed settlement. With the audit/reconciliation proof, you may see an INCONCLUSIVE banner.

**Cause:** the XRPL **Testnet** is a public test network and is sometimes flaky -- requests time out, faucets stall, or settlements don't confirm. The game treats this as recoverable: your supplies are correct locally, and the unsettled receipt is queued, not lost. **The ledger is optional and never blocks play.**

**Fix:**

```bash
trail ledger reconcile
```

This retries the failed settlements against the Testnet. Run it again when the network recovers -- the queued checkpoints settle without affecting your supply counts. `trail ledger status` shows what's still outstanding. (All ledger operations are Testnet-only and opt-in; see [XRPL Ledger](/escape-the-valley/handbook/xrpl-ledger/).)

## 3. A saved game won't resume

**Symptom:** `trail tui --continue` reports no saved game, or the save "exists but could not be loaded."

**Cause:** the save file (`run.json`) was truncated or corrupted -- usually a crash or kill mid-write. To keep one bad write from costing you the file forever, the engine **backs the corrupt file up before refusing it**: it renames `run.json` to `run.json.corrupt-<timestamp>` so your next save can't clobber the evidence.

**Fix:**

- **Check `trail self-check`** -- it reports whether a save was found and whether it loaded.
- **Look for a backup:** in your save directory you'll find `run.json.corrupt-<timestamp>`. That is the file the engine quarantined. If you have tooling to repair JSON, the most recent backup is your best recovery candidate; otherwise it's a record of what was lost.
- **Start fresh** if no backup is recoverable: `trail tui --seed <number>` begins a new run. The same seed plus the same choices reproduces the same world, so a re-run from a known seed is deterministic.

## Still stuck?

`trail self-check` is the single best first step for all three cases above -- it surfaces the GM, save, and model state in one place. Beyond that, the game is fully playable offline with `--gm-off` and with the ledger disabled (the default), so you can always fall back to the deterministic core game.
