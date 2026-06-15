"""Ollama GM integration — 3 profiles, two-prompt loop, graceful fallback."""

from __future__ import annotations

import json
import logging
from collections.abc import Callable
from dataclasses import dataclass

import httpx

from .events import EventSkeleton
from .memory import GMBrief, format_brief_for_prompt
from .models import GMProfile, RunState

logger = logging.getLogger(__name__)

# ─── Profile style headers ──────────────────────────────────────────────

PROFILE_HEADERS = {
    GMProfile.CHRONICLER: (
        'You are "The Chronicler," a grounded, practical narrator. The world is harsh and real. '
        "Folklore is rumor, coincidence, or tired minds—never confirmed. Keep language spare and "
        "period-appropriate. No modern slang. Choices must be practical tradeoffs. Never grant "
        "free resources; the engine decides outcomes."
    ),
    GMProfile.FIRESIDE: (
        'You are "The Fireside Storyteller," a serious campfire '
        "narrator. The world is harsh with occasional moments that "
        "feel larger than they should. Folklore is subtle: uncanny "
        "but never cartoonish. Keep language period-appropriate. "
        "No modern slang. Choices must be practical tradeoffs. "
        "Never grant free resources; the engine decides outcomes. "
        "Occasional wry observations permitted. Not jokes\u2014observations."
    ),
    GMProfile.LANTERN: (
        'You are "The Lantern-Bearer," a serious but uncanny '
        "storyteller. The world is realistic and survival stakes "
        "are real. Strangeness is frequent but ambiguous\u2014never "
        "confirmed as magic. Keep language period-appropriate. "
        "No modern slang. Uncanny details should be sensory "
        "(sound, light, weather, animals, silence) and plausibly "
        "explainable. Never grant free resources; the engine "
        "decides outcomes. Choices must remain practical tradeoffs "
        "even when the scene is strange. If uncanny tokens remain, "
        "you may introduce one strong uncanny detail. "
        "HUMOR RULE: At least one line should be wryly funny\u2014"
        "deadpan observations, dark irony, or absurd situations "
        "played straight. The funniest line is usually the situation "
        "itself, not a quip. Never crack jokes. Never use modern "
        "punchline structure. If in doubt, let the river or the "
        "weather deliver the comedy. "
        'Example: "The river accepts your confidence and returns it as noise." '
        'Example: "The mule regards the bridge with more sense than any of you."'
    ),
}

def _profile_header(profile: GMProfile) -> str:
    """Return the system header for a profile, defaulting to FIRESIDE.

    gm-B-07 — a future GMProfile member (or a profile loaded from a stale
    save) must never crash a live run on a bare KeyError. The fallback is the
    middle-of-the-road serious campfire voice; the test
    `test_every_profile_has_a_header` guards against a member being added
    without a matching header.
    """
    return PROFILE_HEADERS.get(profile, PROFILE_HEADERS[GMProfile.FIRESIDE])


SCENE_SCHEMA = """{
  "scene_id": "string", "title": "string", "narration": "string",
  "profile": "chronicler|fireside|lantern", "uncanny_intensity": "none|hint|strong",
  "choices": [{"id":"A","label":"string",
    "intent":{"action":"...","style":"CAUTIOUS|NEUTRAL|BOLD",
    "notes":"string"},"risk_hint":"string","cost_hint":"string"}],
  "tags": ["string"], "gm_aside": "string",
  "memory_proposals": [{"kind":"npc|omen|place|rumor|promise",
    "title":"string (max 40 chars)", "text":"string (max 300 chars)",
    "tags":["string"], "entities":["string"]}]
}"""

OUTCOME_SCHEMA = """{
  "scene_id": "string", "outcome_title": "string",
  "outcome_narration": "string", "callout": "string", "oregon_nod": "string",
  "memory_proposals": [{"kind":"npc|omen|place|rumor|promise",
    "title":"string (max 40 chars)", "text":"string (max 300 chars)",
    "tags":["string"], "entities":["string"]}]
}"""

# Modern slang ban list for tone lint.
# Only genuine modern slang belongs here. Ordinary English words that happen
# to double as slang (goat=livestock, cap=ridge feature, slay=slaughter an
# animal, literally/basically=adverbs, oof=an exhalation) were removed in
# gm-A-102 after they over-rejected grounded frontier prose and forced
# needless GM retries, then silent deterministic fallback.
BANNED_WORDS = {
    "bro", "lol", "meme", "vibes", "yeet", "sus", "lowkey", "bruh",
    "ngl", "tbh", "fr", "bestie", "rizz",
}


