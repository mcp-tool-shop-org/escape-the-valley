"""Seeded world generation — map, weather, twist deck, party."""

from __future__ import annotations

from .models import (
    DOCTRINE_MODIFIERS,
    Biome,
    Condition,
    Doctrine,
    GMProfile,
    MapNode,
    PartyMember,
    PartyState,
    RunState,
    SeededRNG,
    SuppliesState,
    Taboo,
    TimeOfDay,
    Trait,
    TwistModifier,
    WagonState,
    Weather,
)
from .resources import DEFAULT_SUPPLIES

# Names for party members
FIRST_NAMES = [
    "Elias", "Martha", "Thomas", "Agnes", "Willem", "Ruth",
    "Josiah", "Clara", "Silas", "Mercy", "Caleb", "Hannah",
    "Ezra", "Patience", "Amos", "Lydia", "Nathaniel", "Esther",
]

# Names for map nodes
TOWN_NAMES = [
    "Millford", "Redwater", "Stonecross", "Ashwell", "Blackmere",
    "Thornhaven", "Dryreach", "Coldspring", "Ironbrook", "Fellgate",
    "Whitmarsh", "Copperhill", "Ravenmoor", "Hazelwick", "Dunhollow",
]

LANDMARK_NAMES = [
    "The Narrows", "Devil's Bend", "Widow's Pass", "Three Oaks Fork",
    "Salt Flat", "Bone Ridge", "Fog Hollow", "The Long Descent",
    "Sunken Meadow", "Crow's Perch", "Blind Canyon", "The Switchback",
    "Stillwater Crossing", "Whitefall", "The Gulch", "Ashen Bluffs",
    "Broken Bridge", "The Scar", "Shepherd's Rest", "Moss Landing",
]

# Biome sequences for route flavor
BIOME_PATTERNS = [
    [Biome.PLAINS, Biome.FOREST, Biome.ALPINE, Biome.FOREST, Biome.PLAINS],
    [Biome.PLAINS, Biome.SWAMP, Biome.FOREST, Biome.ALPINE, Biome.FOREST],
    [Biome.FOREST, Biome.PLAINS, Biome.DESERT, Biome.ALPINE, Biome.PLAINS],
    [Biome.PLAINS, Biome.FOREST, Biome.SWAMP, Biome.FOREST, Biome.ALPINE],
    [Biome.FOREST, Biome.ALPINE, Biome.PLAINS, Biome.SWAMP, Biome.FOREST],
]


def generate_map(rng: SeededRNG, num_nodes: int = 25) -> list[MapNode]:
    """Generate a branching route map with towns and landmarks."""
    pattern = rng.choice(BIOME_PATTERNS)
    town_names = list(TOWN_NAMES)
    rng.shuffle(town_names)
    landmark_names = list(LANDMARK_NAMES)
    rng.shuffle(landmark_names)

    nodes: list[MapNode] = []
    town_idx = 0
    landmark_idx = 0

    for i in range(num_nodes):
        progress = i / (num_nodes - 1)  # 0.0 to 1.0
        biome_idx = int(progress * (len(pattern) - 1))
        biome = pattern[min(biome_idx, len(pattern) - 1)]

        # Towns at roughly every 4-6 nodes
        is_town = (i == 0) or (i == num_nodes - 1) or (i % rng.randint(4, 6) == 0)

        if is_town and town_idx < len(town_names):
            name = town_names[town_idx]
            town_idx += 1
        elif landmark_idx < len(landmark_names):
            name = landmark_names[landmark_idx]
            landmark_idx += 1
        else:
            name = f"Mile {i * 10}"

        # Temperature varies by biome and progress (later = colder if alpine)
        base_temp = {
            Biome.PLAINS: 20, Biome.FOREST: 15, Biome.DESERT: 35,
            Biome.SWAMP: 25, Biome.ALPINE: 5,
        }[biome]
        temp = base_temp + rng.randint(-5, 5)

        node = MapNode(
            node_id=f"node_{i:02d}",
            name=name,
            biome=biome,
            hazard=rng.randint(1, 8) + (2 if biome in (Biome.ALPINE, Biome.SWAMP) else 0),
            water_available=biome != Biome.DESERT or rng.random() < 0.3,
            temperature=temp,
            is_town=is_town,
        )
        nodes.append(node)

    # Build connections: mostly linear with occasional branches
    for i in range(len(nodes) - 1):
        dist = rng.randint(8, 25)
        nodes[i].connections.append(nodes[i + 1].node_id)
        nodes[i].distance_to[nodes[i + 1].node_id] = dist

        # Add a branch ~20% of the time (skip to node i+2 if it exists)
        if i + 2 < len(nodes) and rng.random() < 0.2:
            branch_dist = rng.randint(15, 35)
            nodes[i].connections.append(nodes[i + 2].node_id)
            nodes[i].distance_to[nodes[i + 2].node_id] = branch_dist

    _place_caches(rng, nodes)
    return nodes


