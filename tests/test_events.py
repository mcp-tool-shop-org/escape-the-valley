"""Tests for event system."""

import logging

from escape_the_valley.event_loader import load_json_events
from escape_the_valley.events import (
    EventCategory,
    build_event_library,
    can_spend_uncanny_token,
    resolve_event,
    select_event,
)
from escape_the_valley.models import GMProfile, SeededRNG
from escape_the_valley.worldgen import create_new_run


class TestEventLibrary:
    def test_has_events(self):
        lib = build_event_library()
        assert len(lib) >= 25

    def test_has_all_categories(self):
        lib = build_event_library()
        categories = {e.category for e in lib}
        assert EventCategory.SURVIVAL in categories
        assert EventCategory.HUMAN in categories
        assert EventCategory.FOLKLORE in categories
        assert EventCategory.BIG in categories

    def test_all_have_fallbacks(self):
        lib = build_event_library()
        for event in lib:
            assert event.fallback_narration, f"{event.event_id} missing fallback narration"
            assert event.fallback_choices or event.outcome_templates, (
                f"{event.event_id} missing fallback choices"
            )


class TestEventSelection:
    def test_deterministic(self):
        lib = build_event_library()
        state1 = create_new_run(seed=42)
        state2 = create_new_run(seed=42)
        rng1 = SeededRNG(42)
        rng2 = SeededRNG(42)
        e1 = select_event(state1, rng1, lib)
        e2 = select_event(state2, rng2, lib)
        assert e1.event_id == e2.event_id

    def test_profile_affects_selection(self):
        """Different profiles should produce different event distributions over many draws."""
        lib = build_event_library()

        # Run many selections with each profile
        results: dict[str, dict[str, int]] = {}
        for profile in [GMProfile.CHRONICLER, GMProfile.LANTERN]:
            state = create_new_run(seed=42, gm_profile=profile)
            rng = SeededRNG(100)
            counts: dict[str, int] = {}
            for _ in range(100):
                event = select_event(state, rng, lib)
                cat = event.category.value
                counts[cat] = counts.get(cat, 0) + 1
            results[profile.value] = counts

        # Lantern should have more folklore than Chronicler
        chron_folklore = results.get("chronicler", {}).get("folklore", 0)
        lantern_folklore = results.get("lantern", {}).get("folklore", 0)
        assert lantern_folklore >= chron_folklore


