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
# F-a4bc65d6 — every other external call in this file is time-bounded
# (subprocess timeout=30, the winsound safety cap above); engine.speak()
# wasn't. Bound it too so a stalled model load / wedged audio backend /
# hung network TTS call can't freeze the worker thread forever.
_SPEAK_TIMEOUT_S = 20.0


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
        # F-a4bc65d6 — instance attribute (not just a module constant) so
        # tests can shrink it instead of waiting out a real hang.
        self._speak_timeout_s = _SPEAK_TIMEOUT_S

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

        F-1c310d6b / F-b68b2a77 — also clear `_engine`/`_worker` here so
        neither `enqueue()` (gated on `_engine` truthiness) nor `start()`
        (gated on worker liveness, via `toggle()`) can be fooled by a stale
        reference into treating a dead bridge as still running.
        """
        self.last_error = reason
        self._runtime_failed = True
        self.config.enabled = False
        logger.warning("Voice disabled: %s", reason)
        self._stop.set()
        self._engine = None
        self._worker = None

    def start(self) -> bool:
        """Initialize engine and start worker thread.

        Returns True if voice is running after this call — either a fresh
        worker was started successfully, or one was already alive. This is
        the single source of truth `toggle()` defers to; it deliberately
        does NOT trust `self._engine`'s truthiness, since a dead worker can
        leave a stale-but-truthy engine reference behind (F-1c310d6b).
        """
        # gm-B-06 — once runtime infra has failed, do not keep retrying it; a
        # dead audio stack stays dead for the session.
        if not _HAS_VOICE or not self.config.enabled or self._runtime_failed:
            return False

        # F-1c310d6b — already running: don't stack a second engine/worker
        # on top of a live one (e.g. a healthy toggle-off/toggle-on cycle).
        # Liveness, not `_engine` truthiness, is the source of truth.
        if self._worker is not None and self._worker.is_alive():
            return True

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
            # gm-B-06 — engine init failed; record it and flip voice off so
            # the UI can say why, instead of a silent log. _fail_runtime()
            # clears _engine/_worker too.
            logger.warning("Voice DM failed to start", exc_info=True)
            self._fail_runtime(f"voice engine failed to start: {exc}")
            return False

    def enqueue(self, event: NarrationEvent) -> None:
        """Add a narration event to the playback queue.

        F-b68b2a77 — `None` is `stop()`'s internal shutdown sentinel.
        Reject it unconditionally so only `stop()`'s own `put_nowait(None)`
        can ever place one on the queue; a `None` from any other caller
        would otherwise make the worker exit exactly like a real stop,
        while every health flag is left reporting healthy.
        """
        if event is None or not self._engine or self._stop.is_set():
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
        # F-1c310d6b — a stopped bridge has no live engine/worker; clear the
        # references so a later start() rebuilds from scratch instead of
        # trusting stale objects.
        self._engine = None
        self._worker = None

    def toggle(self) -> bool:
        """Toggle voice on/off. Returns whether voice is running afterward.

        F-1c310d6b — the on-branch used to gate restart on `self._engine`
        being a truthy object and then unconditionally return True, so a
        stale post-failure engine reference made this method claim success
        on a recovery attempt that never actually restarted anything.
        `start()` is now the single source of truth for liveness (it
        no-ops safely if voice is already running, and fails honestly if
        the runtime previously died) — this method just relays its answer.
        """
        if self.config.enabled:
            self.config.enabled = False
            self.interrupt()
            return False
        self.config.enabled = True
        return self.start()

    # ── Worker thread ──────────────────────────────────────────────

    def _worker_loop(self) -> None:
        """Background: pull events, synthesize, play."""
        import time as _time

        while not self._stop.is_set():
            try:
                event = self._queue.get(timeout=0.5)
            except Empty:
                continue

            if event is None:
                if self._stop.is_set():
                    # Expected shutdown sentinel from stop().
                    break
                # F-b68b2a77 — enqueue() rejects None, so the only way one
                # reaches here unexpectedly is a future caller bug writing
                # to the queue directly. Exiting silently would leave
                # status()/available reporting healthy forever while
                # nothing is ever spoken again — the exact pattern this
                # module exists to avoid. Surface it like any other
                # failure instead of a silent break.
                self._fail_runtime(
                    "voice worker received an unexpected stop signal"
                )
                break

            if self._stop.is_set():
                break

            try:
                if event.pause_before_ms > 0:
                    _time.sleep(event.pause_before_ms / 1000.0)

                if self._stop.is_set():
                    break

                result = self._speak_with_timeout(event)

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

    def _speak_with_timeout(self, event: NarrationEvent):
        """Run engine.speak() off-thread, bounded by `_speak_timeout_s`.

        F-a4bc65d6 — speak() is an uncontrolled external call (first-run
        model load, native audio backend, network-backed TTS); every other
        external call in this file is time-bounded and this one wasn't.
        Run it on a helper daemon thread so a hang can be detected instead
        of wedging the worker thread forever, then raise so the caller's
        existing except-block routes it into `_fail_runtime` exactly like
        any other synth/playback failure. Python cannot forcibly cancel a
        blocked call, so a genuine hang leaves the helper thread abandoned
        (daemon, so it can't block process exit) rather than actually
        stopped.
        """
        result_box = []
        error_box: list[Exception] = []

        def _run() -> None:
            try:
                result_box.append(self._engine.speak(
                    event.voice_text,
                    voice=self.config.voice_id,
                    speed=self.config.speed,
                    style=self.config.style,
                ))
            except Exception as exc:
                error_box.append(exc)

        runner = threading.Thread(target=_run, daemon=True, name="voice-speak")
        runner.start()
        runner.join(timeout=self._speak_timeout_s)

        if runner.is_alive():
            # Still blocked past the deadline — abandon it and fail over.
            raise TimeoutError(
                f"voice engine speak() timed out after "
                f"{self._speak_timeout_s}s"
            )
        if error_box:
            raise error_box[0]
        return result_box[0]

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
