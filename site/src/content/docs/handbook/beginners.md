---
title: Beginners
description: First-timer walkthrough for Escape the Valley, with worked examples and common mistakes.
sidebar:
  order: 99
---

This page is for players who have never played Escape the Valley before. It walks through your first run step by step, explains what to watch for, and helps you avoid the mistakes that kill most first-timers.

## 1. What is this game?

Escape the Valley is a survival game inspired by Oregon Trail. You lead a party of 4 settlers through procedurally generated wilderness, managing supplies and making hard choices until you reach the valley exit or die trying.

The game runs in your terminal. An optional AI narrator (powered by Ollama) tells the story as you play. An optional XRPL Testnet ledger tracks your supplies on-chain. Both are disabled by default -- the core game works without them.

Every run is seeded. The same seed with the same choices produces the same outcome. This means you can replay a seed to try different strategies.

## 2. Installation and first launch

Install from PyPI:

```bash
pip install escape-the-valley
```

Launch the full-screen TUI (recommended):

```bash
trail tui --seed 42
```

The `--seed 42` flag gives you a reproducible world. You can use any number. Without `--seed`, the game picks a random seed based on the current time.

When the game starts, you will see:

- **Run ID and seed** -- identifies this specific run
- **GM profile** -- the AI narrator voice (default: Fireside)
- **Twists** -- 1-2 run modifiers that change conditions (like Bandit Year or Good Hunting)
- **Party** -- your 4 settlers, each with 1-2 traits

The TUI shows your party status, supplies, wagon condition, and morale at all times.

## 3. Core mechanics in plain language

Each turn you pick one action from camp:

- **Travel** -- move toward the exit. Uses food and water. Risk of wagon breakdown and random events.
- **Rest** -- the party heals and morale recovers. Uses food and water but you make no progress.
- **Hunt** -- spend 1 ammo for a chance at food. Better odds in forests and plains, worse in deserts.
- **Repair** -- spend 1 spare part to fix the wagon. The Mechanic trait and tools improve repair quality.

After traveling, an event may trigger (~60% chance). Events present you with 2-3 choices labeled A, B, C. Cautious choices are safer but slow. Bold choices save time but risk damage. There is no always-right answer -- read the situation and decide.

**Supplies to watch:** You start with 50 food, 50 water, 20 ammo, 3 parts, 5 meds, and several extended supplies (firewood, salt, lantern oil, cloth, rope, tools, boots). Food and water drain every turn. When they hit zero, party members start taking health damage immediately.

**The wagon:** Condition runs 0-100. Breakdowns happen more often as condition drops. If the wagon hits 0 with no spare parts, the run is over. Keep it above 50 whenever you can.

**Morale:** Runs 0-100, starting at 70. Low supplies, deaths, and hard pace drain morale. Below 15, party members become exhausted. Rest and good wagon condition help recover it.

## 4. Your first run walkthrough

Here is a turn-by-turn strategy for your first few turns:

**Turns 1-3: Travel.** You start with plenty of supplies. Move toward the first town. Get distance under your belt while everything is healthy.

**When wagon drops below 60: Repair.** Do not wait for a warning. Spend 1 part now to fix it. If you can, follow up with a rest on the next turn -- this creates a "maintenance window" that reduces breakdown chance for a few turns.

**When food drops below 20: Hunt.** But only if you are in a forest or plains biome. In deserts, trading at the next town is better than wasting ammo.

**When you see an event:** Read the choices carefully. If your wagon and supplies are healthy, a bold choice saves time. If things are tight, pick the cautious option.

**When you reach a town:** Towns refill water and offer trading. They also serve as ledger checkpoints if you are using the backpack.

**When you hit a fork in the road:** The game shows you distances and destination names. Shorter paths are faster but may have harder terrain.

## 5. Common mistakes and how to avoid them

**Mistake: Ignoring wagon condition until it is critical.**
By the time you see a cliff-edge warning, one more breakdown can end the run. Repair when wagon drops below 60, not 30.

**Mistake: Hunting in the desert.**
Desert hunting has a -20% penalty. Your ammo is better spent in forests (+15%) or plains (+10%).

**Mistake: Never resting.**
Rest heals the party and recovers morale. A rest-then-repair sequence creates a maintenance window that dramatically reduces breakdown chance (0.3x normal). Use it proactively every 5-6 travel turns.

**Mistake: Using escape valves as a strategy.**
Hard ration, desperate repair, and abandon cargo are last resorts with harsh side effects. If you need them every few turns, you fell behind on maintenance earlier.

**Mistake: Always choosing the bold option in events.**
Bold choices save time but risk damage. When your wagon is at 40% and you have 1 spare part, cautious is the right call.

**Mistake: Forgetting about extended supplies.**
Salt prevents food spoilage every 3 days. Firewood is consumed every night camp. Lantern oil makes night travel safer. These are easy to overlook but matter over a long run.

## 6. Understanding the UI

The TUI displays several panels:

- **Party panel** -- names, health bars, conditions (healthy/sick/injured/exhausted), and traits for each member
- **Supplies panel** -- all 12 resource types with current amounts
- **Wagon panel** -- condition percentage, animal health, current pace
- **Journal** -- recent events and outcomes. Use `trail journal` to review from the CLI.
- **Action menu** -- numbered options: 1=Travel, 2=Rest, 3=Hunt, 4=Repair, 5=Status, 6=Change Pace, 7=Journal, Q=Save and Quit

Warning callouts appear when resources hit critical levels. In verbose mode (default), you get early warnings and cliff-edge alerts. In minimal mode (`--callouts minimal`), you only see last-moment alerts.

The game autosaves after every action. Press Q to save and quit. Resume with `trail tui --continue`.

## 7. Next steps

Once you have survived (or died) your first run:

- Read the [Survival Guide](/escape-the-valley/handbook/survival-guide/) for advanced strategy
- Try a different [GM Profile](/escape-the-valley/handbook/gm-profiles/) -- Chronicler for dry facts, Lantern-Bearer for eerie atmosphere
- Review [Game Mechanics](/escape-the-valley/handbook/mechanics/) for exact numbers on pace, breakdown curves, and doctrine modifiers
- Enable the [Ledger Backpack](/escape-the-valley/handbook/xrpl-ledger/) to track your run on-chain
- Use `trail stats` to see run statistics and `trail stats --json` for machine-readable output
- Try the same seed with different strategies to see how choices change outcomes
