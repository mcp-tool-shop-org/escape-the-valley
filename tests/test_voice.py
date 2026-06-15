"""Tests for voice bridge — graceful degradation, config, mapping."""

import struct
import sys
import time
import wave

import pytest

from escape_the_valley.voice import (
    DEFAULT_VOICE,
    PACE_SPEED,
    PROFILE_VOICE,
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