class TestUncannyTokens:
    def test_chronicler_restrictive(self):
        state = create_new_run(seed=42, gm_profile=GMProfile.CHRONICLER)
        lib = build_event_library()
        uncanny_events = [e for e in lib if e.costs_uncanny_token]
        low_sev = [e for e in uncanny_events if e.severity == "low"]

        # Chronicler should not spend tokens on low-severity uncanny
        for event in low_sev:
            assert not can_spend_uncanny_token(state, event)

    def test_no_tokens_blocks(self):
        state = create_new_run(seed=42, gm_profile=GMProfile.LANTERN)
        state.uncanny_tokens = 0
        lib = build_event_library()
        uncanny_events = [e for e in lib if e.costs_uncanny_token]
        for event in uncanny_events:
            assert not can_spend_uncanny_token(state, event)

    def test_weirdness_gate_blocks_below_level_2(self):
        """ENG-A-02 / D2: no uncanny-token spend below weirdness_level 2,
        even on the most permissive profile with tokens in hand."""
        lib = build_event_library()
        uncanny_events = [e for e in lib if e.costs_uncanny_token]
        assert uncanny_events  # sanity — there are token-costing events

        for level in (0, 1):
            state = create_new_run(seed=42, gm_profile=GMProfile.LANTERN)
            state.weirdness_level = level
            state.uncanny_tokens = 2  # tokens ARE available
            for event in uncanny_events:
                assert not can_spend_uncanny_token(state, event), (
                    f"{event.event_id} spent a token at weirdness {level}"
                )

    def test_weirdness_gate_allows_at_level_2_per_profile(self):
        """At weirdness_level >= 2 the existing profile rules govern again:
        Lantern can spend on a tagged uncanny event; Chronicler cannot spend
        on a low-severity one."""
        lib = build_event_library()
        uncanny_events = [e for e in lib if e.costs_uncanny_token]
        tagged = [e for e in uncanny_events if "folklore:uncanny" in e.tags]
        assert tagged

        lantern = create_new_run(seed=42, gm_profile=GMProfile.LANTERN)
        lantern.weirdness_level = 2
        lantern.uncanny_tokens = 2
        # Lantern allows any folklore:uncanny event once gated open.
        assert can_spend_uncanny_token(lantern, tagged[0])

        # Chronicler still refuses low-severity uncanny even at level 2.
        chron = create_new_run(seed=42, gm_profile=GMProfile.CHRONICLER)
        chron.weirdness_level = 2
        chron.uncanny_tokens = 2
        low_sev = [e for e in uncanny_events if e.severity == "low"]
        for event in low_sev:
            assert not can_spend_uncanny_token(chron, event)

    # ── F-001af851 / F-5f77f476 regression: JSON-loaded uncanny events must
    # behave like the 5 hand-authored ones (reachable + actually spend a
    # token), not silently fall back to unreachable-for-Chronicler / free. ──

    def test_json_band3_events_carry_uncanny_tag(self):
        """F-001af851: every JSON-loaded band>=3 event must carry the literal
        'folklore:uncanny' tag that can_spend_uncanny_token() checks, matching
        the FolkloreType.UNCANNY the loader already set on the typed field.
        Before the fix, the loader populated the enum but never the tag, so
        the profile-gating check in events.py never matched any JSON event."""
        json_events = load_json_events()
        band3 = [e for e in json_events if e.costs_uncanny_token]
        assert band3, "sanity: the data file must still have band>=3 content"
        for e in band3:
            assert "folklore:uncanny" in e.tags, (
                f"{e.event_id}: costs_uncanny_token but missing the "
                "'folklore:uncanny' tag can_spend_uncanny_token() checks"
            )

    def test_json_uncanny_event_reachable_by_chronicler(self):
        """F-001af851: Chronicler's reachability check is
        `severity in (medium, high) and 'folklore:uncanny' in tags`. Before
        the fix this was always False for JSON content (no JSON event ever
        carried the literal tag), so Chronicler could NEVER select any of the
        15 band>=3 JSON events, under any state."""
        json_events = load_json_events()
        band3 = [e for e in json_events if e.costs_uncanny_token]

        state = create_new_run(seed=42, gm_profile=GMProfile.CHRONICLER)
        state.weirdness_level = 2
        state.uncanny_tokens = 2
        reachable = [e for e in band3 if can_spend_uncanny_token(state, e)]
        assert reachable, "no JSON-loaded uncanny event is reachable by Chronicler"

    def test_json_uncanny_event_spends_token_on_resolve(self):
        """F-5f77f476: resolving a choice on a JSON-loaded band>=3 event
        through the real resolve_event() must decrement state.uncanny_tokens
        by exactly 1 — the absence of this test is what let 15 of ~20
        uncanny-gated events resolve for free."""
        json_events = load_json_events()
        band3 = [e for e in json_events if e.costs_uncanny_token]
        assert band3
        event = band3[0]
        choice_id = next(iter(event.outcome_templates))

        state = create_new_run(seed=42, gm_profile=GMProfile.LANTERN)
        state.uncanny_tokens = 2
        rng = SeededRNG(1)

        resolve_event(state, event, choice_id, rng)

        assert state.uncanny_tokens == 1

    def test_all_json_band3_outcomes_flagged_to_spend(self):
        """Every choice (not just the first) on every band>=3 JSON event must
        carry the spend flag — _convert_profile() runs once per choice, so a
        partial fix could flag choice A but miss B/C/D."""
        json_events = load_json_events()
        band3 = [e for e in json_events if e.costs_uncanny_token]
        assert band3
        for e in band3:
            for cid, outcome in e.outcome_templates.items():
                assert "uncanny_token_spent" in outcome.special_flags, (
                    f"{e.event_id} choice {cid} does not spend an uncanny token"
                )


