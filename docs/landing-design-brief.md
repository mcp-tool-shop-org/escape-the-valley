# Claude Design brief — Escape the Valley landing page (v1.1)

**For:** claude.ai/design (Mike, Max plan, Chrome). **Source:** Claude Design (HTML out). **Verifier/integrator:** Claude Code (wires the exported HTML back into the Astro site + verifies).

**Goal:** a bespoke, atmospheric landing page for the game's v1.1 release — *elevated* beyond the current generic dev-tool theme, but still a clean static page that drops into the existing Astro site.

---

## What the game IS (the soul — do not lose this)

An Oregon-Trail-style **survival game for adults** who grew up on the genre. Three pillars: **an Ollama AI Game Master narrates · a deterministic engine decides outcomes (the rules-lawyer) · the XRP Ledger testnet is the receipt book.** It is period-appropriate, **serious — not cute**, and built on **plausible deniability**: weird things happen on the trail, but they're always explainable as either fatigue/coincidence *or* the uncanny — the player decides. Never cartoonish, never meme-y, no confetti.

**Voice references:** a worn trail journal; campfire folklore told straight; the dry deadpan of an old wagon manual. Think *Sea of Stars / Chained Echoes* production care, not RPG-Maker. The uncanny is sensory (sound, fog, silence, lantern-light), never confirmed magic.

## Design direction

- **Mood:** dusk on the trail. Period-appropriate, a little uncanny. Warm lantern-amber against cold dark. The current accent is `#d97706` (amber) on dark — keep that as the seed, deepen it into a real palette (aged paper, ink, ember, fog-grey, a single uncanny cold accent for the folklore beats).
- **Type:** a readable serif or slab for headers (journal/woodcut feel), clean mono for the terminal/code blocks (the game is a TUI — lean into the terminal aesthetic for the install/play snippets). Avoid generic SaaS sans-everything.
- **Texture, restraint:** subtle paper/grain or a faint topographic/route motif is welcome; **no stock-art people, no cartoon wagons, no emoji.** Atmosphere through type, color, and negative space, not clip-art.
- **The terminal is the hero visual.** This is a terminal game — a stylized faux-TUI panel (a camp screen, a streaming narration line being "typed", a trail-ledger receipt) is the most authentic hero image. Reference the in-game look: status panels (day/distance/supplies), event choices A–G, an end-of-run "trail ledger" with XRPL receipts.

## Content blocks (use this copy; it's the v1.1 story)

**Hero**
- Eyebrow: `Terminal survival game · AI-narrated · on-ledger`
- Headline: **Escape the Valley:** *Ledger Trail.*
- Sub: "Lead a party west through procedurally-generated wilderness. The Game Master narrates your journey **token-by-token — the storyteller writing in real time**. A deterministic engine decides who lives. An optional XRP Ledger tracks your supplies on-chain, with a reconciliation proof the engine can't fake."
- Primary CTA: `Get started` → `#quickstart`. Secondary: `Read the Handbook` → `handbook/`.
- Terminal previews: `pip install escape-the-valley` · `trail tui --seed 42` · `trail tui --continue`

**Features (6 — these are the v1.1 highlights):**
1. **Streaming AI narration** — three GM profiles (Chronicler, Fireside, Lantern-Bearer) narrate in distinct voices, now streamed token-by-token. Runs locally on Ollama, zero API cost. The GM never bricks a run — if the model's down, the trail tells its own story.
2. **The engine decides** — seeded, deterministic, reproducible. The GM tells the story; the engine is the rules-lawyer. Same seed + same choices = the same run, every time.
3. **Real stakes, graded endings** — events can wound and kill; deaths are attributed (injury, dysentery, exposure). Runs no longer just "win or lose" — they end *triumphant / weathered / pyrrhic / lost*, with a narrated epilogue read from who survived, how long it took, and what you sacrificed.
4. **The ledger is the receipt book** — optional on-chain inventory on the XRP Ledger testnet: supply tokens, settlement receipts, parcels you trade with other travelers. A **reconciliation proof** verifies the on-chain memos themselves — the engine cannot fake the ledger.
5. **A terminal that never freezes** — a full-screen Textual TUI with async narration ("the storyteller is thinking…"), reachable last-resort escape valves, an end-of-run postcard, and honest signals when the AI or network is degraded.
6. **Hardened** — 685 tests, **0 critical / 0 high**, crash-safe atomic saves, testnet-only-in-code. Forged by a full dogfood-swarm hardening pass.

**Quick Start (3 terminal cards):** Install (`pip install escape-the-valley` / `[xrpl]` extra / `-e ".[dev,xrpl]"`), Play (`trail tui --seed 42` / `--voice` / `--gm-off`), Commands (`trail tui --continue`, `trail status`, `trail journal -n 5`, `trail ledger enable`, `trail self-check`).

**GM Profiles (table):** Chronicler — *grounded, practical, spare* — for players who want facts. Fireside — *serious campfire narrator (default)* — first playthrough. Lantern-Bearer — *uncanny and liminal* — experienced players. (Caption: "The narrator shapes the tone, not the mechanics. All three play the same game.")

**Footer:** `MIT Licensed — built by MCP Tool Shop`. GitHub link. Logo badge "EV".

## Hard constraints (preserve / don't break)
- It's a **static landing page** — no backend, no auth, no live data; self-contained HTML only.
- Keep the real links: GitHub `https://github.com/mcp-tool-shop-org/escape-the-valley`, Handbook `handbook/`, install `pip install escape-the-valley`.
- The brand mark is the **logo** (`assets/readme-logo.png` in the repo — attach it to Claude Design). Keep the "EV" badge identity.
- Accessibility: legible contrast, not color-alone for meaning, respect reduced-motion if you add any animation.
- **Serious, not cute.** If a choice reads as whimsical/SaaS-y/meme-y, it's wrong. Compare every iteration against the mood reference before calling it done.

## Workflow
1. In claude.ai/design: attach `current-landing-reference.html` (the current page, for the baseline) **and** `readme-logo.png`. Paste this brief.
2. Iterate to a bespoke landing HTML in the game's voice. Export **standalone HTML**.
3. Hand the exported HTML (or the packaged bundle) back to Claude Code — I'll wire it into the Astro site (`site/`), keep the handbook link + base path intact, build-verify, and **compare the result to this brief + the reference before declaring it done**.

## Also available for Claude Design (optional, same session)
- **Handbook visual polish** — the Starlight handbook (9 pages) could get a matching accent/cover treatment. Lower priority than the landing.
- **Marketing imagery** — a dashboard-hero / faux-TUI screenshot for the README `docs/images/`, in the same aesthetic.
