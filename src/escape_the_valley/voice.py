"""Voice bridge — optional wrapper around voice-soundboard.

If voice-soundboard is not installed, all operations are no-ops.
Audio plays in a background daemon thread. Any key press cancels playback.
"""

from __future__ import annotations

import logging
import threading
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from queue import Empty, Queue
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .narration import NarrationEvent

logger = logging.getLogger(__name__)

# ── Optional import ────────────────────────────────────────────────

_HAS_VOICE = False

try:
    from voice_soundboard import Config as _VSConfig
    from voice_soundboard import VoiceEngine as _VSEngine

    _HAS_VOICE = True
except ImportError:
    _VSConfig = None
    _VSEngine = None


# ── Pace ───────────────────────────────────────────────────────────


class VoicePace(StrEnum):
    FAST = "fast"
    NORMAL = "normal"
    SLOW = "slow"


PACE_SPEED: dict[VoicePace, float] = {
    VoicePace.FAST: 1.15,
    VoicePace.NORMAL: 1.0,
    VoicePace.SLOW: 0.85,
}


# ── Profile-to-voice mapping (approved voices only) ───────────────

PROFILE_VOICE: dict[str, dict] = {
    "chronicler": {
        "voice": "bm_george",
        "style": "steadily and seriously",
        "speed_mult": 1.0,
    },
    "fireside": {
        "voice": "bm_lewis",
        "style": "warmly",
        "speed_mult": 0.95,
    },
    "lantern": {
        "voice": "am_fenrir",
        "style": "quietly and eerily",
        "speed_mult": 0.90,
    },
}

DEFAULT_VOICE = "bm_george"

# Safety cap for async playback if a clip's real duration can't be read.
_PLAYBACK_SAFETY_CAP_S = 60.0
# Small tail so the loop doesn't cut the last fraction of audio.
_PLAYBACK_TAIL_S = 0.25


def _wav_duration_seconds(path: Path) -> float | None:
    """Return a WAV clip's duration in seconds, or None if unreadable.

    Used to exit async (winsound) playback near natural completion instead
    of pinning the worker for the full safety cap.
    """
    import wave

    try:
        with wave.open(str(path), "rb") as wf:
            frames = wf.getnframes()
            rate = wf.getframerate()
        if rate <= 0:
            return None
        return frames / float(rate)
    except (OSError, wave.Error, EOFError):
        return None


# ── Config ─────────────────────────────────────────────────────────


@dataclass
class VoiceConfig:
    """Configuration for voice narration."""

    enabled: bool = False
    pace: VoicePace = VoicePace.NORMAL
    profile: str = "fireside"

    @property
    def voice_id(self) -> str:
        return PROFILE_VOICE.get(self.profile, {}).get("voice", DEFAULT_VOICE)

    @property
    def style(self) -> str:
        return PROFILE_VOICE.get(self.profile, {}).get("style", "")

    @property
    def speed(self) -> float:
        profile_mult = PROFILE_VOICE.get(self.profile, {}).get(
            "speed_mult", 1.0,
        )
        return PACE_SPEED[self.pace] * profile_mult


# ── Voice Bridge ───────────────────────────────────────────────────


class NoAudioPlayerError(RuntimeError):
    """Raised when no system audio player can be found to play a clip.

    gm-B-06 — this turns a previously-silent "No audio player found" log into a
    surfaced runtime failure the bridge records and the UI can read.
    """


