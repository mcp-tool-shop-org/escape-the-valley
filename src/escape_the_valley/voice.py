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

    @property
    def available(self) -> bool:
        """True if voice-soundboard is installed."""
        return _HAS_VOICE

    def start(self) -> bool:
        """Initialize engine and start worker thread.

        Returns True if voice started successfully.
        """
        if not _HAS_VOICE or not self.config.enabled:
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
        except Exception:
            logger.warning("Voice DM failed to start", exc_info=True)
            self._engine = None
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
            except Exception:
                logger.warning("Voice playback error", exc_info=True)
                self._playing.clear()

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
            # Poll for completion or interrupt (hard timeout: 60s)
            deadline = _time.monotonic() + 60.0
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
                except FileNotFoundError:
                    logger.warning("No audio player found")
