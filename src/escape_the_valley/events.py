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
    family: str = ""  # Shared choice pattern grouping
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


# ─── Event skeleton library ────────────────────────────────────────────────

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

    # ── NEW SURVIVAL EVENTS (15) ─────────────────────────────────────

    events.append(EventSkeleton(
        event_id="fire_out",
        title="Fire Gone Out",
        category=EventCategory.SURVIVAL,
        tags=["night", "firewood", "cold"],
        severity="medium",
        family="scarcity_crisis",
        time_filter=[TimeOfDay.NIGHT, TimeOfDay.EVENING],
        base_weight=1.2,
        fallback_narration="The fire has died. The cold presses in. No firewood remains.",
        fallback_choices=[
            ChoiceTemplate("A", "Gather wood in the dark.", "INVESTIGATE", "BOLD",
                           "Injury risk.", "Time, possible injury."),
            ChoiceTemplate("B", "Huddle together and endure.", "REST", "CAUTIOUS",
                           "Health cost, safe.", "Health."),
            ChoiceTemplate("C", "Burn wagon planks.", "RISK", "NEUTRAL",
                           "Wagon damage for warmth.", "Wagon condition."),
        ],
        outcome_templates={
            "A": EventOutcome(
                supplies_delta={"firewood": 3}, health_delta=-5,
                time_cost=1,
            ),
            "B": EventOutcome(health_delta=-10, morale_delta=-4),
            "C": EventOutcome(wagon_delta=-10, morale_delta=-2),
        },
    ))

    events.append(EventSkeleton(
        event_id="quicksand",
        title="Quicksand",
        category=EventCategory.SURVIVAL,
        tags=["terrain", "danger", "rope"],
        severity="high",
        family="navigation",
        biome_filter=[Biome.SWAMP],
        base_weight=1.0,
        fallback_narration=(
            "The ground gives way underfoot. Someone is sinking fast."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Use rope to pull them free.", "INVESTIGATE", "NEUTRAL",
                           "Safe if you have rope.", "Rope."),
            ChoiceTemplate("B", "Form a human chain.", "RISK", "BOLD",
                           "Risk more people.", "Health."),
            ChoiceTemplate("C", "Find a branch or plank.", "WAIT", "CAUTIOUS",
                           "Slow but safe.", "Time."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=2),
            "B": EventOutcome(health_delta=-12, morale_delta=-5),
            "C": EventOutcome(time_cost=1, morale_delta=-2),
        },
    ))

    events.append(EventSkeleton(
        event_id="broken_wheel",
        title="Broken Wheel",
        category=EventCategory.SURVIVAL,
        tags=["breakdown", "wagon", "repair", "tools"],
        severity="medium",
        family="breakdown",
        base_weight=1.0,
        fallback_narration=(
            "A spoke snaps, then another. The wheel buckles sideways."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Replace with spare parts.", "REPAIR", "NEUTRAL",
                           "Uses parts.", "1 part, time."),
            ChoiceTemplate("B", "Hammer it straight with tools.", "REPAIR", "BOLD",
                           "Needs tools, partial fix.", "Time."),
            ChoiceTemplate("C", "Drag the wagon to the next town.", "TRAVEL", "CAUTIOUS",
                           "Slow, more wagon damage.", "Distance, wagon."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"parts": -1}, wagon_delta=15, time_cost=1),
            "B": EventOutcome(wagon_delta=-5, time_cost=1),
            "C": EventOutcome(wagon_delta=-10, distance_delta=3),
        },
    ))

    events.append(EventSkeleton(
        event_id="mudslide",
        title="Mudslide",
        category=EventCategory.SURVIVAL,
        tags=["terrain", "swamp", "delay"],
        severity="medium",
        family="navigation",
        biome_filter=[Biome.SWAMP, Biome.FOREST],
        weather_filter=[Weather.RAIN, Weather.STORM],
        base_weight=1.0,
        fallback_narration="A wall of mud slides across the trail. The path is blocked.",
        fallback_choices=[
            ChoiceTemplate("A", "Dig through it.", "WAIT", "NEUTRAL",
                           "Time-consuming.", "Half a day."),
            ChoiceTemplate("B", "Find a way around.", "DETOUR", "CAUTIOUS",
                           "Adds distance.", "Extra miles."),
            ChoiceTemplate("C", "Push the wagon through.", "TRAVEL", "BOLD",
                           "Wagon damage risk.", "Wagon."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1, morale_delta=-2),
            "B": EventOutcome(distance_delta=6),
            "C": EventOutcome(wagon_delta=-12, morale_delta=-3),
        },
    ))

    events.append(EventSkeleton(
        event_id="drought",
        title="Endless Dry",
        category=EventCategory.SURVIVAL,
        tags=["water", "desert", "supplies"],
        severity="high",
        family="scarcity_crisis",
        biome_filter=[Biome.DESERT],
        base_weight=1.2,
        fallback_narration=(
            "Three days without rain. The water barrels are nearly empty."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Strict rationing.", "WAIT", "CAUTIOUS",
                           "Morale drops hard.", "Morale, health."),
            ChoiceTemplate("B", "Send scouts for water.", "INVESTIGATE", "NEUTRAL",
                           "Time cost, may find nothing.", "Time."),
            ChoiceTemplate("C", "Push hard to the next source.", "TRAVEL", "BOLD",
                           "Burns more water.", "Water."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=-8, health_delta=-5),
            "B": EventOutcome(time_cost=1, supplies_delta={"water": 6}),
            "C": EventOutcome(supplies_delta={"water": -8}),
        },
    ))

    events.append(EventSkeleton(
        event_id="berry_patch",
        title="Berry Patch",
        category=EventCategory.SURVIVAL,
        tags=["food", "foraging", "opportunity"],
        severity="low",
        base_weight=1.0,
        biome_filter=[Biome.FOREST, Biome.PLAINS],
        fallback_narration="A thicket heavy with berries. Some look familiar. Others do not.",
        fallback_choices=[
            ChoiceTemplate("A", "Pick only the ones you recognize.", "WAIT", "CAUTIOUS",
                           "Small gain, safe.", "Food."),
            ChoiceTemplate("B", "Pick everything.", "RISK", "BOLD",
                           "More food, poison risk.", "Food or health."),
            ChoiceTemplate("C", "Pass by.", "TRAVEL", "NEUTRAL",
                           "No risk.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": 4}, morale_delta=2),
            "B": EventOutcome(supplies_delta={"food": 8}, health_delta=-8),
            "C": EventOutcome(),
        },
    ))

    events.append(EventSkeleton(
        event_id="wolf_pack",
        title="Wolf Pack",
        category=EventCategory.SURVIVAL,
        tags=["danger", "wildlife", "night"],
        severity="high",
        family="wildlife",
        time_filter=[TimeOfDay.NIGHT, TimeOfDay.EVENING],
        base_weight=0.8,
        fallback_narration="Eyes reflect in the firelight. A pack circles the camp.",
        fallback_choices=[
            ChoiceTemplate("A", "Keep the fire high all night.", "WAIT", "CAUTIOUS",
                           "Uses firewood.", "Firewood."),
            ChoiceTemplate("B", "Fire a warning shot.", "GUARD", "NEUTRAL",
                           "Ammo cost, may scatter them.", "Ammo."),
            ChoiceTemplate("C", "Stand guard with weapons.", "GUARD", "BOLD",
                           "Injury risk.", "Health, ammo."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"firewood": -3}, morale_delta=-3),
            "B": EventOutcome(supplies_delta={"ammo": -2}, morale_delta=-2),
            "C": EventOutcome(
                supplies_delta={"ammo": -1}, health_delta=-8, morale_delta=3,
            ),
        },
    ))

    events.append(EventSkeleton(
        event_id="cliff_path",
        title="Cliff Path",
        category=EventCategory.SURVIVAL,
        tags=["terrain", "danger", "alpine"],
        severity="medium",
        family="navigation",
        biome_filter=[Biome.ALPINE],
        base_weight=1.2,
        fallback_narration="The trail narrows to a ledge. Below is a long fall.",
        fallback_choices=[
            ChoiceTemplate("A", "Go slowly and carefully.", "TRAVEL", "CAUTIOUS",
                           "Safe but slow.", "Time."),
            ChoiceTemplate("B", "Lighten the wagon and cross.", "TRAVEL", "NEUTRAL",
                           "Lose some supplies.", "Supplies."),
            ChoiceTemplate("C", "Push through at speed.", "TRAVEL", "BOLD",
                           "Wagon damage risk.", "Wagon."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1),
            "B": EventOutcome(supplies_delta={"food": -3, "water": -2}),
            "C": EventOutcome(wagon_delta=-15, morale_delta=-4),
        },
    ))

    events.append(EventSkeleton(
        event_id="flash_flood",
        title="Flash Flood",
        category=EventCategory.SURVIVAL,
        tags=["river", "danger", "rope"],
        severity="high",
        family="weather_hazard",
        weather_filter=[Weather.RAIN, Weather.STORM],
        base_weight=0.8,
        fallback_narration=(
            "Water rises fast. The creek bed becomes a torrent in minutes."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Rope the wagon to trees and wait.", "WAIT", "CAUTIOUS",
                           "Uses rope, safe.", "Rope, time."),
            ChoiceTemplate("B", "Race to high ground.", "TRAVEL", "BOLD",
                           "May lose supplies.", "Supplies, wagon."),
            ChoiceTemplate("C", "Brace and hold position.", "WAIT", "NEUTRAL",
                           "Risk of damage.", "Wagon, health."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1, morale_delta=-2),
            "B": EventOutcome(
                supplies_delta={"food": -5}, wagon_delta=-8, morale_delta=-4,
            ),
            "C": EventOutcome(wagon_delta=-12, health_delta=-8, morale_delta=-5),
        },
    ))

    events.append(EventSkeleton(
        event_id="heavy_rain",
        title="Heavy Rain",
        category=EventCategory.SURVIVAL,
        tags=["weather", "rain", "supplies"],
        severity="medium",
        family="weather_hazard",
        weather_filter=[Weather.RAIN, Weather.STORM],
        base_weight=1.5,
        fallback_narration="Rain hammers down without pause. Everything is soaked.",
        fallback_choices=[
            ChoiceTemplate("A", "Cover the supplies and wait.", "WAIT", "CAUTIOUS",
                           "Time cost.", "Time."),
            ChoiceTemplate("B", "Keep moving in the rain.", "TRAVEL", "BOLD",
                           "Morale, health risk.", "Morale, health."),
        ],
        outcome_templates={
            "A": EventOutcome(
                time_cost=1, supplies_delta={"firewood": -2}, morale_delta=-2,
            ),
            "B": EventOutcome(health_delta=-5, morale_delta=-5),
        },
    ))

    events.append(EventSkeleton(
        event_id="heatstroke",
        title="Heatstroke",
        category=EventCategory.SURVIVAL,
        tags=["weather", "health", "desert"],
        severity="high",
        family="weather_hazard",
        biome_filter=[Biome.DESERT],
        base_weight=1.0,
        fallback_narration="The sun is merciless. Someone collapses.",
        fallback_choices=[
            ChoiceTemplate("A", "Rest in shade and use water.", "REST", "CAUTIOUS",
                           "Water cost, best recovery.", "Water."),
            ChoiceTemplate("B", "Keep moving, tend on the road.", "TRAVEL", "BOLD",
                           "Condition may worsen.", "Health."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"water": -4}, morale_delta=-3),
            "B": EventOutcome(health_delta=-15, morale_delta=-6),
        },
    ))

    events.append(EventSkeleton(
        event_id="cave_shelter",
        title="Cave Shelter",
        category=EventCategory.SURVIVAL,
        tags=["terrain", "rest", "alpine", "opportunity"],
        severity="low",
        biome_filter=[Biome.ALPINE],
        base_weight=0.8,
        fallback_narration="A dry cave in the rock face. Shelter from the elements.",
        fallback_choices=[
            ChoiceTemplate("A", "Camp here for the night.", "REST", "CAUTIOUS",
                           "Good rest, safe.", "Time."),
            ChoiceTemplate("B", "Quick rest and move on.", "REST", "NEUTRAL",
                           "Partial recovery.", "Less benefit."),
        ],
        outcome_templates={
            "A": EventOutcome(
                morale_delta=5, health_delta=5, time_cost=1,
            ),
            "B": EventOutcome(morale_delta=2, health_delta=2),
        },
    ))

    events.append(EventSkeleton(
        event_id="rattlesnake_den",
        title="Rattlesnake Den",
        category=EventCategory.SURVIVAL,
        tags=["danger", "wildlife", "terrain"],
        severity="medium",
        family="wildlife",
        biome_filter=[Biome.DESERT, Biome.PLAINS],
        base_weight=0.8,
        fallback_narration=(
            "The trail passes through a rocky outcrop buzzing with rattles."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Detour around the rocks.", "DETOUR", "CAUTIOUS",
                           "Safe, adds distance.", "Distance."),
            ChoiceTemplate("B", "Move through slowly and quietly.", "TRAVEL", "NEUTRAL",
                           "Injury possible.", "Health."),
            ChoiceTemplate("C", "Clear the path with noise.", "RISK", "BOLD",
                           "Fast but risky.", "Health."),
        ],
        outcome_templates={
            "A": EventOutcome(distance_delta=4),
            "B": EventOutcome(health_delta=-8, morale_delta=-3),
            "C": EventOutcome(health_delta=-12, morale_delta=-2),
        },
    ))

    events.append(EventSkeleton(
        event_id="game_drought",
        title="Barren Hunting Grounds",
        category=EventCategory.SURVIVAL,
        tags=["hunt", "food", "supplies"],
        severity="low",
        family="scarcity_crisis",
        base_weight=0.8,
        fallback_narration="The land is empty. No tracks, no movement, no game.",
        fallback_choices=[
            ChoiceTemplate("A", "Set traps and wait.", "WAIT", "NEUTRAL",
                           "Time cost, small chance.", "Time."),
            ChoiceTemplate("B", "Move on and conserve ammo.", "TRAVEL", "CAUTIOUS",
                           "No food gained.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(
                supplies_delta={"food": 3}, time_cost=1, morale_delta=-2,
            ),
            "B": EventOutcome(morale_delta=-3),
        },
    ))

    events.append(EventSkeleton(
        event_id="water_hole",
        title="Hidden Water Hole",
        category=EventCategory.SURVIVAL,
        tags=["water", "desert", "opportunity"],
        severity="low",
        biome_filter=[Biome.DESERT],
        base_weight=1.0,
        fallback_narration=(
            "A pool of water shimmers in a rock basin. Animals have been here."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Fill containers carefully.", "WAIT", "CAUTIOUS",
                           "Good water.", "Time."),
            ChoiceTemplate("B", "Drink and refill quickly.", "TRAVEL", "BOLD",
                           "Less gained, keep moving.", "Speed."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"water": 12}, morale_delta=4),
            "B": EventOutcome(supplies_delta={"water": 5}),
        },
    ))

    # ── NEW HUMAN ENCOUNTERS (10) ─────────────────────────────────────

    events.append(EventSkeleton(
        event_id="merchant_caravan",
        title="Merchant Caravan",
        category=EventCategory.HUMAN,
        tags=["human", "trade", "supplies"],
        severity="low",
        family="human_encounter",
        base_weight=1.0,
        fallback_narration=(
            "A string of pack mules rounds the bend. "
            "The trader tips his hat."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Trade food for medicine.", "TRADE", "NEUTRAL",
                           "Fair exchange.", "Food for meds."),
            ChoiceTemplate("B", "Trade food for parts.", "TRADE", "NEUTRAL",
                           "Fair exchange.", "Food for parts."),
            ChoiceTemplate("C", "Decline and move on.", "TRAVEL", "CAUTIOUS",
                           "No trade.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": -8, "meds": 3}),
            "B": EventOutcome(supplies_delta={"food": -6, "parts": 1}),
            "C": EventOutcome(),
        },
    ))

    events.append(EventSkeleton(
        event_id="refugee_family",
        title="Refugee Family",
        category=EventCategory.HUMAN,
        tags=["human", "moral_choice", "morale"],
        severity="low",
        family="human_encounter",
        base_weight=0.8,
        fallback_narration=(
            "A family sits by the road. Children. "
            "They watch your food with hollow eyes."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Share food and water.", "TRADE", "CAUTIOUS",
                           "Costly, morale boost.", "Food, water."),
            ChoiceTemplate("B", "Give directions and advice.", "WAIT", "NEUTRAL",
                           "No cost.", "Nothing tangible."),
            ChoiceTemplate("C", "Pass without stopping.", "TRAVEL", "BOLD",
                           "Fast but cold.", "Morale hit."),
        ],
        outcome_templates={
            "A": EventOutcome(
                supplies_delta={"food": -5, "water": -3}, morale_delta=8,
            ),
            "B": EventOutcome(morale_delta=2),
            "C": EventOutcome(morale_delta=-6),
        },
    ))

    events.append(EventSkeleton(
        event_id="lost_child",
        title="Lost Child",
        category=EventCategory.HUMAN,
        tags=["human", "moral_choice", "delay"],
        severity="medium",
        base_weight=0.6,
        fallback_narration=(
            "A child stands alone at the trailside, too young to be here."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Search for their family.", "INVESTIGATE", "CAUTIOUS",
                           "Time cost, right thing.", "Half a day."),
            ChoiceTemplate("B", "Leave food and move on.", "TRADE", "NEUTRAL",
                           "Some cost.", "Food."),
            ChoiceTemplate("C", "Keep moving.", "TRAVEL", "BOLD",
                           "Fastest.", "Morale."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1, morale_delta=6),
            "B": EventOutcome(supplies_delta={"food": -3}, morale_delta=2),
            "C": EventOutcome(morale_delta=-8),
        },
    ))

    events.append(EventSkeleton(
        event_id="preacher",
        title="The Preacher",
        category=EventCategory.HUMAN,
        tags=["human", "morale", "rest"],
        severity="low",
        family="human_encounter",
        base_weight=0.8,
        fallback_narration=(
            "A man in black stands at a crossroads. "
            "He offers a sermon and a rest."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Listen and rest.", "REST", "CAUTIOUS",
                           "Morale boost, time cost.", "Time."),
            ChoiceTemplate("B", "Politely decline.", "TRAVEL", "NEUTRAL",
                           "No effect.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=7, time_cost=1),
            "B": EventOutcome(),
        },
    ))

    events.append(EventSkeleton(
        event_id="con_artist",
        title="The Swindler",
        category=EventCategory.HUMAN,
        tags=["human", "danger", "trade"],
        severity="medium",
        base_weight=0.7,
        fallback_narration=(
            "A smooth-talking stranger offers a deal that seems too good."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Take the deal.", "TRADE", "BOLD",
                           "Might lose supplies.", "Supplies."),
            ChoiceTemplate("B", "Decline politely.", "WAIT", "CAUTIOUS",
                           "Safe.", "Nothing."),
            ChoiceTemplate("C", "Confront the swindle.", "RISK", "NEUTRAL",
                           "May recover losses or escalate.", "Uncertain."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": -8, "ammo": -3}, morale_delta=-5),
            "B": EventOutcome(),
            "C": EventOutcome(morale_delta=-3, supplies_delta={"food": 3}),
        },
    ))

    events.append(EventSkeleton(
        event_id="road_gang",
        title="Road Gang",
        category=EventCategory.HUMAN,
        tags=["human", "labor", "delay"],
        severity="low",
        base_weight=0.8,
        fallback_narration="Men clearing a fallen tree from the road. They wave you over.",
        fallback_choices=[
            ChoiceTemplate("A", "Help clear the road.", "WAIT", "NEUTRAL",
                           "Time cost, morale gain.", "Time."),
            ChoiceTemplate("B", "Wait for them to finish.", "WAIT", "CAUTIOUS",
                           "Time cost, no effort.", "Time."),
            ChoiceTemplate("C", "Detour around.", "DETOUR", "BOLD",
                           "Adds distance.", "Distance."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1, morale_delta=4),
            "B": EventOutcome(time_cost=1),
            "C": EventOutcome(distance_delta=5),
        },
    ))

    events.append(EventSkeleton(
        event_id="abandoned_wagon",
        title="Abandoned Wagon",
        category=EventCategory.HUMAN,
        tags=["exploration", "supplies", "danger"],
        severity="medium",
        base_weight=0.8,
        fallback_narration=(
            "A wagon sits on the roadside, tongue-down. "
            "No animals, no people. Flies buzz."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Salvage what you can.", "INVESTIGATE", "NEUTRAL",
                           "Supplies, maybe a trap.", "Time."),
            ChoiceTemplate("B", "Take parts only.", "INVESTIGATE", "CAUTIOUS",
                           "Less risk.", "Parts."),
            ChoiceTemplate("C", "Leave it alone.", "TRAVEL", "CAUTIOUS",
                           "No risk.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(
                supplies_delta={"food": 6, "parts": 1, "rope": 1},
                time_cost=1,
            ),
            "B": EventOutcome(supplies_delta={"parts": 1}),
            "C": EventOutcome(),
        },
    ))

    events.append(EventSkeleton(
        event_id="map_seller",
        title="The Map Seller",
        category=EventCategory.HUMAN,
        tags=["human", "trade", "navigation"],
        severity="low",
        base_weight=0.6,
        fallback_narration="An old man sells hand-drawn maps. He claims to know the valley.",
        fallback_choices=[
            ChoiceTemplate("A", "Buy a map (food).", "TRADE", "NEUTRAL",
                           "May be useful.", "Food."),
            ChoiceTemplate("B", "Ask for free advice.", "WAIT", "CAUTIOUS",
                           "May learn something.", "Time."),
        ],
        outcome_templates={
            "A": EventOutcome(supplies_delta={"food": -4}, morale_delta=3),
            "B": EventOutcome(morale_delta=1),
        },
    ))

    events.append(EventSkeleton(
        event_id="wedding_feast",
        title="Wedding Feast",
        category=EventCategory.HUMAN,
        tags=["human", "morale", "food"],
        severity="low",
        base_weight=0.5,
        fallback_narration=(
            "Music carries from a settlement. "
            "A wedding. You are waved over."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Join the celebration.", "REST", "NEUTRAL",
                           "Morale boost, food cost.", "Food, time."),
            ChoiceTemplate("B", "Politely decline and pass.", "TRAVEL", "CAUTIOUS",
                           "No effect.", "Nothing."),
        ],
        outcome_templates={
            "A": EventOutcome(
                supplies_delta={"food": -4}, morale_delta=10, time_cost=1,
            ),
            "B": EventOutcome(morale_delta=-1),
        },
    ))

    events.append(EventSkeleton(
        event_id="bounty_hunters",
        title="Bounty Hunters",
        category=EventCategory.HUMAN,
        tags=["human", "danger", "combat"],
        severity="high",
        base_weight=0.5,
        fallback_narration=(
            "Armed riders block the trail. "
            "They claim someone in your party is wanted."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Talk calmly and prove identity.", "WAIT", "CAUTIOUS",
                           "Time cost.", "Time."),
            ChoiceTemplate("B", "Stand your ground.", "GUARD", "BOLD",
                           "May escalate.", "Health, ammo."),
            ChoiceTemplate("C", "Offer a bribe.", "PAY", "NEUTRAL",
                           "Costly but safe.", "Supplies."),
        ],
        outcome_templates={
            "A": EventOutcome(time_cost=1, morale_delta=-3),
            "B": EventOutcome(
                supplies_delta={"ammo": -2}, health_delta=-10, morale_delta=3,
            ),
            "C": EventOutcome(supplies_delta={"food": -6, "ammo": -2}, morale_delta=-4),
        },
    ))

    # ── NEW FOLKLORE (5) ──────────────────────────────────────────────

    events.append(EventSkeleton(
        event_id="counting_crows",
        title="Counting Crows",
        category=EventCategory.FOLKLORE,
        tags=["folklore:omen", "morale", "morning"],
        severity="low",
        folklore_type=FolkloreType.MISINTERPRETATION,
        time_filter=[TimeOfDay.MORNING],
        base_weight=0.7,
        fallback_narration=(
            "A line of crows watches from a fence. "
            "Someone counts them and goes pale."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Dismiss it as superstition.", "WAIT", "NEUTRAL",
                           "May ease or worry the party.", "Morale."),
            ChoiceTemplate("B", "Take it as a sign to rest.", "REST", "CAUTIOUS",
                           "Morale boost, time cost.", "Time."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=-2),
            "B": EventOutcome(morale_delta=3, time_cost=1),
        },
        gm_aside="Old rhyme about crows and fortune. Just birds.",
    ))

    events.append(EventSkeleton(
        event_id="whispering_tree",
        title="The Whispering Tree",
        category=EventCategory.FOLKLORE,
        tags=["forest", "folklore:uncanny", "morale"],
        severity="medium",
        folklore_type=FolkloreType.UNCANNY,
        costs_uncanny_token=True,
        biome_filter=[Biome.FOREST],
        base_weight=0.5,
        fallback_narration=(
            "A great oak at the trail fork. Wind through its "
            "branches sounds like speech."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Listen carefully.", "INVESTIGATE", "NEUTRAL",
                           "Curiosity.", "Morale."),
            ChoiceTemplate("B", "Keep moving, don't look back.", "TRAVEL", "CAUTIOUS",
                           "Safe.", "Morale."),
        ],
        outcome_templates={
            "A": EventOutcome(
                morale_delta=-5, special_flags=["uncanny_token_spent"],
            ),
            "B": EventOutcome(
                morale_delta=-2, special_flags=["uncanny_token_spent"],
            ),
        },
        gm_aside="Wind resonance through a hollow trunk. Unsettling.",
    ))

    events.append(EventSkeleton(
        event_id="compass_spin",
        title="Spinning Compass",
        category=EventCategory.FOLKLORE,
        tags=["navigation", "folklore:natural_oddity", "morale"],
        severity="low",
        folklore_type=FolkloreType.NATURAL_ODDITY,
        base_weight=0.6,
        fallback_narration=(
            "The compass needle spins lazily. "
            "It refuses to settle on any heading."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Navigate by the sun instead.", "TRAVEL", "NEUTRAL",
                           "Slower but works.", "Time."),
            ChoiceTemplate("B", "Wait for the compass to settle.", "WAIT", "CAUTIOUS",
                           "May waste time.", "Time."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=-2),
            "B": EventOutcome(time_cost=1, morale_delta=-1),
        },
        gm_aside="Magnetic anomaly from iron deposits. Real phenomenon.",
    ))

    events.append(EventSkeleton(
        event_id="doubled_tracks",
        title="Doubled Tracks",
        category=EventCategory.FOLKLORE,
        tags=["terrain", "folklore:uncanny", "morale"],
        severity="medium",
        folklore_type=FolkloreType.UNCANNY,
        costs_uncanny_token=True,
        base_weight=0.4,
        fallback_narration=(
            "Your own wagon tracks ahead of you on a trail "
            "no one has used in weeks."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Follow the tracks.", "TRAVEL", "BOLD",
                           "Unnerving.", "Morale."),
            ChoiceTemplate("B", "Take a different path.", "DETOUR", "CAUTIOUS",
                           "Adds distance.", "Distance."),
            ChoiceTemplate("C", "Examine the tracks closely.", "INVESTIGATE", "NEUTRAL",
                           "Time cost, may explain.", "Time."),
        ],
        outcome_templates={
            "A": EventOutcome(
                morale_delta=-8, special_flags=["uncanny_token_spent"],
            ),
            "B": EventOutcome(
                distance_delta=4, morale_delta=-3,
                special_flags=["uncanny_token_spent"],
            ),
            "C": EventOutcome(
                time_cost=1, morale_delta=-1,
                special_flags=["uncanny_token_spent"],
            ),
        },
        gm_aside=(
            "Previous party with identical wagon type. "
            "Or a loop in the trail. Never confirm."
        ),
    ))

    events.append(EventSkeleton(
        event_id="the_stranger",
        title="The Stranger",
        category=EventCategory.FOLKLORE,
        tags=["human", "folklore:social", "morale"],
        severity="medium",
        folklore_type=FolkloreType.SOCIAL,
        base_weight=0.5,
        fallback_narration=(
            "A traveler shares your fire. "
            "He knows your names and where you are headed."
        ),
        fallback_choices=[
            ChoiceTemplate("A", "Ask how he knows.", "INVESTIGATE", "NEUTRAL",
                           "May learn something. Or not.", "Morale."),
            ChoiceTemplate("B", "Share the fire and say nothing.", "WAIT", "CAUTIOUS",
                           "Unsettling but harmless.", "Morale."),
            ChoiceTemplate("C", "Ask him to leave.", "RISK", "BOLD",
                           "Direct.", "Morale."),
        ],
        outcome_templates={
            "A": EventOutcome(morale_delta=-4),
            "B": EventOutcome(morale_delta=-2),
            "C": EventOutcome(morale_delta=-5),
        },
        gm_aside=(
            "Information travels ahead on faster routes. "
            "Or a lucky guess. Social folklore."
        ),
    ))

    # ── Load data-driven events from JSON ─────────────────────────────
    from .event_loader import load_json_events  # noqa: E402
    events.extend(load_json_events())

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