def _place_caches(
    rng: SeededRNG, nodes: list[MapNode], count: int = 2,
) -> None:
    """Place supply caches on non-town nodes, biased toward high-hazard.

    Caches reward risk — they appear on dangerous nodes, not free lunch.
    """
    eligible = [n for n in nodes[1:-1] if not n.is_town]
    if len(eligible) < count:
        count = len(eligible)
    if count <= 0:
        return

    # Weight by hazard — higher hazard = more likely to get a cache
    weights = [float(n.hazard + 1) for n in eligible]
    cache_nodes: list[MapNode] = []
    for _ in range(count):
        if not eligible:
            break
        pick = rng.weighted_choice(eligible, weights)
        idx = eligible.index(pick)
        cache_nodes.append(pick)
        eligible.pop(idx)
        weights.pop(idx)

    cache_templates = [
        {"food": 6, "water": 5},
        {"parts": 1, "cloth": 2},
        {"food": 4, "meds": 1, "ammo": 3},
        {"water": 6, "salt": 2},
        {"food": 5, "parts": 1, "rope": 1},
    ]

    for node in cache_nodes:
        template = rng.choice(cache_templates)
        node.cache_supplies = dict(template)


def generate_weather(rng: SeededRNG, biome: Biome, day: int) -> Weather:
    """Generate weather for a given biome and day."""
    weights: dict[Weather, float] = {
        Weather.CLEAR: 3.0,
        Weather.OVERCAST: 2.0,
        Weather.RAIN: 1.5,
        Weather.STORM: 0.5,
        Weather.FOG: 1.0,
        Weather.SNOW: 0.0,
        Weather.HOT: 0.0,
    }

    if biome == Biome.DESERT:
        weights[Weather.HOT] = 4.0
        weights[Weather.RAIN] = 0.2
        weights[Weather.FOG] = 0.1
    elif biome == Biome.SWAMP:
        weights[Weather.FOG] = 3.0
        weights[Weather.RAIN] = 2.5
    elif biome == Biome.ALPINE:
        weights[Weather.SNOW] = 2.0
        weights[Weather.STORM] = 1.5
        weights[Weather.CLEAR] = 1.5
    elif biome == Biome.FOREST:
        weights[Weather.FOG] = 2.0
        weights[Weather.RAIN] = 2.0

    # Late-season gets worse weather
    if day > 20:
        weights[Weather.STORM] += 1.0
        weights[Weather.SNOW] += 0.5

    items = list(weights.keys())
    w = list(weights.values())
    return rng.weighted_choice(items, w)


def generate_party(rng: SeededRNG, size: int = 4) -> PartyState:
    """Generate a starting party with random traits."""
    names = list(FIRST_NAMES)
    rng.shuffle(names)
    all_traits = list(Trait)

    members = []
    for i in range(size):
        num_traits = rng.randint(1, 2)
        traits = rng.sample(all_traits, num_traits)
        member = PartyMember(
            name=names[i],
            health=rng.randint(80, 100),
            condition=Condition.HEALTHY,
            traits=traits,
        )
        members.append(member)

    return PartyState(members=members, morale=70)


def pick_twists(rng: SeededRNG) -> list[TwistModifier]:
    """Pick 1-2 twist modifiers for the run."""
    all_twists = list(TwistModifier)
    count = rng.randint(1, 2)
    return rng.sample(all_twists, count)


def create_new_run(
    seed: int | None = None,
    gm_profile: GMProfile = GMProfile.FIRESIDE,
    weirdness_level: int = 2,
) -> RunState:
    """Create a complete new game run."""
    import time

    if seed is None:
        seed = int(time.time()) % 1_000_000

    rng = SeededRNG(seed)
    nodes = generate_map(rng)
    party = generate_party(rng)
    twists = pick_twists(rng)

    # Assign doctrine + taboo from seed
    doctrine = rng.choice(list(Doctrine))
    taboo = rng.choice(list(Taboo))

    # First connection distance
    first_dist = nodes[0].distance_to.get(nodes[1].node_id, 15) if len(nodes) > 1 else 0

    # Total distance estimate
    total = 0
    for node in nodes[:-1]:
        if node.connections:
            total += node.distance_to.get(node.connections[0], 15)

    # Apply doctrine modifiers at creation time
    wagon = WagonState()
    mods = DOCTRINE_MODIFIERS.get(doctrine.value, {})
    if "capacity_mult" in mods:
        wagon.capacity = int(wagon.capacity * mods["capacity_mult"])

    run = RunState(
        run_id=RunState.generate_run_id(seed),
        seed=seed,
        day=1,
        time_of_day=TimeOfDay.MORNING,
        location_id=nodes[0].node_id,
        destination_id=nodes[1].node_id if len(nodes) > 1 else nodes[0].node_id,
        distance_remaining=first_dist,
        total_distance=total,
        distance_traveled=0,
        weirdness_level=weirdness_level,
        uncanny_tokens=2,
        gm_profile=gm_profile,
        twists=twists,
        party=party,
        wagon=wagon,
        supplies=SuppliesState(items=dict(DEFAULT_SUPPLIES)),
        map_nodes=nodes,
        rng_counter=rng.counter,
        doctrine=doctrine.value,
        taboo=taboo.value,
    )
    return run
