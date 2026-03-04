"""Tests for GM client — JSON parsing, validation, tone lint."""

from escape_the_valley.gm import _parse_json, _tone_check, _validate_scene


class TestJsonParsing:
    def test_clean_json(self):
        text = '{"scene_id": "test", "narration": "hello", "choices": [{"id": "A", "label": "ok"}]}'
        result = _parse_json(text)
        assert result is not None
        assert result["scene_id"] == "test"

    def test_markdown_fenced(self):
        text = '```json\n{"scene_id": "test", "narration": "hello"}\n```'
        result = _parse_json(text)
        assert result is not None
        assert result["scene_id"] == "test"

    def test_text_before_json(self):
        text = 'Here is the scene:\n{"scene_id": "test", "narration": "ok"}'
        result = _parse_json(text)
        assert result is not None

    def test_garbage_returns_none(self):
        assert _parse_json("not json at all") is None

    def test_empty_returns_none(self):
        assert _parse_json("") is None


class TestSceneValidation:
    def test_valid_scene(self):
        data = {
            "scene_id": "test",
            "narration": "Something happened.",
            "choices": [
                {"id": "A", "label": "Do this"},
                {"id": "B", "label": "Do that"},
            ],
        }
        assert _validate_scene(data) is True

    def test_missing_narration(self):
        data = {"choices": [{"id": "A", "label": "ok"}, {"id": "B", "label": "ok"}]}
        assert _validate_scene(data) is False

    def test_too_few_choices(self):
        data = {"narration": "hello", "choices": [{"id": "A", "label": "ok"}]}
        assert _validate_scene(data) is False

    def test_too_many_choices(self):
        data = {
            "narration": "hello",
            "choices": [{"id": c, "label": "ok"} for c in "ABCDE"],
        }
        assert _validate_scene(data) is False

    def test_choice_missing_id(self):
        data = {
            "narration": "hello",
            "choices": [{"label": "ok"}, {"id": "B", "label": "ok"}],
        }
        assert _validate_scene(data) is False


class TestToneLint:
    def test_clean_text_passes(self):
        assert _tone_check("The wind howls through the canyon. Night falls.") is True

    def test_modern_slang_fails(self):
        assert _tone_check("Bro that storm was wild, totally sus vibes") is False

    def test_single_banned_word_fails(self):
        assert _tone_check("The trail ahead looks lowkey dangerous") is False

    def test_case_insensitive(self):
        assert _tone_check("That was a total MEME of a situation") is False

    def test_punchline_plot_twist_rejected(self):
        assert _tone_check("Plot twist: the bridge collapsed anyway.") is False

    def test_punchline_spoiler_alert_rejected(self):
        assert _tone_check("Spoiler alert, nobody survives.") is False

    def test_punchline_wait_for_it_rejected(self):
        assert _tone_check("The mule stops. Wait for it. Then bolts.") is False

    def test_gallows_humor_passes(self):
        assert _tone_check(
            "The river accepts your offering and returns it with interest."
        ) is True

    def test_expanded_banned_words(self):
        assert _tone_check("That was literally the worst crossing") is False
        assert _tone_check("Basically, everyone died") is False