@dataclass
class GMConfig:
    host: str = "http://localhost:11434"
    model: str = "llama3.2"
    # 30s allows for first-load model warm-up on slower hardware.
    # This timeout governs scene/outcome generation.
    timeout: float = 30.0
    # gm-B-08 — reachability probes (is_available) get their own short budget
    # so a dead host fails fast instead of stalling for the full generation
    # window. cli.py self_check runs its own independent reachability probe.
    probe_timeout: float = 2.5
    max_retries: int = 1
    enabled: bool = True

    @property
    def generate_url(self) -> str:
        return f"{self.host}/api/generate"


@dataclass
class SceneResponse:
    scene_id: str
    title: str
    narration: str
    profile: str
    uncanny_intensity: str
    choices: list[dict]
    tags: list[str]
    gm_aside: str
    raw_json: dict
    memory_proposals: list[dict]

    @classmethod
    def from_dict(cls, data: dict) -> SceneResponse:
        return cls(
            scene_id=data.get("scene_id", ""),
            title=data.get("title", ""),
            narration=data.get("narration", ""),
            profile=data.get("profile", ""),
            uncanny_intensity=data.get("uncanny_intensity", "none"),
            choices=data.get("choices", []),
            tags=data.get("tags", []),
            gm_aside=data.get("gm_aside", ""),
            raw_json=data,
            memory_proposals=data.get("memory_proposals", []),
        )


@dataclass
class OutcomeResponse:
    scene_id: str
    outcome_title: str
    outcome_narration: str
    callout: str
    oregon_nod: str
    memory_proposals: list[dict]

    @classmethod
    def from_dict(cls, data: dict) -> OutcomeResponse:
        return cls(
            scene_id=data.get("scene_id", ""),
            outcome_title=data.get("outcome_title", ""),
            outcome_narration=data.get("outcome_narration", ""),
            callout=data.get("callout", ""),
            oregon_nod=data.get("oregon_nod", ""),
            memory_proposals=data.get("memory_proposals", []),
        )


