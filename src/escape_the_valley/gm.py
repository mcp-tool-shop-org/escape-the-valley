"""Ollama GM integration — 3 profiles, two-prompt loop, graceful fallback."""

from __future__ import annotations

import json
import logging
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
    ) -> SceneResponse | None:
        """Generate a scene narration from the GM. Returns None on failure."""
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
    ) -> OutcomeResponse | None:
        """Generate outcome narration. Returns None on failure."""
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
        )

    def _request_scene(
        self, system: str, user: str, requested_profile: str = "",
    ) -> SceneResponse | None:
        """Make the Ollama request with retry + validation.

        gm-B-03 — each branch bumps self.stats so the 'stats' command can
        report attempts/successes and *why* the GM fell back.
        gm-B-04 — a tone-only miss (valid JSON+schema, narration present, the
        ONLY problem is a banned word) is repaired locally rather than spending
        a full multi-second round-trip to re-fail identically.
        """
        for attempt in range(self.config.max_retries + 1):
            try:
                self.stats["attempts"] += 1
                resp = self._client.post(
                    self.config.generate_url,
                    json={
                        "model": self.config.model,
                        "prompt": user,
                        "system": system,
                        "stream": False,
                        "options": {"temperature": 0.7},
                    },
                )
                if resp.status_code != 200:
                    logger.warning("GM returned %d", resp.status_code)
                    self.stats["json_rejects"] += 1
                    continue

                text = resp.json().get("response", "")
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
        self, system: str, user: str, requested_profile: str = "",
    ) -> OutcomeResponse | None:
        """Make outcome request with retry. See _request_scene for stats/repair."""
        for _attempt in range(self.config.max_retries + 1):
            try:
                self.stats["attempts"] += 1
                resp = self._client.post(
                    self.config.generate_url,
                    json={
                        "model": self.config.model,
                        "prompt": user,
                        "system": system,
                        "stream": False,
                        "options": {"temperature": 0.7},
                    },
                )
                if resp.status_code != 200:
                    self.stats["json_rejects"] += 1
                    continue

                text = resp.json().get("response", "")
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


def _find_node(state: RunState):
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None