class VoiceBridge:
    """Non-blocking voice playback bridge.

    If voice-soundboard is not installed, all methods are safe no-ops.
    """

    def __init__(self, config: VoiceConfig | None = None) -> None:
        self.config = config or VoiceConfig()
        self._engine = None
        self._queue: Queue[NarrationEvent | None] = Queue(maxsize=4)
        self._worker: threading.Thread | None = None
        self._stop = threading.Event()
        self._playing = threading.Event()
        # gm-B-06 — runtime voice honesty. _HAS_VOICE only says the library
        # imported; it cannot know whether an audio player exists or whether
        # the first synth/playback will raise. When the infra actually fails
        # at runtime we record the reason here and flip voice off, instead of
        # swallowing it to a silent log the player never sees. The UI reads
        # `status()` to notify the player that the DM has gone quiet and why.
        self._runtime_failed = False
        self.last_error: str | None = None

    @property
    def available(self) -> bool:
        """True if voice-soundboard is installed AND no runtime infra failure.

        gm-B-06 — distinct from `installed`: the library can be importable yet
        unusable at runtime (no audio player on PATH, synth raised). Once a
        runtime failure is recorded, voice is no longer available.
        """
        return _HAS_VOICE and not self._runtime_failed

    @property
    def installed(self) -> bool:
        """True if the voice-soundboard library is importable (no runtime claim)."""
        return _HAS_VOICE

    def status(self) -> dict:
        """Readable voice status for the UI (gm-B-06).

        Keys:
          - installed: voice-soundboard import succeeded
          - available: installed AND no runtime infra failure so far
          - enabled: the player has voice turned on in config
          - last_error: human-readable reason voice went quiet, or None
        """
        return {
            "installed": _HAS_VOICE,
            "available": self.available,
            "enabled": self.config.enabled,
            "last_error": self.last_error,
        }

    def _fail_runtime(self, reason: str) -> None:
        """Record a runtime infra failure, surface it, and flip voice off.

        gm-B-06 — the single place an infra failure becomes visible: set the
        last-error string the UI reads, mark voice unavailable, disable the
        config so nothing re-enqueues, and stop the worker. Never raises.
        """
        self.last_error = reason
        self._runtime_failed = True
        self.config.enabled = False
        logger.warning("Voice disabled: %s", reason)
        self._stop.set()

    def start(self) -> bool:
        """Initialize engine and start worker thread.

        Returns True if voice started successfully.
        """
        # gm-B-06 — once runtime infra has failed, do not keep retrying it; a
        # dead audio stack stays dead for the session.
        if not _HAS_VOICE or not self.config.enabled or self._runtime_failed:
            return False

        try:
            cache_dir = Path.home() / ".trail" / "voice_cache"
            self._engine = _VSEngine(_VSConfig(
                output_dir=cache_dir,
                default_voice=self.config.voice_id,
                default_speed=self.config.speed,
            ))
            self._stop.clear()
            self._worker = threading.Thread(
                target=self._worker_loop,
                daemon=True,
                name="voice-dm",
            )
            self._worker.start()
            logger.info(
                "Voice DM started: voice=%s pace=%s",
                self.config.voice_id, self.config.pace.value,
            )
            return True
        except Exception as exc:
            # gm-B-06 — engine init failed; record it and flip voice off so the
            # UI can say why, instead of a silent log.
            logger.warning("Voice DM failed to start", exc_info=True)
            self._engine = None
            self._fail_runtime(f"voice engine failed to start: {exc}")
            return False

    def enqueue(self, event: NarrationEvent) -> None:
        """Add a narration event to the playback queue."""
        if not self._engine or self._stop.is_set():
            return
        try:
            self._queue.put_nowait(event)
        except Exception:
            # Queue full — drop oldest, add new
            try:
                self._queue.get_nowait()
            except Empty:
                pass
            try:
                self._queue.put_nowait(event)
            except Exception:
                pass

    def interrupt(self) -> None:
        """Stop current audio and drain queue."""
        self._playing.clear()
        while not self._queue.empty():
            try:
                self._queue.get_nowait()
            except Empty:
                break
        # Stop Windows audio playback
        try:
            import winsound
            winsound.PlaySound(None, winsound.SND_PURGE)
        except Exception:
            pass

    def stop(self) -> None:
        """Shut down the worker thread."""
        self._stop.set()
        self.interrupt()
        try:
            self._queue.put_nowait(None)
        except Exception:
            pass
        if self._worker and self._worker.is_alive():
            self._worker.join(timeout=2.0)

    def toggle(self) -> bool:
        """Toggle voice on/off. Returns new state."""
        if self.config.enabled:
            self.config.enabled = False
            self.interrupt()
            return False
        self.config.enabled = True
        if not self._engine:
            self.start()
        return True

    # ── Worker thread ──────────────────────────────────────────────

    def _worker_loop(self) -> None:
        """Background: pull events, synthesize, play."""
        import time as _time

        while not self._stop.is_set():
            try:
                event = self._queue.get(timeout=0.5)
            except Empty:
                continue

            if event is None or self._stop.is_set():
                break

            try:
                if event.pause_before_ms > 0:
                    _time.sleep(event.pause_before_ms / 1000.0)

                if self._stop.is_set():
                    break

                result = self._engine.speak(
                    event.voice_text,
                    voice=self.config.voice_id,
                    speed=self.config.speed,
                    style=self.config.style,
                )

                self._playing.set()
                self._play_audio(result.audio_path)
                self._playing.clear()
            except Exception as exc:
                # gm-B-06 — the first synth/playback failure is real infra
                # trouble (missing player, broken engine). Surface it and flip
                # voice off rather than logging into the void every turn.
                self._playing.clear()
                self._fail_runtime(f"voice playback failed: {exc}")
                break

    def _play_audio(self, path: Path) -> None:
        """Play WAV with interrupt support."""
        import sys
        import time as _time

        if sys.platform == "win32":
            import winsound

            winsound.PlaySound(
                str(path),
                winsound.SND_FILENAME
                | winsound.SND_ASYNC
                | winsound.SND_NODEFAULT,
            )
            # winsound plays async and never signals completion, so derive
            # the real clip length and exit near it. The 60s cap is only a
            # safety net for clips whose duration we can't read.
            duration = _wav_duration_seconds(path)
            if duration is not None:
                play_until = min(
                    duration + _PLAYBACK_TAIL_S, _PLAYBACK_SAFETY_CAP_S
                )
            else:
                play_until = _PLAYBACK_SAFETY_CAP_S
            deadline = _time.monotonic() + play_until
            while self._playing.is_set() and not self._stop.is_set():
                if _time.monotonic() >= deadline:
                    break
                _time.sleep(0.05)
            winsound.PlaySound(None, winsound.SND_PURGE)
        else:
            import subprocess

            try:
                subprocess.run(
                    ["aplay", "-q", str(path)],
                    timeout=30,
                    check=False,
                )
            except FileNotFoundError:
                try:
                    subprocess.run(
                        [
                            "ffplay", "-nodisp", "-autoexit",
                            "-loglevel", "quiet", str(path),
                        ],
                        timeout=30,
                        check=False,
                    )
                except FileNotFoundError as exc:
                    # gm-B-06 — no player on PATH. Raise so the worker records
                    # it via _fail_runtime instead of swallowing to a log.
                    raise NoAudioPlayerError(
                        "no audio player found (tried aplay, ffplay)"
                    ) from exc