class GMClient:
    """Ollama GameMaster client with strict JSON parsing and graceful fallback."""

    def __init__(self, config: GMConfig | None = None):
        self.config = config or GMConfig()
        self._client = httpx.Client(timeout=self.config.timeout)
        # gm-B-03 — per-outcome counters the 'stats' command surfaces. These
        # are the observable shape of the GM's reliability: how often it was
        # asked, how often it produced usable JSON, and why it didn't when it
        # didn't. cli-tui reads this dict; do not rename keys without updating
        # the consumer. profile_drifts is an extra honesty signal (the model
        # returned a different profile than the one we asked it to wear).
        self.stats: dict[str, int] = {
            "attempts": 0,
            "successes": 0,
            "json_rejects": 0,
            "tone_rejects": 0,
            "timeouts": 0,
            "connect_errors": 0,
            "profile_drifts": 0,
        }

    def is_available(self) -> bool:
        """Check if Ollama is reachable.

        Reachability helper for use as a pre-flight probe. Any transport
        failure (connect, timeout, read, protocol, pool) resolves to False
        rather than propagating — a pre-flight check must never raise.

        gm-B-08 — this probe uses its own short timeout (PROBE_TIMEOUT_S),
        separate from the 30s generation budget, so a dead host fails fast
        instead of stalling the UI for the full generation window.
        """
        if not self.config.enabled:
            return False
        try:
            resp = self._client.get(
                f"{self.config.host}/api/tags",
                timeout=self.config.probe_timeout,
            )
            return resp.status_code == 200
        except httpx.HTTPError:
            return False

    def generate_scene(
        self,
        state: RunState,
        event: EventSkeleton,
        weather_str: str,
        brief: GMBrief | None = None,
        on_token: Callable[[str], None] | None = None,
    ) -> SceneResponse | None:
        """Generate a scene narration from the GM. Returns None on failure.

        gm-feat-01 — when ``on_token`` is supplied, the narration prose is
        streamed: ``on_token(delta)`` fires with each new run of decoded
        narration text as it arrives, while the method STILL returns a fully
        parsed + tone-linted ``SceneResponse`` (or None on any failure, after
        the usual retry). When ``on_token`` is None the behavior — and the
        request payload — is byte-identical to the non-streamed path.
        """
        if not self.config.enabled:
            return None

        node = _find_node(state)
        biome = node.biome.value if node else "unknown"

        # Build party summary
        alive = [m for m in state.party.members if m.is_alive()]
        party_summary = ", ".join(
            f"{m.name} (health:{m.health}, {m.condition.value})" for m in alive
        )

        supplies = state.supplies
        supplies_summary = " ".join(
            f"{k}:{v}" for k, v in supplies.to_dict().items() if v > 0
        )

        wagon_summary = (
            f"condition:{state.wagon.condition}/100 "
            f"animals:{state.wagon.animals_health}/100 "
            f"pace:{state.wagon.pace.value}"
        )

        system_prompt = (
            f"{_profile_header(state.gm_profile)}\n"
            f"You MUST output valid JSON ONLY (no markdown), following the schema below.\n"
            f"If you cannot comply, output an empty JSON object {{}}.\n\n"
            f"SCHEMA (scene response):\n{SCENE_SCHEMA}"
        )

        user_prompt = (
            f"Generate a scene for Escape the Valley.\n\n"
            f"FACTS YOU MUST RESPECT:\n"
            f"- Day: {state.day}\n"
            f"- Time: {state.time_of_day.value}\n"
            f"- Biome: {biome}\n"
            f"- Weather: {weather_str}\n"
            f"- Party: {party_summary}\n"
            f"- Wagon: {wagon_summary}\n"
            f"- Supplies: {supplies_summary}\n"
            f"- Pace: {state.wagon.pace.value}\n"
            f"- Morale: {state.party.morale}/100\n"
            f"- Weirdness level: {state.weirdness_level} (0-3)\n"
            f"- Uncanny tokens remaining: {state.uncanny_tokens}\n"
            f"- Engine event skeleton:\n"
            f"   event_id={event.event_id}\n"
            f"   event_type={event.category.value}\n"
            f"   severity={event.severity}\n"
            f"   tags={','.join(event.tags)}\n\n"
        )

        # Inject GM brief if available
        if brief:
            user_prompt += f"{format_brief_for_prompt(brief)}\n\n"

        user_prompt += (
            "REQUIREMENTS:\n"
            "- Write 2-4 choices with clear tradeoffs.\n"
            "- Keep narration concise (3-7 sentences).\n"
            "- Keep choices grounded; no magic solutions.\n"
            "- Do NOT grant or remove supplies. The engine decides deltas.\n"
            "- Use period-appropriate language (no modern slang).\n"
            "- Optionally propose up to 2 memory_proposals (NPCs, omens, places, "
            "rumors, or promises worth remembering). Keep titles under 40 chars, "
            "text under 300 chars. Do NOT reference supply quantities.\n"
        )

        if event.gm_aside:
            user_prompt += f"\nGM ASIDE: {event.gm_aside}\n"

        return self._request_scene(
            system_prompt, user_prompt, state.gm_profile.value,
            on_token=on_token,
        )

    def generate_outcome(
        self,
        state: RunState,
        event: EventSkeleton,
        scene_title: str,
        choice_id: str,
        choice_label: str,
        outcome_facts: dict,
        brief: GMBrief | None = None,
        on_token: Callable[[str], None] | None = None,
    ) -> OutcomeResponse | None:
        """Generate outcome narration. Returns None on failure.

        gm-feat-01 — ``on_token`` streams the ``outcome_narration`` prose the
        same way ``generate_scene`` streams scene narration; the method still
        returns a fully parsed + tone-linted ``OutcomeResponse`` (or None).
        ``on_token=None`` is byte-identical to the non-streamed path.
        """
        if not self.config.enabled:
            return None

        system_prompt = (
            f"{_profile_header(state.gm_profile)}\n"
            f"You MUST output valid JSON ONLY, following schema below.\n\n"
            f"SCHEMA (outcome response):\n{OUTCOME_SCHEMA}"
        )

        user_prompt = (
            f"Narrate the outcome for this scene.\n\n"
            f"Scene id: {event.event_id}\n"
            f"Scene title: {scene_title}\n"
            f"Chosen option: {choice_id} — {choice_label}\n\n"
            f"Engine outcome facts (DO NOT contradict):\n"
        )

        for key, val in outcome_facts.items():
            user_prompt += f"- {key}: {val}\n"

        # Inject GM brief if available
        if brief:
            user_prompt += f"\n{format_brief_for_prompt(brief)}\n"

        user_prompt += (
            "\nREQUIREMENTS:\n"
            "- Outcome narration: 4-10 sentences, grounded and specific.\n"
            "- Include sensory detail.\n"
            "- Add a single-sentence callout summarizing the practical result.\n"
            "- Optionally propose up to 2 memory_proposals (NPCs, omens, "
            "places, rumors, or promises). Do NOT reference supply quantities.\n"
        )

        return self._request_outcome(
            system_prompt, user_prompt, state.gm_profile.value,
            on_token=on_token,
        )

    def _post_text(
        self,
        system: str,
        user: str,
        *,
        narration_key: str,
        on_token: Callable[[str], None] | None,
    ) -> tuple[int, str]:
        """Issue the generate request; return (status_code, full_response_text).

        gm-feat-01 — when ``on_token`` is None this is the exact non-streamed
        round-trip used since launch (``"stream": False``; the payload is
        byte-identical). When ``on_token`` is provided we set ``"stream": True``,
        consume the NDJSON chunk stream, accumulate the full ``response`` text
        for normal end-of-stream parsing, and surface each new run of decoded
        ``narration_key`` prose via ``on_token`` as it arrives. Either way the
        returned ``text`` is the complete raw model response, so the caller's
        parse/validate/tone path is unchanged.
        """
        payload = {
            "model": self.config.model,
            "prompt": user,
            "system": system,
            "options": {"temperature": 0.7},
        }
        if on_token is None:
            payload["stream"] = False
            resp = self._client.post(self.config.generate_url, json=payload)
            if resp.status_code != 200:
                return resp.status_code, ""
            return 200, resp.json().get("response", "")

        # Streamed path.
        payload["stream"] = True
        streamer = _NarrationStreamer(narration_key)
        with self._client.stream(
            "POST", self.config.generate_url, json=payload,
        ) as resp:
            if resp.status_code != 200:
                # Drain so the connection can be reused/closed cleanly.
                resp.read()
                return resp.status_code, ""
            for line in resp.iter_lines():
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except json.JSONDecodeError:
                    continue
                fragment = obj.get("response", "")
                if fragment:
                    _safe_emit(on_token, streamer.feed(fragment))
        return 200, streamer.raw

    def _request_scene(
        self,
        system: str,
        user: str,
        requested_profile: str = "",
        on_token: Callable[[str], None] | None = None,
    ) -> SceneResponse | None:
        """Make the Ollama request with retry + validation.

        gm-B-03 — each branch bumps self.stats so the 'stats' command can
        report attempts/successes and *why* the GM fell back.
        gm-B-04 — a tone-only miss (valid JSON+schema, narration present, the
        ONLY problem is a banned word) is repaired locally rather than spending
        a full multi-second round-trip to re-fail identically.
        gm-feat-01 — when ``on_token`` is set the request is streamed and the
        narration prose is surfaced progressively; the parse/validate/tone path
        below is identical to the non-streamed path, so fallback-never-bricks
        (a streaming error/!200/invalid/tone-fail still retries-then-None) is
        preserved.
        """
        for attempt in range(self.config.max_retries + 1):
            try:
                self.stats["attempts"] += 1
                status, text = self._post_text(
                    system, user, narration_key="narration", on_token=on_token,
                )
                if status != 200:
                    logger.warning("GM returned %d", status)
                    self.stats["json_rejects"] += 1
                    continue

                data = _parse_json(text)
                if data and _validate_scene(data):
                    narration = data.get("narration", "")
                    repaired = _tone_repair(narration)
                    if repaired is None:
                        # Hard tone failure (punchline structure) — local repair
                        # can't fix it, so retry with an explicit nudge.
                        self.stats["tone_rejects"] += 1
                        logger.warning("Tone check failed (hard), retrying")
                        user = _nudge_prompt(user)
                        continue
                    if repaired != narration:
                        # Tone-only miss repaired in place; do not burn a retry.
                        logger.info("Tone repaired locally; accepting scene")
                        data["narration"] = repaired
                    self._count_profile_drift(
                        data.get("profile", ""), requested_profile,
                    )
                    self.stats["successes"] += 1
                    return SceneResponse.from_dict(data)
                self.stats["json_rejects"] += 1
                logger.warning("Invalid scene JSON (attempt %d)", attempt + 1)

            except (httpx.ConnectError, httpx.TimeoutException) as e:
                self._count_transport_error(e)
                logger.warning("GM connection error: %s", e)
                return None
            except Exception as e:
                logger.warning("GM error: %s", e)

        return None

    def _request_outcome(
        self,
        system: str,
        user: str,
        requested_profile: str = "",
        on_token: Callable[[str], None] | None = None,
    ) -> OutcomeResponse | None:
        """Make outcome request with retry. See _request_scene for stats/repair.

        gm-feat-01 — ``on_token`` streams the ``outcome_narration`` prose; the
        parse/tone/fallback path is otherwise identical to the non-streamed one.
        """
        for _attempt in range(self.config.max_retries + 1):
            try:
                self.stats["attempts"] += 1
                status, text = self._post_text(
                    system, user,
                    narration_key="outcome_narration", on_token=on_token,
                )
                if status != 200:
                    self.stats["json_rejects"] += 1
                    continue

                data = _parse_json(text)
                if data and "outcome_narration" in data:
                    narration = data.get("outcome_narration", "")
                    repaired = _tone_repair(narration)
                    if repaired is None:
                        self.stats["tone_rejects"] += 1
                        logger.warning("Outcome tone check failed (hard), retrying")
                        user = _nudge_prompt(user)
                        continue
                    if repaired != narration:
                        logger.info("Tone repaired locally; accepting outcome")
                        data["outcome_narration"] = repaired
                    self.stats["successes"] += 1
                    return OutcomeResponse.from_dict(data)
                self.stats["json_rejects"] += 1
                logger.warning(
                    "Invalid outcome JSON (attempt %d)", _attempt + 1
                )

            except (httpx.ConnectError, httpx.TimeoutException) as e:
                self._count_transport_error(e)
                return None
            except Exception as e:
                logger.warning("GM outcome error: %s", e)

        return None

    def _count_transport_error(self, exc: Exception) -> None:
        """gm-B-03 — bucket a transport failure into timeouts vs connect_errors."""
        if isinstance(exc, httpx.TimeoutException):
            self.stats["timeouts"] += 1
        else:
            self.stats["connect_errors"] += 1

    def _count_profile_drift(self, returned: str, requested: str) -> None:
        """gm-B-03 — count when the model wore a different profile than asked."""
        if requested and returned and returned.lower() != requested.lower():
            self.stats["profile_drifts"] += 1

    def close(self) -> None:
        self._client.close()


