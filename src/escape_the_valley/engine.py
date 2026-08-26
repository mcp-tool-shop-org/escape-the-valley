"""Game engine — main loop, turn processing, event orchestration."""

from __future__ import annotations

import logging

from .events import (
    EventOutcome,
    apply_outcome,
    build_event_library,
    resolve_event,
    select_event,
)
from .gm import GMClient, GMConfig
from .memory import build_gm_brief
from .models import (
    DOCTRINE_MODIFIERS,
    JournalEntry,
    RunState,
    SeededRNG,
    TimeOfDay,
)
from .physics import (
    abandon_cargo,
    apply_breakdown,
    attempt_hunt,
    attempt_repair,
    can_abandon_cargo,
    can_desperate_repair,
    can_hard_ration,
    check_breakdown,
    check_game_over,
    check_health_effects,
    check_night_travel_danger,
    check_spoilage,
    compute_daily_consumption,
    compute_travel_distance,
    desperate_repair,
    halve_consumption,
    hard_ration,
    rest_day,
    update_morale,
)
from .save import save_game
from .step_engine import compute_ending
from .ui import (
    console,
    show_action_menu,
    show_event_scene,
    show_game_over,
    show_journal,
    show_message,
    show_outcome,
    show_pace_menu,
    show_route_choice,
    show_status,
)
from .worldgen import generate_weather

log = logging.getLogger(__name__)

# F-d4a8ed17 / F-15a1534a: same letter set and template cap as
# step_engine.py -- GameEngine's GM-scene branch used to forward raw
# scene.choices, so non-letter GM ids made every offered choice a miss.
_EVENT_CHOICE_LETTERS = "ABCDEFG"


def _template_choice_letters(event) -> list[str]:
    """A-G letters this event's outcome_templates actually define.

    GM scenes may list 2-4 choices; the library's templates are A/B or A/B/C.
    Offering a letter resolve_event cannot honor is the engine lying.
    """
    return [letter for letter in _EVENT_CHOICE_LETTERS if letter in event.outcome_templates]


