"""Tests for GM client — JSON parsing, validation, tone lint, fallback."""

import json

import httpx

from escape_the_valley.events import EventCategory, EventSkeleton
from escape_the_valley.gm import (
    PROFILE_HEADERS,
    GMClient,
    GMConfig,
    OutcomeResponse,
    SceneResponse,
    _parse_json,
    _profile_header,
    _tone_check,
    _tone_repair,
    _validate_outcome,
    _validate_scene,
    build_deterministic_epilogue,
)
from escape_the_valley.memory_emitters import validate_gm_cards
from escape_the_valley.models import EndingResult, GMProfile
from escape_the_valley.worldgen import create_new_run


class _FakeResp:
    """Minimal stand-in for an httpx.Response."""

    def __init__(self, status_code: int = 200, payload: str = ""):
        self.status_code = status_code
        self._payload = payload

    @property
    def text(self) -> str:
        return self._payload

    def json(self) -> dict:
        if self.status_code != 200:
            if not self._payload:
                return {}
            try:
                data = json.loads(self._payload)
            except json.JSONDecodeError:
                return {}
            return data if isinstance(data, dict) else {}
        return {"response": self._payload}


class _FakeStream:
    """Context-manager stand-in for httpx.Client.stream() returning NDJSON lines.

    gm-feat-01 — Ollama streams its full JSON object as a sequence of NDJSON
    objects, each carrying a ``response`` text fragment. This fake replays a
    pre-split list of raw fragments so a test can prove on_token receives the
    narration progressively while the final accumulated raw still parses.
    """

    def __init__(
        self,
        fragments: list[str],
        status_code: int = 200,
        error_body: str = "",
    ):
        self._fragments = fragments
        self.status_code = status_code
        self._error_body = error_body

    def __enter__(self):
        return self

    def __exit__(self, *_a):
        return False

    def read(self) -> bytes:  # drained on non-200
        return self._error_body.encode("utf-8")

    @property
    def text(self) -> str:
        return self._error_body

    def json(self) -> dict:
        if not self._error_body:
            return {}
        try:
            data = json.loads(self._error_body)
        except json.JSONDecodeError:
            return {}
        return data if isinstance(data, dict) else {}

    def iter_lines(self):
        for frag in self._fragments:
            yield json.dumps({"response": frag})


def _stream_factory(fragments, status_code=200):
    """Build a monkeypatch replacement for client._client.stream."""

    def _stream(_method, _url, **_kwargs):
        return _FakeStream(fragments, status_code)

    return _stream


def _chunk(text: str, size: int = 3) -> list[str]:
    """Split a string into size-bounded fragments (to exercise partial chunks)."""
    return [text[i : i + size] for i in range(0, len(text), size)] or [""]


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

    def test_list_narration_is_coerced_to_str(self):
        # F-621ef743 — a truthy list used to pass validation then TypeError
        # in _tone_repair. Join array-of-sentences into usable prose.
        data = {
            "scene_id": "s1",
            "narration": ["The ford runs wide.", "The mule will not move."],
            "choices": [
                {"id": "A", "label": "Ford it"},
                {"id": "B", "label": "Wait for morning"},
            ],
        }
        assert _validate_scene(data) is True
        assert data["narration"] == "The ford runs wide. The mule will not move."
        assert isinstance(data["narration"], str)

    def test_non_str_narration_rejected(self):
        data = {
            "narration": {"text": "nope"},
            "choices": [
                {"id": "A", "label": "Ford it"},
                {"id": "B", "label": "Wait"},
            ],
        }
        assert _validate_scene(data) is False

    def test_list_choice_label_is_coerced_to_str(self):
        data = {
            "narration": "The ford runs wide.",
            "choices": [
                {"id": "A", "label": ["Ford it", "now"]},
                {"id": "B", "label": "Wait"},
            ],
        }
        assert _validate_scene(data) is True
        assert data["choices"][0]["label"] == "Ford it now"


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

    def test_grounded_prose_no_longer_over_rejected(self):
        # gm-A-102 — ordinary English words that double as slang were over-
        # rejecting grounded frontier prose. These must now pass.
        assert _tone_check("A goat strayed from the herd.") is True
        assert _tone_check("Snow crowned the cap of the ridge.") is True
        assert _tone_check("They had to slay the lame ox.") is True
        assert _tone_check("The ground was literally frozen.") is True
        assert _tone_check("Basically, the well had run dry.") is True
        assert _tone_check("She let out an oof as the pack landed.") is True

    def test_real_slang_still_rejected(self):
        # The genuine slang bans must remain in force.
        assert _tone_check("Ngl that storm gave me bad vibes, bestie.") is False


class TestIsAvailable:
    """gm-A-006 — a reachability probe must resolve, never raise."""

    def test_read_error_returns_false(self, monkeypatch):
        client = GMClient(GMConfig())

        def _raise_read_error(*_a, **_k):
            raise httpx.ReadError("connection reset")

        monkeypatch.setattr(client._client, "get", _raise_read_error)
        # Must return False, not propagate the httpx error.
        assert client.is_available() is False
        assert client.last_error is not None
        assert "ollama serve" in client.last_error
        assert "--gm-off" in client.last_error

    def test_disabled_returns_false(self):
        client = GMClient(GMConfig(enabled=False))
        assert client.is_available() is False
        assert client.last_error is None