class TestVarietyGuards:
    def test_cooldown_reduces_repeats(self):
        """Tag cooldown should reduce consecutive same-family events."""
        lib = build_event_library()
        state = create_new_run(seed=42)
        rng = SeededRNG(99)

        # Draw 50 events and track primary tag families
        families = []
        for _ in range(50):
            event = select_event(state, rng, lib)
            primary = event.tags[0] if event.tags else event.category.value
            families.append(primary)

        # Count consecutive same-family pairs
        consecutive = sum(
            1 for i in range(1, len(families)) if families[i] == families[i - 1]
        )
        # With cooldown, consecutive repeats should be < 30% of draws
        assert consecutive < len(families) * 0.3, (
            f"Too many consecutive repeats: {consecutive}/{len(families)}"
        )

    def test_cooldown_buffer_grows(self):
        """Selecting events should populate recent_event_tags."""
        lib = build_event_library()
        state = create_new_run(seed=42)
        rng = SeededRNG(42)
        assert len(state.recent_event_tags) == 0
        select_event(state, rng, lib)
        assert len(state.recent_event_tags) == 1

    def test_twist_boosts_river_events(self):
        """FLOOD_YEAR twist should increase river event frequency."""
        from escape_the_valley.models import TwistModifier
        lib = build_event_library()

        # Without twist
        state_no = create_new_run(seed=42)
        rng_no = SeededRNG(100)
        river_no = sum(
            1 for _ in range(200)
            if "river" in select_event(state_no, rng_no, lib).tags
        )

        # With flood year twist
        state_flood = create_new_run(seed=42)
        state_flood.twists = [TwistModifier.FLOOD_YEAR]
        rng_flood = SeededRNG(100)
        river_flood = sum(
            1 for _ in range(200)
            if "river" in select_event(state_flood, rng_flood, lib).tags
        )

        assert river_flood >= river_no


class TestWeatherFilter:
    """ENG-A-06: weather-gated events are excluded when the current weather
    doesn't match and included when it does."""

    def _weather_lib(self):
        from escape_the_valley.events import EventOutcome, EventSkeleton
        from escape_the_valley.models import Weather

        storm_only = EventSkeleton(
            event_id="storm_gated",
            title="Storm Only",
            category=EventCategory.SURVIVAL,
            tags=["weather"],
            base_weight=1.0,
            weather_filter=[Weather.STORM],
            fallback_narration="x",
            outcome_templates={"A": EventOutcome()},
        )
        always = EventSkeleton(
            event_id="always",
            title="Always",
            category=EventCategory.SURVIVAL,
            tags=["survival"],
            base_weight=1.0,
            fallback_narration="x",
            outcome_templates={"A": EventOutcome()},
        )
        return [storm_only, always], storm_only, always

    def test_excluded_when_weather_mismatch(self):
        from escape_the_valley.models import Weather

        lib, storm_only, _ = self._weather_lib()
        state = create_new_run(seed=42)
        # Draw many times in CLEAR weather — the storm-gated event must never win.
        for i in range(50):
            rng = SeededRNG(i)
            chosen = select_event(state, rng, lib, weather=Weather.CLEAR)
            assert chosen.event_id != "storm_gated"

    def test_included_when_weather_matches(self):
        from escape_the_valley.models import Weather

        lib, _, _ = self._weather_lib()
        state = create_new_run(seed=42)
        ids = set()
        for i in range(50):
            rng = SeededRNG(i)
            ids.add(select_event(state, rng, lib, weather=Weather.STORM).event_id)
        # In STORM weather the gated event is eligible and should appear.
        assert "storm_gated" in ids

    def test_none_weather_skips_filter(self):
        """Back-compat: callers that pass no weather keep weather-gated events
        eligible (the pre-fix behavior, but now explicit)."""
        from escape_the_valley.models import Weather

        lib, _, _ = self._weather_lib()
        state = create_new_run(seed=42)
        ids = set()
        for i in range(50):
            rng = SeededRNG(i)
            ids.add(select_event(state, rng, lib).event_id)  # weather=None
        assert "storm_gated" in ids
        # And confirm the symmetric case really does gate when weather is set.
        clear_ids = {
            select_event(state, SeededRNG(i), lib, weather=Weather.CLEAR).event_id
            for i in range(50)
        }
        assert "storm_gated" not in clear_ids


