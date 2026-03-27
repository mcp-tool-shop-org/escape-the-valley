---
title: GM Profiles
description: The three AI narrators and how they shape the story.
sidebar:
  order: 4
---

The AI Game Master narrates your journey but never changes the mechanics. All three profiles play the same game — only the storytelling voice differs.

## Profiles

### Chronicler

**Tone:** Grounded, practical, spare.

The Chronicler reports what happened. Minimal folklore, no embellishment. Events are described plainly. Consequences are stated as facts. If you want the game to feel like a logbook, this is your narrator.

**Best for:** Players who want facts. Repeat players who already know the world.

### Fireside

**Tone:** Serious campfire narrator. Subtle uncanny moments.

Fireside is the default profile. It tells the story like someone recounting their journey around a fire — mostly grounded, with occasional moments that feel slightly off. Not horror, not fantasy. Just the uneasy feeling that the valley has a memory.

**Best for:** First playthrough. Players who want atmosphere without spectacle.

### Lantern-Bearer

**Tone:** Uncanny and liminal, but grounded in consequences.

The Lantern-Bearer narrates the same events but frames them through a stranger lens. The valley feels alive. Coincidences feel deliberate. The weird folklore percentage is higher (~15% vs Fireside's ~5%). Still grounded — no magic, no supernatural mechanics — but the tone is distinctly eerie.

**Best for:** Experienced players who want a different feel. Players who enjoy atmospheric tension.

## Switching profiles

Set the GM profile at launch:

```bash
# Use the default (Fireside)
trail tui --seed 42

# Switch to Chronicler
trail tui --seed 42 --gm-profile chronicler

# Switch to Lantern-Bearer
trail tui --seed 42 --gm-profile lantern
```

## Weirdness level

The `--weirdness` flag (0-3) controls how much folklore and uncanny content the GM injects. Higher values unlock more strange details. The default is 2. Each run starts with 2 uncanny tokens that the GM can spend on strong uncanny moments.

```bash
# Maximum folklore (Lantern-Bearer recommended)
trail new --seed 42 --gm-profile lantern --weirdness 3

# Minimal folklore
trail new --seed 42 --weirdness 0
```

## Choosing a model

The `--model` flag selects which Ollama model generates narration. The default is `llama3.2`. Larger models produce richer text but are slower:

```bash
trail tui --seed 42 --model mistral
```

## Voice narration

Enable spoken narration with `--voice`. Each GM profile maps to a different voice and pacing style. Control playback speed with `--voice-pace`:

```bash
trail tui --seed 42 --voice --voice-pace slow
```

Voice pacing options: `fast`, `normal` (default), `slow`. Requires the `voice-soundboard` package. Audio plays in the background and can be interrupted with any key press.

## Disabling the GM

Run without AI narration entirely:

```bash
trail tui --seed 42 --gm-off
```

The game plays identically -- you just see fallback text instead of GM narration. Useful for speed runs or systems without Ollama.

## Requirements

The GM requires [Ollama](https://ollama.com/) running locally. No API keys, no cloud calls, no cost. The game auto-detects whether Ollama is available and falls back gracefully if it isn't. The default 30-second timeout allows for first-load model warm-up on slower hardware.