class TestOutcomeToneLint:
    """gm-A-001 — outcome narration is tone-linted like scenes.

    gm-B-04 — a *slang-only* miss is now repaired locally (the banned word is
    stripped and the narration accepted) rather than burning the single retry
    on an identical regeneration. A *punchline* miss is still a hard failure.
    """

    def test_slang_only_outcome_repaired_in_one_call(self, monkeypatch):
        # gm-B-04 — slang words are stripped locally; no retry is spent.
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
        assert result is not None  # repaired, not dropped to fallback
        assert calls["n"] == 1  # the single retry was NOT burned
        # The banned words are gone; the substance survives.
        low = result.outcome_narration.lower()
        assert "lol" not in low.split()
        assert "bro" not in low.split()
        assert "sus" not in low.split()
        assert "crossing" in low
        assert client.stats["successes"] == 1

    def test_punchline_outcome_still_hard_rejected(self, monkeypatch):
        # A structural punchline cannot be word-stripped — still falls back
        # after exhausting retries.
        config = GMConfig(max_retries=1)
        client = GMClient(config)
        state = create_new_run(seed=1)
        event = _make_event()

        calls = {"n": 0}
        bad = json.dumps({
            "scene_id": "s1",
            "outcome_narration": "Plot twist: the ford swallowed the wagon.",
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
        assert client.stats["tone_rejects"] >= 1

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
        assert client.stats["json_rejects"] == 0

    def test_punchline_scene_returns_none(self, monkeypatch):
        # A structural punchline (gm-B-04 hard miss) cannot be repaired and
        # must fall through to None after exhausting retries.
        client, state, event = self._client_and_world(max_retries=1)
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(
                200, _valid_scene_json("Plot twist: the bridge gave way.")
            )

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_scene(state, event, "clear skies")
        assert result is None
        assert calls["n"] == 2

    def test_punchline_fail_then_clean_recovers(self, monkeypatch):
        # A hard punchline miss spends a retry; the second (clean) attempt is
        # accepted. The retried prompt carries a tone nudge (gm-B-04).
        client, state, event = self._client_and_world(max_retries=1)
        responses = [
            _FakeResp(200, _valid_scene_json("Wait for it. The mule bolts.")),
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
        assert client.stats["json_rejects"] == 0

    def test_disabled_returns_none_without_calling(self, monkeypatch):
        client, state, event = self._client_and_world(enabled=False)

        def _boom(*_a, **_k):
            raise AssertionError("should not call the model when disabled")

        monkeypatch.setattr(client._client, "post", _boom)
        assert client.generate_scene(state, event, "clear skies") is None
        assert client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {},
        ) is None


class TestNarrationMustBeStr:
    """F-621ef743 — array-of-sentences narration must not TypeError
    `_tone_repair`. Coerce list/tuple of str; un-coerceable shapes
    increment json_rejects so stats still explain the miss.
    """

    def test_generate_scene_array_narration_succeeds(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        event = _make_event()
        payload = json.dumps({
            "scene_id": "s1",
            "narration": ["The ford runs wide.", "The mule will not move."],
            "choices": [
                {"id": "A", "label": "Ford it"},
                {"id": "B", "label": "Wait for morning"},
            ],
        })
        monkeypatch.setattr(
            client._client, "post", lambda *_a, **_k: _FakeResp(200, payload),
        )
        result = client.generate_scene(state, event, "clear skies")
        assert result is not None
        assert result.narration == "The ford runs wide. The mule will not move."
        assert client.stats["successes"] == 1
        assert client.stats["json_rejects"] == 0
        assert client.stats["tone_rejects"] == 0

    def test_generate_scene_object_narration_json_rejects(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=0))
        state = create_new_run(seed=1)
        event = _make_event()
        payload = json.dumps({
            "scene_id": "s1",
            "narration": {"text": "The ford runs wide."},
            "choices": [
                {"id": "A", "label": "Ford it"},
                {"id": "B", "label": "Wait for morning"},
            ],
        })
        monkeypatch.setattr(
            client._client, "post", lambda *_a, **_k: _FakeResp(200, payload),
        )
        result = client.generate_scene(state, event, "clear skies")
        assert result is None
        assert client.stats["attempts"] == 1
        assert client.stats["successes"] == 0
        assert client.stats["json_rejects"] == 1
        assert client.stats["tone_rejects"] == 0

    def test_generate_outcome_array_narration_succeeds(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=0))
        state = create_new_run(seed=1)
        event = _make_event()
        payload = json.dumps({
            "scene_id": "s1",
            "outcome_narration": [
                "The ford runs wide.",
                "The mule will not move.",
            ],
        })
        monkeypatch.setattr(
            client._client, "post", lambda *_a, **_k: _FakeResp(200, payload),
        )
        result = client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {"result": "ok"},
        )
        assert result is not None
        assert result.outcome_narration == (
            "The ford runs wide. The mule will not move."
        )
        assert client.stats["successes"] == 1
        assert client.stats["json_rejects"] == 0

    def test_generate_outcome_object_narration_json_rejects(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=0))
        state = create_new_run(seed=1)
        event = _make_event()
        payload = json.dumps({
            "scene_id": "s1",
            "outcome_narration": {"text": "The water takes the wagon."},
        })
        monkeypatch.setattr(
            client._client, "post", lambda *_a, **_k: _FakeResp(200, payload),
        )
        result = client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {"result": "ok"},
        )
        assert result is None
        assert client.stats["attempts"] == 1
        assert client.stats["json_rejects"] == 1
        assert client.stats["tone_rejects"] == 0

    def test_validate_outcome_list_is_coerced_to_str(self):
        data = {
            "outcome_narration": [
                "The ford runs wide.",
                "The mule will not move.",
            ],
        }
        assert _validate_outcome(data) is True
        assert data["outcome_narration"] == (
            "The ford runs wide. The mule will not move."
        )