class GameEngine:
    """Main game engine that orchestrates turns, events, and GM interaction."""

    def __init__(self, state: RunState, gm_config: GMConfig | None = None):
        self.state = state
        self.rng = SeededRNG(state.seed, state.rng_counter)
        # ENG-A-01: restore the exact PRNG position from the full saved state.
        # Counter-replay is lossy (variable draws per call), so prefer the
        # serialized Mersenne-Twister state when the save carries it. Legacy
        # saves without rng_state fall back to counter-replay (unchanged).
        if state.rng_state is not None:
            try:
                self.rng.setstate(state.rng_state)
            except (TypeError, ValueError):
                # F-76bab66d: SeededRNG(seed, counter) above already replayed
                # from rng_counter. A malformed payload must not brick construct.
                log.warning(
                    "malformed rng_state; falling back to counter-replay "
                    "(rng_counter=%s)",
                    state.rng_counter,
                )
        self.event_library = build_event_library()
        self.gm = GMClient(gm_config)

    def run(self) -> None:
        """Main game loop."""
        show_status(self.state)

        while not self.state.game_over:
            action = self._prompt_camp_action()

            if action == "1":
                self._do_travel()
            elif action == "2":
                self._do_rest()
            elif action == "3":
                self._do_hunt()
            elif action == "4":
                self._do_repair()
            elif action == "5":
                show_status(self.state)
            elif action == "6":
                self._do_change_pace()
            elif action == "7":
                show_journal(self.state.journal)
            elif action == "8":
                self._do_abandon_cargo()
            elif action == "9":
                self._do_desperate_repair()
            elif action == "0":
                self._do_hard_ration()
            elif action == "Q":
                self._save()
                show_message("Game saved.", "bold green")
                return

            self._check_game_over()

            # Autosave after each action
            self._save()

        if self.state.ending is not None:
            style = "bold green" if self.state.victory else "bold red"
            show_message(
                f"{self.state.ending.tier}: {self.state.ending.headline}",
                style,
            )
        show_game_over(self.state)
        self._save()

    def _do_travel(self) -> None:
        """Process a travel action — move, consume, events, advance time."""
        self.state.last_action = "TRAVEL"

        if self.state.maintained_turns_remaining > 0:
            self.state.maintained_turns_remaining -= 1

        if self.state.escape_valve_cooldown > 0:
            self.state.escape_valve_cooldown -= 1

        distance = compute_travel_distance(self.state)

        # Check for route choice at branching nodes. False means the fork
        # has no legitimate route to advance along this action
        # (F-dd6869ca/F-0877c51a: every raw connection is dangling, a
        # self-edge, or both) -- refuse to fake a travel leg: no distance,
        # no supplies, no time, no RNG draws for a journey that can't
        # happen. RNG draw order for every other path (no fork, a
        # multi-route fork, a fork with exactly one real route) is
        # unchanged -- this early return only removes draws that used to
        # happen against a fabricated destination.
        if not self._check_route_choice():
            return

        # Move
        self.state.distance_remaining -= distance
        self.state.distance_traveled += distance

        # Consume supplies. F-6e017443: is_travel=True so night travel
        # spends lantern_oil, matching StepEngine._do_travel.
        consumption = compute_daily_consumption(self.state, is_travel=True)
        self.state.supplies.apply_delta(consumption)

        if self.state.rationing_steps > 0:
            self.state.rationing_steps -= 1
            if self.state.rationing_steps <= 0:
                show_message("Rationing has ended.", "dim")

        # Check for arrival
        if self.state.distance_remaining <= 0:
            self._arrive_at_next_node()
        else:
            show_message(
                f"Traveled {distance} miles. {self.state.distance_remaining} miles remaining.",
                "bold",
            )

        # Advance time
        self._advance_time()

        # F-20fa3769: spoilage after travel, matching StepEngine._do_travel.
        spoilage = check_spoilage(self.state, self.rng)
        if spoilage:
            self.state.supplies.apply_delta(spoilage)
            loss = abs(spoilage.get("food", 0))
            show_message(f"Food spoiled! Lost {loss} food (no salt).", "yellow")

        # Check for breakdown
        breakdown = check_breakdown(self.state, self.rng)
        if breakdown:
            damage = breakdown["wagon_damage"]
            deltas = apply_breakdown(self.state, damage)
            if deltas:
                show_message(
                    f"Wagon breakdown! Repaired. Damage: {damage}",
                    "yellow",
                )
            else:
                show_message(f"Wagon breakdown! No parts for repair. Damage: {damage}", "red bold")

        # F-6e017443: night travel without lantern oil, matching
        # StepEngine._do_travel (after breakdown, after _advance_time).
        night_danger = check_night_travel_danger(self.state, self.rng)
        if night_danger:
            damage = night_danger["wagon_damage"]
            apply_breakdown(self.state, damage)
            show_message(
                f"Dark travel mishap! Wagon damage: {damage}",
                "yellow",
            )

        # Health effects
        effects = check_health_effects(self.state, self.rng)
        for eff in effects:
            if eff["type"] == "died":
                # F-1a1edd7a: cause is already on the effect.
                cause = eff.get("cause") or "the trail"
                show_message(
                    f"{eff['member']} has died ({cause}).",
                    "red bold",
                )
            elif eff["type"] == "fell_sick":
                show_message(f"{eff['member']} has fallen ill.", "yellow")
            elif eff["type"] == "healed":
                show_message(f"{eff['member']} was treated and is recovering.", "green")

        # Random event
        self._trigger_event()

        # Morale update
        update_morale(self.state)

        # Show status after travel
        show_status(self.state)

    def _do_rest(self) -> None:
        """Process a rest action."""
        # F-20fa3769: rest-after-repair opens the maintenance window.
        if self.state.last_action == "REPAIR":
            doc_mods = DOCTRINE_MODIFIERS.get(self.state.doctrine, {})
            duration = 2 + int(doc_mods.get("maintenance_bonus", 0))
            self.state.maintained_turns_remaining = duration
            self.state.supplies.water = max(
                0, self.state.supplies.water - 3,
            )
            show_message(
                "Maintenance window: the wagon rides steady. (-3 water)",
                "green",
            )
        self.state.last_action = "REST"

        consumption = compute_daily_consumption(self.state)
        self.state.supplies.apply_delta(consumption)

        effects = rest_day(self.state, self.rng)
        self._advance_time()

        show_message("The party rests for a day.", "bold")
        for eff in effects:
            if eff["type"] == "recovered":
                show_message(f"  {eff['member']} has recovered.", "green")

        update_morale(self.state)
        show_status(self.state)

    def _do_hunt(self) -> None:
        """Process a hunt action."""
        self.state.last_action = "HUNT"
        if self.state.supplies.ammo <= 0:
            show_message("No ammunition for hunting.", "red")
            return

        result = dict(attempt_hunt(self.state, self.rng))
        injured = str(result.pop("injured", "") or "")
        self.state.supplies.apply_delta(result)

        if int(result.get("food", 0) or 0) > 0:
            show_message(f"Hunt successful! Gained {result['food']} food. Used 1 ammo.", "green")
        else:
            show_message("The hunt yielded nothing. 1 ammo spent.", "yellow")
        if injured:
            show_message(f"{injured} was injured on the hunt.", "yellow")

        # Half-day action — partial consumption (F-4d750550: round toward
        # zero, not floor -- see physics.halve_consumption)
        half_consumption = halve_consumption(compute_daily_consumption(self.state))
        self.state.supplies.apply_delta(half_consumption)

        update_morale(self.state)
        show_status(self.state)

    def _do_repair(self) -> None:
        """Process a repair action."""
        if self.state.supplies.parts <= 0:
            show_message("No parts for repairs.", "red")
            return

        if self.state.wagon.condition >= 90:
            show_message("Wagon is in good condition. No repair needed.", "dim")
            return

        # F-20fa3769: repair-after-rest opens the maintenance window.
        if self.state.last_action == "REST":
            doc_mods = DOCTRINE_MODIFIERS.get(self.state.doctrine, {})
            duration = 2 + int(doc_mods.get("maintenance_bonus", 0))
            self.state.maintained_turns_remaining = duration
            self.state.supplies.water = max(
                0, self.state.supplies.water - 3,
            )
            show_message(
                "Maintenance window: the wagon rides steady. (-3 water)",
                "green",
            )
        self.state.last_action = "REPAIR"

        deltas = attempt_repair(self.state)
        self.state.supplies.apply_delta(deltas)
        show_message(
            f"Wagon repaired. Condition: {self.state.wagon.condition}/100. Used 1 part.",
            "green",
        )

        # Half-day action (F-4d750550: round toward zero, not floor -- see
        # physics.halve_consumption)
        half_consumption = halve_consumption(compute_daily_consumption(self.state))
        self.state.supplies.apply_delta(half_consumption)

        show_status(self.state)

    def _do_change_pace(self) -> None:
        """Change travel pace."""
        new_pace = show_pace_menu(self.state.wagon.pace)
        self.state.wagon.pace = new_pace
        show_message(f"Pace set to {new_pace.value}.", "bold")

    def _prompt_camp_action(self) -> str:
        """Classic 1-7 plus escape valves when their physics gates open.

        F-20fa3769: show_action_menu is 1-7/Q only. Extra keys (8/9/0)
        dispatch abandon cargo / desperate repair / hard ration without
        merging this loop with StepEngine.
        """
        extras: list[tuple[str, str]] = []
        if can_abandon_cargo(self.state):
            extras.append(("8", "Abandon cargo"))
        if can_desperate_repair(self.state):
            extras.append(("9", "Desperate repair"))
        if can_hard_ration(self.state):
            extras.append(("0", "Hard ration"))
        if not extras:
            return show_action_menu()

        console.print()
        actions = [
            ("1", "Travel"),
            ("2", "Rest"),
            ("3", "Hunt"),
            ("4", "Repair wagon"),
            ("5", "Check supplies"),
            ("6", "Change pace"),
            ("7", "View journal"),
            *extras,
            ("Q", "Quit and save"),
        ]
        for key, label in actions:
            console.print(f"  [bold]{key}[/bold]. {label}")
        console.print()
        valid = {a[0] for a in actions}
        while True:
            answer = console.input("[bold]What do you do? [/bold]").strip().upper()
            if answer in valid:
                return answer
            console.print(f"  [dim]Choose: {', '.join(sorted(valid))}[/dim]")

    def _do_abandon_cargo(self) -> None:
        self.state.last_action = "ABANDON_CARGO"
        if not can_abandon_cargo(self.state):
            show_message(
                "Wagon is not damaged enough to justify abandoning cargo.",
                "dim",
            )
            return

        self.state.escape_valve_cooldown = 3
        result = abandon_cargo(self.state)
        dropped_items = [
            f"-{abs(v)} {k}" for k, v in result.items() if v < 0
        ]
        show_message(
            "Abandoned cargo to lighten the wagon. "
            f"Wagon +25. "
            f"Dropped: {', '.join(dropped_items) or 'nothing'}. "
            "Morale fell.",
            "yellow",
        )

    def _do_desperate_repair(self) -> None:
        self.state.last_action = "DESPERATE_REPAIR"
        if not can_desperate_repair(self.state):
            show_message(
                "Desperate repair requires a badly damaged wagon "
                "and no spare parts.",
                "dim",
            )
            return

        self.state.escape_valve_cooldown = 3
        result = desperate_repair(self.state, self.rng)
        if result.get("success"):
            show_message(
                f"Desperate repair succeeded! "
                f"Wagon +{result.get('wagon_delta', 15)}.",
                "green",
            )
        else:
            injured = result.get("injured", "someone")
            show_message(
                f"Desperate repair failed! "
                f"Wagon {result.get('wagon_delta', -10)}. "
                f"{injured} was injured in the attempt.",
                "red",
            )

    def _hard_ration_refusal_line(self) -> str:
        """Name the closed gate. Same three sentences as StepEngine."""
        state = self.state
        alive = state.party.alive_count
        if alive <= 0 or state.supplies.food >= alive * 3:
            return "Food is not low enough to ration."
        if state.escape_valve_cooldown > 0:
            return (
                f"Cannot ration again for {state.escape_valve_cooldown} "
                "more actions."
            )
        if state.rationing_steps > 0:
            return "Already rationing."
        return "Cannot ration further right now."

    def _do_hard_ration(self) -> None:
        self.state.last_action = "HARD_RATION"
        if not can_hard_ration(self.state):
            show_message(self._hard_ration_refusal_line(), "dim")
            return

        self.state.escape_valve_cooldown = 3
        hard_ration(self.state)
        show_message(
            "Hard rationing imposed for 2 days. "
            "Food and water consumption halved. "
            "Morale -10, everyone weakened.",
            "yellow",
        )

    def _check_game_over(self) -> None:
        result = check_game_over(self.state)
        if not result:
            return
        if result == "VICTORY":
            self.state.victory = True
            self.state.game_over = True
        else:
            self.state.cause_of_death = result
            self.state.game_over = True
        # F-20fa3769: grade once on the terminal transition (pure, no RNG).
        ending = compute_ending(self.state)
        self.state.ending = ending

    def _backpack_persist_hook(self, state: RunState) -> None:
        """Mid-settlement persist. GameEngine always autosaves to CWD."""
        save_game(state)

    def _settle_checkpoint(self, dest) -> None:
        """Settle Ledger Backpack at a town checkpoint.

        Pairwise with StepEngine: if settle() raises before enqueue, log
        and guarantee a pending SettlementRecord so the delta is retryable.
        """
        try:
            from .backpack import BackpackManager

            mgr = BackpackManager(persist=self._backpack_persist_hook)
            try:
                result = mgr.settle(self.state, dest.name)
                if result.success and result.txids:
                    show_message(result.message, "green")
                elif not result.success and result.message:
                    show_message(result.message, "yellow")
            finally:
                mgr.close()
        except Exception as e:  # graceful degradation — game continues
            log.warning("Checkpoint settlement at %s failed: %s", dest.name, e)
            self._enqueue_pending_settlement(dest.name)

    def _enqueue_pending_settlement(self, location: str) -> None:
        """Record the current unsettled delta as a pending SettlementRecord."""
        from datetime import UTC, datetime

        from .backpack_models import XRPL_RESOURCES, SettlementRecord

        bp = self.state.backpack
        deltas: dict[str, int] = {}
        for key in XRPL_RESOURCES:
            diff = self.state.supplies.get(key) - bp.last_settled_supplies.get(key, 0)
            if diff != 0:
                deltas[key] = diff
        if not deltas:
            return
        if any(r.day == self.state.day for r in bp.pending_settlements):
            return
        bp.pending_settlements.append(SettlementRecord(
            day=self.state.day,
            location=location,
            deltas=deltas,
            txids=[],
            status="pending",
            memo=f"TRAIL|RUN:{self.state.run_id}|DAY:{self.state.day}",
            timestamp=datetime.now(UTC).isoformat(),
        ))

    def _check_parcels(self, dest) -> None:
        """Check for incoming parcels at a town."""
        try:
            from .backpack import BackpackManager

            mgr = BackpackManager(persist=self._backpack_persist_hook)
            try:
                parcels = mgr.check_parcels(self.state)
                for parcel in parcels:
                    sender_short = parcel.sender[:8] + "..."
                    contents = ", ".join(
                        f"{v} {k}" for k, v in parcel.contents.items()
                    )
                    show_message(
                        f"A parcel arrived from {sender_short}: {contents}",
                        "green",
                    )
            finally:
                mgr.close()
        except Exception as e:  # graceful degradation
            log.warning("Parcel check at %s failed: %s", dest.name, e)

    def _trigger_event(self) -> None:
        """Select and run a random event."""
        # ~60% chance of event per travel action
        if self.rng.random() > 0.6:
            return

        # ENG-A-06: derive weather BEFORE selection so weather-gated events are
        # actually filtered, and reuse the same value for GM narration.
        node = _find_node(self.state)
        weather = generate_weather(self.rng, node.biome, self.state.day) if node else None
        event = select_event(self.state, self.rng, self.event_library, weather)

        # Try GM narration first
        scene = None
        if self.gm.config.enabled:
            weather_str = weather.value if weather else "unknown"
            # gm-A-101: pass the GM brief so the D2 weirdness gate
            # (uncanny only at weirdness_level >= 2 with tokens) is enforced
            # in-prompt on the CLI play/continue path, same as the TUI.
            brief = build_gm_brief(self.state)
            scene = self.gm.generate_scene(
                self.state, event, weather_str, brief=brief
            )

        # Use GM scene or fallback
        if scene and scene.choices:
            title = scene.title or event.title
            narration = scene.narration or event.fallback_narration
            # F-d4a8ed17: coerce positionally onto A-G rather than trusting
            # the GM's id field. F-15a1534a: cap to keys that exist in
            # event.outcome_templates so a 4-choice GM scene on a 2-template
            # event offers A/B, never an unresolvable letter. zip drops
            # extras; fallback_choices below already match templates 1:1.
            letters = _template_choice_letters(event)
            choices = [
                {
                    "id": letter,
                    "label": c.get("label", "?"),
                    "risk_hint": c.get("risk_hint", ""),
                    "cost_hint": c.get("cost_hint", ""),
                }
                for letter, c in zip(letters, scene.choices, strict=False)
            ]
        else:
            title = event.title
            narration = event.fallback_narration
            choices = [
                {
                    "id": c.choice_id,
                    "label": c.label,
                    "risk_hint": c.risk_hint,
                    "cost_hint": c.cost_hint,
                }
                for c in event.fallback_choices
            ]

        if not choices:
            return

        # Show scene and get player choice
        choice_id = show_event_scene(title, narration, choices)

        # F-d10a2a2f: validate choice_id against what was actually offered
        # before committing to it, rather than trusting show_event_scene (UI,
        # or unvalidated GM JSON on the scene.choices branch) to only ever
        # return an offered id. Mirrors this file's OWN _check_route_choice
        # (F-6e5e72a8) fallback-with-log-warning convention: GameEngine is
        # synchronous with no EVENT phase to re-prompt from (unlike
        # step_engine.py._handle_event_choice's ENG-B-06 reject-and-retry),
        # so an unrecognized id falls back to the first offered choice
        # instead of sailing through to resolve_event() unchecked. Without
        # this, resolve_event() silently no-ops on an unmatched id (a blank
        # EventOutcome, F-fa99f19f) and choice_label below stays "", so the
        # journal entry reads "{choice_id}: " with no label -- wrong-but-
        # safe, but still worth closing at the source.
        valid_ids = [c.get("id") for c in choices]
        if choice_id not in valid_ids:
            log.warning(
                "show_event_scene returned unrecognized choice_id %r; "
                "expected one of %r -- defaulting to first choice",
                choice_id, valid_ids,
            )
            choice_id = valid_ids[0]

        # Resolve outcome
        outcome = resolve_event(self.state, event, choice_id, self.rng)
        apply_outcome(self.state, outcome)

        # ENG-A-05 (resolved): charge the event's time_cost to the clock here (the
        # engine clock hook that pure apply_outcome lacks). Each unit advances one
        # time-of-day slot, additive to the per-action slot _do_travel already
        # charged, so WAIT/DETOUR/REST choices carry a real opportunity cost
        # (nocturnal firewood/lantern-oil drain, the day-tick spoilage check, a
        # weather reroll) without double-charging consumption — resource costs ride
        # on supplies_delta. _advance_time draws no RNG, so determinism holds.
        for _ in range(max(0, outcome.time_cost)):
            self._advance_time()

        # Get choice label
        choice_label = ""
        for c in choices:
            if c.get("id") == choice_id:
                choice_label = c.get("label", "")
                break

        # Try GM outcome narration
        outcome_narration = ""
        outcome_title = ""
        callout = ""

        if self.gm.config.enabled and scene:
            outcome_facts = {
                "Supplies delta": outcome.supplies_delta,
                "Health effects": outcome.health_delta,
                "Wagon effects": outcome.wagon_delta,
                "Morale effects": outcome.morale_delta,
                "Time cost": outcome.time_cost,
                "Special": outcome.special_flags,
            }
            # gm-A-101: rebuild the brief against post-outcome state so the
            # D2 weirdness gate is enforced on the outcome prompt too.
            outcome_brief = build_gm_brief(self.state)
            gm_outcome = self.gm.generate_outcome(
                self.state,
                event,
                title,
                choice_id,
                choice_label,
                outcome_facts,
                brief=outcome_brief,
            )
            if gm_outcome:
                outcome_narration = gm_outcome.outcome_narration
                outcome_title = gm_outcome.outcome_title
                callout = gm_outcome.callout

        if not outcome_narration:
            outcome_title = "Outcome"
            callout = _build_fallback_callout(outcome)
            outcome_narration = callout

        show_outcome(outcome_title, outcome_narration, callout, outcome.supplies_delta)

        # Journal entry
        self.state.journal.append(JournalEntry(
            day=self.state.day,
            location=node.name if node else "unknown",
            event_id=event.event_id,
            scene_title=title,
            narration=narration[:300],
            choice_made=f"{choice_id}: {choice_label}",
            outcome=outcome_narration[:300],
            deltas=outcome.supplies_delta,
            tags=event.tags,
        ))

        # Update RNG counter for determinism
        self.state.rng_counter = self.rng.counter

    def _arrive_at_next_node(self) -> None:
        """Handle arrival at the next node."""
        # Find the destination node
        dest_node = None
        for node in self.state.map_nodes:
            if node.node_id == self.state.destination_id:
                dest_node = node
                break

        if not dest_node:
            # F-e86c2e71: destination_id points at no real node. ENG-B-09 used
            # to "recover" by snapping to map_nodes[-1] -- but that fabricates
            # arrival at an arbitrary node, including the FINAL one, which
            # check_game_over() reads as an outright win. That beeline is
            # reachable purely by hand-editing destination_id in a save with
            # the map/connections left pristine (save.py loads it with no
            # referential check), so no amount of connections-graph
            # validation elsewhere in this method can ever close it.
            #
            # Director's policy: fail safe to the party's CURRENT node,
            # don't stall, don't map_nodes[-1]. By this point location_id
            # has NOT been overwritten yet -- it still holds the node the
            # party departed from, which is guaranteed valid -- so leave it
            # exactly as is. distance_remaining is clamped to 0: this leg's
            # distance/supply cost already ran above (in _do_travel, before
            # this method was even called), so there is nothing left to
            # refuse retroactively.
            #
            # destination_id is repointed at THIS node (self-reference)
            # rather than left holding the original dangling value -- a
            # deliberate departure from "leave it untouched". This mirrors
            # _check_route_choice's own zero-legitimate-connections case
            # just below, which likewise refuses to fabricate a destination.
            # It also matters mechanically: _check_route_choice's
            # single-connection guard validates node.connections[0] in
            # isolation and never compares it against destination_id, so a
            # dangling destination_id left in place could never satisfy
            # that check on any later action -- a node with one perfectly
            # valid onward connection would pay every subsequent leg's cost,
            # never arrive, and drain toward a manufactured false DEATH
            # instead of the false VICTORY this fix closes. Self-
            # referencing lets the next travel action match itself in the
            # search above, run this method's own arrival tail harmlessly
            # against the unchanged current node, and have THAT tail wire
            # the real next hop from this node's actual connections.
            log.warning(
                "destination_id %r not found among map_nodes; failing safe "
                "to current location %r instead of fabricating arrival",
                self.state.destination_id, self.state.location_id,
            )
            show_message(
                "The way ahead doesn't match the map. The party holds "
                "its ground rather than press on blind.",
                "yellow",
            )
            self.state.distance_remaining = 0
            self.state.destination_id = self.state.location_id
            return

        self.state.location_id = dest_node.node_id
        self.state.distance_remaining = 0
        show_message(f"Arrived at {dest_node.name}!", "bold green")

        # F-6e017443: port StepEngine arrival extras (pairwise, not a merge).
        # Water refill at nodes with water sources
        if dest_node.water_available:
            old_water = self.state.supplies.water
            refill = min(20, 50 - old_water)  # Up to 20, capped at 50
            if refill > 0:
                self.state.supplies.water += refill
                show_message(f"Found water. +{refill} water.", "green")

        # Supply cache pickup (one-time)
        if dest_node.cache_supplies:
            cache = dest_node.cache_supplies
            self.state.supplies.apply_delta(cache)
            cache_items = ", ".join(
                f"+{v} {k}" for k, v in cache.items()
            )
            show_message(f"Found a supply cache! {cache_items}", "green")
            dest_node.cache_supplies = None  # consumed

        if dest_node.is_town:
            # Town trade: morale-gated + doctrine-boosted (mirrors StepEngine)
            doc_mods = DOCTRINE_MODIFIERS.get(self.state.doctrine, {})
            trade_chance = 0.30 + doc_mods.get("trade_bonus", 0)
            if (
                self.state.party.morale > 60
                and self.rng.random() < trade_chance
            ):
                food_offer = self.rng.randint(3, 9)
                self.state.supplies.food += food_offer
                show_message(
                    f"Traded at the settlement. +{food_offer} food.",
                    "green",
                )
            else:
                show_message(
                    "This is a settlement. Supplies may be available.",
                    "dim",
                )

            # F-20fa3769: settle the ledger backpack at town when already on.
            if self.state.backpack.enabled:
                self._settle_checkpoint(dest_node)
                self._check_parcels(dest_node)

        # Set up next destination
        if dest_node.connections:
            if len(dest_node.connections) == 1:
                # F-32a5a4a9: the fork arm of this same `if` (>1
                # connections) is validated by _check_route_choice, which
                # excludes a self-edge and requires the id resolve to a
                # real map_nodes entry (F-0877c51a/F-3abad222). This lone-
                # connection arm used to wire destination_id straight from
                # the raw id with no such check -- a dangling id (only
                # reachable via a corrupted/altered save; generate_map()
                # never produces this) would sail through here unnoticed,
                # then get "discovered" on a LATER leg by this method's own
                # dest-not-found fail-safe above (F-e86c2e71; pre-wave-10
                # that beelined to map_nodes[-1] and manufactured a false
                # VICTORY). Apply the same policy here: if the sole
                # connection is a self-edge or doesn't
                # resolve, leave destination_id/distance_remaining exactly
                # as already set above (this node, distance 0) instead of
                # committing to it. _check_route_choice's matching guard
                # turns that stalled state into a loud, zero-cost refusal
                # on the next travel action instead of a silent re-arrival
                # loop that drains supplies/time forever (F-803bd813).
                next_id = dest_node.connections[0]
                if next_id != dest_node.node_id and any(
                    n.node_id == next_id for n in self.state.map_nodes
                ):
                    self.state.destination_id = next_id
                    self.state.distance_remaining = dest_node.distance_to.get(next_id, 15)
            else:
                # Route choice handled next travel action
                pass

    def _check_route_choice(self) -> bool:
        """Check if the player needs to choose a route.

        Returns True when travel should proceed normally this action: no
        fork, still mid-journey, a multi-route fork just resolved via
        show_route_choice, or a fork with exactly one legitimate route
        was taken automatically. Returns False when the fork has no
        legitimate route to advance along this action -- the caller
        (_do_travel) must not fake a travel leg (distance, supplies,
        time, RNG draws) when this returns False.
        """
        node = _find_node(self.state)
        if not node:
            return True

        if len(node.connections) <= 1:
            # F-32a5a4a9: a lone connection skips the fork-choice
            # machinery below entirely (by design -- no player choice is
            # needed for a single path), so it was never subjected to the
            # same self-edge/dangling-id validation a fork's raw
            # connections get. _arrive_at_next_node now refuses to wire
            # destination_id from an invalid sole connection (see its
            # "Set up next destination" comment) and instead leaves
            # destination_id/distance_remaining pointed at THIS node (0
            # remaining). Catch that stalled shape here and refuse to
            # advance, loudly, at zero cost -- every subsequent travel
            # action, not just the first -- instead of silently
            # re-arriving at this same node forever (F-803bd813) or,
            # once a stale distance_remaining ran out on a leg that was
            # never real, hitting _arrive_at_next_node's dest-not-found
            # fail-safe (F-e86c2e71; pre-wave-10 this beelined to
            # map_nodes[-1]).
            if len(node.connections) == 1 and self.state.distance_remaining <= 0:
                conn_id = node.connections[0]
                valid = conn_id != node.node_id and any(
                    n.node_id == conn_id for n in self.state.map_nodes
                )
                if not valid:
                    log.warning(
                        "node %r has one connection (%r) that does not "
                        "resolve to a real, non-self map node; no "
                        "legitimate route exists -- refusing to advance "
                        "travel instead of fabricating progress",
                        node.node_id, conn_id,
                    )
                    show_message(
                        "The trail forks, but every route is broken or "
                        f"leads nowhere. The party can't press on from "
                        f"{node.name} this way.",
                        "red bold",
                    )
                    return False
            return True

        if self.state.distance_remaining > 0:
            return True  # Not at a junction yet

        # Build connection info, skipping any connection id that resolves
        # back to THIS node. F-0877c51a: a self-edge can never be a
        # legitimate route -- counting it toward "how many resolved"
        # (whether it lands alone or alongside other real connections)
        # lets the branches below commit to it, which reproduces the
        # original infinite-travel bug this method exists to close: the
        # party "arrives" at the node it never left, forever.
        connections = []
        for conn_id in node.connections:
            if conn_id == node.node_id:
                continue
            for n in self.state.map_nodes:
                if n.node_id == conn_id:
                    dist = node.distance_to.get(conn_id, 15)
                    connections.append((conn_id, n.name, dist))
                    break

        if len(connections) > 1:
            chosen_id = show_route_choice(connections)
            # F-6e5e72a8: validate chosen_id against the options actually
            # offered rather than trusting the caller (ui.show_route_choice)
            # alone -- mirrors step_engine.py._handle_route_choice's post-
            # F-7d3e005b rejection. The engine, not the UI, is the
            # enforcement boundary for what constitutes a valid choice.
            # (ui.show_route_choice's own input loop already can't return an
            # out-of-range index today, so this is a no-op on the real
            # terminal path and only bites a misbehaving/future caller.)
            valid_ids = [c[0] for c in connections]
            if chosen_id not in valid_ids:
                log.warning(
                    "show_route_choice returned unrecognized choice_id %r; "
                    "expected one of %r -- defaulting to first route",
                    chosen_id, valid_ids,
                )
                chosen_id = valid_ids[0]
            self.state.destination_id = chosen_id
            self.state.distance_remaining = node.distance_to.get(chosen_id, 15)
            return True

        if connections:
            # Exactly one non-self connection resolved to a real node.
            # That is a legitimate route (not a corrupted one) -- take it
            # directly, same outcome StepEngine reaches for this shape (a
            # single pending route is still offered and, once chosen,
            # committed verbatim; no reason to discard real route data in
            # favor of an unrelated fallback node here). F-049017cc: this
            # gets its own accurate message -- it is not a fallback and
            # not "the last known waypoint", it is simply the one real
            # option, so it must not share text with the genuine
            # zero-resolve recovery below. (show_route_choice is
            # deliberately NOT used here: it blocks on interactive
            # console input, which would turn this automatic recovery
            # path into a hang for any non-interactive caller.)
            dest_id, dest_name, dist = connections[0]
            log.info(
                "node %r has %d connection(s) but only one resolves to a "
                "real, non-self map node; taking the sole real route to %r",
                node.node_id, len(node.connections), dest_id,
            )
            show_message(
                f"Only one route is passable; the trail continues toward {dest_name}.",
                "yellow",
            )
            self.state.destination_id = dest_id
            self.state.distance_remaining = dist
            return True

        # Zero legitimate (non-self) connections resolved: every raw
        # connection is either dangling or points back at this same node
        # (a corrupted/altered save -- generate_map() never produces this
        # shape). F-dd6869ca: do NOT fall back to map_nodes[-1] here --
        # on a mid-map fork that silently beelines the party past
        # unvisited content into a fabricated, unearned VICTORY.
        # F-0877c51a: do NOT commit to a self-edge either -- that
        # reproduces the original infinite-travel bug this method exists
        # to prevent (destination_id == this node's own id, a permanent
        # silent self-referential loop with distance_traveled and supply
        # drain climbing for zero real progress). There is no legitimate
        # destination to recover to, so this leaves destination_id/
        # distance_remaining completely untouched -- no fake arrival, no
        # fake route -- and tells the caller to refuse to advance.
        log.warning(
            "node %r has %d connection(s) but none resolve to a real, "
            "non-self map node; no legitimate route exists -- refusing "
            "to advance travel instead of fabricating progress",
            node.node_id, len(node.connections),
        )
        show_message(
            "The trail forks, but every route is broken or leads nowhere. "
            f"The party can't press on from {node.name} this way.",
            "red bold",
        )
        return False

    def _advance_time(self) -> None:
        """Advance time of day and day counter."""
        times = [TimeOfDay.MORNING, TimeOfDay.AFTERNOON, TimeOfDay.EVENING, TimeOfDay.NIGHT]
        current_idx = times.index(self.state.time_of_day)

        if current_idx < len(times) - 1:
            self.state.time_of_day = times[current_idx + 1]
        else:
            self.state.time_of_day = TimeOfDay.MORNING
            self.state.day += 1

    def _save(self) -> None:
        """Autosave current state."""
        self.state.rng_counter = self.rng.counter
        self.state.rng_state = self.rng.getstate()
        save_game(self.state)


def _build_fallback_callout(outcome: EventOutcome) -> str:
    """Build a simple callout from outcome data."""
    parts = []
    for key, val in outcome.supplies_delta.items():
        if val > 0:
            parts.append(f"+{val} {key}")
        elif val < 0:
            parts.append(f"{val} {key}")

    if outcome.health_delta:
        parts.append(f"health {'+'if outcome.health_delta > 0 else ''}{outcome.health_delta}")
    if outcome.wagon_delta:
        parts.append(f"wagon {'+'if outcome.wagon_delta > 0 else ''}{outcome.wagon_delta}")
    if outcome.morale_delta:
        parts.append(f"morale {'+'if outcome.morale_delta > 0 else ''}{outcome.morale_delta}")
    # ENG-A-05 (resolved): the engine now advances the clock by time_cost on event
    # resolution (see _trigger_event), so reporting "time lost" is truthful.
    if outcome.time_cost:
        parts.append(f"{outcome.time_cost} time lost")

    return ", ".join(parts) if parts else "No significant effect."


def _find_node(state: RunState):
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None
