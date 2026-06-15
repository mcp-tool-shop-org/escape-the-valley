"""Tests for GM client — JSON parsing, validation, tone lint, fallback."""

import json

import httpx

from escape_the_valley.events import EventCategory, EventSkeleton
from escape_the_valley.gm import (
    GMClient,
    GMConfig,
    _parse_json,
    _tone_check,
    _validate_scene,
)
from escape_the_valley.worldgen import create_new_run


class _FakeResp:
    """Minimal stand-in for an httpx.Response."""

    def __init__(self, status_code: int = 200, payload: str = ""):
        self.status_code = status_code
        self._payload = payload

    def json(self) -> dict:
        return {"response": self._payload}


def _make_event() -> EventSkeleton:
    return EventSkeleton(
        event_id="river_ford",
        title="The Ford",
        category=EventCategory.SURVIVAL,
        tags=["river", "crossing"],
        fallback_narration="The river runs wide and cold.",
    )


def _valid_scene_json(narration: str) -> str:
    return json.dumps({
        "scene_id": "s1",
        "narration": narration,
        "choices": [
            {"id": "A", "label": "Ford it"},
            {"id": "B", "label": "Wait for morning"},
        ],
    })


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


class TestIsAvailable:
    """gm-A-006 — a reachability probe must resolve, never raise."""

    def test_read_error_returns_false(self, monkeypatch):
        client = GMClient(GMConfig())

        def _raise_read_error(*_a, **_k):
            raise httpx.ReadError("connection reset")

        monkeypatch.setattr(client._client, "get", _raise_read_error)
        # Must return False, not propagate the httpx error.
        assert client.is_available() is False

    def test_disabled_returns_false(self):
        client = GMClient(GMConfig(enabled=False))
        assert client.is_available() is False


class TestOutcomeToneLint:
    """gm-A-001 — outcome narration is tone-linted like scenes."""

    def test_tone_violating_outcome_rejected(self, monkeypatch):
        config = GMConfig(max_retries=1)
        client = GMClient(config)
        state = create_new_run(seed=1)
        event = _make_event()

        calls = {"n": 0}
        bad = json.dumps({
            "scene_id": "s1",
            "outcome_narration": "Lol bro that crossing was totally sus.",
            "callout": "You crossed.",
        })

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(200, bad)

        monkeypatch.setattr(client._client, "post", _fake_post)

        result = client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {"result": "ok"},
        )
        assert result is None  # tone-fail → deterministic fallback
        assert calls["n"] == config.max_retries + 1  # retried then gave up

    def test_clean_outcome_accepted(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        event = _make_event()
        good = json.dumps({
            "scene_id": "s1",
            "outcome_narration": "The water takes the wagon to its knees.",
            "callout": "You crossed, soaked but whole.",
        })
        monkeypatch.setattr(
            client._client, "post", lambda *a, **k: _FakeResp(200, good),
        )
        result = client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {"result": "ok"},
        )
        assert result is not None
        assert "knees" in result.outcome_narration


class TestGMFallbackNeverBricks:
    """A-05 — the load-bearing invariant: GM failure never bricks a run.

    Each failure mode must fall through to None (the engine then uses its
    deterministic non-GM fallback text), after exactly max_retries+1 tries
    for retryable failures.
    """

    def _client_and_world(self, **cfg):
        client = GMClient(GMConfig(**cfg))
        return client, create_new_run(seed=1), _make_event()

    def test_garbage_text_scene_returns_none(self, monkeypatch):
        client, state, event = self._client_and_world(max_retries=1)
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(200, "this is not json at all")

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_scene(state, event, "clear skies")
        assert result is None
        assert calls["n"] == 2  # max_retries + 1

    def test_non_200_scene_returns_none(self, monkeypatch):
        client, state, event = self._client_and_world(max_retries=1)
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(500, "")

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_scene(state, event, "clear skies")
        assert result is None
        assert calls["n"] == 2

    def test_tone_violating_scene_returns_none(self, monkeypatch):
        client, state, event = self._client_and_world(max_retries=1)
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(200, _valid_scene_json("Bro that was sus, lol."))

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_scene(state, event, "clear skies")
        assert result is None
        assert calls["n"] == 2

    def test_tone_fail_then_pass_recovers(self, monkeypatch):
        client, state, event = self._client_and_world(max_retries=1)
        responses = [
            _FakeResp(200, _valid_scene_json("Totally sus vibes, bro.")),
            _FakeResp(200, _valid_scene_json("The river runs wide and cold.")),
        ]

        def _fake_post(*_a, **_k):
            return responses.pop(0)

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_scene(state, event, "clear skies")
        assert result is not None  # second attempt is clean
        assert "river" in result.narration

    def test_connection_error_scene_returns_none(self, monkeypatch):
        client, state, event = self._client_and_world(max_retries=1)

        def _raise(*_a, **_k):
            raise httpx.ConnectError("refused")

        monkeypatch.setattr(client._client, "post", _raise)
        assert client.generate_scene(state, event, "clear skies") is None

    def test_garbage_outcome_returns_none(self, monkeypatch):
        client, state, event = self._client_and_world(max_retries=1)
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(200, "absolutely not json")

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {"result": "ok"},
        )
        assert result is None
        assert calls["n"] == 2

    def test_non_200_outcome_returns_none(self, monkeypatch):
        client, state, event = self._client_and_world(max_retries=1)
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(503, "")

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {"result": "ok"},
        )
        assert result is None
        assert calls["n"] == 2

    def test_disabled_returns_none_without_calling(self, monkeypatch):
        client, state, event = self._client_and_world(enabled=False)

        def _boom(*_a, **_k):
            raise AssertionError("should not call the model when disabled")

        monkeypatch.setattr(client._client, "post", _boom)
        assert client.generate_scene(state, event, "clear skies") is None
        assert client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {},
        ) is None
