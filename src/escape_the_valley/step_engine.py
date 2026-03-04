"""Step-based game engine — pure logic, no UI imports.

Usage:
    engine = StepEngine(state, gm_config)
    engine.step(PlayerIntent(IntentAction.TRAVEL))
    frame = state_to_frame(engine)   # adapter.py
"""

from __future__ import annotations

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
from .models import (
    JournalEntry,
    Pace,
    RunState,
    SeededRNG,
    TimeOfDay,
)
from .physics import (
    apply_breakdown,
    attempt_hunt,
    attempt_repair,
    check_breakdown,
    check_game_over,
    check_health_effects,
    compute_daily_consumption,
    compute_travel_distance,
    rest_day,
    update_morale,
)
from .save import save_game
from .worldgen import generate_weather

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
        self.event_library = build_event_library()
        self.gm = GMClient(gm_config or GMConfig())
        self.phase = GamePhase.CAMP
        self.msgs = StepMessages()

        # Pending state for multi-step flows
        self._pending_event: EventSkeleton | None = None
        self._pending_event_choices: list[EventChoiceInfo] = []
        self._pending_event_scene: object | None = None
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

    def _do_travel(self) -> None:
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
        consumption = compute_daily_consumption(self.state)
        self.state.supplies.apply_delta(consumption)

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

        # Breakdown
        breakdown = check_breakdown(self.state, self.rng)
        if breakdown:
            damage = breakdown["wagon_damage"]
            deltas = apply_breakdown(self.state, damage)
            if deltas:
                self.msgs.lines.append(
                    f"Wagon breakdown! Repaired. Damage: {damage}"
                )
            else:
                self.msgs.lines.append(
                    f"Wagon breakdown! No parts. Damage: {damage}"
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

        # Random event (~60% chance)
        self._maybe_trigger_event()

        # Morale
        update_morale(self.state)

    def _do_rest(self) -> None:
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

    # ── EVENT phase ─────────────────────────────────────────────────

    def _maybe_trigger_event(self) -> None:
        """Roll for a random event. If triggered, switch to EVENT
        phase so UI can show choices."""
        if self.rng.random() > 0.6:
            return

        event = select_event(
            self.state, self.rng, self.event_library,
        )
        node = _find_node(self.state)
        weather = (
            generate_weather(self.rng, node.biome, self.state.day)
            if node
            else None
        )

        # Try GM narration
        scene = None
        if self.gm.config.enabled:
            weather_str = weather.value if weather else "unknown"
            scene = self.gm.generate_scene(
                self.state, event, weather_str,
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
        outcome = resolve_event(
            self.state, event, choice_id, self.rng,
        )
        apply_outcome(self.state, outcome)

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
        if self.gm.config.enabled and scene:
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
            )
            if gm_out:
                outcome_narration = gm_out.outcome_narration
                outcome_title = gm_out.outcome_title

        if not outcome_narration:
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

        # Clear pending, back to camp
        self._pending_event = None
        self._pending_event_choices = []
        self._pending_event_scene = None
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
            return

        self.state.location_id = dest.node_id
        self.state.distance_remaining = 0
        self.msgs.lines.append(f"Arrived at {dest.name}!")

        if dest.is_town:
            self.msgs.lines.append(
                "This is a settlement. Supplies may be available."
            )

        # Set up next leg
        if dest.connections:
            if len(dest.connections) == 1:
                next_id = dest.connections[0]
                self.state.destination_id = next_id
                self.state.distance_remaining = (
                    dest.distance_to.get(next_id, 15)
                )
            # Multi-connection → ROUTE phase on next travel

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
                self.state.cause_of_death = result
                self.state.game_over = True
                self.msgs.lines.append(f"The journey ends: {result}")
            self.phase = GamePhase.GAME_OVER

    def _save(self) -> None:
        self.state.rng_counter = self.rng.counter
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
    if outcome.time_cost:
        parts.append(f"{outcome.time_cost} time lost")

    return ", ".join(parts) if parts else "No significant effect."


def _find_node(state: RunState):
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None