# ── Tag family cooldown ────────────────────────────────────────────────

# Tags that form "families" for cooldown purposes.
# If the player just saw a river event, other river events get downweighted.
_TAG_FAMILIES = {"river", "weather", "human", "folklore", "survival", "ford", "bridge"}

_COOLDOWN_WINDOW = 6  # remember last N events' primary tags
_COOLDOWN_PENALTY = 0.25  # multiply weight by this for each recent match


def _record_event_tags(state: RunState, event: EventSkeleton) -> None:
    """Record an event's primary tags in the cooldown buffer."""
    family_tags = [t for t in event.tags if t in _TAG_FAMILIES]
    if family_tags:
        state.recent_event_tags.append(family_tags[0])
    else:
        state.recent_event_tags.append(event.category.value)

    # Track high-severity for consecutive dampening
    if event.severity == "high":
        state.recent_event_tags.append("_high_sev")

    # Trim to window size (doubled to accommodate severity markers)
    max_window = _COOLDOWN_WINDOW * 2
    if len(state.recent_event_tags) > max_window:
        state.recent_event_tags = state.recent_event_tags[-max_window:]


def _cooldown_factor(state: RunState, event: EventSkeleton) -> float:
    """Compute cooldown multiplier based on recent tag history."""
    if not state.recent_event_tags:
        return 1.0
    family_tags = {t for t in event.tags if t in _TAG_FAMILIES}
    if not family_tags:
        family_tags = {event.category.value}
    hits = sum(1 for t in state.recent_event_tags if t in family_tags)
    if hits == 0:
        return 1.0
    return _COOLDOWN_PENALTY ** hits


