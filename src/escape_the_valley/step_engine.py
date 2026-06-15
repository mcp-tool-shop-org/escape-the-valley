"""Step-based game engine — pure logic, no UI imports.

Usage:
    engine = StepEngine(state, gm_config)
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    frame = state_to_frame(engine)   # adapter.py
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field

from .events import (
    EventOutcome,
    EventSkeleton,
    apply_outcome,
    build_event_library,
    resolve_event,
    select_event,
)
from .gm import GMClient, GMConfig
from .intent import GamePhase, IntentAction, PlayerIntent
from .memory import build_gm_brief
from .memory_emitters import (
    check_resource_crises,
    emit_arrival_card,
    emit_escape_valve_card,
    emit_event_card,
    emit_health_cards,
    emit_wagon_card,
    validate_gm_cards,
)
from .models import (
    EndingResult,
    JournalEntry,
    Pace,
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
    determine_cause_of_death,
    hard_ration,
    rest_day,
    update_morale,
)
from .save import save_game
from .worldgen import generate_weather

log = logging.getLogger(__name__)

# ── Message types ───────────────────────────────────────────────────

@dataclass
class EventChoiceInfo:
    """A single choice the player can pick during an event."""

    id: str
    label: str
    risk_hint: str = ""
    cost_hint: str = ""


@dataclass
class RouteOption:
    """A fork in the road."""

    node_id: str
    name: str
    distance: int


@dataclass
class StepMessages:
    """Everything that happened during one step — UI reads this."""

    lines: list[str] = field(default_factory=list)
    event_title: str = ""
    event_narration: str = ""
    event_choices: list[EventChoiceInfo] = field(default_factory=list)
    route_options: list[RouteOption] = field(default_factory=list)
    outcome_title: str = ""
    outcome_narration: str = ""
    outcome_deltas: dict[str, int] = field(default_factory=dict)
    # ENG-B-05 (CONTRACT): the GM was asked for narration but returned None and
    # the engine fell back to deterministic narration for this step. cli-tui
    # renders these so the player knows the trail's own voice is speaking.
    gm_degraded: bool = False
    gm_degraded_reason: str = ""
    # EC-04 (CONTRACT): the graded ending, populated only on the step that
    # transitions to GAME_OVER (victory or death). GM narrates it; cli-tui
    # renders it on the end screen. None on every non-terminal step.
    ending: EndingResult | None = None


# ── Engine ──────────────────────────────────────────────────────────

class StepEngine:
    """Game engine that processes one PlayerIntent per step().

    No UI imports. Returns StepMessages describing what happened.
    """

    def __init__(
        self,
        state: RunState,
        gm_config: GMConfig | None = None,
    ):
        self.state = state
        self.rng = SeededRNG(state.seed, state.rng_counter)
        # ENG-A-01: restore the exact PRNG position from the full saved state.
        # Counter-replay is lossy (variable draws per call), so prefer the
        # serialized Mersenne-Twister state when the save carries it. Legacy
        # saves without rng_state fall back to counter-replay (unchanged).
        if state.rng_state is not None:
            self.rng.setstate(state.rng_state)
        self.event_library = build_event_library()
        self.gm = GMClient(gm_config or GMConfig())
        self.phase = GamePhase.CAMP
        self.msgs = StepMessages()

        # Diagnostics counters
        self.diagnostics: dict[str, int] = {
            "wagon_breakdowns": 0,
            "events_high_sev": 0,
            "events_total": 0,
            "maintenance_windows": 0,
            "caches_found": 0,
            "escape_valves_used": 0,
            # ENG-B-05 (CONTRACT): GM call accounting, surfaced by cli-tui 'stats'.
            "gm_calls": 0,
            "gm_fallbacks": 0,
        }

        # ENG-B-05: emit the "GM narration unavailable" note only once per session.
        self._gm_degraded_noted = False

        # Pending state for multi-step flows
        self._pending_event: EventSkeleton | None = None
        self._pending_event_choices: list[EventChoiceInfo] = []
        self._pending_event_scene: object | None = None
        # ENG-B-06: keep the offered scene text so an invalid-choice retry can
        # re-present the event verbatim instead of losing it.
        self._pending_event_title: str = ""
        self._pending_event_narration: str = ""
        self._pending_routes: list[RouteOption] = []

        # Check initial conditions
        self._check_initial_phase()

    def _check_initial_phase(self) -> None:
        """Set phase based on current state (e.g. after load)."""
        if self.state.game_over:
            self.phase = GamePhase.GAME_OVER
            return

        # Check if at a fork
        node = _find_node(self.state)
        if (
            node
            and len(node.connections) > 1
            and self.state.distance_remaining <= 0
        ):
            self._build_route_choices(node)
            self.phase = GamePhase.ROUTE

    def step(self, intent: PlayerIntent) -> StepMessages:
        """Process one player action. Returns messages for the UI."""
        self.msgs = StepMessages()

        if self.phase == GamePhase.GAME_OVER:
            self.msgs.lines.append("This run is over.")
            return self.msgs

        # ENG-B-08: never roll events or call the GM against a dead party. If
        # everyone has perished, settle into game-over immediately rather than
        # processing the action (which could trigger an event on corpses).
        if self.state.party.alive_count == 0:
            self._check_game_over()
            self.msgs.lines.append("This run is over.")
            self._save()
            return self.msgs

        if self.phase == GamePhase.EVENT:
            self._handle_event_choice(intent)
        elif self.phase == GamePhase.ROUTE:
            self._handle_route_choice(intent)
        elif self.phase == GamePhase.CAMP:
            self._handle_camp(intent)

        # Check game over after any action
        self._check_game_over()

        # Autosave
        self._save()

        return self.msgs

    # ── CAMP phase handlers ─────────────────────────────────────────

    def _handle_camp(self, intent: PlayerIntent) -> None:
        if intent.action == IntentAction.TRAVEL:
            self._do_travel()
        elif intent.action == IntentAction.REST:
            self._do_rest()
        elif intent.action == IntentAction.HUNT:
            self._do_hunt()
        elif intent.action == IntentAction.REPAIR:
            self._do_repair()
        elif intent.action == IntentAction.CHANGE_PACE:
            self._do_change_pace(intent.pace)
        elif intent.action == IntentAction.ABANDON_CARGO:
            self._do_abandon_cargo()
        elif intent.action == IntentAction.DESPERATE_REPAIR:
            self._do_desperate_repair()
        elif intent.action == IntentAction.HARD_RATION:
            self._do_hard_ration()

    def _do_travel(self) -> None:
        self.state.last_action = "TRAVEL"

        # Decrement maintenance window
        if self.state.maintained_turns_remaining > 0:
            self.state.maintained_turns_remaining -= 1

        # Decrement escape valve cooldown
        if self.state.escape_valve_cooldown > 0:
            self.state.escape_valve_cooldown -= 1

        # Check if at a fork first
        node = _find_node(self.state)
        if (
            node
            and len(node.connections) > 1
            and self.state.distance_remaining <= 0
        ):
            self._build_route_choices(node)
            self.phase = GamePhase.ROUTE
            self.msgs.lines.append(
                "The trail forks ahead. Choose your path."
            )
            return

        distance = compute_travel_distance(self.state)

        # Move
        self.state.distance_remaining -= distance
        self.state.distance_traveled += distance

        # Consume
        consumption = compute_daily_consumption(
            self.state, is_travel=True,
        )
        self.state.supplies.apply_delta(consumption)

        # Decrement rationing countdown
        if self.state.rationing_steps > 0:
            self.state.rationing_steps -= 1
            if self.state.rationing_steps <= 0:
                self.msgs.lines.append("Rationing has ended.")

        # Arrival check
        if self.state.distance_remaining <= 0:
            self._arrive_at_next_node()
        else:
            self.msgs.lines.append(
                f"Traveled {distance} miles. "
                f"{self.state.distance_remaining} miles remaining."
            )

        # Advance time
        self._advance_time()

        # Spoilage check
        spoilage = check_spoilage(self.state, self.rng)
        if spoilage:
            self.state.supplies.apply_delta(spoilage)
            loss = abs(spoilage.get("food", 0))
            self.msgs.lines.append(
                f"Food spoiled! Lost {loss} food (no salt)."
            )

        # Breakdown
        breakdown = check_breakdown(self.state, self.rng)
        if breakdown:
            self.diagnostics["wagon_breakdowns"] += 1
            damage = breakdown["wagon_damage"]
            had_parts = self.state.supplies.parts > 0
            deltas = apply_breakdown(self.state, damage)
            if deltas:
                self.msgs.lines.append(
                    f"Wagon breakdown! Repaired. Damage: {damage}"
                )
            else:
                self.msgs.lines.append(
                    f"Wagon breakdown! No parts. Damage: {damage}"
                )
            emit_wagon_card(self.state, damage, had_parts)
            # ENG-B-02: a breakdown is a milestone worth remembering.
            self._record_milestone(
                "wagon:breakdown",
                "Wagon Breakdown",
                f"The wagon broke down. Damage: {damage}."
                + ("" if had_parts else " No spare parts on hand."),
                tags=["wagon", "breakdown"],
            )

        # Night travel danger (no lantern oil)
        night_danger = check_night_travel_danger(
            self.state, self.rng,
        )
        if night_danger:
            damage = night_danger["wagon_damage"]
            apply_breakdown(self.state, damage)
            self.msgs.lines.append(
                f"Dark travel mishap! Wagon damage: {damage}"
            )

        # Health effects
        effects = check_health_effects(self.state, self.rng)
        for eff in effects:
            if eff["type"] == "died":
                self.msgs.lines.append(f"{eff['member']} has died.")
            elif eff["type"] == "fell_sick":
                self.msgs.lines.append(
                    f"{eff['member']} has fallen ill."
                )
            elif eff["type"] == "healed":
                self.msgs.lines.append(
                    f"{eff['member']} is recovering."
                )
        self._record_death_milestones(effects)
        emit_health_cards(self.state, effects)

        # Resource crisis check after consumption
        check_resource_crises(self.state)

        # Random event (~60% chance)
        self._maybe_trigger_event()

        # Morale
        update_morale(self.state)

    def _do_rest(self) -> None:
        # Maintenance window: rest after repair
        if self.state.last_action == "REPAIR":
            from .models import DOCTRINE_MODIFIERS
            doc_mods = DOCTRINE_MODIFIERS.get(self.state.doctrine, {})
            duration = 2 + int(doc_mods.get("maintenance_bonus", 0))
            self.state.maintained_turns_remaining = duration
            self.diagnostics["maintenance_windows"] += 1
            # Maintenance costs extra water (thorough work)
            self.state.supplies.water = max(
                0, self.state.supplies.water - 3,
            )
            self.msgs.lines.append(
                "Maintenance window: the wagon rides steady. "
                "(-3 water)"
            )
        self.state.last_action = "REST"

        consumption = compute_daily_consumption(self.state)
        self.state.supplies.apply_delta(consumption)

        effects = rest_day(self.state, self.rng)
        self._advance_time()

        self.msgs.lines.append("The party rests for a day.")
        for eff in effects:
            if eff["type"] == "recovered":
                self.msgs.lines.append(
                    f"  {eff['member']} has recovered."
                )

        update_morale(self.state)

    def _do_hunt(self) -> None:
        self.state.last_action = "HUNT"
        if self.state.supplies.ammo <= 0:
            self.msgs.lines.append("No ammunition for hunting.")
            return

        deltas = attempt_hunt(self.state, self.rng)
        self.state.supplies.apply_delta(deltas)

        food_gain = deltas.get("food", 0)
        if food_gain > 0:
            self.msgs.lines.append(
                f"Hunt successful! +{food_gain} food. -1 ammo."
            )
        else:
            self.msgs.lines.append(
                "The hunt yielded nothing. -1 ammo."
            )

        # Half-day consumption
        half = {
            k: v // 2
            for k, v in compute_daily_consumption(self.state).items()
        }
        self.state.supplies.apply_delta(half)
        update_morale(self.state)

    def _do_repair(self) -> None:
        if self.state.supplies.parts <= 0:
            self.msgs.lines.append("No parts for repairs.")
            return

        if self.state.wagon.condition >= 90:
            self.msgs.lines.append(
                "Wagon is in good shape. No repair needed."
            )
            return

        # Maintenance window: repair after rest
        if self.state.last_action == "REST":
            from .models import DOCTRINE_MODIFIERS
            doc_mods = DOCTRINE_MODIFIERS.get(self.state.doctrine, {})
            duration = 2 + int(doc_mods.get("maintenance_bonus", 0))
            self.state.maintained_turns_remaining = duration
            self.diagnostics["maintenance_windows"] += 1
            # Maintenance costs extra water (thorough work)
            self.state.supplies.water = max(
                0, self.state.supplies.water - 3,
            )
            self.msgs.lines.append(
                "Maintenance window: the wagon rides steady. "
                "(-3 water)"
            )
        self.state.last_action = "REPAIR"

        deltas = attempt_repair(self.state)
        self.state.supplies.apply_delta(deltas)
        self.msgs.lines.append(
            f"Wagon repaired to {self.state.wagon.condition}/100. "
            "-1 part."
        )

        # Half-day consumption
        half = {
            k: v // 2
            for k, v in compute_daily_consumption(self.state).items()
        }
        self.state.supplies.apply_delta(half)

    def _do_change_pace(self, pace_str: str) -> None:
        try:
            new_pace = Pace(pace_str.lower())
        except ValueError:
            self.msgs.lines.append(
                f"Unknown pace: {pace_str}. "
                "Use slow, steady, or hard."
            )
            return

        self.state.wagon.pace = new_pace
        self.msgs.lines.append(f"Pace set to {new_pace.value}.")

    # ── Escape valve handlers ────────────────────────────────────────

    def _do_abandon_cargo(self) -> None:
        self.state.last_action = "ABANDON_CARGO"
        if not can_abandon_cargo(self.state):
            self.msgs.lines.append(
                "Wagon is not damaged enough to justify "
                "abandoning cargo."
            )
            return

        self.diagnostics["escape_valves_used"] += 1
        self.state.escape_valve_cooldown = 3
        result = abandon_cargo(self.state)
        dropped = result.get("dropped", {})
        dropped_items = [
            f"-{abs(v)} {k}" for k, v in dropped.items() if v < 0
        ]
        self.msgs.lines.append(
            "Abandoned cargo to lighten the wagon. "
            f"Wagon +{result.get('wagon_repair', 25)}. "
            f"Dropped: {', '.join(dropped_items) or 'nothing'}. "
            "Morale fell."
        )
        emit_escape_valve_card(
            self.state, "abandon_cargo",
            "Abandoned cargo to save the wagon.",
        )

    def _do_desperate_repair(self) -> None:
        self.state.last_action = "DESPERATE_REPAIR"
        if not can_desperate_repair(self.state):
            self.msgs.lines.append(
                "Desperate repair requires a badly damaged wagon "
                "and no spare parts."
            )
            return

        self.diagnostics["escape_valves_used"] += 1
        self.state.escape_valve_cooldown = 3
        result = desperate_repair(self.state, self.rng)
        if result.get("success"):
            self.msgs.lines.append(
                f"Desperate repair succeeded! "
                f"Wagon +{result.get('wagon_delta', 15)}."
            )
        else:
            injured = result.get("injured", "someone")
            self.msgs.lines.append(
                f"Desperate repair failed! "
                f"Wagon {result.get('wagon_delta', -10)}. "
                f"{injured} was injured in the attempt."
            )
        detail = (
            "Repair succeeded." if result.get("success")
            else f"Repair failed. {result.get('injured', 'Someone')} injured."
        )
        emit_escape_valve_card(self.state, "desperate_repair", detail)

    def _do_hard_ration(self) -> None:
        self.state.last_action = "HARD_RATION"
        if not can_hard_ration(self.state):
            self.msgs.lines.append(
                "Cannot ration further right now."
            )
            return

        self.diagnostics["escape_valves_used"] += 1
        self.state.escape_valve_cooldown = 3
        hard_ration(self.state)
        self.msgs.lines.append(
            "Hard rationing imposed for 2 days. "
            "Food and water consumption halved. "
            "Morale -10, everyone weakened."
        )
        emit_escape_valve_card(
            self.state, "hard_ration",
            "Hard rationing imposed — half rations for 2 days.",
        )

    # ── Journal milestones (ENG-B-02) ───────────────────────────────

    def _record_milestone(
        self,
        event_id: str,
        scene_title: str,
        outcome: str,
        *,
        tags: list[str] | None = None,
    ) -> None:
        """Append a non-event milestone to the journal so show_journal renders
        it (ENG-B-02).

        Reuses the JournalEntry shape with a synthetic event_id (e.g.
        'death:starvation') so a long run's deaths, breakdowns, and town
        arrivals all leave a durable record alongside resolved events.
        """
        node = _find_node(self.state)
        self.state.journal.append(JournalEntry(
            day=self.state.day,
            location=node.name if node else "unknown",
            event_id=event_id,
            scene_title=scene_title,
            narration="",
            choice_made="",
            outcome=outcome[:300],
            deltas={},
            tags=tags or [],
        ))

    def _record_death_milestones(self, effects: list[dict]) -> None:
        """Write a journal milestone for each death in this batch of health
        effects (ENG-B-02), carrying member, day, cause, and location."""
        node = _find_node(self.state)
        location = node.name if node else "unknown"
        for eff in effects:
            if eff.get("type") != "died":
                continue
            member = eff.get("member", "Someone")
            cause = eff.get("cause", "the trail")
            self._record_milestone(
                f"death:{cause.lower()}",
                f"Death: {member}",
                f"{member} died of {cause.lower()} "
                f"on day {self.state.day} near {location}.",
                tags=["death", cause.lower()],
            )

    # ── GM observability ────────────────────────────────────────────

    def _note_gm_fallback(self, reason: str) -> None:
        """Record that a GM call returned nothing and the engine fell back to
        deterministic narration (ENG-B-05 / CONTRACT).

        Sets the per-step degraded signal the UI renders, bumps the
        gm_fallbacks diagnostic, and — the first time it happens in a session —
        appends a single, calm line so the player understands the shift in voice
        without it nagging on every event thereafter.
        """
        self.diagnostics["gm_fallbacks"] += 1
        self.msgs.gm_degraded = True
        self.msgs.gm_degraded_reason = reason
        if not self._gm_degraded_noted:
            self._gm_degraded_noted = True
            self.msgs.lines.append(
                "GM narration unavailable -- using the trail's own voice."
            )

    # ── EVENT phase ─────────────────────────────────────────────────

    def _maybe_trigger_event(self) -> None:
        """Roll for a random event. If triggered, switch to EVENT
        phase so UI can show choices."""
        if self.rng.random() > 0.6:
            return

        # ENG-A-06: derive weather BEFORE selection so weather-gated events are
        # actually filtered, and reuse the same value for GM narration.
        node = _find_node(self.state)
        weather = (
            generate_weather(self.rng, node.biome, self.state.day)
            if node
            else None
        )

        event = select_event(
            self.state, self.rng, self.event_library, weather,
        )
        self.diagnostics["events_total"] += 1
        if event.severity == "high":
            self.diagnostics["events_high_sev"] += 1

        # Try GM narration
        scene = None
        if self.gm.config.enabled:
            self.diagnostics["gm_calls"] += 1
            weather_str = weather.value if weather else "unknown"
            brief = build_gm_brief(self.state)
            scene = self.gm.generate_scene(
                self.state, event, weather_str, brief=brief,
            )

        # Build choices from GM or fallback
        if scene and scene.choices:
            title = scene.title or event.title
            narration = scene.narration or event.fallback_narration
            choices = [
                EventChoiceInfo(
                    id=c.get("id", "?"),
                    label=c.get("label", "?"),
                    risk_hint=c.get("risk_hint", ""),
                    cost_hint=c.get("cost_hint", ""),
                )
                for c in scene.choices
            ]
        else:
            # ENG-B-05: GM was enabled but produced nothing usable — fall back to
            # the deterministic skeleton and surface the degraded signal.
            if self.gm.config.enabled:
                self._note_gm_fallback("scene narration unavailable")
            title = event.title
            narration = event.fallback_narration
            choices = [
                EventChoiceInfo(
                    id=c.choice_id,
                    label=c.label,
                    risk_hint=c.risk_hint,
                    cost_hint=c.cost_hint,
                )
                for c in event.fallback_choices
            ]

        if not choices:
            return

        # Store pending event and switch phase
        self._pending_event = event
        self._pending_event_choices = choices
        self._pending_event_scene = scene
        self._pending_event_title = title
        self._pending_event_narration = narration
        self.phase = GamePhase.EVENT

        self.msgs.event_title = title
        self.msgs.event_narration = narration
        self.msgs.event_choices = choices

    def _handle_event_choice(self, intent: PlayerIntent) -> None:
        """Resolve the pending event with the player's choice."""
        if intent.action != IntentAction.CHOOSE:
            self.msgs.lines.append(
                "Choose an option (1-4) for the current event."
            )
            return

        event = self._pending_event
        if not event:
            self.phase = GamePhase.CAMP
            return

        choice_id = intent.choice_id

        # ENG-B-06: an invalid choice id must not silently fizzle into an
        # arbitrary outcome. Tell the player what's on offer and return WITHOUT
        # clearing the pending event, so they can retry rather than lose it.
        offered_ids = [c.id for c in self._pending_event_choices]
        if choice_id not in offered_ids:
            self.msgs.lines.append(
                "That option isn't available -- choose one of: "
                f"{'/'.join(offered_ids)}."
            )
            self.msgs.event_title = self._pending_event_title
            self.msgs.event_narration = self._pending_event_narration
            self.msgs.event_choices = self._pending_event_choices
            return

        outcome = resolve_event(
            self.state, event, choice_id, self.rng,
        )
        apply_outcome(self.state, outcome)

        # ENG-A-05 (resolved): charge the event's time_cost to the clock. ENG-A-05
        # deferred this ("time_cost remains parsed on EventOutcome for future use")
        # because apply_outcome is pure events.py with no engine clock hook — the
        # hook lives here. Each unit advances one time-of-day slot (a quarter-day),
        # applied AFTER the resource deltas, so WAIT/DETOUR/REST choices carry a
        # real opportunity cost: nocturnal firewood/lantern-oil drain, the day-tick
        # spoilage check, a weather reroll, and nocturnal-event eligibility. No
        # double-charge — resource costs ride on supplies_delta, time_cost rides on
        # the clock alone (this step charges no consumption). _advance_time draws no
        # RNG, so determinism (rng_counter, synced at the end of this method) holds.
        for _ in range(max(0, outcome.time_cost)):
            self._advance_time()

        # Find choice label
        choice_label = ""
        for c in self._pending_event_choices:
            if c.id == choice_id:
                choice_label = c.label
                break

        # Try GM outcome narration
        outcome_narration = ""
        outcome_title = ""

        scene = self._pending_event_scene
        brief = build_gm_brief(self.state) if self.gm.config.enabled else None
        gm_out = None
        if self.gm.config.enabled and scene:
            self.diagnostics["gm_calls"] += 1
            outcome_facts = {
                "Supplies delta": outcome.supplies_delta,
                "Health effects": outcome.health_delta,
                "Wagon effects": outcome.wagon_delta,
                "Morale effects": outcome.morale_delta,
                "Time cost": outcome.time_cost,
                "Special": outcome.special_flags,
            }
            gm_out = self.gm.generate_outcome(
                self.state,
                event,
                self.msgs.event_title or event.title,
                choice_id,
                choice_label,
                outcome_facts,
                brief=brief,
            )
            if gm_out:
                outcome_narration = gm_out.outcome_narration
                outcome_title = gm_out.outcome_title

        if not outcome_narration:
            # ENG-B-05: GM enabled (had a scene) but no usable outcome narration
            # came back — fall back deterministically and surface the signal.
            if self.gm.config.enabled and scene:
                self._note_gm_fallback("outcome narration unavailable")
            outcome_title = "Outcome"
            outcome_narration = _build_fallback_callout(outcome)

        self.msgs.outcome_title = outcome_title
        self.msgs.outcome_narration = outcome_narration
        self.msgs.outcome_deltas = outcome.supplies_delta

        # Journal
        node = _find_node(self.state)
        self.state.journal.append(JournalEntry(
            day=self.state.day,
            location=node.name if node else "unknown",
            event_id=event.event_id,
            scene_title=self.msgs.event_title or event.title,
            narration=(
                self.msgs.event_narration
                or event.fallback_narration
            )[:300],
            choice_made=f"{choice_id}: {choice_label}",
            outcome=outcome_narration[:300],
            deltas=outcome.supplies_delta,
            tags=event.tags,
        ))

        # Memory emitters — engine cards from event
        emit_event_card(self.state, event)

        # Validate GM-proposed memory cards
        if scene and hasattr(scene, "memory_proposals"):
            from .memory import add_card
            for card in validate_gm_cards(
                self.state, scene.memory_proposals,
            ):
                add_card(self.state, card)
        if gm_out and hasattr(gm_out, "memory_proposals"):
            from .memory import add_card
            for card in validate_gm_cards(
                self.state, gm_out.memory_proposals,
            ):
                add_card(self.state, card)

        # Check resource crises after outcome applied
        check_resource_crises(self.state)

        # Clear pending, back to camp
        self._pending_event = None
        self._pending_event_choices = []
        self._pending_event_scene = None
        self._pending_event_title = ""
        self._pending_event_narration = ""
        self.phase = GamePhase.CAMP

        self.state.rng_counter = self.rng.counter

    # ── ROUTE phase ─────────────────────────────────────────────────

    def _build_route_choices(self, node) -> None:
        """Build fork options from a node with multiple connections."""
        options = []
        for conn_id in node.connections:
            for n in self.state.map_nodes:
                if n.node_id == conn_id:
                    dist = node.distance_to.get(conn_id, 15)
                    options.append(RouteOption(conn_id, n.name, dist))
                    break
        self._pending_routes = options
        self.msgs.route_options = options

    def _handle_route_choice(self, intent: PlayerIntent) -> None:
        """Pick a fork."""
        if intent.action != IntentAction.CHOOSE:
            self.msgs.lines.append("Choose a path (1/2).")
            self.msgs.route_options = self._pending_routes
            return

        # Map choice_id to route option
        idx_map = {"A": 0, "B": 1, "C": 2, "D": 3}
        idx = idx_map.get(intent.choice_id, 0)

        if idx < len(self._pending_routes):
            route = self._pending_routes[idx]
            node = _find_node(self.state)
            self.state.destination_id = route.node_id
            self.state.distance_remaining = (
                node.distance_to.get(route.node_id, 15)
                if node
                else 15
            )
            self.msgs.lines.append(
                f"Heading toward {route.name} "
                f"({route.distance} miles)."
            )
        else:
            self.msgs.lines.append("Invalid choice.")
            self.msgs.route_options = self._pending_routes
            return

        self._pending_routes = []
        self.phase = GamePhase.CAMP

    # ── Helpers ──────────────────────────────────────────────────────

    def _arrive_at_next_node(self) -> None:
        dest = None
        for node in self.state.map_nodes:
            if node.node_id == self.state.destination_id:
                dest = node
                break

        if not dest:
            # ENG-B-09: the destination_id points at no real node (corrupt save,
            # data drift, or a fork that never resolved). Don't silently strand
            # the party at distance 0 with nowhere to go — warn, log, and recover
            # by snapping to the final node so the journey can still conclude.
            log.warning(
                "destination_id %r not found among map_nodes; recovering",
                self.state.destination_id,
            )
            self.msgs.lines.append(
                "The way ahead doesn't match the map. "
                "Pressing on toward the last known waypoint."
            )
            if self.state.map_nodes:
                dest = self.state.map_nodes[-1]
                self.state.destination_id = dest.node_id
            else:
                # No map at all — nothing to recover to; leave state untouched.
                return

        self.state.location_id = dest.node_id
        self.state.distance_remaining = 0
        self.msgs.lines.append(f"Arrived at {dest.name}!")

        # Water refill at nodes with water sources
        if dest.water_available:
            old_water = self.state.supplies.water
            refill = min(20, 50 - old_water)  # Up to 20, capped at 50
            if refill > 0:
                self.state.supplies.water += refill
                self.msgs.lines.append(
                    f"Found water. +{refill} water."
                )

        # Supply cache pickup (one-time)
        if dest.cache_supplies:
            cache = dest.cache_supplies
            self.state.supplies.apply_delta(cache)
            cache_items = ", ".join(
                f"+{v} {k}" for k, v in cache.items()
            )
            self.msgs.lines.append(
                f"Found a supply cache! {cache_items}"
            )
            self.diagnostics["caches_found"] += 1
            dest.cache_supplies = None  # consumed

        # Emit arrival memory card for towns
        emit_arrival_card(self.state, dest)

        if dest.is_town:
            # ENG-B-02: reaching a town is a milestone — leave a journal record.
            self._record_milestone(
                "town:arrival",
                f"Arrived at {dest.name}",
                f"The party reached {dest.name} on day {self.state.day}.",
                tags=["town", "arrival"],
            )
            # Town trade: morale-gated + doctrine-boosted
            from .models import DOCTRINE_MODIFIERS
            doc_mods = DOCTRINE_MODIFIERS.get(self.state.doctrine, {})
            trade_chance = 0.30 + doc_mods.get("trade_bonus", 0)
            if (
                self.state.party.morale > 60
                and self.rng.random() < trade_chance
            ):
                food_offer = self.rng.randint(3, 9)
                self.state.supplies.food += food_offer
                self.msgs.lines.append(
                    f"Traded at the settlement. +{food_offer} food."
                )
            else:
                self.msgs.lines.append(
                    "This is a settlement. Supplies may be available."
                )

            # Ledger Backpack: settle checkpoint at towns
            if self.state.backpack.enabled:
                self._settle_checkpoint(dest)
                self._check_parcels(dest)

        # Set up next leg
        if dest.connections:
            if len(dest.connections) == 1:
                next_id = dest.connections[0]
                self.state.destination_id = next_id
                self.state.distance_remaining = (
                    dest.distance_to.get(next_id, 15)
                )
            # Multi-connection → ROUTE phase on next travel

    def _settle_checkpoint(self, dest) -> None:
        """Settle Ledger Backpack at a town checkpoint.

        ledger-006: if settle() raises before it can enqueue its own pending
        record (e.g. the manager fails to construct, or the error escapes the
        inner try in settle()), the unsettled delta would be lost silently. We
        narrow the catch, log a warning, and guarantee a pending SettlementRecord
        is enqueued so the delta stays retryable via `ledger reconcile`.
        """
        try:
            from .backpack import BackpackManager

            mgr = BackpackManager()
            try:
                result = mgr.settle(self.state, dest.name)
                if result.success and result.txids:
                    self.msgs.lines.append(result.message)
                elif not result.success and result.message:
                    self.msgs.lines.append(result.message)
            finally:
                mgr.close()
        except Exception as e:  # graceful degradation — game continues
            log.warning("Checkpoint settlement at %s failed: %s", dest.name, e)
            self._enqueue_pending_settlement(dest.name)

    def _enqueue_pending_settlement(self, location: str) -> None:
        """Record the current unsettled delta as a pending SettlementRecord so a
        settle() that raised before enqueuing does not silently drop the delta.

        Idempotent-ish: skips if settle() already enqueued a pending record for
        this day (so we don't double-record when the error escaped after enqueue).
        """
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
        # Avoid duplicating a pending record settle() may already have queued.
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

            mgr = BackpackManager()
            try:
                parcels = mgr.check_parcels(self.state)
                for parcel in parcels:
                    sender_short = parcel.sender[:8] + "..."
                    contents = ", ".join(
                        f"{v} {k}" for k, v in parcel.contents.items()
                    )
                    self.msgs.lines.append(
                        f"A parcel arrived from {sender_short}: {contents}"
                    )
            finally:
                mgr.close()
        except Exception as e:  # graceful degradation
            log.warning("Parcel check at %s failed: %s", dest.name, e)

    def _advance_time(self) -> None:
        times = list(TimeOfDay)
        idx = times.index(self.state.time_of_day)

        if idx < len(times) - 1:
            self.state.time_of_day = times[idx + 1]
        else:
            self.state.time_of_day = TimeOfDay.MORNING
            self.state.day += 1

    def _check_game_over(self) -> None:
        result = check_game_over(self.state)
        if result:
            if result == "VICTORY":
                self.state.victory = True
                self.state.game_over = True
                self.msgs.lines.append("You made it! Victory!")
            else:
                cause = determine_cause_of_death(self.state)
                self.state.cause_of_death = cause
                self.state.game_over = True
                self.msgs.lines.append(
                    f"The journey ends. Cause: {cause}."
                )
            self.phase = GamePhase.GAME_OVER
            # EC-04: grade the ending exactly once, on the terminal transition.
            # Reads only existing state, draws no RNG → deterministic with GM off.
            ending = compute_ending(self.state)
            self.state.ending = ending
            self.msgs.ending = ending

    def _save(self) -> None:
        self.state.rng_counter = self.rng.counter
        self.state.rng_state = self.rng.getstate()
        save_game(self.state)