class TestSeverityCurve:
    def test_severity_shifts_late_game(self):
        """Late-game state should produce more high-severity events."""
        lib = build_event_library()
        high_sev = [e for e in lib if e.severity == "high"]
        if not high_sev:
            return  # Skip if no high-severity events

        # Early game (progress ~0)
        state_early = create_new_run(seed=42)
        state_early.distance_traveled = 0
        state_early.total_distance = 200
        rng_early = SeededRNG(100)
        early_high = sum(
            1 for _ in range(500)
            if select_event(state_early, rng_early, lib).severity == "high"
        )

        # Late game (progress ~0.9)
        state_late = create_new_run(seed=42)
        state_late.distance_traveled = 180
        state_late.total_distance = 200
        rng_late = SeededRNG(100)
        late_high = sum(
            1 for _ in range(500)
            if select_event(state_late, rng_late, lib).severity == "high"
        )

        assert late_high >= early_high


class TestEventResolution:
    def test_resolve_produces_outcome(self):
        state = create_new_run(seed=42)
        lib = build_event_library()
        rng = SeededRNG(42)
        event = lib[0]  # first event
        if event.fallback_choices:
            choice_id = event.fallback_choices[0].choice_id
            outcome = resolve_event(state, event, choice_id, rng)
            assert outcome is not None


