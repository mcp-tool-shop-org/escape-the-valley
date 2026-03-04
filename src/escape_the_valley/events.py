"""Event system — skeleton library + weighted selection."""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum

from .models import (
    Biome,
    GMProfile,
    RunState,
    SeededRNG,
    TimeOfDay,
    TwistModifier,
    Weather,
)


class EventCategory(StrEnum):
    SURVIVAL = "survival"
    HUMAN = "human"
    FOLKLORE = "folklore"
    BIG = "big"


class FolkloreType(StrEnum):
    MISINTERPRETATION = "misinterpretation"
    SOCIAL = "social"
    NATURAL_ODDITY = "natural_oddity"
    UNCANNY = "uncanny"


@dataclass
class EventOutcome:
    """Template for what the engine computes."""
    supplies_delta: dict[str, int] = field(default_factory=dict)
    health_delta: int = 0
    wagon_delta: int = 0
    morale_delta: int = 0
    time_cost: int = 0  # extra hours/half-days
    distance_delta: int = 0
    special_flags: list[str] = field(default_factory=list)


@dataclass
class ChoiceTemplate:
    """Pre-defined choice for deterministic fallback."""
    choice_id: str
    label: str
    action: str
    style: str  # CAUTIOUS / NEUTRAL / BOLD
    risk_hint: str
    cost_hint: str
    outcome_weights: dict[str, float] = field(default_factory=dict)


@dataclass
class EventSkeleton:
    event_id: str
    title: str
    category: EventCategory
    tags: list[str] = field(default_factory=list)
    severity: str = "medium"  # low / medium / high
    biome_filter: list[Biome] | None = None
    weather_filter: list[Weather] | None = None
    time_filter: list[TimeOfDay] | None = None
    preconditions: list[str] = field(default_factory=list)
    base_weight: float = 1.0
    folklore_type: FolkloreType | None = None
    costs_uncanny_token: bool = False
    oregon_nod: bool = False
    fallback_narration: str = ""
    fallback_choices: list[ChoiceTemplate] = field(default_factory=list)
    outcome_templates: dict[str, EventOutcome] = field(default_factory=dict)
    gm_aside: str = ""


# ─── Event skeleton library (~30 events) ───────────────────────────────────

