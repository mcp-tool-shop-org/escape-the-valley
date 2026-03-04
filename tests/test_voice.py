"""Tests for voice bridge — graceful degradation, config, mapping."""

from escape_the_valley.voice import (
    DEFAULT_VOICE,
    PACE_SPEED,
    PROFILE_VOICE,
    VoiceBridge,
    VoiceConfig,
    VoicePace,
)


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