# ── Module-level helpers ────────────────────────────────────────────

def _build_fallback_callout(outcome: EventOutcome) -> str:
    parts = []
    for key, val in outcome.supplies_delta.items():
        if val > 0:
            parts.append(f"+{val} {key}")
        elif val < 0:
            parts.append(f"{val} {key}")

    if outcome.health_delta:
        sign = "+" if outcome.health_delta > 0 else ""
        parts.append(f"health {sign}{outcome.health_delta}")
    if outcome.wagon_delta:
        sign = "+" if outcome.wagon_delta > 0 else ""
        parts.append(f"wagon {sign}{outcome.wagon_delta}")
    if outcome.morale_delta:
        sign = "+" if outcome.morale_delta > 0 else ""
        parts.append(f"morale {sign}{outcome.morale_delta}")
    # ENG-A-05 (resolved): the engine now advances the clock by time_cost on event
    # resolution (see _handle_event_choice), so reporting "time lost" is truthful
    # — it names a cost the engine actually charges.
    if outcome.time_cost:
        parts.append(f"{outcome.time_cost} time lost")

    return ", ".join(parts) if parts else "No significant effect."


# ── EC-04: graded endings ───────────────────────────────────────────

# Par is a deterministic distance->days yardstick. At STEADY pace the wagon
# covers ~5 miles per travel-day, and a clean run mixes travel with rest/repair,
# so we budget a little slack: par_days ≈ total_distance / 4, floored at 8 so a
# very short map still has a meaningful target. Reading total_distance only, this
# is pure and seed-stable.
_PAR_MILES_PER_DAY = 4
_PAR_DAYS_FLOOR = 8


