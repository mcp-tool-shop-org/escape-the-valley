"""Tests for voice bridge — graceful degradation, config, mapping."""

import struct
import sys
import threading
import time
import wave

import pytest

from escape_the_valley.voice import (
    DEFAULT_VOICE,
    PACE_SPEED,
    PROFILE_VOICE,
    NoAudioPlayerError,
    VoiceBridge,
    VoiceConfig,
    VoicePace,
    _wav_duration_seconds,
)


def _write_wav(path, seconds: float, rate: int = 8000) -> None:
    """Write a tiny silent mono WAV of the given duration."""
    nframes = int(seconds * rate)
    with wave.open(str(path), "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(rate)
        wf.writeframes(struct.pack("<" + "h" * nframes, *([0] * nframes)))


class TestVoiceConfig:
    def test_defaults(self):
        config = VoiceConfig()
        assert config.enabled is False
        assert config.pace == VoicePace.NORMAL
        assert config.profile == "fireside"

    def test_voice_id_from_profile(self):
        for profile, mapping in PROFILE_VOICE.items():
            config = VoiceConfig(profile=profile)
            assert config.voice_id == mapping["voice"]

    def test_unknown_profile_uses_default(self):
        config = VoiceConfig(profile="unknown")
        assert config.voice_id == DEFAULT_VOICE

    def test_speed_combines_pace_and_profile(self):
        config = VoiceConfig(pace=VoicePace.SLOW, profile="lantern")
        expected = PACE_SPEED[VoicePace.SLOW] * PROFILE_VOICE["lantern"]["speed_mult"]
        assert abs(config.speed - expected) < 0.01

    def test_style_from_profile(self):
        config = VoiceConfig(profile="chronicler")
        assert config.style == "steadily and seriously"


class TestVoicePace:
    def test_all_paces_have_speeds(self):
        for pace in VoicePace:
            assert pace in PACE_SPEED

    def test_fast_is_faster(self):
        assert PACE_SPEED[VoicePace.FAST] > PACE_SPEED[VoicePace.NORMAL]

    def test_slow_is_slower(self):
        assert PACE_SPEED[VoicePace.SLOW] < PACE_SPEED[VoicePace.NORMAL]


class TestProfileVoiceMapping:
    def test_all_profiles_mapped(self):
        for profile in ("chronicler", "fireside", "lantern"):
            assert profile in PROFILE_VOICE

    def test_approved_voices_only(self):
        approved = {
            "af_aoede", "af_jessica", "af_sky",
            "am_eric", "am_fenrir", "am_liam", "am_onyx",
            "bf_alice", "bf_emma", "bf_isabella",
            "bm_george", "bm_lewis",
        }
        for mapping in PROFILE_VOICE.values():
            assert mapping["voice"] in approved


class TestVoiceBridge:
    def test_not_started_is_safe(self):
        bridge = VoiceBridge()
        # These should all be no-ops, not crash
        bridge.enqueue(None)  # type: ignore
        bridge.interrupt()
        bridge.stop()

    def test_start_disabled_returns_false(self):
        bridge = VoiceBridge(VoiceConfig(enabled=False))
        assert bridge.start() is False

    def test_toggle_flips_state(self):
        config = VoiceConfig(enabled=True)
        bridge = VoiceBridge(config)
        # Toggle off
        assert bridge.toggle() is False
        assert config.enabled is False
        # Toggle on (start may fail without voice-soundboard, but state flips)
        bridge.toggle()
        assert config.enabled is True


class TestWavDuration:
    def test_reads_clip_duration(self, tmp_path):
        wav = tmp_path / "clip.wav"
        _write_wav(wav, seconds=0.4)
        dur = _wav_duration_seconds(wav)
        assert dur is not None
        assert abs(dur - 0.4) < 0.05

    def test_missing_file_returns_none(self, tmp_path):
        assert _wav_duration_seconds(tmp_path / "nope.wav") is None

    def test_non_wav_returns_none(self, tmp_path):
        bogus = tmp_path / "bogus.wav"
        bogus.write_bytes(b"not a wave file at all")
        assert _wav_duration_seconds(bogus) is None


class TestPlaybackDuration:
    @pytest.mark.skipif(
        sys.platform != "win32", reason="winsound playback path is Windows-only"
    )
    def test_short_clip_exits_near_duration_not_cap(self, tmp_path):
        # A short clip's playback loop must exit near the clip duration,
        # not pin the worker until the 60s safety cap (gm-A-003).
        wav = tmp_path / "short.wav"
        _write_wav(wav, seconds=0.3)
        bridge = VoiceBridge(VoiceConfig(enabled=True))
        bridge._playing.set()
        start = time.monotonic()
        bridge._play_audio(wav)
        elapsed = time.monotonic() - start
        assert elapsed < 5.0  # nowhere near the 60s safety cap


class TestVoiceRuntimeStatus:
    """gm-B-06 — voice infra honesty: surface failures, don't swallow them."""

    def test_status_shape_default(self):
        bridge = VoiceBridge(VoiceConfig(enabled=True))
        st = bridge.status()
        assert set(st) == {"installed", "available", "enabled", "last_error"}
        assert st["enabled"] is True
        assert st["last_error"] is None

    def test_fail_runtime_flips_voice_off_and_records_reason(self):
        bridge = VoiceBridge(VoiceConfig(enabled=True))
        bridge._fail_runtime("no audio player found")
        assert bridge.available is False  # even if the library is installed
        assert bridge.config.enabled is False  # voice flipped off
        assert bridge.last_error == "no audio player found"
        st = bridge.status()
        assert st["available"] is False
        assert st["last_error"] == "no audio player found"

    def test_start_is_noop_after_runtime_failure(self):
        bridge = VoiceBridge(VoiceConfig(enabled=True))
        bridge._fail_runtime("dead audio stack")
        # config got flipped off; force it back on as a paranoid caller might.
        bridge.config.enabled = True
        # A dead audio stack stays dead — start must not pretend otherwise.
        assert bridge.start() is False

    def test_no_audio_player_raises(self, tmp_path, monkeypatch):
        # gm-B-06 — the previously-silent "No audio player found" path now
        # raises a typed error the worker turns into a surfaced failure.
        import subprocess

        wav = tmp_path / "clip.wav"
        _write_wav(wav, seconds=0.1)

        def _no_player(*_a, **_k):
            raise FileNotFoundError("not installed")

        monkeypatch.setattr(subprocess, "run", _no_player)
        monkeypatch.setattr(sys, "platform", "linux")
        bridge = VoiceBridge(VoiceConfig(enabled=True))
        with pytest.raises(NoAudioPlayerError):
            bridge._play_audio(wav)

    def test_worker_records_playback_failure_and_disables(self):
        # Drive the worker loop with a fake engine whose speak() raises; the
        # first failure must flip voice off and record last_error, not loop
        # forever logging silently.
        from escape_the_valley.narration import NarrationEvent, NarrationType

        bridge = VoiceBridge(VoiceConfig(enabled=True))

        class _BoomEngine:
            def speak(self, *_a, **_k):
                raise RuntimeError("synth backend gone")

        bridge._engine = _BoomEngine()
        bridge._stop.clear()
        worker = threading.Thread(target=bridge._worker_loop, daemon=True)
        worker.start()
        bridge._queue.put(NarrationEvent(
            type=NarrationType.SCENE_OPEN, voice_text="The river is wide.",
        ))
        worker.join(timeout=3.0)
        assert not worker.is_alive()  # worker exited, not stuck
        assert bridge.available is False
        assert bridge.config.enabled is False
        assert bridge.last_error is not None
        assert "synth backend gone" in bridge.last_error