class TestMemoryProposalsNullSafety:
    """F-9b0797f9 — a small local model emitting an explicit JSON `null`
    for the optional `memory_proposals` field (a plausible way to say
    "nothing to add") is schema-legal and passes `_validate_scene`
    unchanged, since that validator never inspects the field. Because this
    happens on the GM's *success* path, none of the retry/tone-fallback
    machinery in `_request_scene`/`_request_outcome` can protect against it
    — `data.get("memory_proposals", [])` returns None (not the default)
    because the key IS present, and the None used to reach
    `validate_gm_cards`'s `proposed[:2]` as a bare TypeError.

    These tests pin both layers of the fix: `from_dict` normalizes null to
    `[]` (gm.py), and `validate_gm_cards` independently tolerates a
    non-list/None `proposed`, non-dict elements, and null `tags`/`entities`
    (memory_emitters.py defense in depth).
    """

    def test_scene_from_dict_null_memory_proposals(self):
        data = {
            "scene_id": "s1",
            "narration": "hello",
            "choices": [{"id": "A", "label": "ok"}],
            "memory_proposals": None,
        }
        result = SceneResponse.from_dict(data)
        assert result.memory_proposals == []

    def test_outcome_from_dict_null_memory_proposals(self):
        data = {
            "scene_id": "s1",
            "outcome_narration": "It happened.",
            "memory_proposals": None,
        }
        result = OutcomeResponse.from_dict(data)
        assert result.memory_proposals == []

    def test_scene_from_dict_missing_key_still_defaults_to_list(self):
        # The key absent entirely (the ordinary case dict.get's default
        # handles) must keep working, not just the explicit-null case.
        data = {"scene_id": "s1", "narration": "hello", "choices": []}
        assert SceneResponse.from_dict(data).memory_proposals == []

    def test_outcome_from_dict_missing_key_still_defaults_to_list(self):
        data = {"scene_id": "s1", "outcome_narration": "It happened."}
        assert OutcomeResponse.from_dict(data).memory_proposals == []

    def test_generate_scene_null_memory_proposals_does_not_crash(self, monkeypatch):
        """End-to-end reproduction of the crash chain via the real GM call.

        A 200 response whose JSON has `"memory_proposals": null` is a GM
        'success' (valid JSON, passes `_validate_scene`), so it must be
        counted as one — but `memory_proposals` on the returned
        SceneResponse must be a list, since that is exactly what
        `validate_gm_cards` receives downstream in step_engine.
        """
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        event = _make_event()

        payload = json.dumps({
            "scene_id": "s1",
            "narration": "The river runs wide and cold.",
            "choices": [
                {"id": "A", "label": "Ford it"},
                {"id": "B", "label": "Wait for morning"},
            ],
            "memory_proposals": None,
        })

        def _fake_post(*_a, **_k):
            return _FakeResp(200, payload)

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_scene(state, event, "clear skies")

        assert result is not None
        assert client.stats["successes"] == 1
        assert result.memory_proposals == []
        # The exact downstream call that used to raise TypeError.
        assert validate_gm_cards(state, result.memory_proposals) == []

    def test_generate_outcome_null_memory_proposals_does_not_crash(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        event = _make_event()

        payload = json.dumps({
            "scene_id": "s1",
            "outcome_narration": "The wagon crossed without incident.",
            "callout": "You made it across.",
            "memory_proposals": None,
        })

        def _fake_post(*_a, **_k):
            return _FakeResp(200, payload)

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {"result": "ok"},
        )

        assert result is not None
        assert client.stats["successes"] == 1
        assert result.memory_proposals == []
        assert validate_gm_cards(state, result.memory_proposals) == []

    def test_validate_gm_cards_none_input_returns_empty(self):
        # Belt-and-suspenders: even if a None ever reached validate_gm_cards
        # directly (bypassing from_dict entirely), it must degrade to []
        # rather than raise.
        state = create_new_run(seed=1)
        assert validate_gm_cards(state, None) == []

    def test_validate_gm_cards_non_dict_elements_skipped(self):
        # Note: validate_gm_cards slices to the first _GM_MAX_PER_PROPOSAL
        # (2) elements before filtering, so both a garbage element and a
        # valid one must sit within that window to exercise "skip garbage,
        # keep the valid one" in a single call.
        state = create_new_run(seed=1)
        proposed = [
            None,
            {"kind": "npc", "title": "The Ferryman", "text": "At the crossing."},
        ]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 1
        assert cards[0].title == "The Ferryman"

    def test_validate_gm_cards_all_non_dict_elements_return_empty(self):
        state = create_new_run(seed=1)
        proposed = [None, "a string, not a proposal", 42]
        assert validate_gm_cards(state, proposed) == []

    def test_validate_gm_cards_null_tags_and_entities(self):
        state = create_new_run(seed=1)
        proposed = [{
            "kind": "omen",
            "title": "Dark Sign",
            "text": "A crow circles thrice.",
            "tags": None,
            "entities": None,
        }]
        cards = validate_gm_cards(state, proposed)
        assert len(cards) == 1
        assert cards[0].tags == []
        assert cards[0].entities == []