def _parse_json(text: str) -> dict | None:
    """Try to parse JSON from GM response, handling markdown fences."""
    text = text.strip()

    # Strip markdown fences if present
    if text.startswith("```"):
        lines = text.split("\n")
        lines = [ln for ln in lines if not ln.strip().startswith("```")]
        text = "\n".join(lines).strip()

    # Try direct parse
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass

    # Try to find JSON object in the text
    start = text.find("{")
    end = text.rfind("}")
    if start != -1 and end != -1 and end > start:
        try:
            return json.loads(text[start : end + 1])
        except json.JSONDecodeError:
            pass

    return None


def _validate_scene(data: dict) -> bool:
    """Validate scene response has required fields."""
    if not isinstance(data, dict):
        return False
    if not data.get("narration"):
        return False
    choices = data.get("choices", [])
    if not isinstance(choices, list) or len(choices) < 2 or len(choices) > 4:
        return False
    for choice in choices:
        if not isinstance(choice, dict) or not choice.get("id") or not choice.get("label"):
            return False
    return True


_PUNCHLINE_PATTERNS = [
    r"(?i)\bplot twist\b",
    r"(?i)\bspoiler alert\b",
    r"(?i)\bwait for it\b",
    r"(?i)\bnot gonna lie\b",
    r"(?i)\bi mean\b.*\bright\?",
]


