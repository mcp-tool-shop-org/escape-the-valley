"""Tests for Phase 0 Part 1 bug fixes:
1. Reload at a fork (issue #41): reloading a save taken on fork arrival in
   CAMP must stay in CAMP and match the uninterrupted run across the next 30
   steps (state hash and shown text).
2. Silent deaths (issue #42 + comment): every alive -> dead transition in any
   phase (event, hunt, rest, repair, travel, plus desperate_repair/hard_ration)
   emits one `<name> has died (<cause>).` message, one `died` effect, and one
   `death:<cause>` journal milestone, with zero extra RNG draws.
3. RNG identity: across 200 seeds with no death and no fork reload, per-step
   state hashes equal the `ab628aa` fixture hashes step-for-step.
"""

from __future__ import annotations

import hashlib
import json
import random
from pathlib import Path

from escape_the_valley.adapter import state_to_frame
from escape_the_valley.events import (
    ChoiceTemplate,
    EventCategory,
    EventOutcome,
    EventSkeleton,
)
from escape_the_valley.gm import GMConfig
from escape_the_valley.intent import GamePhase, IntentAction, PlayerIntent
from escape_the_valley.models import (
    Condition,
    RunState,
)
from escape_the_valley.physics import (
    can_abandon_cargo,
    can_desperate_repair,
    can_hard_ration,
)
from escape_the_valley.save import _state_to_dict, load_game, save_game
from escape_the_valley.step_engine import EventChoiceInfo, StepEngine, _find_node
from escape_the_valley.worldgen import create_new_run

FIXTURE_PATH = Path(__file__).parent / "fixtures" / "rng_identity_200_ab628aa.json"
FIXTURE_SHA256 = "a47e9e161dfdbda0f54368d2fcd20a39508739c960df3ab71d42ac466e29d53e"

FORK_SEEDS_22: tuple[int, ...] = (
    2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15, 17, 18, 19, 20, 21, 22, 23, 24,
)


def _deterministic_run(seed: int) -> RunState:
    st = create_new_run(seed=seed)
    st.run_id = f"{st.run_id.split('-')[0]}-00000000"
    return st