# ── Twist-based weight biases ─────────────────────────────────────────

# Map twist → set of tags that get boosted
_TWIST_TAG_BOOSTS: dict[TwistModifier, set[str]] = {
    TwistModifier.BANDIT_YEAR: {"human", "bandits"},
    TwistModifier.SICK_SEASON: {"sickness", "survival"},
    TwistModifier.FLOOD_YEAR: {"river", "ford", "bridge", "weather", "rain"},
    TwistModifier.EARLY_WINTER: {"weather", "snow", "wind"},
    TwistModifier.GOOD_HUNTING: {"hunt", "forest"},
}


def select_event(state: RunState, rng: SeededRNG, library: list[EventSkeleton]) -> EventSkeleton:
    """Select an event using weighted probabilities with variety guards."""
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

        # Twist modifiers — expanded tag matching
        event_tags = set(event.tags)
        for twist in state.twists:
            boost_tags = _TWIST_TAG_BOOSTS.get(twist, set())
            if event_tags & boost_tags:
                weight *= 1.5

        # Severity curve — capped at 1.25× (was 1.5×)
        from .physics import journey_pressure  # noqa: E402

        pressure = journey_pressure(state)
        if event.severity == "high":
            weight *= 1.0 + 0.25 * pressure  # up to 1.25× late game
            # Consecutive high-severity dampening
            recent_high = sum(
                1 for t in state.recent_event_tags[-6:]
                if t == "_high_sev"
            )
            if recent_high >= 2:
                weight *= 0.7
        elif event.severity == "low":
            weight *= 1.0 - 0.25 * pressure  # down to 0.75× late game

        # Low supplies increase relevant event weights
        if state.supplies.food < 10 and "food" in event_tags:
            weight *= 1.3
        if state.supplies.water < 10 and "water" in event_tags:
            weight *= 1.3

        # Tag family cooldown — downweight recently seen families
        weight *= _cooldown_factor(state, event)

        candidates.append(event)
        weights.append(weight)

    if not candidates:
        # Fallback: just pick a survival event
        survival = [e for e in library if e.category == EventCategory.SURVIVAL]
        return rng.choice(survival) if survival else library[0]

    selected = rng.weighted_choice(candidates, weights)

    # Record this event's tags for future cooldown
    _record_event_tags(state, selected)

    return selected


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