def _has_punchline(text: str) -> bool:
    """True if modern punchline structure is present.

    Punchlines are a *structural* tone failure — there is no single word to
    strip — so a hit means the response must be regenerated (gm-B-04), not
    locally repaired.
    """
    import re

    for pattern in _PUNCHLINE_PATTERNS:
        if re.search(pattern, text):
            logger.warning("Punchline pattern detected: %s", pattern)
            return True
    return False


def _banned_words_in(text: str) -> set[str]:
    """Return the set of banned modern-slang words present in text."""
    words = {w.strip(".,!?;:\"'()") for w in text.lower().split()}
    return words & BANNED_WORDS


def _tone_check(text: str) -> bool:
    """Light tone lint — reject if modern slang or punchline structure detected."""
    violations = _banned_words_in(text)
    if violations:
        logger.warning("Tone violation: %s", violations)
        return False
    return not _has_punchline(text)


def _tone_repair(text: str) -> str | None:
    """Repair a tone-only miss in place; return the cleaned text.

    gm-B-04 — a valid, schema-conforming narration whose ONLY tone problem is
    a banned slang word does not deserve a full multi-second GM round-trip to
    re-fail on the same prompt. We strip the offending word(s) locally and
    accept, collapsing the whitespace they leave behind. Returns:

      - the original text unchanged when it was already clean,
      - a cleaned copy when banned words were present and removed,
      - None when the failure is structural (a punchline pattern) and cannot
        be fixed by word removal — the caller should regenerate with a nudge.
    """
    import re

    if _has_punchline(text):
        return None

    violations = _banned_words_in(text)
    if not violations:
        return text

    # Strip each banned word as a whole token, case-insensitively, then
    # tidy the doubled spaces / dangling punctuation the removal leaves.
    cleaned = text
    for word in violations:
        cleaned = re.sub(rf"(?i)\b{re.escape(word)}\b", "", cleaned)
    # Drop space before punctuation a removed word left dangling.
    cleaned = re.sub(r"\s+([,.!?;:])", r"\1", cleaned)
    # Collapse runs of punctuation left adjacent (", ." → ".").
    cleaned = re.sub(r"([,;:])\s*([,.!?;:])", r"\2", cleaned)
    cleaned = re.sub(r"\s{2,}", " ", cleaned).strip()
    cleaned = re.sub(r"^[\s,;:]+", "", cleaned).strip()

    # If stripping the slang left nothing of substance, there is nothing to
    # accept — treat it as a hard miss so the caller regenerates/falls back.
    if not re.search(r"[A-Za-z]", cleaned):
        return None
    return cleaned