class TestSceneAndOutcomeFieldNullSafety:
    """F-7cd35cbf — the F-9b0797f9 fix guarded exactly one field
    (`memory_proposals`) on `SceneResponse.from_dict` / `OutcomeResponse.
    from_dict`, leaving every sibling field on the unguarded
    `data.get(key, default)` form. An explicit JSON `null` for an optional
    field is schema-legal (the key IS present) and is exactly what a small
    local model routinely emits to mean "nothing here" — the same premise
    as F-9b0797f9 — so the two-arg `.get` default never fires and None
    propagates. `tags` is the highest-risk sibling: `_validate_scene` never
    inspects it, so a response with `"tags": null` is counted as a GM
    success and `scene.tags` is `None` — the most ordinary use,
    `",".join(scene.tags)`, then raises TypeError. This class pins the fix
    across every field on both dataclasses, not just the one reported.
    """

    def test_scene_all_optional_fields_null_do_not_propagate_none(self):
        data = {
            "scene_id": None,
            "title": None,
            "narration": None,
            "profile": None,
            "uncanny_intensity": None,
            "choices": None,
            "tags": None,
            "gm_aside": None,
            "memory_proposals": None,
        }
        scene = SceneResponse.from_dict(data)
        assert scene.scene_id == ""
        assert scene.title == ""
        assert scene.narration == ""
        assert scene.profile == ""
        assert scene.uncanny_intensity == "none"
        assert scene.choices == []
        assert scene.tags == []
        assert scene.gm_aside == ""
        assert scene.memory_proposals == []

    def test_scene_null_tags_reproduction_matches_validate_scene_pass(self):
        """The finding's exact repro: a schema-legal scene (passes
        `_validate_scene`) whose `tags` is an explicit JSON null must not
        hand the caller a None where `",".join(...)` — the most ordinary
        possible use of a documented list[str] field — would raise.
        """
        data = {
            "scene_id": "s1",
            "narration": "The river runs wide and cold.",
            "choices": [
                {"id": "A", "label": "Ford it"},
                {"id": "B", "label": "Wait for morning"},
            ],
            "tags": None,
            "title": None,
            "profile": None,
            "gm_aside": None,
        }
        assert _validate_scene(data) is True  # tags isn't inspected at all
        scene = SceneResponse.from_dict(data)
        assert scene.tags == []
        ",".join(scene.tags)  # must not raise TypeError

    def test_scene_choices_null_does_not_propagate_none(self):
        # choices is validated (2-4 entries) before from_dict is normally
        # reached via _request_scene, but from_dict itself must still be
        # safe standalone — it is unit-tested and called directly above.
        data = {"scene_id": "s1", "narration": "hi", "choices": None}
        assert SceneResponse.from_dict(data).choices == []

    def test_outcome_all_optional_fields_null_do_not_propagate_none(self):
        data = {
            "scene_id": None,
            "outcome_title": None,
            "outcome_narration": None,
            "callout": None,
            "oregon_nod": None,
            "memory_proposals": None,
        }
        outcome = OutcomeResponse.from_dict(data)
        assert outcome.scene_id == ""
        assert outcome.outcome_title == ""
        assert outcome.outcome_narration == ""
        assert outcome.callout == ""
        assert outcome.oregon_nod == ""
        assert outcome.memory_proposals == []

    def test_scene_missing_keys_still_default_correctly(self):
        # The ordinary "key absent" case (plain dict.get default) must keep
        # working exactly as before — only the explicit-null case was ever
        # broken.
        scene = SceneResponse.from_dict({})
        assert scene.scene_id == ""
        assert scene.title == ""
        assert scene.narration == ""
        assert scene.profile == ""
        assert scene.uncanny_intensity == "none"
        assert scene.choices == []
        assert scene.tags == []
        assert scene.gm_aside == ""

    def test_outcome_missing_keys_still_default_correctly(self):
        outcome = OutcomeResponse.from_dict({})
        assert outcome.scene_id == ""
        assert outcome.outcome_title == ""
        assert outcome.outcome_narration == ""
        assert outcome.callout == ""
        assert outcome.oregon_nod == ""


class TestToneRepair:
    """gm-B-04 — local repair of slang-only misses; hard fail on punchlines."""

    def test_clean_text_unchanged(self):
        text = "The wind howls through the canyon."
        assert _tone_repair(text) == text

    def test_slang_stripped_and_substance_kept(self):
        repaired = _tone_repair("Bro the river was lowkey dangerous.")
        assert repaired is not None
        words = repaired.lower().split()
        assert "bro" not in words
        assert "lowkey" not in words
        assert "river" in repaired.lower()
        assert "dangerous" in repaired.lower()

    def test_punchline_returns_none(self):
        assert _tone_repair("Plot twist: the bridge collapsed.") is None

    def test_all_slang_leaves_nothing_returns_none(self):
        # Stripping every word leaves no substance — treat as a hard miss.
        assert _tone_repair("bro lol sus") is None

    def test_slang_only_scene_repaired_in_one_call(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        event = _make_event()
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(
                200, _valid_scene_json("Bro the ford runs wide and cold.")
            )

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_scene(state, event, "clear skies")
        assert result is not None  # repaired, not dropped
        assert calls["n"] == 1  # the retry was NOT burned re-failing identically
        assert "bro" not in result.narration.lower().split()
        assert "ford" in result.narration.lower()


class TestGMStats:
    """gm-B-03 — the stats dict cli-tui surfaces; each outcome bumps a counter."""

    def _client_world_event(self, **cfg):
        return GMClient(GMConfig(**cfg)), create_new_run(seed=1), _make_event()

    def test_stats_dict_has_contract_keys(self):
        client = GMClient(GMConfig())
        for key in (
            "attempts", "successes", "json_rejects",
            "tone_rejects", "timeouts", "connect_errors",
        ):
            assert key in client.stats
            assert client.stats[key] == 0

    def test_success_increments_attempts_and_successes(self, monkeypatch):
        client, state, event = self._client_world_event(max_retries=1)
        good = _valid_scene_json("The river runs wide and cold.")
        monkeypatch.setattr(
            client._client, "post", lambda *a, **k: _FakeResp(200, good),
        )
        client.generate_scene(state, event, "clear skies")
        assert client.stats["attempts"] == 1
        assert client.stats["successes"] == 1
        assert client.stats["json_rejects"] == 0

    def test_garbage_increments_json_rejects(self, monkeypatch):
        client, state, event = self._client_world_event(max_retries=1)
        monkeypatch.setattr(
            client._client, "post", lambda *a, **k: _FakeResp(200, "not json"),
        )
        client.generate_scene(state, event, "clear skies")
        assert client.stats["json_rejects"] == 2  # max_retries + 1 misses
        assert client.stats["successes"] == 0

    def test_punchline_increments_tone_rejects(self, monkeypatch):
        client, state, event = self._client_world_event(max_retries=1)
        bad = _valid_scene_json("Spoiler alert, the bridge fell.")
        monkeypatch.setattr(
            client._client, "post", lambda *a, **k: _FakeResp(200, bad),
        )
        client.generate_scene(state, event, "clear skies")
        assert client.stats["tone_rejects"] >= 1

    def test_timeout_increments_timeouts(self, monkeypatch):
        client, state, event = self._client_world_event(max_retries=1)

        def _timeout(*_a, **_k):
            raise httpx.TimeoutException("slow")

        monkeypatch.setattr(client._client, "post", _timeout)
        client.generate_scene(state, event, "clear skies")
        assert client.stats["timeouts"] == 1
        assert client.stats["connect_errors"] == 0

    def test_connect_error_increments_connect_errors(self, monkeypatch):
        client, state, event = self._client_world_event(max_retries=1)

        def _refuse(*_a, **_k):
            raise httpx.ConnectError("refused")

        monkeypatch.setattr(client._client, "post", _refuse)
        client.generate_scene(state, event, "clear skies")
        assert client.stats["connect_errors"] == 1
        assert client.stats["timeouts"] == 0

    def test_profile_drift_counted(self, monkeypatch):
        # Requested fireside (the default new-run profile) but the model
        # returned 'lantern' → one drift recorded.
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)  # default profile == fireside
        event = _make_event()
        drifted = json.dumps({
            "scene_id": "s1",
            "narration": "The river runs wide and cold.",
            "profile": "lantern",
            "choices": [
                {"id": "A", "label": "Ford it"},
                {"id": "B", "label": "Wait"},
            ],
        })
        monkeypatch.setattr(
            client._client, "post", lambda *a, **k: _FakeResp(200, drifted),
        )
        client.generate_scene(state, event, "clear skies")
        assert client.stats["profile_drifts"] == 1