class TestUndefinedChoiceMiss:
    """F-fa99f19f: step_engine.py letters a GM scene's choices positionally
    (A..D, up to 4) independent of how many entries an event's own
    outcome_templates defines. Measured against the live library, 153/260
    events have exactly 2 outcome_templates and 107/260 have 3 -- none has a
    4th -- so a 3- or 4-choice GM scene routinely hands resolve_event() a
    letter ("C" or "D") the underlying event never defined. Pre-fix,
    resolve_event() answered that with a blank EventOutcome() indistinguishable
    from a legitimate zero-delta outcome: the player picks a labelled,
    on-screen option and nothing detectably happens. These tests pin the
    fix: a real, non-blank, logged outcome instead of silence.
    """

    def _lettered_event(self):
        from escape_the_valley.events import EventOutcome, EventSkeleton

        # Mirrors the real shape: two defined outcomes (the majority case in
        # the live library, per the 153/260 measurement above), so "C"/"D"
        # are exactly the kind of GM-offered-but-undefined letters at issue.
        return EventSkeleton(
            event_id="test_two_choice_event",
            title="Test",
            category=EventCategory.SURVIVAL,
            fallback_narration="x",
            outcome_templates={
                "A": EventOutcome(supplies_delta={"food": -1}),
                "B": EventOutcome(time_cost=1),
            },
        )

    def test_offered_letter_beyond_templates_is_not_a_silent_noop(self):
        """A scene offering more choices than the event has outcomes must not
        produce a selectable option that silently does nothing. Fails
        pre-fix: resolve_event() returned EventOutcome() (all-zero, no
        special_flags), identical to a legitimate zero-delta outcome."""
        from escape_the_valley.events import EventOutcome

        state = create_new_run(seed=42)
        event = self._lettered_event()
        rng = SeededRNG(42)

        outcome = resolve_event(state, event, "C", rng)

        assert outcome != EventOutcome(), (
            "an offered-but-undefined choice must not resolve to a blank, "
            "indistinguishable-from-legitimate no-op outcome"
        )
        assert "undefined_choice_miss" in outcome.special_flags

    def test_fourth_letter_beyond_templates_is_also_a_visible_miss(self):
        """Same as above for the 4-choice-scene case ("D") -- the brief's
        measurement found 0/260 events define a 4th outcome at all, so any
        4-choice GM scene hits this on every single event in the library."""
        from escape_the_valley.events import EventOutcome

        state = create_new_run(seed=42)
        event = self._lettered_event()
        rng = SeededRNG(42)

        outcome = resolve_event(state, event, "D", rng)

        assert outcome != EventOutcome()
        assert "undefined_choice_miss" in outcome.special_flags

    def test_visible_miss_does_not_draw_rng(self):
        """The miss path must stay a pure, deterministic fallback -- drawing
        RNG here would shift the seeded draw sequence for every resolve()
        call that follows in the same run, silently breaking replay.
        SeededRNG.counter increments once per draw (models.py), so an
        unchanged counter is a direct proof of zero draws."""
        state = create_new_run(seed=42)
        event = self._lettered_event()
        rng = SeededRNG(42)

        before = rng.counter
        resolve_event(state, event, "Z", rng)

        assert rng.counter == before

    def test_missing_template_choice_logs_a_warning(self, caplog):
        """Matches event_loader.py's established diagnostic style (F-fa99f19f
        fix): a WARNING naming both the event_id and the offending choice_id,
        where previously events.py logged nothing at all."""
        state = create_new_run(seed=42)
        event = self._lettered_event()
        rng = SeededRNG(42)

        with caplog.at_level(logging.WARNING):
            resolve_event(state, event, "D", rng)

        matches = [r for r in caplog.records if "test_two_choice_event" in r.message]
        assert matches, "expected a warning naming the event_id"
        assert any("'D'" in r.message or '"D"' in r.message for r in matches)

    def test_known_choice_ids_are_unaffected(self):
        """Guard against over-fixing: a choice_id that IS in outcome_templates
        must still resolve to that template's real deltas, not the miss."""
        state = create_new_run(seed=42)
        event = self._lettered_event()
        rng = SeededRNG(42)

        outcome = resolve_event(state, event, "B", rng)

        assert outcome.time_cost == 1
        assert "undefined_choice_miss" not in outcome.special_flags


class TestAnimalsHealthOutcome:
    """ENG-A-03: animals_health deltas affect the wagon team, not party health."""

    def test_apply_outcome_changes_wagon_animals_not_party(self):
        from escape_the_valley.events import EventOutcome, apply_outcome

        state = create_new_run(seed=42)
        state.wagon.animals_health = 80
        party_health_before = [m.health for m in state.party.members]

        apply_outcome(state, EventOutcome(animals_health_delta=-25))

        assert state.wagon.animals_health == 55
        # Party member health is untouched.
        assert [m.health for m in state.party.members] == party_health_before

    def test_animals_health_clamped(self):
        from escape_the_valley.events import EventOutcome, apply_outcome

        state = create_new_run(seed=42)
        state.wagon.animals_health = 10
        apply_outcome(state, EventOutcome(animals_health_delta=-50))
        assert state.wagon.animals_health == 0  # clamped low

        state.wagon.animals_health = 90
        apply_outcome(state, EventOutcome(animals_health_delta=50))
        assert state.wagon.animals_health == 100  # clamped high

    def test_resolve_carries_animals_health_from_template(self):
        from escape_the_valley.events import (
            EventOutcome,
            EventSkeleton,
            resolve_event,
        )

        state = create_new_run(seed=42)
        event = EventSkeleton(
            event_id="test_animals",
            title="Test",
            category=EventCategory.SURVIVAL,
            outcome_templates={"A": EventOutcome(animals_health_delta=-12)},
        )
        rng = SeededRNG(42)
        outcome = resolve_event(state, event, "A", rng)
        assert outcome.animals_health_delta == -12
