"""Tests for narration bus — extraction, condensation, guardrails."""

from escape_the_valley.narration import (
    MAX_UTTERANCE_CHARS,
    NarrationType,
    _condense,
    _sanitize,
    extract_narration,
)
from escape_the_valley.step_engine import StepMessages


class TestSanitize:
    def test_strips_windows_paths(self):
        text = "Found data at C:\\Users\\admin\\secrets.txt"
        result = _sanitize(text)
        assert "C:\\" not in result

    def test_strips_unix_paths(self):
        text = "Config at /home/user/.env loaded"
        result = _sanitize(text)
        assert "/home/" not in result

    def test_strips_api_keys(self):
        text = "Using api_key ABC123 for auth"
        result = _sanitize(text)
        assert "api_key" not in result

    def test_truncates_at_sentence_boundary(self):
        long_text = "First sentence. " * 20
        result = _sanitize(long_text)
        assert len(result) <= MAX_UTTERANCE_CHARS
        assert result.endswith(".")

    def test_truncates_with_ellipsis_if_no_period(self):
        long_text = "A" * 300
        result = _sanitize(long_text)
        assert len(result) <= MAX_UTTERANCE_CHARS
        assert result.endswith("...")

    def test_short_text_unchanged(self):
        text = "The wagon creaks forward."
        assert _sanitize(text) == text


class TestCondense:
    def test_title_plus_two_sentences(self):
        title = "Fog Riders"
        narration = (
            "The fog rolls in thick. Visibility drops to nothing. "
            "Two riders appear. They raise a hand."
        )
        result = _condense(title, narration)
        assert result.startswith("Fog Riders.")
        # Should include first 2 sentences
        assert "fog rolls in thick" in result
        assert "Visibility drops" in result

    def test_single_sentence_narration(self):
        result = _condense("Storm", "Lightning splits the sky.")
        assert result == "Storm. Lightning splits the sky."

    def test_respects_max_length(self):
        title = "Event"
        narration = "Very long sentence. " * 20
        result = _condense(title, narration)
        assert len(result) <= MAX_UTTERANCE_CHARS


class TestExtractNarration:
    def test_scene_open_from_event(self):
        msgs = StepMessages(
            event_title="River Crossing",
            event_narration="The river is swollen. Current looks dangerous.",
        )
        events = extract_narration(msgs, "event")
        scene_events = [e for e in events if e.type == NarrationType.SCENE_OPEN]
        assert len(scene_events) == 1
        assert "River Crossing" in scene_events[0].voice_text

    def test_outcome_only_after_event_resolved(self):
        msgs = StepMessages(
            outcome_title="Safe Passage",
            outcome_narration="The party crosses without incident.",
        )
        # During event phase — no outcome narration
        events = extract_narration(msgs, "event")
        outcomes = [e for e in events if e.type == NarrationType.OUTCOME]
        assert len(outcomes) == 0

        # After event resolved (camp phase)
        events = extract_narration(msgs, "camp")
        outcomes = [e for e in events if e.type == NarrationType.OUTCOME]
        assert len(outcomes) == 1

    def test_no_events_for_plain_travel(self):
        msgs = StepMessages(lines=["Traveled 8 miles."])
        events = extract_narration(msgs, "camp")
        assert len(events) == 0

    def test_arrival_detected(self):
        msgs = StepMessages(lines=["Arrived at Stonecross."])
        events = extract_narration(msgs, "camp")
        arrivals = [e for e in events if e.type == NarrationType.ARRIVAL]
        assert len(arrivals) == 1
        assert "Stonecross" in arrivals[0].voice_text

    def test_game_over_detected(self):
        msgs = StepMessages(lines=["Victory! The journey ends here."])
        events = extract_narration(msgs, "over")
        game_overs = [e for e in events if e.type == NarrationType.GAME_OVER]
        assert len(game_overs) == 1

    def test_cliff_warning_narrated(self):
        msgs = StepMessages()
        warnings = ["Food for one day. After that, the hunger starts."]
        events = extract_narration(msgs, "camp", warnings=warnings)
        warns = [e for e in events if e.type == NarrationType.WARNING]
        assert len(warns) == 1
        assert "one day" in warns[0].voice_text

    def test_non_cliff_warning_skipped(self):
        msgs = StepMessages()
        warnings = ["Low Ammo (3)"]
        events = extract_narration(msgs, "camp", warnings=warnings)
        warns = [e for e in events if e.type == NarrationType.WARNING]
        assert len(warns) == 0

    def test_empty_messages_empty_events(self):
        msgs = StepMessages()
        events = extract_narration(msgs, "camp")
        assert events == []

    def test_outcome_has_pause(self):
        msgs = StepMessages(
            outcome_title="Result",
            outcome_narration="Things worked out.",
        )
        events = extract_narration(msgs, "camp")
        outcomes = [e for e in events if e.type == NarrationType.OUTCOME]
        assert outcomes[0].pause_before_ms > 0