class TestProfileHeaders:
    """gm-B-07 — every profile resolves to a header; lookup never crashes."""

    def test_every_profile_has_a_header(self):
        # A future GMProfile member without a matching header would crash a
        # live run on the bare subscript this replaced. Keep them in lockstep.
        assert set(GMProfile) == set(PROFILE_HEADERS)

    def test_known_profile_resolves(self):
        for profile in GMProfile:
            assert _profile_header(profile) == PROFILE_HEADERS[profile]

    def test_unknown_profile_defaults_to_fireside(self):
        # Simulate a profile not present in PROFILE_HEADERS without mutating
        # the real enum: a bare string the .get() can't match falls back.
        assert _profile_header("phantom_profile") == (  # type: ignore[arg-type]
            PROFILE_HEADERS[GMProfile.FIRESIDE]
        )


class TestIsAvailableTimeout:
    """gm-B-08 — the reachability probe uses its own short timeout."""

    def test_probe_uses_probe_timeout(self, monkeypatch):
        client = GMClient(GMConfig(timeout=30.0, probe_timeout=2.5))
        seen = {}

        class _OK:
            status_code = 200

            def json(self):
                return {"models": [{"name": "llama3.2:latest"}]}

        def _get(_url, *, timeout=None, **_k):
            seen["timeout"] = timeout
            return _OK()

        monkeypatch.setattr(client._client, "get", _get)
        assert client.is_available() is True
        # The probe must NOT use the 30s generation budget.
        assert seen["timeout"] == 2.5
        assert client.last_error is None

    def test_probe_default_is_short(self):
        # The default probe budget is seconds, not the 30s generation window.
        assert GMConfig().probe_timeout <= 5.0

    def test_missing_model_is_unavailable(self, monkeypatch):
        # F-254eedb6 — host up with other models is not "available".
        client = GMClient(GMConfig(model="llama3.2"))

        class _Tags:
            status_code = 200

            def json(self):
                return {"models": [{"name": "mistral:latest"}, {"name": "qwen2.5"}]}

        monkeypatch.setattr(client._client, "get", lambda *_a, **_k: _Tags())
        assert client.is_available() is False
        assert client.last_error is not None
        assert "llama3.2" in client.last_error
        assert "ollama pull llama3.2" in client.last_error
        assert "--gm-off" in client.last_error
        st = client.status()
        assert st["installed"] is True
        assert st["available"] is False
        assert st["last_error"] == client.last_error

    def test_tagged_model_matches_bare_name(self, monkeypatch):
        client = GMClient(GMConfig(model="llama3.2"))

        class _Tags:
            status_code = 200

            def json(self):
                return {"models": [{"name": "llama3.2:latest"}]}

        monkeypatch.setattr(client._client, "get", lambda *_a, **_k: _Tags())
        assert client.is_available() is True
        assert client.last_error is None
        assert client.status()["available"] is True