def _canonical_state_hash(state: RunState) -> str:
    d = _state_to_dict(state)
    raw = json.dumps(d, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def _shown_text_snapshot(eng: StepEngine) -> dict[str, object]:
    frame = state_to_frame(eng)
    return {
        "phase": eng.phase.value,
        "lines": list(eng.msgs.lines),
        "event_title": eng.msgs.event_title,
        "event_narration": eng.msgs.event_narration,
        "outcome_title": eng.msgs.outcome_title,
        "outcome_narration": eng.msgs.outcome_narration,
        "route_options": [
            (r.node_id, r.name, r.distance) for r in eng.msgs.route_options
        ],
        "frame_narration": frame.narration,
        "frame_prompt_title": frame.prompt_title,
        "frame_choices": [
            (c.id, c.label, c.risk_hint, c.cost_hint) for c in frame.choices
        ],
    }


def _fork_policy_intent(eng: StepEngine, prng: random.Random) -> PlayerIntent:
    if eng.phase == GamePhase.EVENT:
        choices = [c.id for c in eng._pending_event_choices] or ["A"]
        return PlayerIntent(IntentAction.CHOOSE, choice_id=prng.choice(choices))
    if eng.phase == GamePhase.ROUTE:
        opts = ["A", "B", "C", "D"][: max(1, len(eng._pending_routes))]
        return PlayerIntent(IntentAction.CHOOSE, choice_id=prng.choice(opts))
    if eng.state.supplies.food < 12 and eng.state.supplies.ammo > 0:
        return PlayerIntent(IntentAction.HUNT)
    if eng.state.wagon.condition < 50 and eng.state.supplies.parts > 0:
        return PlayerIntent(IntentAction.REPAIR)
    if any(m.is_alive() and m.health < 45 for m in eng.state.party.members):
        return PlayerIntent(IntentAction.REST)
    return PlayerIntent(IntentAction.TRAVEL)


def _rng_identity_policy_intent(eng: StepEngine, prng: random.Random) -> PlayerIntent:
    if eng.phase == GamePhase.EVENT:
        choices = [c.id for c in eng._pending_event_choices] or ["A"]
        return PlayerIntent(IntentAction.CHOOSE, choice_id=prng.choice(choices))
    if eng.phase == GamePhase.ROUTE:
        opts = ["A", "B", "C", "D"][: max(1, len(eng._pending_routes))]
        return PlayerIntent(IntentAction.CHOOSE, choice_id=prng.choice(opts))
    r = prng.random()
    if can_desperate_repair(eng.state) and r < 0.35:
        return PlayerIntent(IntentAction.DESPERATE_REPAIR)
    if can_hard_ration(eng.state) and r < 0.50:
        return PlayerIntent(IntentAction.HARD_RATION)
    if can_abandon_cargo(eng.state) and r < 0.55:
        return PlayerIntent(IntentAction.ABANDON_CARGO)
    if r < 0.68:
        return PlayerIntent(IntentAction.TRAVEL)
    if r < 0.78:
        return PlayerIntent(IntentAction.HUNT)
    if r < 0.88:
        return PlayerIntent(IntentAction.REST)
    if r < 0.96:
        return PlayerIntent(IntentAction.REPAIR)
    pace = prng.choice(["slow", "steady", "hard"])
    return PlayerIntent(IntentAction.CHANGE_PACE, pace=pace)


def _capture_death_effects(eng: StepEngine) -> list[dict]:
    """Hook `_record_death_milestones` so we capture emitted `died` effects both
    before and after StepMessages.effects exists."""
    captured: list[dict] = []
    orig = eng._record_death_milestones

    def _wrapped(effects: list[dict]) -> None:
        for eff in effects:
            if eff.get("type") == "died":
                captured.append(dict(eff))
        orig(effects)

    eng._record_death_milestones = _wrapped  # type: ignore[method-assign]
    return captured


# ── 1. Bug 1: Reload at a fork (#41) ──────────────────────────────────


def test_fork_arrival_reload_fails_at_ab628aa_seed_2(tmp_path: Path) -> None:
    """Issue #41 regression test: arriving at a fork on seed 2 leaves
    phase == CAMP with distance_remaining == 0. Reloading a save taken at
    that fork arrival must stay in CAMP (not flip to ROUTE in
    _check_initial_phase) and produce the exact same next step as the
    uninterrupted run."""
    st = _deterministic_run(2)
    eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
    prng = random.Random(2 ^ 0xA5A5A5A5)

    for _ in range(8):
        eng.step(_fork_policy_intent(eng, prng))

    node = _find_node(eng.state)
    assert node is not None and len(node.connections) > 1
    assert eng.state.distance_remaining == 0
    assert eng.phase == GamePhase.CAMP

    save_game(eng.state, tmp_path)
    loaded = load_game(tmp_path)
    assert loaded is not None
    reloaded = StepEngine(loaded, GMConfig(enabled=False), autosave=False)

    assert reloaded.phase == GamePhase.CAMP
    assert reloaded.state.rng_counter == eng.state.rng_counter

    prng_rel = random.Random(2 ^ 0xA5A5A5A5)
    prng_rel.setstate(prng.getstate())

    for step_idx in range(1, 31):
        intent_u = _fork_policy_intent(eng, prng)
        intent_r = _fork_policy_intent(reloaded, prng_rel)
        assert intent_r == intent_u, f"step {step_idx}: intent diverged"
        eng.step(intent_u)
        reloaded.step(intent_r)
        assert _canonical_state_hash(reloaded.state) == _canonical_state_hash(
            eng.state
        ), f"step {step_idx}: state hash diverged"
        assert _shown_text_snapshot(reloaded) == _shown_text_snapshot(
            eng
        ), f"step {step_idx}: shown text diverged"


def test_fork_arrival_reload_20_seeds_next_30_steps_equal_uninterrupted(
    tmp_path: Path,
) -> None:
    """For 22 seeds (>= 20) that reach a fork in CAMP, save at the fork
    arrival and reload; the reloaded run's next 30 steps (state hash and
    shown text) must equal the uninterrupted run's."""
    assert len(FORK_SEEDS_22) >= 20
    for seed in FORK_SEEDS_22:
        st = _deterministic_run(seed)
        eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
        prng = random.Random(seed ^ 0xA5A5A5A5)

        reached_fork = False
        for _ in range(80):
            if eng.phase == GamePhase.GAME_OVER:
                break
            eng.step(_fork_policy_intent(eng, prng))
            node = _find_node(eng.state)
            if (
                eng.phase == GamePhase.CAMP
                and node is not None
                and len(node.connections) > 1
                and eng.state.distance_remaining <= 0
            ):
                reached_fork = True
                break

        assert reached_fork, f"seed {seed} did not reach fork in CAMP"

        seed_dir = tmp_path / f"seed_{seed}"
        seed_dir.mkdir(parents=True, exist_ok=True)
        save_game(eng.state, seed_dir)
        loaded = load_game(seed_dir)
        assert loaded is not None
        reloaded = StepEngine(loaded, GMConfig(enabled=False), autosave=False)

        assert reloaded.phase == eng.phase == GamePhase.CAMP
        assert reloaded.state.rng_counter == eng.state.rng_counter

        prng_rel = random.Random(seed ^ 0xA5A5A5A5)
        prng_rel.setstate(prng.getstate())

        for step_idx in range(1, 31):
            intent_u = _fork_policy_intent(eng, prng)
            intent_r = _fork_policy_intent(reloaded, prng_rel)
            assert intent_r == intent_u, f"seed {seed} step {step_idx}: intent diverged"
            eng.step(intent_u)
            reloaded.step(intent_r)
            assert _canonical_state_hash(reloaded.state) == _canonical_state_hash(
                eng.state
            ), f"seed {seed} step {step_idx}: state hash diverged"
            assert _shown_text_snapshot(reloaded) == _shown_text_snapshot(
                eng
            ), f"seed {seed} step {step_idx}: shown text diverged"


# ── 2. Bug 2: Silent deaths (#42 + comment) ──────────────────────────


def _assert_single_death_recorded(
    eng: StepEngine,
    msgs,
    captured_effects: list[dict],
    member_name: str,
    expected_cause: str,
) -> None:
    member = next(m for m in eng.state.party.members if m.name == member_name)
    assert not member.is_alive()
    assert member.death_cause == expected_cause

    expected_line = f"{member_name} has died ({expected_cause})."
    death_lines = [line for line in msgs.lines if " has died (" in line]
    assert death_lines == [expected_line]

    assert captured_effects == [
        {"member": member_name, "type": "died", "cause": expected_cause}
    ]
    if hasattr(msgs, "effects"):
        msg_deaths = [e for e in msgs.effects if e.get("type") == "died"]
        assert msg_deaths == [
            {"member": member_name, "type": "died", "cause": expected_cause}
        ]

    death_milestones = [
        j for j in eng.state.journal if j.event_id.startswith("death:")
    ]
    assert len(death_milestones) == 1
    assert death_milestones[0].event_id == f"death:{expected_cause.lower()}"
    assert death_milestones[0].scene_title == f"Death: {member_name}"


def test_death_in_travel_phase() -> None:
    """Travel phase death (baseline working at ab628aa): starvation during
    TRAVEL emits one message, one `died` effect, and one milestone."""
    st = _deterministic_run(42)
    victim = st.party.members[0]
    assert victim.name == "Ruth"
    victim.health = 5
    for m in st.party.members[1:]:
        m.health = 100
    st.supplies.food = 0
    st.supplies.water = 50

    eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
    captured = _capture_death_effects(eng)

    msgs = eng.step(PlayerIntent(IntentAction.TRAVEL))
    _assert_single_death_recorded(eng, msgs, captured, "Ruth", "Starvation")


def test_death_in_event_phase() -> None:
    """Event phase death (#42): lethal health_delta inside an event choice
    must emit one message, one `died` effect, and one milestone."""
    st = _deterministic_run(42)
    victim = st.party.members[0]
    assert victim.name == "Ruth"
    victim.health = 10
    for m in st.party.members[1:]:
        m.health = 100
    st.supplies.food = 50
    st.supplies.water = 50

    eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
    event = EventSkeleton(
        event_id="test_lethal_ambush",
        title="Rockfall",
        category=EventCategory.SURVIVAL,
        severity="high",
        tags=["danger"],
        fallback_narration="Boulders crash down the slope.",
        fallback_choices=[
            ChoiceTemplate(
                choice_id="A",
                label="Shield the wagon",
                action="CHOOSE",
                style="BOLD",
                risk_hint="Dangerous",
                cost_hint="Health",
            )
        ],
        outcome_templates={
            "A": EventOutcome(
                supplies_delta={},
                health_delta=-15,
                wagon_delta=0,
                morale_delta=-5,
            )
        },
    )
    eng.phase = GamePhase.EVENT
    eng._pending_event = event
    eng._pending_event_choices = [
        EventChoiceInfo(
            id="A",
            label="Shield the wagon",
            risk_hint="Dangerous",
            cost_hint="Health",
        )
    ]
    eng._pending_event_title = event.title
    eng._pending_event_narration = event.fallback_narration

    captured = _capture_death_effects(eng)
    assert eng.rng.counter == 198

    msgs = eng.step(PlayerIntent(IntentAction.CHOOSE, choice_id="A"))
    _assert_single_death_recorded(eng, msgs, captured, "Ruth", "Injury")
    assert eng.rng.counter == 199


def test_death_in_hunt_phase() -> None:
    """Hunt phase death (#42): a failed hunt that fatally injures a low-HP
    member must emit one message, one `died` effect, and one milestone."""
    st = _deterministic_run(42)
    victim = st.party.members[0]
    assert victim.name == "Ruth"
    victim.health = 5
    for m in st.party.members[1:]:
        m.health = 100
    st.supplies.ammo = 10
    st.supplies.food = 50
    st.supplies.water = 50

    eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
    captured = _capture_death_effects(eng)

    eng.rng.seed = 15
    eng.rng._rng.seed(15)
    eng.rng.counter = 0

    msgs = eng.step(PlayerIntent(IntentAction.HUNT))
    _assert_single_death_recorded(eng, msgs, captured, "Ruth", "Injury")
    assert eng.rng.counter == 4


def test_death_in_rest_phase(monkeypatch) -> None:
    """Rest phase death (#42): a fatal health drop during REST must emit one
    message, one `died` effect, and one milestone with no extra RNG draws."""
    import escape_the_valley.step_engine as step_engine_mod
    from escape_the_valley.physics import rest_day as real_rest_day

    st = _deterministic_run(42)
    victim = st.party.members[0]
    assert victim.name == "Ruth"
    victim.health = 5
    victim.condition = Condition.SICK
    for m in st.party.members[1:]:
        m.health = 100
    st.supplies.food = 50
    st.supplies.water = 50

    def _lethal_rest_day(state: RunState, rng) -> list[dict]:
        effs = real_rest_day(state, rng)
        state.party.members[0].health = 0
        state.party.members[0].condition = Condition.SICK
        return effs

    monkeypatch.setattr(step_engine_mod, "rest_day", _lethal_rest_day)

    eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
    captured = _capture_death_effects(eng)

    msgs = eng.step(PlayerIntent(IntentAction.REST))
    _assert_single_death_recorded(eng, msgs, captured, "Ruth", "Disease")


def test_death_in_repair_phase(monkeypatch) -> None:
    """Repair phase death (#42): a fatal health drop during REPAIR must emit
    one message, one `died` effect, and one milestone with no extra RNG draws."""
    import escape_the_valley.step_engine as step_engine_mod
    from escape_the_valley.physics import attempt_repair as real_attempt_repair

    st = _deterministic_run(42)
    st.wagon.condition = 40
    st.supplies.parts = 3
    st.supplies.food = 50
    st.supplies.water = 50
    victim = st.party.members[0]
    assert victim.name == "Ruth"
    victim.health = 5
    victim.condition = Condition.EXHAUSTED
    for m in st.party.members[1:]:
        m.health = 100

    def _lethal_attempt_repair(state: RunState) -> dict[str, int]:
        deltas = real_attempt_repair(state)
        state.party.members[0].health = 0
        return deltas

    monkeypatch.setattr(step_engine_mod, "attempt_repair", _lethal_attempt_repair)

    eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
    captured = _capture_death_effects(eng)
    assert eng.rng.counter == 198

    msgs = eng.step(PlayerIntent(IntentAction.REPAIR))
    _assert_single_death_recorded(eng, msgs, captured, "Ruth", "Exhaustion")
    assert eng.rng.counter == 198


def test_death_in_desperate_repair_and_hard_ration() -> None:
    """Escape-valve deaths (#42): both DESPERATE_REPAIR (failed repair injury)
    and HARD_RATION (-5 HP cost) can kill a low-HP member and must emit one
    message, one `died` effect, and one milestone."""
    # 1. DESPERATE_REPAIR fatal injury
    st = _deterministic_run(42)
    st.wagon.condition = 20
    st.supplies.parts = 0
    st.supplies.food = 50
    st.supplies.water = 50
    victim = st.party.members[0]
    assert victim.name == "Ruth"
    victim.health = 5
    for m in st.party.members[1:]:
        m.health = 100

    eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
    eng.rng.seed = 2
    eng.rng._rng.seed(2)
    eng.rng.counter = 0
    captured = _capture_death_effects(eng)

    msgs = eng.step(PlayerIntent(IntentAction.DESPERATE_REPAIR))
    _assert_single_death_recorded(eng, msgs, captured, "Ruth", "Injury")
    assert eng.rng.counter == 3

    # 2. HARD_RATION fatal -5 HP cost
    st2 = _deterministic_run(42)
    st2.supplies.food = 4
    st2.supplies.water = 50
    victim2 = st2.party.members[0]
    assert victim2.name == "Ruth"
    victim2.health = 5
    victim2.condition = Condition.EXHAUSTED
    for m in st2.party.members[1:]:
        m.health = 100

    eng2 = StepEngine(st2, GMConfig(enabled=False), autosave=False)
    captured2 = _capture_death_effects(eng2)
    assert eng2.rng.counter == 198

    msgs2 = eng2.step(PlayerIntent(IntentAction.HARD_RATION))
    _assert_single_death_recorded(eng2, msgs2, captured2, "Ruth", "Exhaustion")
    assert eng2.rng.counter == 198


def test_silent_death_invariant_1000_seeds_x_120_steps() -> None:
    """Sweep over 1,000 seeds × 120 steps (GM off) asserting that on every
    step, every alive -> dead member has its `<name> has died (<cause>).`
    message, `died` effect, and `death:<cause>` milestone."""
    total_deaths = 0
    for seed in range(1, 1001):
        st = _deterministic_run(seed)
        eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
        prng = random.Random(seed ^ 0x5A5A5A5A)
        for step_idx in range(1, 121):
            if eng.phase == GamePhase.GAME_OVER:
                break
            alive_before = {
                m.name for m in eng.state.party.members if m.is_alive()
            }
            milestones_before = [
                j for j in eng.state.journal if j.event_id.startswith("death:")
            ]
            captured = _capture_death_effects(eng)
            intent = _rng_identity_policy_intent(eng, prng)
            msgs = eng.step(intent)

            newly_dead = [
                m
                for m in eng.state.party.members
                if m.name in alive_before and not m.is_alive()
            ]
            if not newly_dead:
                continue

            total_deaths += len(newly_dead)
            milestones_after = [
                j for j in eng.state.journal if j.event_id.startswith("death:")
            ]
            new_milestones = milestones_after[len(milestones_before) :]

            assert len(new_milestones) == len(newly_dead), (
                f"seed {seed} step {step_idx} ({intent.action.value}): "
                f"{len(newly_dead)} died ({[m.name for m in newly_dead]}) "
                f"but {len(new_milestones)} death milestones recorded"
            )
            assert len(captured) == len(newly_dead), (
                f"seed {seed} step {step_idx} ({intent.action.value}): "
                f"expected {len(newly_dead)} `died` effects, got {captured}"
            )
            for m in newly_dead:
                assert m.death_cause, (
                    f"seed {seed} step {step_idx}: {m.name} died with empty death_cause"
                )
                expected_line = f"{m.name} has died ({m.death_cause})."
                assert msgs.lines.count(expected_line) == 1, (
                    f"seed {seed} step {step_idx} ({intent.action.value}): "
                    f"expected 1 {expected_line!r} in {msgs.lines}"
                )

    assert total_deaths >= 500


# ── 3. RNG identity across 200 no-death / no-fork-reload seeds ───────


def test_rng_identity_200_seeds_no_death_no_fork_reload() -> None:
    """For 200 seeds with no death and no fork reload, per-step state hashes
    must match the `ab628aa` fixture hashes step-for-step (200/200)."""
    raw_bytes = FIXTURE_PATH.read_bytes()
    actual_sha256 = hashlib.sha256(raw_bytes).hexdigest()
    assert actual_sha256 == FIXTURE_SHA256

    fixture = json.loads(raw_bytes.decode("utf-8"))
    assert fixture["baseline_commit"].startswith("ab628aa")
    assert fixture["seed_count"] == 200
    steps_per_seed = int(fixture["steps_per_seed"])

    matched_seeds = 0
    for entry in fixture["runs"]:
        seed = int(entry["seed"])
        expected_hashes: list[str] = list(entry["step_hashes"])
        assert len(expected_hashes) == steps_per_seed

        st = _deterministic_run(seed)
        eng = StepEngine(st, GMConfig(enabled=False), autosave=False)
        prng = random.Random(seed ^ 0x12345678)

        actual_hashes: list[str] = []
        for step_idx in range(steps_per_seed):
            intent = _rng_identity_policy_intent(eng, prng)
            eng.step(intent)
            assert all(m.is_alive() for m in eng.state.party.members), (
                f"seed {seed} step {step_idx + 1}: unexpected death in no-death seed"
            )
            actual_hashes.append(_canonical_state_hash(eng.state))

        assert actual_hashes == expected_hashes, (
            f"seed {seed}: per-step state hashes diverged from ab628aa"
        )
        matched_seeds += 1

    assert matched_seeds == 200
