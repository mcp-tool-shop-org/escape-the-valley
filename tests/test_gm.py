"""Tests for GM client — JSON parsing, validation, tone lint, fallback."""

import json

import httpx

from escape_the_valley.events import EventCategory, EventSkeleton
from escape_the_valley.gm import (
    PROFILE_HEADERS,
    GMClient,
    GMConfig,
    _parse_json,
    _profile_header,
    _tone_check,
    _tone_repair,
    _validate_scene,
)
from escape_the_valley.models import GMProfile
from escape_the_valley.worldgen import create_new_run


class _FakeResp:
    """Minimal stand-in for an httpx.Response."""

    def __init__(self, status_code: int = 200, payload: str = ""):
        self.status_code = status_code
        self._payload = payload

    def json(self) -> dict:
        return {"response": self._payload}


class _FakeStream:
    """Context-manager stand-in for httpx.Client.stream() returning NDJSON lines.

    gm-feat-01 — Ollama streams its full JSON object as a sequence of NDJSON
    objects, each carrying a ``response`` text fragment. This fake replays a
    pre-split list of raw fragments so a test can prove on_token receives the
    narration progressively while the final accumulated raw still parses.
    """

    def __init__(self, fragments: list[str], status_code: int = 200):
        self._fragments = fragments
        self.status_code = status_code

    def __enter__(self):
        return self

    def __exit__(self, *_a):
        return False

    def read(self) -> bytes:  # drained on non-200
        return b""

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

    def test_disabled_returns_false(self):
        client = GMClient(GMConfig(enabled=False))
        assert client.is_available() is False


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

    def test_disabled_returns_none_without_calling(self, monkeypatch):
        client, state, event = self._client_and_world(enabled=False)

        def _boom(*_a, **_k):
            raise AssertionError("should not call the model when disabled")

        monkeypatch.setattr(client._client, "post", _boom)
        assert client.generate_scene(state, event, "clear skies") is None
        assert client.generate_outcome(
            state, event, "The Ford", "A", "Ford it", {},
        ) is None


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

        def _get(_url, *, timeout=None, **_k):
            seen["timeout"] = timeout
            return _OK()

        monkeypatch.setattr(client._client, "get", _get)
        assert client.is_available() is True
        # The probe must NOT use the 30s generation budget.
        assert seen["timeout"] == 2.5

    def test_probe_default_is_short(self):
        # The default probe budget is seconds, not the 30s generation window.
        assert GMConfig().probe_timeout <= 5.0


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
        assert client.stats["json_rejects"] == 2

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