class TestOllama404ModelMissing:
    """F-254eedb6 — HTTP 404 is model-missing, not a JSON reject."""

    def _world(self, **cfg):
        return GMClient(GMConfig(max_retries=1, **cfg)), create_new_run(seed=1), _make_event()

    def _404(self, model: str = "llama3.2") -> _FakeResp:
        return _FakeResp(
            404,
            json.dumps({"error": f"model '{model}' not found"}),
        )

    def _assert_model_missing(self, client: GMClient, calls: dict, model: str = "llama3.2"):
        assert calls["n"] == 1  # do not retry 4xx
        assert client.stats["json_rejects"] == 0
        assert client.stats["attempts"] == 1
        assert client.stats["successes"] == 0
        assert client.last_error is not None
        assert "not found" in client.last_error
        assert f"ollama pull {model}" in client.last_error
        assert "--gm-off" in client.last_error
        st = client.status()
        assert set(st) == {"installed", "available", "enabled", "last_error"}
        assert st["available"] is False
        assert st["enabled"] is True
        assert st["last_error"] == client.last_error
        assert st["installed"] is True

    def test_generate_scene_404_is_not_json_reject(self, monkeypatch):
        client, state, event = self._world()
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return self._404()

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_scene(state, event, "clear skies")
        assert result is None
        self._assert_model_missing(client, calls)

    def test_generate_outcome_404_is_not_json_reject(self, monkeypatch):
        client, state, event = self._world()
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return self._404()

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {"result": "ok"},
        )
        assert result is None
        self._assert_model_missing(client, calls)

    def test_generate_scene_stream_404_does_not_retry(self, monkeypatch):
        client, state, event = self._world()
        calls = {"n": 0}
        err = json.dumps({"error": "model 'llama3.2' not found"})

        def _stream(_method, _url, **_k):
            calls["n"] += 1
            return _FakeStream([], status_code=404, error_body=err)

        monkeypatch.setattr(client._client, "stream", _stream)
        result = client.generate_scene(
            state, event, "clear skies", on_token=lambda _d: None,
        )
        assert result is None
        self._assert_model_missing(client, calls)

    def test_epilogue_404_falls_back_without_json_reject(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        ending = _ending()
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return self._404()

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_epilogue(state, ending)
        assert result == build_deterministic_epilogue(state, ending)
        self._assert_model_missing(client, calls)

    def test_http_400_does_not_retry(self, monkeypatch):
        client, state, event = self._world()
        calls = {"n": 0}

        def _fake_post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(400, json.dumps({"error": "invalid request"}))

        monkeypatch.setattr(client._client, "post", _fake_post)
        result = client.generate_scene(state, event, "clear skies")
        assert result is None
        assert calls["n"] == 1
        assert client.stats["json_rejects"] == 0
        assert client.last_error is not None
        assert "--gm-off" in client.last_error

    def test_status_keys_match_voicebridge_shape(self):
        client = GMClient(GMConfig())
        st = client.status()
        assert set(st) == {"installed", "available", "enabled", "last_error"}
        assert st["enabled"] is True
        assert st["available"] is False
        assert st["last_error"] is None

    def test_connect_error_sets_last_error(self, monkeypatch):
        client, state, event = self._world()

        def _refuse(*_a, **_k):
            raise httpx.ConnectError("refused")

        monkeypatch.setattr(client._client, "post", _refuse)
        assert client.generate_scene(state, event, "clear skies") is None
        assert client.stats["connect_errors"] == 1
        assert client.stats["json_rejects"] == 0
        assert client.last_error is not None
        assert "ollama serve" in client.last_error
        assert "--gm-off" in client.last_error
        assert client.status()["available"] is False
        assert client.status()["installed"] is False


class TestStreamingNarration:
    """gm-feat-01 — on_token streams narration progressively, still parses.

    The invariants under test:
      - on_token receives the narration prose in arriving order, and the
        accumulated deltas equal the final SceneResponse.narration;
      - the fully-parsed SceneResponse is still returned and passes tone-lint;
      - a streaming failure (non-200 / transport / invalid JSON / tone-fail)
        still falls through to None after the usual retry (fallback never
        bricks), and on_token never raises into the model loop;
      - on_token=None is byte-identical to the non-streamed path (still hits
        client._client.post, never .stream).
    """

    def _world(self):
        return create_new_run(seed=1), _make_event()

    def test_scene_streams_progressive_narration(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state, event = self._world()
        narration = "The ford runs wide and cold, and the mule will not move."
        full = _valid_scene_json(narration)
        # Split the raw model output into many small fragments to exercise the
        # incremental decoder across chunk boundaries.
        monkeypatch.setattr(
            client._client, "stream", _stream_factory(_chunk(full, 4)),
        )
        # post must NOT be used on the streaming path.
        monkeypatch.setattr(
            client._client, "post",
            lambda *a, **k: (_ for _ in ()).throw(
                AssertionError("streaming path must not call post")
            ),
        )

        seen: list[str] = []
        result = client.generate_scene(
            state, event, "clear skies", on_token=seen.append,
        )
        assert result is not None
        assert result.narration == narration
        # The streamed deltas, concatenated, reconstruct the narration exactly.
        assert "".join(seen) == narration
        # Progressive: more than one delta arrived (it was not one dump).
        assert len(seen) >= 2
        # And the final response still passes tone-lint.
        assert _tone_check(result.narration)
        assert client.stats["successes"] == 1

    def test_scene_stream_non200_falls_through_to_none(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state, event = self._world()
        calls = {"n": 0}

        def _stream(_method, _url, **_k):
            calls["n"] += 1
            return _FakeStream([], status_code=500)

        monkeypatch.setattr(client._client, "stream", _stream)
        seen: list[str] = []
        result = client.generate_scene(
            state, event, "clear skies", on_token=seen.append,
        )
        assert result is None  # fallback-never-bricks
        assert calls["n"] == 2  # max_retries + 1
        assert seen == []  # nothing decoded
        # F-254eedb6 — HTTP non-200 is not a JSON reject.
        assert client.stats["json_rejects"] == 0

    def test_scene_stream_invalid_json_falls_through(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state, event = self._world()
        monkeypatch.setattr(
            client._client, "stream",
            _stream_factory(_chunk("this is not json at all", 5)),
        )
        result = client.generate_scene(
            state, event, "clear skies", on_token=lambda _d: None,
        )
        assert result is None

    def test_scene_stream_tone_fail_falls_through(self, monkeypatch):
        # A streamed punchline still hard-fails after retries → None.
        client = GMClient(GMConfig(max_retries=1))
        state, event = self._world()
        bad = _valid_scene_json("Plot twist: the bridge gave way.")
        calls = {"n": 0}

        def _stream(_method, _url, **_k):
            calls["n"] += 1
            return _FakeStream(_chunk(bad, 6))

        monkeypatch.setattr(client._client, "stream", _stream)
        result = client.generate_scene(
            state, event, "clear skies", on_token=lambda _d: None,
        )
        assert result is None
        assert calls["n"] == 2
        assert client.stats["tone_rejects"] >= 1

    def test_on_token_exception_never_propagates(self, monkeypatch):
        # A buggy renderer must not brick generation: the scene still parses.
        client = GMClient(GMConfig(max_retries=1))
        state, event = self._world()
        narration = "The river is high and the rope is frayed."
        monkeypatch.setattr(
            client._client, "stream",
            _stream_factory(_chunk(_valid_scene_json(narration), 4)),
        )

        def _explode(_delta):
            raise RuntimeError("renderer blew up")

        result = client.generate_scene(
            state, event, "clear skies", on_token=_explode,
        )
        assert result is not None  # callback failure swallowed
        assert result.narration == narration
        assert client.stats["successes"] == 1

    def test_on_token_none_uses_post_not_stream(self, monkeypatch):
        # gm-feat-01 — the None path is byte-identical to today: it uses post,
        # never stream.
        client = GMClient(GMConfig(max_retries=1))
        state, event = self._world()
        good = _valid_scene_json("The wind drives the dust before it.")
        monkeypatch.setattr(
            client._client, "post", lambda *a, **k: _FakeResp(200, good),
        )
        monkeypatch.setattr(
            client._client, "stream",
            lambda *a, **k: (_ for _ in ()).throw(
                AssertionError("non-stream path must not call stream")
            ),
        )
        result = client.generate_scene(state, event, "clear skies")
        assert result is not None
        assert "dust" in result.narration

    def test_outcome_streams_progressive_narration(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state, event = self._world()
        narration = "The water takes the wagon to its knees and lets go slowly."
        full = json.dumps({
            "scene_id": "s1",
            "outcome_narration": narration,
            "callout": "You crossed, soaked but whole.",
        })
        monkeypatch.setattr(
            client._client, "stream", _stream_factory(_chunk(full, 5)),
        )
        seen: list[str] = []
        result = client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {"result": "ok"},
            on_token=seen.append,
        )
        assert result is not None
        assert result.outcome_narration == narration
        assert "".join(seen) == narration
        assert len(seen) >= 2
        assert _tone_check(result.outcome_narration)

    def test_stream_handles_escapes_across_chunks(self, monkeypatch):
        # A narration containing escaped quotes + a unicode escape, split so
        # the escapes straddle chunk boundaries, must still decode cleanly.
        client = GMClient(GMConfig(max_retries=1))
        state, event = self._world()
        narration = 'She said "go" and the café lamp guttered out.'
        full = _valid_scene_json(narration)
        # Size-1 fragments guarantee every escape is split.
        monkeypatch.setattr(
            client._client, "stream", _stream_factory(_chunk(full, 1)),
        )
        seen: list[str] = []
        result = client.generate_scene(
            state, event, "clear skies", on_token=seen.append,
        )
        assert result is not None
        assert result.narration == narration
        assert "".join(seen) == narration

    def test_midstream_readerror_buckets_transport_and_returns_none(
        self, monkeypatch,
    ):
        # gm-feat-01b — a transport drop AFTER a 200 (httpx.ReadError raised
        # while consuming iter_lines) must be bucketed like an open-time
        # failure: connect_errors increments, and the scene still resolves to
        # None (fallback-never-bricks). Before the fix it fell through the broad
        # 'except Exception' and incremented no counter, undercounting drops.
        client = GMClient(GMConfig(max_retries=1))
        state, event = self._world()

        class _DroppingStream:
            status_code = 200

            def __enter__(self):
                return self

            def __exit__(self, *_a):
                return False

            def iter_lines(self):
                # Deliver one good fragment, then drop mid-stream.
                yield json.dumps({"response": '{"narration":"The ford '})
                raise httpx.ReadError("connection reset mid-stream")

        calls = {"n": 0}

        def _stream(_method, _url, **_k):
            calls["n"] += 1
            return _DroppingStream()

        monkeypatch.setattr(client._client, "stream", _stream)
        # post must not be touched on the streaming path.
        monkeypatch.setattr(
            client._client, "post",
            lambda *a, **k: (_ for _ in ()).throw(
                AssertionError("streaming path must not call post")
            ),
        )

        seen: list[str] = []
        result = client.generate_scene(
            state, event, "clear skies", on_token=seen.append,
        )
        # Resolves to None — the run uses its deterministic fallback text.
        assert result is None
        # The mid-stream drop is bucketed as a transport failure, not lost.
        assert client.stats["connect_errors"] == 1
        assert client.stats["timeouts"] == 0
        # A ReadError is a transport drop, not a JSON/tone rejection.
        assert client.stats["json_rejects"] == 0
        assert client.stats["tone_rejects"] == 0
        # The first transport drop returns immediately (like ConnectError);
        # it does not burn the whole retry budget re-failing.
        assert calls["n"] == 1


def _ending(
    tier: str = "triumphant",
    *,
    victory: bool = True,
    survivors: int = 4,
    party_size: int = 4,
    days: int = 30,
    par_days: int = 32,
    taboo: str | None = "never_night",
    taboo_kept: bool = True,
    uncanny_unspent: int = 0,
    deaths: dict | None = None,
    headline: str = "All reached the valley.",
) -> EndingResult:
    return EndingResult(
        tier=tier,
        headline=headline,
        facts={
            "victory": victory,
            "survivors": survivors,
            "party_size": party_size,
            "days": days,
            "par_days": par_days,
            "taboo": taboo,
            "taboo_kept": taboo_kept,
            "uncanny_tokens_unspent": uncanny_unspent,
            "deaths_by_cause": deaths or {},
            "distance": 600,
            "total_distance": 600,
            "cause_of_death": "",
        },
    )


class TestDeterministicEpilogue:
    """EC-04 — the GM-free floor: grounded, deterministic, invents nothing."""

    def test_leads_with_headline(self):
        state = create_new_run(seed=1)
        ending = _ending(headline="They made it through.")
        text = build_deterministic_epilogue(state, ending)
        assert text.startswith("They made it through.")

    def test_deterministic_for_same_ending(self):
        state = create_new_run(seed=1)
        ending = _ending()
        a = build_deterministic_epilogue(state, ending)
        b = build_deterministic_epilogue(state, ending)
        assert a == b
        assert a  # non-empty

    def test_pyrrhic_names_the_loss(self):
        state = create_new_run(seed=1)
        ending = _ending(
            tier="pyrrhic", survivors=3, party_size=5,
            deaths={"starvation": 2}, headline="Reached, but at cost.",
        )
        text = build_deterministic_epilogue(state, ending)
        assert "2 did not finish" in text
        assert "starvation" in text

    def test_broken_taboo_reflected(self):
        state = create_new_run(seed=1)
        ending = _ending(tier="pyrrhic", taboo_kept=False)
        text = build_deterministic_epilogue(state, ending)
        assert "vow did not survive" in text.lower()

    def test_lost_tier_closer(self):
        state = create_new_run(seed=1)
        ending = _ending(
            tier="lost", victory=False, survivors=0,
            headline="None reached the valley.",
        )
        text = build_deterministic_epilogue(state, ending)
        assert "silence" in text.lower()
        # A loss epilogue passes tone-lint (no slang, no punchline).
        assert _tone_check(text)

    def test_no_resources_invented(self):
        # The deterministic floor never references supply quantities.
        state = create_new_run(seed=1)
        ending = _ending()
        text = build_deterministic_epilogue(state, ending).lower()
        for word in ("food:", "water:", "parts:", "+", "supplies"):
            assert word not in text


class TestGenerateEpilogue:
    """EC-04 (gm half) — narrated when possible, deterministic floor always."""

    def test_gm_off_yields_deterministic(self):
        client = GMClient(GMConfig(enabled=False))
        state = create_new_run(seed=1)
        ending = _ending()
        result = client.generate_epilogue(state, ending)
        assert result == build_deterministic_epilogue(state, ending)

    def test_mocked_gm_yields_narrated(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        ending = _ending()
        prose = (
            "The valley opened below them at last, green and indifferent. "
            "They had spent everything but the vow, and the vow had held. "
            "No one spoke of the days behind. The fires that night were small "
            "and warm and earned."
        )
        monkeypatch.setattr(
            client._client, "post", lambda *a, **k: _FakeResp(200, prose),
        )
        result = client.generate_epilogue(state, ending)
        assert result == prose
        assert result != build_deterministic_epilogue(state, ending)
        assert client.stats["successes"] == 1

    def test_narrated_epilogue_is_tone_linted(self, monkeypatch):
        # A punchline epilogue is hard-rejected → deterministic fallback.
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        ending = _ending()
        bad = "Plot twist: they all lived happily ever after."
        calls = {"n": 0}

        def _post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(200, bad)

        monkeypatch.setattr(client._client, "post", _post)
        result = client.generate_epilogue(state, ending)
        assert result == build_deterministic_epilogue(state, ending)
        assert calls["n"] == 2  # retried then gave up
        assert client.stats["tone_rejects"] >= 1

    def test_slang_only_epilogue_repaired(self, monkeypatch):
        # A slang-only epilogue is repaired locally, not dropped to fallback.
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        ending = _ending()
        prose = "Bro, the valley was wide and the fires burned low that night."
        calls = {"n": 0}

        def _post(*_a, **_k):
            calls["n"] += 1
            return _FakeResp(200, prose)

        monkeypatch.setattr(client._client, "post", _post)
        result = client.generate_epilogue(state, ending)
        assert calls["n"] == 1  # repaired in one call
        assert "bro" not in result.lower().split()
        assert "valley" in result.lower()

    def test_transport_failure_yields_deterministic(self, monkeypatch):
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        ending = _ending()

        def _refuse(*_a, **_k):
            raise httpx.ConnectError("refused")

        monkeypatch.setattr(client._client, "post", _refuse)
        result = client.generate_epilogue(state, ending)
        assert result == build_deterministic_epilogue(state, ending)
        assert client.stats["connect_errors"] == 1

    def test_junk_response_yields_deterministic(self, monkeypatch):
        # Empty/junk prose falls through to the deterministic floor.
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        ending = _ending()
        monkeypatch.setattr(
            client._client, "post", lambda *a, **k: _FakeResp(200, "   "),
        )
        result = client.generate_epilogue(state, ending)
        assert result == build_deterministic_epilogue(state, ending)

    def test_json_wrapped_prose_is_lifted(self, monkeypatch):
        # A model that wraps the epilogue in JSON still yields the prose.
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        ending = _ending()
        wrapped = json.dumps({
            "epilogue": "The valley held them gently after the long road down.",
        })
        monkeypatch.setattr(
            client._client, "post", lambda *a, **k: _FakeResp(200, wrapped),
        )
        result = client.generate_epilogue(state, ending)
        assert result == "The valley held them gently after the long road down."

    def test_never_returns_none(self, monkeypatch):
        # The contract: generate_epilogue NEVER returns None.
        client = GMClient(GMConfig(max_retries=1))
        state = create_new_run(seed=1)
        ending = _ending(tier="lost", victory=False, survivors=0)
        monkeypatch.setattr(
            client._client, "post", lambda *a, **k: _FakeResp(500, ""),
        )
        result = client.generate_epilogue(state, ending)
        assert result is not None
        assert isinstance(result, str)
        assert result  # non-empty