def build_event_library() -> list[EventSkeleton]:
    """Build the full event skeleton library."""
    events: list[EventSkeleton] = []

    # ── SURVIVAL (70%) ──────────────────────────────────────────────────

    events.append(EventSkeleton(
        event_id="storm_sudden",
        title="Sudden Storm",
        category=EventCategory.SURVIVAL,
        tags=["weather", "storm", "delay"],
        severity="medium",
        base_weight=2.0,
        fallback_narration="Dark clouds mass without warning. The wind picks up sharply.",
        fallback_choices=[
            ChoiceTemplate("A", "Make camp and wait it out.", "WAIT", "CAUTIOUS",
                           "Safe but slow.", "Time and supplies."),
            ChoiceTemplate("B", "Push through the storm.", "TRAVEL", "BOLD",
                           "Wagon damage and injury risk.", "Speed at a cost."),
            ChoiceTemplate("C", "Find shelter nearby.", "INVESTIGATE", "NEUTRAL",
                           "Depends on terrain.", "Time to search."),
        ],
        outcome_templates={
            "A": EventOutcome(
                supplies_delta={"food": -2, "water": -2},
                time_cost=1, morale_delta=-3,
            ),
            "B": EventOutcome(wagon_delta=-15, health_delta=-10, morale_delta=-5),
            "C": EventOutcome(time_cost=1, morale_delta=-1),
        },
    ))

    events.append(EventSkeleton(
        event_id="river_crossing",
        title="River Crossing",
        category=EventCategory.SURVIVAL,
        tags=["river", "navigation", "nod_ok"],
        severity="medium",
        base_weight=1.5,
        fallback_narration="A river blocks the trail. The current looks strong.",
        fallback_choices=[
            ChoiceTemplate("A", "Ford the river.", "FORD", "BOLD",
                           "Wagon damage, loss of supplies.", "Direct but dangerous."),
            ChoiceTemplate("B", "Search for a shallow crossing.", "INVESTIGATE", "NEUTRAL",
                           "Takes time, may find nothing.", "Time."),
            ChoiceTemplate("C", "Wait for the water to drop.", "WAIT", "CAUTIOUS",
                           "Safe but slow.", "A day lost."),
        ],
        outcome_templates={
            "A": EventOutcome(wagon_delta=-10, supplies_delta={"food": -3}, morale_delta=-4),
            "B": EventOutcome(time_cost=1),
            "C": EventOutcome(supplies_delta={"food": -2, "water": -2}, time_cost=1),
        },
        gm_aside="Classic Oregon Trail moment. Nod allowed.",
    ))

    events.append(EventSkeleton(
        event_id="spoiled_food",
        title="Spoiled Rations",
        category=EventCategory.SURVIVAL,
        tags=["supplies", "food", "sickness"],
        severity="medium",
        base_weight=1.5,
        fallback_narration="Something smells wrong in the provisions. Mold has taken hold.",
        fallback_choices=[
            ChoiceTemplate("A", "Throw out all suspect food.", "WAIT", "CAUTIOUS",
                           "Safe but costly.", "Food loss."),
            ChoiceTemplate("B", "Sort carefully, keep what looks safe.", "INVESTIGATE", "NEUTRAL",
                           "Some sickness risk.", "Less food lost."),
            ChoiceTemplate("C", "Eat it anyway. Waste nothing.", "RISK", "BOLD",
                           "Sickness likely.", "No food lost."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": -10}, morale_delta=-3),
            "B": EventOutcome(supplies_delta={"food": -5}, health_delta=-5),
            "C": EventOutcome(health_delta=-15, morale_delta=-5),
        },
    ))

    events.append(EventSkeleton(
        event_id="broken_axle",
        title="Broken Axle",
        category=EventCategory.SURVIVAL,
        tags=["breakdown", "wagon", "repair", "nod_ok"],
        severity="high",
        base_weight=1.0,
        fallback_narration=(
            "A crack like a gunshot. The wagon lurches sideways. "
            "The rear axle has split."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Repair with spare parts.", "REPAIR", "NEUTRAL",
                           "Uses parts.", "1 part, time."),
            ChoiceTemplate("B", "Improvise a fix from wood.", "REPAIR", "BOLD",
                           "Risky repair, may not hold.", "Time, no parts."),
            ChoiceTemplate("C", "Lighten the load and limp forward.", "TRAVEL", "CAUTIOUS",
                           "Lose supplies to reduce strain.", "Supplies lost."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"parts": -1}, wagon_delta=10, time_cost=1),
            "B": EventOutcome(wagon_delta=-5, time_cost=1),
            "C": EventOutcome(supplies_delta={"food": -5, "water": -3}, wagon_delta=-5),
        },
        gm_aside="The axle screaming quote lives here.",
    ))

    events.append(EventSkeleton(
        event_id="lost_trail",
        title="Lost Trail",
        category=EventCategory.SURVIVAL,
        tags=["navigation", "delay", "fog"],
        severity="low",
        base_weight=1.5,
        weather_filter=[Weather.FOG, Weather.OVERCAST],
        fallback_narration="The trail fades into unmarked ground. The way forward is unclear.",
        fallback_choices=[
            ChoiceTemplate("A", "Backtrack to the last marker.", "TRAVEL", "CAUTIOUS",
                           "Safe, costs distance.", "Distance lost."),
            ChoiceTemplate("B", "Scout ahead on foot.", "INVESTIGATE", "NEUTRAL",
                           "Time cost, may find shortcut.", "Time."),
            ChoiceTemplate("C", "Press forward and hope.", "TRAVEL", "BOLD",
                           "May go wrong.", "Risk getting more lost."),
        ],
        outcome_templates={
            "A": EventOutcome(distance_delta=5, time_cost=1),
            "B": EventOutcome(time_cost=1, morale_delta=-2),
            "C": EventOutcome(distance_delta=3, morale_delta=-4),
        },
    ))

    events.append(EventSkeleton(
        event_id="dry_spring",
        title="Dry Spring",
        category=EventCategory.SURVIVAL,
        tags=["water", "desert", "supplies"],
        severity="medium",
        biome_filter=[Biome.DESERT, Biome.PLAINS],
        base_weight=1.5,
        fallback_narration=(
            "The spring marked on your map is bone dry. "
            "Cracked mud and nothing else."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Ration water strictly.", "WAIT", "CAUTIOUS",
                           "Morale drops.", "Morale."),
            ChoiceTemplate("B", "Search the area for groundwater.", "INVESTIGATE", "NEUTRAL",
                           "May find water or waste time.", "Time."),
            ChoiceTemplate("C", "Push on to the next source.", "TRAVEL", "BOLD",
                           "Uses more water faster.", "Water."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=-5),
            "B": EventOutcome(time_cost=1, supplies_delta={"water": 5}),
            "C": EventOutcome(supplies_delta={"water": -5}),
        },
    ))

    events.append(EventSkeleton(
        event_id="animal_lame",
        title="Lame Animal",
        category=EventCategory.SURVIVAL,
        tags=["animals", "pace", "wagon"],
        severity="medium",
        base_weight=1.0,
        fallback_narration="One of the draft animals is favoring its left foreleg badly.",
        fallback_choices=[
            ChoiceTemplate("A", "Rest the animal for a day.", "REST", "CAUTIOUS",
                           "Time cost, animal recovers.", "A day."),
            ChoiceTemplate("B", "Slow the pace and keep moving.", "TRAVEL", "NEUTRAL",
                           "Slower progress.", "Speed."),
            ChoiceTemplate("C", "Push on at normal pace.", "TRAVEL", "BOLD",
                           "Animal may worsen.", "Animal health."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1, supplies_delta={"food": -2, "water": -2}),
            "B": EventOutcome(distance_delta=3),
            "C": EventOutcome(morale_delta=-3),
        },
    ))

    events.append(EventSkeleton(
        event_id="sickness_camp",
        title="Fever in Camp",
        category=EventCategory.SURVIVAL,
        tags=["sickness", "health", "meds"],
        severity="high",
        base_weight=1.0,
        fallback_narration="Someone is burning with fever. The rest of the camp watches uneasily.",
        fallback_choices=[
            ChoiceTemplate("A", "Use medicine to treat them.", "WAIT", "CAUTIOUS",
                           "Uses meds.", "1 med."),
            ChoiceTemplate("B", "Let them rest without medicine.", "REST", "NEUTRAL",
                           "May recover slowly or worsen.", "Time and risk."),
            ChoiceTemplate("C", "Keep moving, tend them on the road.", "TRAVEL", "BOLD",
                           "Condition may worsen.", "Health risk."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"meds": -1}, morale_delta=2),
            "B": EventOutcome(health_delta=-8, time_cost=1),
            "C": EventOutcome(health_delta=-15, morale_delta=-5),
        },
    ))

    events.append(EventSkeleton(
        event_id="rockslide",
        title="Rockslide",
        category=EventCategory.SURVIVAL,
        tags=["terrain", "danger", "alpine"],
        severity="high",
        biome_filter=[Biome.ALPINE],
        base_weight=1.5,
        fallback_narration="Stones tumble from the slope above. The trail is half-buried.",
        fallback_choices=[
            ChoiceTemplate("A", "Clear the path by hand.", "WAIT", "NEUTRAL",
                           "Time-consuming.", "Half a day."),
            ChoiceTemplate("B", "Find a way around.", "DETOUR", "CAUTIOUS",
                           "Adds distance.", "Extra miles."),
            ChoiceTemplate("C", "Rush through before more falls.", "TRAVEL", "BOLD",
                           "Injury and wagon risk.", "Danger."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1, morale_delta=-2),
            "B": EventOutcome(distance_delta=5),
            "C": EventOutcome(health_delta=-12, wagon_delta=-10, morale_delta=-4),
        },
    ))

    events.append(EventSkeleton(
        event_id="good_water",
        title="Clear Spring",
        category=EventCategory.SURVIVAL,
        tags=["water", "good_fortune"],
        severity="low",
        base_weight=1.0,
        fallback_narration="A clean spring bubbles from the rocks. The water tastes sweet.",
        fallback_choices=[
            ChoiceTemplate("A", "Fill all containers.", "WAIT", "NEUTRAL",
                           "No downside.", "Time to fill."),
            ChoiceTemplate("B", "Quick drink and move on.", "TRAVEL", "BOLD",
                           "Less water gained.", "Speed."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"water": 10}, morale_delta=3),
            "B": EventOutcome(supplies_delta={"water": 4}),
        },
    ))

    # ── HUMAN ENCOUNTERS (15%) ──────────────────────────────────────────

    events.append(EventSkeleton(
        event_id="toll_bridge",
        title="Toll Bridge",
        category=EventCategory.HUMAN,
        tags=["human", "trade", "toll"],
        severity="low",
        base_weight=1.5,
        fallback_narration="A man sits at the near end of a narrow bridge. He does not move aside.",
        fallback_choices=[
            ChoiceTemplate("A", "Pay the toll (5 food).", "PAY", "CAUTIOUS",
                           "Safe passage.", "Food."),
            ChoiceTemplate("B", "Refuse and find another way.", "DETOUR", "NEUTRAL",
                           "Adds distance.", "Time and miles."),
            ChoiceTemplate("C", "Negotiate or intimidate.", "RISK", "BOLD",
                           "May work, may escalate.", "Unpredictable."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": -5}),
            "B": EventOutcome(distance_delta=8, time_cost=1),
            "C": EventOutcome(morale_delta=-3),
        },
    ))

    events.append(EventSkeleton(
        event_id="traveling_healer",
        title="Traveling Healer",
        category=EventCategory.HUMAN,
        tags=["human", "healer", "trade"],
        severity="low",
        base_weight=1.0,
        fallback_narration="A woman with a heavy pack waves from the roadside. She carries herbs.",
        fallback_choices=[
            ChoiceTemplate("A", "Trade food for medicine.", "TRADE", "NEUTRAL",
                           "Fair trade.", "Food for meds."),
            ChoiceTemplate("B", "Ask for advice, trade nothing.", "WAIT", "CAUTIOUS",
                           "Free information.", "Nothing."),
            ChoiceTemplate("C", "Pass without stopping.", "TRAVEL", "BOLD",
                           "No interaction.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": -5, "meds": 2}),
            "B": EventOutcome(morale_delta=2),
            "C": EventOutcome(),
        },
    ))

    events.append(EventSkeleton(
        event_id="hostile_locals",
        title="Hostile Locals",
        category=EventCategory.HUMAN,
        tags=["human", "danger", "town"],
        severity="medium",
        base_weight=0.8,
        fallback_narration=(
            "The settlement watches you approach with visible suspicion. "
            "No one offers a greeting."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Keep moving through quietly.", "TRAVEL", "CAUTIOUS",
                           "Avoid trouble.", "No rest or trade."),
            ChoiceTemplate("B", "Attempt friendly contact.", "TRADE", "NEUTRAL",
                           "May warm up or escalate.", "Uncertain."),
            ChoiceTemplate("C", "Detour around the settlement.", "DETOUR", "CAUTIOUS",
                           "Safe but slow.", "Distance."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=-3),
            "B": EventOutcome(morale_delta=-5, health_delta=-5),
            "C": EventOutcome(distance_delta=5, time_cost=1),
        },
    ))

    events.append(EventSkeleton(
        event_id="friendly_travelers",
        title="Fellow Travelers",
        category=EventCategory.HUMAN,
        tags=["human", "trade", "morale"],
        severity="low",
        base_weight=1.2,
        fallback_narration=(
            "Another party camps nearby. They share a fire "
            "and news of the road ahead."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Share a meal and trade news.", "TRADE", "NEUTRAL",
                           "Morale boost, food cost.", "Food."),
            ChoiceTemplate("B", "Trade supplies.", "TRADE", "NEUTRAL",
                           "Exchange what you have.", "Depends on trade."),
            ChoiceTemplate("C", "Keep to yourselves.", "WAIT", "CAUTIOUS",
                           "No interaction.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": -3}, morale_delta=8),
            "B": EventOutcome(supplies_delta={"food": -3, "meds": 1}),
            "C": EventOutcome(),
        },
    ))

    events.append(EventSkeleton(
        event_id="deserter",
        title="The Deserter",
        category=EventCategory.HUMAN,
        tags=["human", "danger", "moral_choice"],
        severity="medium",
        base_weight=0.7,
        fallback_narration="A man stumbles from the brush, wild-eyed and begging for water.",
        fallback_choices=[
            ChoiceTemplate("A", "Give him water and food.", "TRADE", "CAUTIOUS",
                           "Kind but costly.", "Supplies."),
            ChoiceTemplate("B", "Give him directions only.", "WAIT", "NEUTRAL",
                           "No cost.", "Morale depends on party."),
            ChoiceTemplate("C", "Send him away.", "RISK", "BOLD",
                           "Quick but cold.", "Morale hit."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": -3, "water": -3}, morale_delta=5),
            "B": EventOutcome(morale_delta=1),
            "C": EventOutcome(morale_delta=-5),
        },
    ))

    # ── FOLKLORE (10%) ──────────────────────────────────────────────────

    events.append(EventSkeleton(
        event_id="fog_bell",
        title="A Bell Where None Should Be",
        category=EventCategory.FOLKLORE,
        tags=["night", "fog", "folklore:uncanny", "navigation", "nod_ok"],
        severity="medium",
        folklore_type=FolkloreType.UNCANNY,
        costs_uncanny_token=True,
        base_weight=0.8,
        time_filter=[TimeOfDay.NIGHT, TimeOfDay.EVENING],
        weather_filter=[Weather.FOG, Weather.OVERCAST],
        fallback_narration="A bell rings once in the fog. Clean and bright. Then silence.",
        fallback_choices=[
            ChoiceTemplate("A", "Keep the fire high and wait for morning.", "WAIT", "CAUTIOUS",
                           "Safe but slow.", "Time and supplies."),
            ChoiceTemplate(
                "B", "Send two to investigate, rope-tethered.",
                "INVESTIGATE", "NEUTRAL",
                "Might prevent a bigger problem.", "Time or injury.",
            ),
            ChoiceTemplate("C", "Break camp and move now, slow and quiet.", "TRAVEL", "BOLD",
                           "Traveling blind in fog.", "Wagon wear, delay."),
        ],
        outcome_templates={
            "A": EventOutcome(
                supplies_delta={"food": -2, "water": -2},
                time_cost=1, morale_delta=-3,
                special_flags=["uncanny_token_spent"],
            ),
            "B": EventOutcome(time_cost=1, morale_delta=-5, special_flags=["uncanny_token_spent"]),
            "C": EventOutcome(wagon_delta=-8, distance_delta=2, morale_delta=-4,
                              special_flags=["uncanny_token_spent"]),
        },
        gm_aside=(
            "Can be a distant town bell carried by fog, "
            "a traveler's lure, or fatigue. Keep ambiguous."
        ),
    ))

    events.append(EventSkeleton(
        event_id="saints_footprints",
        title="Saint's Footprints",
        category=EventCategory.FOLKLORE,
        tags=["morning", "mud", "folklore:omen", "morale", "nod_ok"],
        severity="low",
        folklore_type=FolkloreType.MISINTERPRETATION,
        base_weight=0.8,
        time_filter=[TimeOfDay.MORNING],
        fallback_narration=(
            "Footprints in the mud lead away from camp. "
            "They end abruptly at dry ground."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Follow the tracks.", "INVESTIGATE", "NEUTRAL",
                           "Curiosity may cost time.", "Time."),
            ChoiceTemplate("B", "Ignore them and move on.", "TRAVEL", "CAUTIOUS",
                           "No cost.", "Nothing."),
            ChoiceTemplate("C", "Ask if anyone left camp in the night.", "WAIT", "CAUTIOUS",
                           "Might ease minds or worry them.", "Morale."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1, morale_delta=2),
            "B": EventOutcome(),
            "C": EventOutcome(morale_delta=-2),
        },
        gm_aside="Animal tracks misread. Or someone sleepwalking. The player decides.",
    ))

    events.append(EventSkeleton(
        event_id="quiet_mile",
        title="The Quiet Mile",
        category=EventCategory.FOLKLORE,
        tags=["forest", "evening", "folklore:uncanny", "morale", "uncanny_token"],
        severity="medium",
        folklore_type=FolkloreType.UNCANNY,
        costs_uncanny_token=True,
        biome_filter=[Biome.FOREST, Biome.SWAMP],
        time_filter=[TimeOfDay.EVENING, TimeOfDay.NIGHT],
        base_weight=0.6,
        fallback_narration=(
            "For a stretch of trail, all sound stops. No birds. "
            "No wind. Even your footsteps seem muffled."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Stop and listen carefully.", "WAIT", "CAUTIOUS",
                           "May learn something.", "Time."),
            ChoiceTemplate("B", "Walk faster. Get through it.", "TRAVEL", "BOLD",
                           "Morale risk.", "Anxiety."),
            ChoiceTemplate("C", "Talk loudly. Break the silence.", "RISK", "NEUTRAL",
                           "May settle nerves or attract attention.", "Unpredictable."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1, morale_delta=-5, special_flags=["uncanny_token_spent"]),
            "B": EventOutcome(morale_delta=-8, special_flags=["uncanny_token_spent"]),
            "C": EventOutcome(morale_delta=-3, special_flags=["uncanny_token_spent"]),
        },
        gm_aside="Sound dampening, predator behavior, or psychological stress. Still plausible.",
    ))

    events.append(EventSkeleton(
        event_id="wrong_stars",
        title="The Wrong Stars",
        category=EventCategory.FOLKLORE,
        tags=["night", "folklore:omen", "navigation", "morale"],
        severity="low",
        folklore_type=FolkloreType.MISINTERPRETATION,
        time_filter=[TimeOfDay.NIGHT],
        base_weight=0.7,
        fallback_narration=(
            "Someone claims the stars look wrong tonight. "
            "The constellations seem shifted."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Check your compass and maps.", "INVESTIGATE", "CAUTIOUS",
                           "Reassurance.", "Time."),
            ChoiceTemplate("B", "Dismiss it as nerves.", "WAIT", "NEUTRAL",
                           "Might be right.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=3),
            "B": EventOutcome(morale_delta=-3),
        },
        gm_aside="Atmospheric refraction, unfamiliar latitude, or tired eyes.",
    ))

    events.append(EventSkeleton(
        event_id="superstitious_town",
        title="The Wary Settlement",
        category=EventCategory.FOLKLORE,
        tags=["town", "folklore:social", "trade", "morale"],
        severity="low",
        folklore_type=FolkloreType.SOCIAL,
        base_weight=0.8,
        fallback_narration=(
            "The town will trade, but they insist you leave "
            "before nightfall. They won't say why."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Trade quickly and leave.", "TRADE", "CAUTIOUS",
                           "Limited trade window.", "Time pressure."),
            ChoiceTemplate("B", "Ask what they're afraid of.", "INVESTIGATE", "NEUTRAL",
                           "Might learn something or nothing.", "Time."),
            ChoiceTemplate("C", "Ignore the warning and stay.", "RISK", "BOLD",
                           "May be fine or may not.", "Unknown."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": 5, "water": 5}, morale_delta=-2),
            "B": EventOutcome(morale_delta=-4, time_cost=1),
            "C": EventOutcome(morale_delta=-6),
        },
        gm_aside="Local superstition about travelers, or genuine danger they won't name.",
    ))

    events.append(EventSkeleton(
        event_id="ball_lightning",
        title="Strange Lights",
        category=EventCategory.FOLKLORE,
        tags=["weather", "folklore:natural_oddity", "morale"],
        severity="low",
        folklore_type=FolkloreType.NATURAL_ODDITY,
        weather_filter=[Weather.STORM, Weather.OVERCAST],
        base_weight=0.6,
        fallback_narration=(
            "Pale lights drift across the open ground. "
            "They hover, split, and recombine."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Watch from a safe distance.", "WAIT", "CAUTIOUS",
                           "Fascinating but time-consuming.", "Time."),
            ChoiceTemplate("B", "Move away immediately.", "TRAVEL", "BOLD",
                           "Fast but unsettling.", "Morale."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1, morale_delta=2),
            "B": EventOutcome(morale_delta=-4),
        },
        gm_aside="Ball lightning or swamp gas. Scientifically documented but still unnerving.",
    ))

    events.append(EventSkeleton(
        event_id="mirror_pond",
        title="The Mirror Pond",
        category=EventCategory.FOLKLORE,
        tags=["water", "folklore:uncanny", "morale"],
        severity="medium",
        folklore_type=FolkloreType.UNCANNY,
        costs_uncanny_token=True,
        biome_filter=[Biome.FOREST, Biome.SWAMP],
        base_weight=0.5,
        fallback_narration=(
            "A still pond reflects the trees. Someone says "
            "the reflection shows the wrong sky."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Look closer.", "INVESTIGATE", "NEUTRAL",
                           "Curiosity.", "Morale."),
            ChoiceTemplate("B", "Fill water containers and leave.", "WAIT", "CAUTIOUS",
                           "Practical.", "Water gained."),
            ChoiceTemplate("C", "Don't touch the water.", "TRAVEL", "BOLD",
                           "Miss water opportunity.", "No water."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=-6, special_flags=["uncanny_token_spent"]),
            "B": EventOutcome(supplies_delta={"water": 8}, morale_delta=-2,
                              special_flags=["uncanny_token_spent"]),
            "C": EventOutcome(morale_delta=-3, special_flags=["uncanny_token_spent"]),
        },
        gm_aside="Optical illusion from tannin-stained water, or just nerves. Never confirm.",
    ))

    # ── BIG EVENTS (5%) ─────────────────────────────────────────────────

    events.append(EventSkeleton(
        event_id="bandits",
        title="Bandits on the Road",
        category=EventCategory.BIG,
        tags=["danger", "bandits", "combat"],
        severity="high",
        base_weight=0.5,
        fallback_narration="Armed figures step onto the trail ahead. They are not smiling.",
        fallback_choices=[
            ChoiceTemplate("A", "Surrender supplies to pass.", "PAY", "CAUTIOUS",
                           "Costly but safe.", "Supplies."),
            ChoiceTemplate("B", "Stand and fight.", "GUARD", "BOLD",
                           "Risk injury, keep supplies.", "Health, ammo."),
            ChoiceTemplate("C", "Turn and run.", "TRAVEL", "NEUTRAL",
                           "May lose distance or wagon damage.", "Distance, wagon."),
        ],
        outcome_templates={
            "A": EventOutcome(
                supplies_delta={"food": -10, "water": -5, "ammo": -5},
                morale_delta=-8,
            ),
            "B": EventOutcome(supplies_delta={"ammo": -3}, health_delta=-15, morale_delta=5),
            "C": EventOutcome(wagon_delta=-10, distance_delta=8, morale_delta=-5),
        },
    ))

    events.append(EventSkeleton(
        event_id="major_storm",
        title="The Great Storm",
        category=EventCategory.BIG,
        tags=["weather", "storm", "danger"],
        severity="high",
        base_weight=0.4,
        weather_filter=[Weather.STORM],
        fallback_narration="The sky turns the color of a bruise. This is no ordinary rain.",
        fallback_choices=[
            ChoiceTemplate("A", "Hunker down and ride it out.", "WAIT", "CAUTIOUS",
                           "Safest option.", "Time and supplies."),
            ChoiceTemplate("B", "Seek higher ground.", "TRAVEL", "NEUTRAL",
                           "Avoid flooding, risk exposure.", "Distance."),
            ChoiceTemplate("C", "Push through it.", "TRAVEL", "BOLD",
                           "Dangerous.", "Everything."),
        ],
        outcome_templates={
            "A": EventOutcome(
                supplies_delta={"food": -4, "water": -3},
                time_cost=2, morale_delta=-5,
            ),
            "B": EventOutcome(time_cost=1, morale_delta=-3, health_delta=-5),
            "C": EventOutcome(wagon_delta=-20, health_delta=-15, morale_delta=-10),
        },
    ))

    events.append(EventSkeleton(
        event_id="dysentery",
        title="Illness Sweeps the Camp",
        category=EventCategory.BIG,
        tags=["sickness", "health", "meds", "nod_ok"],
        severity="high",
        base_weight=0.4,
        fallback_narration="Half the party is down. The symptoms are unmistakable.",
        fallback_choices=[
            ChoiceTemplate("A", "Use all available medicine.", "WAIT", "CAUTIOUS",
                           "Best chance of recovery.", "Meds."),
            ChoiceTemplate("B", "Rest and hope it passes.", "REST", "NEUTRAL",
                           "Slow recovery, risk of spread.", "Time."),
            ChoiceTemplate("C", "Keep moving. They'll recover on the road.", "TRAVEL", "BOLD",
                           "Dangerous to the sick.", "Health."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"meds": -2}, time_cost=1, morale_delta=-3),
            "B": EventOutcome(health_delta=-12, time_cost=2, morale_delta=-6),
            "C": EventOutcome(health_delta=-20, morale_delta=-10),
        },
        gm_aside="The Oregon Trail classic. Nod allowed: 'Diagnosis: Dysentery.'",
        oregon_nod=True,
    ))

    # A few more survival events for variety
    events.append(EventSkeleton(
        event_id="wild_game",
        title="Game Spotted",
        category=EventCategory.SURVIVAL,
        tags=["hunt", "food", "opportunity"],
        severity="low",
        base_weight=1.0,
        biome_filter=[Biome.FOREST, Biome.PLAINS],
        fallback_narration="Fresh tracks. Something large passed through recently.",
        fallback_choices=[
            ChoiceTemplate("A", "Hunt it.", "HUNT", "NEUTRAL",
                           "Ammo cost, food reward.", "Ammo."),
            ChoiceTemplate("B", "Keep moving.", "TRAVEL", "CAUTIOUS",
                           "No risk.", "Missed opportunity."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"ammo": -1, "food": 10}, morale_delta=3),
            "B": EventOutcome(),
        },
    ))

    events.append(EventSkeleton(
        event_id="abandoned_camp",
        title="Abandoned Camp",
        category=EventCategory.SURVIVAL,
        tags=["exploration", "supplies", "mystery"],
        severity="low",
        base_weight=0.8,
        fallback_narration="A camp left in a hurry. Supplies scattered. No sign of the owners.",
        fallback_choices=[
            ChoiceTemplate("A", "Scavenge what you can.", "INVESTIGATE", "NEUTRAL",
                           "Free supplies, maybe.", "Time."),
            ChoiceTemplate("B", "Leave it alone.", "TRAVEL", "CAUTIOUS",
                           "Don't invite trouble.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": 5, "parts": 1}, time_cost=1),
            "B": EventOutcome(),
        },
    ))

    events.append(EventSkeleton(
        event_id="snake_bite",
        title="Snake Bite",
        category=EventCategory.SURVIVAL,
        tags=["danger", "health", "meds"],
        severity="medium",
        biome_filter=[Biome.SWAMP, Biome.DESERT, Biome.PLAINS],
        base_weight=0.8,
        fallback_narration=(
            "A sharp cry. Someone clutches their ankle. "
            "A snake disappears into the grass."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Treat with medicine immediately.", "WAIT", "CAUTIOUS",
                           "Best outcome.", "Meds."),
            ChoiceTemplate("B", "Rest and keep the limb still.", "REST", "NEUTRAL",
                           "Slower recovery.", "Time."),
            ChoiceTemplate("C", "Keep moving and hope for the best.", "TRAVEL", "BOLD",
                           "Risky.", "Health."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"meds": -1}),
            "B": EventOutcome(health_delta=-10, time_cost=1),
            "C": EventOutcome(health_delta=-20, morale_delta=-5),
        },
    ))

    events.append(EventSkeleton(
        event_id="cold_night",
        title="Bitter Cold Night",
        category=EventCategory.SURVIVAL,
        tags=["weather", "cold", "health"],
        severity="medium",
        biome_filter=[Biome.ALPINE],
        time_filter=[TimeOfDay.NIGHT, TimeOfDay.EVENING],
        base_weight=1.2,
        fallback_narration="The temperature drops hard after sunset. Breath freezes in the air.",
        fallback_choices=[
            ChoiceTemplate("A", "Build a large fire and huddle close.", "WAIT", "CAUTIOUS",
                           "Uses supplies for warmth.", "Supplies."),
            ChoiceTemplate("B", "Share body heat and endure.", "REST", "NEUTRAL",
                           "Uncomfortable but free.", "Health."),
            ChoiceTemplate("C", "March through the night to stay warm.", "TRAVEL", "BOLD",
                           "Exhausting but generates heat.", "Health and morale."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": -3}, morale_delta=-2),
            "B": EventOutcome(health_delta=-8, morale_delta=-4),
            "C": EventOutcome(health_delta=-5, morale_delta=-6),
        },
    ))

    # One more folklore
    events.append(EventSkeleton(
        event_id="singing_stones",
        title="The Singing Stones",
        category=EventCategory.FOLKLORE,
        tags=["terrain", "folklore:natural_oddity", "morale"],
        severity="low",
        folklore_type=FolkloreType.NATURAL_ODDITY,
        biome_filter=[Biome.ALPINE, Biome.DESERT],
        base_weight=0.6,
        fallback_narration=(
            "The wind through a rock formation produces a low, "
            "musical hum. It rises and falls like breathing."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Investigate the formation.", "INVESTIGATE", "NEUTRAL",
                           "Curiosity.", "Time."),
            ChoiceTemplate("B", "Pass by without stopping.", "TRAVEL", "CAUTIOUS",
                           "No delay.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=4, time_cost=1),
            "B": EventOutcome(morale_delta=-1),
        },
        gm_aside="Aeolian harp effect. Real phenomenon. Still eerie.",
    ))

    return events


# ─── Event selection engine ─────────────────────────────────────────────

# Profile-specific weight multipliers
PROFILE_FOLKLORE_WEIGHT = {
    GMProfile.CHRONICLER: 0.5,
    GMProfile.FIRESIDE: 1.0,
    GMProfile.LANTERN: 1.6,
}

PROFILE_UNCANNY_WEIGHT = {
    GMProfile.CHRONICLER: 0.3,
    GMProfile.FIRESIDE: 1.0,
    GMProfile.LANTERN: 2.0,
}


def can_spend_uncanny_token(state: RunState, event: EventSkeleton) -> bool:
    """Check if an uncanny token can be spent based on profile rules."""
    if state.uncanny_tokens <= 0:
        return False
    if not event.costs_uncanny_token:
        return True  # doesn't need one

    profile = state.gm_profile
    tags = set(event.tags)

    if profile == GMProfile.CHRONICLER:
        return event.severity in ("medium", "high") and "folklore:uncanny" in tags

    if profile == GMProfile.FIRESIDE:
        if "folklore:uncanny" in tags:
            return True
        node = _find_node(state)
        if node and state.time_of_day in (TimeOfDay.NIGHT,) and state.party.morale < 40:
            return "fog" in tags or node.biome in (Biome.FOREST, Biome.SWAMP)
        return False

    if profile == GMProfile.LANTERN:
        if "folklore:uncanny" in tags:
            return True
        node = _find_node(state)
        if node and state.time_of_day in (TimeOfDay.NIGHT, TimeOfDay.EVENING):
            return node.biome in (Biome.FOREST, Biome.SWAMP) or "fog" in tags
        return False

    return False


def select_event(state: RunState, rng: SeededRNG, library: list[EventSkeleton]) -> EventSkeleton:
    """Select an event using weighted probabilities."""
    node = _find_node(state)
    candidates: list[EventSkeleton] = []
    weights: list[float] = []

    for event in library:
        # Check biome filter
        if event.biome_filter and node and node.biome not in event.biome_filter:
            continue

        # Check weather filter
        if event.weather_filter:
            # We'll check this loosely — caller should pass current weather context
            pass

        # Check time filter
        if event.time_filter and state.time_of_day not in event.time_filter:
            continue

        # Check uncanny token availability
        if event.costs_uncanny_token and not can_spend_uncanny_token(state, event):
            continue

        # Compute weight
        weight = event.base_weight

        # Category weighting
        if event.category == EventCategory.SURVIVAL:
            weight *= 1.0  # baseline
        elif event.category == EventCategory.HUMAN:
            weight *= 0.8
        elif event.category == EventCategory.FOLKLORE:
            weight *= PROFILE_FOLKLORE_WEIGHT[state.gm_profile]
            if event.folklore_type == FolkloreType.UNCANNY:
                weight *= PROFILE_UNCANNY_WEIGHT[state.gm_profile]
        elif event.category == EventCategory.BIG:
            weight *= 0.4

        # Twist modifiers
        for twist in state.twists:
            if twist == TwistModifier.BANDIT_YEAR and "bandits" in event.tags:
                weight *= 1.5
            if twist == TwistModifier.SICK_SEASON and "sickness" in event.tags:
                weight *= 1.5
            if twist == TwistModifier.FLOOD_YEAR and "river" in event.tags:
                weight *= 1.5
            if twist == TwistModifier.GOOD_HUNTING and "hunt" in event.tags:
                weight *= 1.5

        # Low supplies increase relevant event weights
        if state.supplies.food < 10 and "food" in event.tags:
            weight *= 1.3
        if state.supplies.water < 10 and "water" in event.tags:
            weight *= 1.3

        candidates.append(event)
        weights.append(weight)

    if not candidates:
        # Fallback: just pick a survival event
        survival = [e for e in library if e.category == EventCategory.SURVIVAL]
        return rng.choice(survival) if survival else library[0]

    return rng.weighted_choice(candidates, weights)


def resolve_event(
    state: RunState,
    event: EventSkeleton,
    choice_id: str,
    rng: SeededRNG,
) -> EventOutcome:
    """Resolve an event choice into concrete outcome."""
    template = event.outcome_templates.get(choice_id)
    if not template:
        return EventOutcome()

    outcome = EventOutcome(
        supplies_delta=dict(template.supplies_delta),
        health_delta=template.health_delta,
        wagon_delta=template.wagon_delta,
        morale_delta=template.morale_delta,
        time_cost=template.time_cost,
        distance_delta=template.distance_delta,
        special_flags=list(template.special_flags),
    )

    # Add some randomness to deltas
    if outcome.health_delta != 0:
        outcome.health_delta += rng.randint(-3, 3)

    # Spend uncanny token if flagged
    if "uncanny_token_spent" in outcome.special_flags and state.uncanny_tokens > 0:
        state.uncanny_tokens -= 1

    return outcome


def apply_outcome(state: RunState, outcome: EventOutcome) -> None:
    """Apply an event outcome to game state."""
    state.supplies.apply_delta(outcome.supplies_delta)

    if outcome.health_delta != 0:
        for member in state.party.members:
            if member.is_alive():
                member.health = max(0, min(100, member.health + outcome.health_delta))

    if outcome.wagon_delta != 0:
        state.wagon.condition = max(0, min(100, state.wagon.condition + outcome.wagon_delta))

    if outcome.morale_delta != 0:
        state.party.morale = max(0, min(100, state.party.morale + outcome.morale_delta))

    if outcome.distance_delta != 0:
        state.distance_remaining += outcome.distance_delta


def _find_node(state: RunState):
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None