def _nudge_prompt(user: str) -> str:
    """Append a one-line tone nudge so a regeneration is less likely to repeat.

    gm-B-04 — when we DO spend a retry (only for structural punchline misses),
    we steer it rather than re-issuing the identical prompt that just failed.
    """
    nudge = (
        "\nTONE NOTE: Avoid modern punchline structure "
        "(no 'plot twist', 'spoiler alert', 'wait for it'). "
        "Keep the voice period-appropriate and grounded."
    )
    if nudge in user:
        return user
    return user + nudge


class _NarrationStreamer:
    """Incrementally decode one JSON string field's text from a raw stream.

    gm-feat-01 — the model streams its full JSON object as a sequence of raw
    text chunks (Ollama NDJSON `response` fragments). We accumulate the raw
    text for final parsing exactly as the non-streamed path does, and *in
    addition* surface the narration prose progressively: as soon as the
    ``"<key>":"`` value opens we decode and emit each new run of text up to
    (but not including) the unescaped closing quote.

    The decoder is deliberately small and tolerant of being fed arbitrary
    partial chunks — a quote, an escape, or a unicode `\\uXXXX` sequence may be
    split across two chunks. State carried between ``feed`` calls:

      - ``_in_value``   — have we crossed the opening quote of the value?
      - ``_done``       — has the closing quote been seen (stop emitting)?
      - ``_pending``    — a trailing backslash / partial escape held back until
                          the next chunk completes it.

    When the value never opens (e.g. the field is absent, or the JSON is
    malformed) nothing is emitted — the caller still parses the accumulated raw
    at the end, so fallback-never-bricks is preserved either way.
    """

    def __init__(self, key: str = "narration"):
        # The literal token that opens the value: e.g.  "narration":"
        # We match it ignoring whitespace around the colon by scanning the
        # accumulated raw for the key then the next opening quote.
        self._key = key
        self._raw = ""
        self._scanned = 0  # index in _raw up to which we've emitted/consumed
        self._in_value = False
        self._done = False
        self._pending = ""  # held-back partial escape ("\" or "\uAB")

    def feed(self, chunk: str) -> str:
        """Append a raw chunk; return the newly-decoded narration delta (maybe "")."""
        if self._done or not chunk:
            self._raw += chunk
            return ""
        self._raw += chunk

        if not self._in_value:
            # Look for  "<key>" : "  in the accumulated raw. Find the key,
            # then the next colon, then the opening quote of the value.
            anchor = f'"{self._key}"'
            ki = self._raw.find(anchor, self._scanned)
            if ki == -1:
                # Key not present yet; keep the tail in case the key token
                # itself is split across chunks.
                self._scanned = max(0, len(self._raw) - len(anchor))
                return ""
            colon = self._raw.find(":", ki + len(anchor))
            if colon == -1:
                return ""
            quote = self._raw.find('"', colon + 1)
            if quote == -1:
                return ""
            self._in_value = True
            self._scanned = quote + 1  # first char of the value's content

        return self._consume_value()

    def _consume_value(self) -> str:
        """Decode value characters from _scanned until close quote or run-out."""
        out: list[str] = []
        i = self._scanned
        n = len(self._raw)

        # Resume a partial escape held from the previous chunk.
        if self._pending:
            consumed, emitted, done_or_wait = self._resume_pending(i, n)
            if done_or_wait == "wait":
                return ""
            out.append(emitted)
            i = consumed

        while i < n:
            ch = self._raw[i]
            if ch == "\\":
                if i + 1 >= n:
                    # Escape opener at the very end; hold for next chunk.
                    self._pending = "\\"
                    self._scanned = n
                    return "".join(out)
                esc = self._raw[i + 1]
                if esc == "u":
                    if i + 6 > n:
                        self._pending = self._raw[i:n]
                        self._scanned = n
                        return "".join(out)
                    out.append(self._decode_u(self._raw[i + 2 : i + 6]))
                    i += 6
                else:
                    out.append(_ESCAPES.get(esc, esc))
                    i += 2
                continue
            if ch == '"':
                # Unescaped close quote — value complete.
                self._done = True
                self._scanned = i + 1
                return "".join(out)
            out.append(ch)
            i += 1

        self._scanned = i
        return "".join(out)

    def _resume_pending(self, i: int, n: int) -> tuple[int, str, str]:
        """Complete a held-back partial escape. Returns (new_i, emitted, status)."""
        p = self._pending
        if p == "\\":
            if i >= n:
                return i, "", "wait"
            esc = self._raw[i]
            if esc == "u":
                if i + 5 > n:
                    self._pending = "\\" + self._raw[i:n]
                    self._scanned = n
                    return i, "", "wait"
                self._pending = ""
                return i + 5, self._decode_u(self._raw[i + 1 : i + 5]), "ok"
            self._pending = ""
            return i + 1, _ESCAPES.get(esc, esc), "ok"
        # p is a partial "\uXX" sequence; gather to 6 chars total ("\uXXXX").
        need = 6 - len(p)
        if n - i < need:
            self._pending = p + self._raw[i:n]
            self._scanned = n
            return i, "", "wait"
        full = p + self._raw[i : i + need]
        self._pending = ""
        return i + need, self._decode_u(full[2:6]), "ok"

    @staticmethod
    def _decode_u(hexdigits: str) -> str:
        try:
            return chr(int(hexdigits, 16))
        except ValueError:
            return ""

    @property
    def raw(self) -> str:
        return self._raw


_ESCAPES = {
    '"': '"', "\\": "\\", "/": "/",
    "b": "\b", "f": "\f", "n": "\n", "r": "\r", "t": "\t",
}


def _safe_emit(on_token: Callable[[str], None] | None, delta: str) -> None:
    """Call on_token with a delta, swallowing any exception it raises.

    gm-feat-01 — the streaming callback runs UI code from inside the model
    loop. A buggy or slow renderer must never propagate an exception up and
    brick generation (fallback-never-bricks). A delta is only emitted when it
    is non-empty.
    """
    if on_token is None or not delta:
        return
    try:
        on_token(delta)
    except Exception as e:  # noqa: BLE001 — callback is untrusted UI code
        logger.warning("on_token callback raised (ignored): %s", e)


def _find_node(state: RunState):
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None
