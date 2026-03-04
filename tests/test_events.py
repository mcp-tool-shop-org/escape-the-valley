"""Tests for event system."""

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
        state = create_new_run(seed=42)
        lib = build_event_library()
        rng1 = SeededRNG(42)
        rng2 = SeededRNG(42)
        e1 = select_event(state, rng1, lib)
        e2 = select_event(state, rng2, lib)
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