def compute_par_days(total_distance: int) -> int:
    """Deterministic 'par' day count for a journey of ``total_distance`` miles."""
    if total_distance <= 0:
        return _PAR_DAYS_FLOOR
    return max(_PAR_DAYS_FLOOR, -(-total_distance // _PAR_MILES_PER_DAY))


def _taboo_kept(state: RunState) -> bool:
    """Best-effort, deterministic read of whether the run's taboo held.

    EC-04 reads ONLY existing state. There is no dedicated taboo-violation
    counter, so we infer from observable signals:

    - LEAVE_NOTHING ("leave no one behind"): kept iff every member is still
      alive at game-over.
    - NEVER_RIVER: kept unless the journal shows a river event resolved with a
      ford choice (tags carry 'river'/'ford'; the chosen label mentions 'ford').
    - NEVER_NIGHT: kept unless the journal shows an event resolved at night.

    When a run has no taboo assigned, it is vacuously 'kept'. Defaulting to kept
    is the honest call: we only flip to broken on positive evidence in state.
    """
    taboo = state.taboo
    if not taboo:
        return True

    if taboo == "leave_nothing":
        return all(m.is_alive() for m in state.party.members)

    if taboo == "never_river":
        for entry in state.journal:
            tagset = {t.lower() for t in entry.tags}
            choice = entry.choice_made.lower()
            if ({"river", "ford"} & tagset) and "ford" in choice:
                return False
        return True

    if taboo == "never_night":
        for entry in state.journal:
            if "night" in {t.lower() for t in entry.tags}:
                return False
        return True

    return True


def _deaths_by_cause(state: RunState) -> dict[str, int]:
    """Count fallen members grouped by their attributed death_cause."""
    counts: dict[str, int] = {}
    for m in state.party.members:
        if not m.is_alive():
            cause = m.death_cause or "Unknown"
            counts[cause] = counts.get(cause, 0) + 1
    return counts


def compute_ending(state: RunState) -> EndingResult:
    """Grade the run's ending from existing state (EC-04).

    Pure and RNG-free: the tier and facts are a deterministic function of the
    party, the clock, distance, taboo, and uncanny tokens already on ``state``.
    Called once when the engine transitions to GAME_OVER (victory or death).
    """
    members = state.party.members
    party_size = len(members)
    survivors = sum(1 for m in members if m.is_alive())
    days = state.day
    par_days = compute_par_days(state.total_distance)
    taboo_kept = _taboo_kept(state)
    deaths_by_cause = _deaths_by_cause(state)

    facts: dict[str, object] = {
        "victory": state.victory,
        "survivors": survivors,
        "party_size": party_size,
        "days": days,
        "par_days": par_days,
        "taboo": state.taboo,
        "taboo_kept": taboo_kept,
        "uncanny_tokens_unspent": state.uncanny_tokens,
        "deaths_by_cause": deaths_by_cause,
        "distance": state.distance_traveled,
        "total_distance": state.total_distance,
        "cause_of_death": state.cause_of_death,
    }

    # ── Tier grading ──
    if not state.victory:
        # The valley was never reached.
        tier = "lost"
        if survivors >= party_size:
            headline = "The trail turned them back."
        elif survivors > 0:
            headline = (
                f"{survivors} of {party_size} were still standing when "
                "the journey failed."
            )
        else:
            headline = "None of them reached the valley."
    elif survivors < party_size or not taboo_kept:
        # Reached the valley, but the cost was real.
        tier = "pyrrhic"
        if survivors < party_size:
            lost = party_size - survivors
            headline = (
                f"The valley was reached — but {lost} did not live to see it."
            )
        else:
            headline = "The valley was reached, but a vow was broken to get there."
    elif days > par_days:
        # Whole party alive, taboo held, but slow.
        tier = "weathered"
        headline = (
            f"All {party_size} reached the valley, weathered and late "
            f"({days} days against {par_days})."
        )
    else:
        tier = "triumphant"
        headline = (
            f"All {party_size} reached the valley intact, on time, vow unbroken."
        )

    return EndingResult(tier=tier, facts=facts, headline=headline)


def _find_node(state: RunState):
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None
