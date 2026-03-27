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

Twelve resource types govern your survival, split into consumables and gear:

### Core consumables

| Resource | Consumed By | Replenished By |
|----------|-------------|----------------|
| **Food** | Every turn (2 per person per day, scaled by pace) | Hunting, trading at towns |
| **Water** | Every turn (extra in desert biomes) | Towns, water-available nodes |
| **Meds** | Auto-consumed when Healer trait treats illness | Trading, events |
| **Ammo** | Hunting (1 per attempt) | Trading, events |
| **Firewood** | 1 per night camp (2 in alpine biomes) | Events, caches |
| **Lantern Oil** | 1 per night travel action | Events, caches |
| **Salt** | Prevents food spoilage (checked every 3 days) | Events, trading |
| **Cloth** | Desperate repairs, trading | Events, caches |

### Gear

| Resource | Purpose |
|----------|---------|
| **Parts** | Wagon repairs (1 per repair action) |
| **Tools** | Improve repair quality (+10 condition) |
| **Rope** | Helps with river crossings and rescue events |
| **Boots** | Worn by rough terrain, repaired with cloth |

**Wagon Condition** (0-100) degrades from travel and events, restored by repair actions and maintenance windows.

## Physics Engine

The game uses deterministic physics with seeded RNG. Given the same seed and same choices, the outcome is identical every time.

- **Scarcity curve** -- consumption scales up to 1.12x as you approach the end of the trail (journey pressure)
- **Breakdown probability** -- scales with wagon condition: well-maintained (>60%) gets 0.3x chance, damaged (<30%) gets 1.8x. Hard pace adds +3% on top
- **Doctrine modifiers** -- adjust specific rates (consumption, repair quality, travel speed)
- **Night mechanics** -- traveling at evening/night without lantern oil has a 15% chance of bonus breakdown or injury. Firewood is consumed during evening/night camps

## Party Traits

Each party member spawns with 1-2 traits that provide passive bonuses:

| Trait | Effect |
|-------|--------|
| **Tough** | General resilience |
| **Healer** | Auto-treats sick members using meds (+10 health, cures sickness) |
| **Tracker** | +15% hunting success chance |
| **Mechanic** | -8 breakdown damage, +10 repair quality, +20% desperate repair success |
| **Lucky** | General fortune |
| **Anxious** | Narrative flavor |
| **Steady** | General resilience |
| **Sharp Eye** | +10% hunting success chance |

## Biomes

The route passes through five terrain types, each with gameplay effects:

| Biome | Water | Food Spoilage | Sickness Risk | Hunting | Notes |
|-------|-------|--------------|---------------|---------|-------|
| **Plains** | Available | Normal | Normal | Good | Balanced terrain |
| **Forest** | Available | Normal | Normal | Best | Hunting bonus +15% |
| **Desert** | Scarce | Normal | Normal | Poor | Water consumption 1.5x |
| **Swamp** | Available | Normal | High | Normal | Sickness chance +4% |
| **Alpine** | Available | Normal | Normal | Normal | Food consumption 1.2x, firewood 2x |

## Twists

Each run generates 1-2 twist modifiers from the seed that change global conditions:

- **Bandit Year** -- More hostile events and ambushes
- **Sick Season** -- Higher baseline sickness rates
- **Flood Year** -- River crossings are more dangerous
- **Early Winter** -- Cold weather arrives sooner
- **Good Hunting** -- Hunting yields are improved

Twists are shown at the start of each run.

## Events

80+ events interrupt travel with choices. Each event offers 2-3 options:

- **Cautious** — Safer but costs time or supplies
- **Bold** — Faster but risks damage or loss
- **Creative** — Sometimes available; unconventional approaches with mixed outcomes

Events aren't random — they're drawn from a seeded deck and influenced by terrain, party state, and doctrine. The same seed produces the same event sequence.

## Escape Valves

Emergency actions available when things get desperate. Each has side effects and a cooldown:

| Valve | Trigger Condition | Effect | Side Effect | Cooldown |
|-------|-------------------|--------|-------------|----------|
| **Hard Ration** | Food < 3 per person | Halves food and water consumption for 2 travel steps | -10 morale, -5 health to all members | Shared cooldown |
| **Desperate Repair** | Wagon < 30% AND no parts | 50% chance: +15 wagon condition (success) or -10 and someone injured (failure) | May consume cloth if available; Mechanic trait adds +20% success | Shared cooldown |
| **Abandon Cargo** | Wagon < 30% condition | Drops half of non-essential supplies (salt, cloth, rope, boots), +25 wagon condition | -8 morale | Shared cooldown |

## Pace

| Pace | Distance/Turn | Supply Burn | Breakdown Bonus |
|------|--------------|-------------|-----------------|
| **Slow** | 3 miles | 0.8x | +0% |
| **Steady** | 5 miles | 1.0x | +1% |
| **Hard** | 8 miles | 1.3x | +3% |

Steady is the default and usually optimal. Hard pace is for when you're close to the exit with supplies to burn. Slow pace is for when the wagon is one bump from breaking.

## Doctrines and Taboos

Each run assigns one doctrine and one taboo from the seed. These modify the rules for the entire run.

### Doctrines

| Doctrine | Consumption | Travel | Repair | Other |
|----------|-------------|--------|--------|-------|
| **Travel Light** | 0.8x | Normal | Normal | +5% breakdown chance, +10% hunting bonus |
| **Careful Hands** | Normal | -1 mile/turn | 1.5x repair quality | Maintenance bonus +1 |
| **No Debts** | Normal | Normal | Normal | Morale floor 20, 0.85x wagon capacity, +15% trade bonus |

### Taboos

- **Never Night** -- The party refuses to travel at night
- **Never River** -- The party refuses to drink river water
- **Leave Nothing** -- The party leaves nothing behind

Taboos are narrative constraints that the GM incorporates into storytelling.

## Maintenance Windows

Resting and repairing in consecutive turns (either order) triggers a maintenance window -- a temporary state where breakdown probability drops to 30% of normal. This is the primary tool for keeping your wagon alive through the mid-game.

## Warning Callouts

The game warns you when resources hit critical levels:

- **Verbose mode** (default) — Early warnings and cliff-edge alerts
- **Minimal mode** — Only cliff-edge warnings (last-moment, critical threats)

Switch with `--callouts minimal` or `--callouts verbose`.
