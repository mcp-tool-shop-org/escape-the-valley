---
title: Game Mechanics
description: Actions, physics, events, escape valves, and how the engine works.
sidebar:
  order: 3
---

## Camp Actions

Each turn you choose one action from camp:

| Action | What It Does |
|--------|-------------|
| **Travel** | Move toward the valley exit. Costs food and water. Risk of breakdown and events. |
| **Rest** | Heal the party, recover morale. Costs supplies but no progress. |
| **Hunt** | Spend ammo for a chance at food. Better in forests and plains. |
| **Repair** | Spend a spare part to fix the wagon. Critical for survival. |

## Supplies

Five resources govern your survival:

| Resource | Consumed By | Replenished By |
|----------|-------------|----------------|
| **Food** | Every turn (scaled by pace and party size) | Hunting, trading at towns |
| **Water** | Every turn (scaled by terrain) | Towns (auto-refill) |
| **Ammo** | Hunting (1 per attempt) | Trading, events |
| **Parts** | Repair action (1 per repair) | Trading, events |
| **Wagon Condition** | Travel (wear), events (damage) | Repair action, maintenance windows |

## Physics Engine

The game uses deterministic physics with seeded RNG. Given the same seed and same choices, the outcome is identical every time.

- **Scarcity curves** control how quickly supplies drain based on terrain, pace, and party size
- **Breakdown probability** increases as wagon condition drops — it's not linear, it accelerates
- **Doctrine modifiers** adjust specific rates (consumption, repair quality, travel speed)

## Events

80+ events interrupt travel with choices. Each event offers 2-3 options:

- **Cautious** — Safer but costs time or supplies
- **Bold** — Faster but risks damage or loss
- **Creative** — Sometimes available; unconventional approaches with mixed outcomes

Events aren't random — they're drawn from a seeded deck and influenced by terrain, party state, and doctrine. The same seed produces the same event sequence.

## Escape Valves

Emergency actions available when things get desperate. Each has side effects and a cooldown:

| Valve | Effect | Side Effect | Cooldown |
|-------|--------|-------------|----------|
| **Hard Ration** | Halves food consumption for one turn | Hurts morale and party health | 3 turns |
| **Desperate Repair** | Coin flip: +20 or -10 wagon condition | Can make things worse | 5 turns |
| **Abandon Cargo** | Dumps supplies to reduce wagon load | Loses food/ammo permanently | Once per run |

## Pace

| Pace | Travel Speed | Supply Burn | Breakdown Risk |
|------|-------------|-------------|----------------|
| **Slow** | Low | Normal | Low |
| **Steady** | Normal | Normal | Normal |
| **Hard** | High | High | High |

Steady is the default and usually optimal. Hard pace is for when you're close to the exit with supplies to burn. Slow pace is for when the wagon is one bump from breaking.

## Maintenance Windows

Resting and repairing in consecutive turns (either order) triggers a maintenance window — a temporary state where breakdown probability drops significantly. This is the primary tool for keeping your wagon alive through the mid-game.

## Warning Callouts

The game warns you when resources hit critical levels:

- **Verbose mode** (default) — Early warnings and cliff-edge alerts
- **Minimal mode** — Only cliff-edge warnings (last-moment, critical threats)

Switch with `--callouts minimal` or `--callouts verbose`.
