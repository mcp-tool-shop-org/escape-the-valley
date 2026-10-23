"""Tests for CLI stats and --version features."""

import json

from typer.testing import CliRunner

from escape_the_valley.cli import app
from escape_the_valley.models import (
    GMProfile,
    PartyMember,
    PartyState,
    RunState,
    SuppliesState,
    WagonState,
)
from escape_the_valley.worldgen import create_new_run

runner = CliRunner()


class TestVersionFlag:
    def test_version_flag(self):
        """--version outputs version string and exits 0."""
        result = runner.invoke(app, ["--version"])
        assert result.exit_code == 0
        assert "trail" in result.output

    def test_version_short_flag(self):
        """- V outputs version string."""
        result = runner.invoke(app, ["-V"])
        assert result.exit_code == 0
        assert "trail" in result.output

    def test_version_command(self):
        """trail version still works."""
        result = runner.invoke(app, ["version"])
        assert result.exit_code == 0
        assert "Ledger Trail" in result.output


class TestStatsCommand:
    def test_stats_no_save(self, monkeypatch):
        """Stats with no save prints message and exits 0."""
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: None)
        result = runner.invoke(app, ["stats"])
        assert result.exit_code == 0
        assert "No saved game" in result.output

    def test_stats_with_save(self, monkeypatch):
        """Stats with a saved game shows run summary."""
        state = RunState(
            run_id="test-run",
            seed=42,
            day=5,
            distance_traveled=120,
            total_distance=500,
            distance_remaining=380,
            gm_profile=GMProfile.FIRESIDE,
            party=PartyState(
                members=[
                    PartyMember(name="Alice", health=80),
                    PartyMember(name="Bob", health=60),
                    PartyMember(name="Charlie", health=0),
                ],
            ),
            wagon=WagonState(condition=80),
            supplies=SuppliesState(),
        )
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        result = runner.invoke(app, ["stats"])
        assert result.exit_code == 0
        assert "test-run" in result.output
        assert "2/3 alive" in result.output
        assert "24%" in result.output

    def test_stats_json(self, monkeypatch):
        """Stats --json outputs valid JSON."""
        state = RunState(
            run_id="json-run",
            seed=99,
            day=3,
            party=PartyState(
                members=[PartyMember(name="Solo", health=100)],
            ),
            wagon=WagonState(condition=100),
            supplies=SuppliesState(),
        )
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        result = runner.invoke(app, ["stats", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert data["run_id"] == "json-run"
        assert data["party_alive"] == 1
        assert data["wagon_condition"] == 100

    def test_stats_gm_flag_surfaces_counters(self, monkeypatch):
        """gm-B-03 consumer: `stats --gm` surfaces the GMClient.stats shape."""
        state = create_new_run(seed=5)
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)

        from escape_the_valley.gm import GMClient

        # Probe must not hit the network; force a deterministic 'unreachable'.
        monkeypatch.setattr(GMClient, "is_available", lambda self: False)

        result = runner.invoke(app, ["stats", "--gm"])
        assert result.exit_code == 0
        assert "GM calls (this session)" in result.output
        assert "attempts" in result.output
        assert "unreachable" in result.output

    def test_stats_gm_json_includes_block(self, monkeypatch):
        """`stats --gm --json` includes a structured gm block with counters."""
        state = create_new_run(seed=5)
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)

        from escape_the_valley.gm import GMClient

        monkeypatch.setattr(GMClient, "is_available", lambda self: True)

        result = runner.invoke(app, ["stats", "--gm", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert "gm" in data
        assert data["gm"]["reachable"] is True
        # The full stats shape committed by gm-B-03 must be present.
        counters = data["gm"]["session_counters"]
        for key in (
            "attempts", "successes", "json_rejects",
            "tone_rejects", "timeouts", "connect_errors",
        ):
            assert key in counters

    def test_stats_without_gm_flag_omits_block(self, monkeypatch):
        """No --gm means no GM probe and no gm block (stays fast/offline)."""
        state = create_new_run(seed=5)
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)

        result = runner.invoke(app, ["stats", "--json"])
        assert result.exit_code == 0
        data = json.loads(result.output)
        assert "gm" not in data

    def test_stats_game_over(self, monkeypatch):
        """Stats shows outcome when game is over."""
        state = RunState(
            run_id="over-run",
            seed=1,
            day=10,
            game_over=True,
            victory=False,
            cause_of_death="starvation",
            party=PartyState(members=[]),
            wagon=WagonState(condition=0),
            supplies=SuppliesState(),
        )
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        result = runner.invoke(app, ["stats"])
        assert result.exit_code == 0
        assert "starvation" in result.output


class TestTuiOptions:
    """D3 / cli-tui-004: `trail tui` accepts --gm-profile and --weirdness.

    The README + handbook document these flags; without them a documented
    command errors live with `No such option`.
    """

    def test_tui_help_lists_new_options(self, monkeypatch):
        """--help shows the documented options (no `No such option`).

        Rich renders help in a width-sensitive panel: under a narrow no-TTY
        width (as on CI) it wraps long option lines, so a naive substring check
        on the raw output is flaky and platform-dependent. Pin a wide Rich
        console so the option names never wrap, then strip ANSI styling before
        asserting — deterministic across terminal width, platform, and CI.
        """
        import re

        from rich.console import Console
        from typer import rich_utils

        monkeypatch.setattr(
            rich_utils,
            "_get_rich_console",
            lambda stderr=False: Console(width=200, force_terminal=False),
        )
        result = runner.invoke(app, ["tui", "--help"])
        assert result.exit_code == 0
        clean = re.sub(r"\x1b\[[0-9;]*m", "", result.output)
        assert "--gm-profile" in clean
        assert "--weirdness" in clean

    def test_tui_params_exist_on_command(self):
        """The tui command registers gm-profile + weirdness params."""
        import inspect

        cmd = next(
            c for c in app.registered_commands
            if c.callback.__name__ == "tui"
        )
        params = set(inspect.signature(cmd.callback).parameters)
        assert "profile" in params
        assert "weirdness" in params

    def test_tui_documented_invocation_parses(self, monkeypatch):
        """`tui --gm-profile chronicler --weirdness 0 --seed 1` parses cleanly.

        We stub the engine + app so the TUI never launches; the point is that
        the option grammar is accepted and the new-run path is reached with the
        chosen profile/weirdness threaded into create_new_run.
        """
        captured = {}

        def _fake_create_new_run(seed=None, gm_profile=None, weirdness_level=2):
            captured["seed"] = seed
            captured["gm_profile"] = gm_profile
            captured["weirdness"] = weirdness_level
            return create_new_run(
                seed=seed,
                gm_profile=gm_profile or GMProfile.FIRESIDE,
                weirdness_level=weirdness_level,
            )

        class _FakeApp:
            def __init__(self, *a, **k):
                pass

            def run(self):
                pass

        monkeypatch.setattr(
            "escape_the_valley.cli.create_new_run", _fake_create_new_run,
        )
        monkeypatch.setattr(
            "escape_the_valley.tui_app.LedgerTrailApp", _FakeApp,
        )

        result = runner.invoke(
            app,
            ["tui", "--gm-profile", "chronicler", "--weirdness", "0", "--seed", "1"],
        )
        assert "No such option" not in result.output
        assert result.exit_code == 0
        assert captured["gm_profile"] == GMProfile.CHRONICLER
        assert captured["weirdness"] == 0
        assert captured["seed"] == 1

    def test_tui_invalid_profile_exits_nonzero(self):
        """An unknown profile is rejected before the TUI launches."""
        result = runner.invoke(app, ["tui", "--gm-profile", "bogus"])
        assert result.exit_code == 1
        assert "Unknown profile" in result.output

    def test_new_prints_doctrine_and_taboo(self, monkeypatch):
        """F-42243a2c: trail new names doctrine + taboo next to twists."""
        monkeypatch.setattr("escape_the_valley.cli.has_save", lambda: False)

        class _FakeEngine:
            def __init__(self, *a, **k):
                pass

            def run(self):
                pass

        monkeypatch.setattr("escape_the_valley.cli.GameEngine", _FakeEngine)
        result = runner.invoke(app, ["new", "--seed", "7", "--gm-off"])
        assert result.exit_code == 0
        assert "Twists:" in result.output
        assert "sick_season" in result.output
        assert "Doctrine:" in result.output
        assert "travel_light" in result.output
        assert "Taboo:" in result.output
        assert "leave_nothing" in result.output

    def test_tui_weirdness_clamped(self, monkeypatch):
        """Out-of-range weirdness is clamped to 0-3 before create_new_run."""
        captured = {}

        def _fake_create_new_run(seed=None, gm_profile=None, weirdness_level=2):
            captured["weirdness"] = weirdness_level
            return create_new_run(
                seed=seed,
                gm_profile=gm_profile or GMProfile.FIRESIDE,
                weirdness_level=weirdness_level,
            )

        class _FakeApp:
            def __init__(self, *a, **k):
                pass

            def run(self):
                pass

        monkeypatch.setattr(
            "escape_the_valley.cli.create_new_run", _fake_create_new_run,
        )
        monkeypatch.setattr(
            "escape_the_valley.tui_app.LedgerTrailApp", _FakeApp,
        )

        result = runner.invoke(app, ["tui", "--weirdness", "9"])
        assert result.exit_code == 0
        assert captured["weirdness"] == 3


def _enabled_state(**overrides):
    """A run with the backpack enabled and a real wallet address."""
    state = create_new_run(seed=5)
    state.backpack.enabled = True
    state.backpack.wallet_address = "rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh"
    state.backpack.wallet_secret = "sEDfakeseed"
    for k, v in overrides.items():
        setattr(state.backpack, k, v)
    return state


class TestParcelSendValidation:
    """cli-tui-002: address shape is validated before any network call."""

    def test_invalid_address_exits_nonzero_before_send(self, monkeypatch):
        """A garbage recipient exits 1 and never reaches send_parcel."""
        state = _enabled_state()
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)

        from escape_the_valley.backpack import BackpackManager

        called = {"send": False}

        def _spy_send(self, *a, **k):
            called["send"] = True
            raise AssertionError("send_parcel must not be reached")

        monkeypatch.setattr(BackpackManager, "send_parcel", _spy_send)

        result = runner.invoke(app, ["parcel", "send", "not-an-address", "food", "5"])
        assert result.exit_code == 1
        assert "ERR_BAD_ADDRESS" in result.output
        assert called["send"] is False

    def test_valid_address_passes_validation(self, monkeypatch):
        """A well-formed classic address passes the shape check and sends."""
        state = _enabled_state()
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        monkeypatch.setattr("escape_the_valley.save.save_game", lambda s: None)

        from escape_the_valley.backpack import BackpackManager, SendResult

        sent = {"called": False}

        def _ok_send(self, state, recipient, supply, amount):
            sent["called"] = True
            return SendResult(success=True, message="sent", txid="ABC")

        monkeypatch.setattr(BackpackManager, "send_parcel", _ok_send)
        monkeypatch.setattr(BackpackManager, "close", lambda self: None)

        # A different, valid classic address (not self).
        result = runner.invoke(
            app,
            ["parcel", "send", "rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe", "food", "5"],
        )
        assert result.exit_code == 0
        assert sent["called"] is True


class TestLedgerExitCodes:
    """cli-tui-003: ledger failures must surface via non-zero exit codes."""

    def test_settle_failure_exits_nonzero(self, monkeypatch):
        """A failed settle exits 1 so automation can detect it."""
        state = _enabled_state()
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        monkeypatch.setattr("escape_the_valley.save.save_game", lambda s: None)

        from escape_the_valley.backpack import BackpackManager, SettlementResult

        monkeypatch.setattr(
            BackpackManager, "settle",
            lambda self, st, loc: SettlementResult(success=False, message="network down"),
        )
        monkeypatch.setattr(BackpackManager, "close", lambda self: None)

        result = runner.invoke(app, ["ledger", "settle"])
        assert result.exit_code == 1
        assert "network down" in result.output

    def test_settle_success_exits_zero(self, monkeypatch):
        """A successful settle still exits 0."""
        state = _enabled_state()
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        monkeypatch.setattr("escape_the_valley.save.save_game", lambda s: None)

        from escape_the_valley.backpack import BackpackManager, SettlementResult

        monkeypatch.setattr(
            BackpackManager, "settle",
            lambda self, st, loc: SettlementResult(success=True, message="settled"),
        )
        monkeypatch.setattr(BackpackManager, "close", lambda self: None)

        result = runner.invoke(app, ["ledger", "settle"])
        assert result.exit_code == 0

    def test_reconcile_still_pending_exits_nonzero(self, monkeypatch):
        """Reconcile that leaves settlements pending exits 1."""
        from escape_the_valley.backpack_models import SettlementRecord

        state = _enabled_state()
        # One pending settlement that the retry will NOT clear.
        state.backpack.pending_settlements.append(
            SettlementRecord(day=2, location="Millford", status="pending"),
        )
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        monkeypatch.setattr("escape_the_valley.save.save_game", lambda s: None)

        from escape_the_valley.backpack import BackpackManager

        # _retry_pending is a no-op here → the pending one remains.
        monkeypatch.setattr(BackpackManager, "_retry_pending", lambda self, st: None)
        monkeypatch.setattr(BackpackManager, "close", lambda self: None)

        result = runner.invoke(app, ["ledger", "reconcile"])
        assert result.exit_code == 1
        assert "still pending" in result.output

    def test_reconcile_all_clear_exits_zero(self, monkeypatch):
        """Reconcile that clears all pending exits 0."""
        from escape_the_valley.backpack_models import SettlementRecord

        state = _enabled_state()
        state.backpack.pending_settlements.append(
            SettlementRecord(day=2, location="Millford", status="pending"),
        )
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        monkeypatch.setattr("escape_the_valley.save.save_game", lambda s: None)

        from escape_the_valley.backpack import BackpackManager

        def _clear(self, st):
            st.backpack.pending_settlements.clear()

        monkeypatch.setattr(BackpackManager, "_retry_pending", _clear)
        monkeypatch.setattr(BackpackManager, "close", lambda self: None)

        result = runner.invoke(app, ["ledger", "reconcile"])
        assert result.exit_code == 0
        assert "All clear" in result.output


class TestNetworkHints:
    """cli-tui-B-04: a failed network round-trip prints a structured hint."""

    def test_settle_failure_prints_hint(self, monkeypatch):
        state = _enabled_state()
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        monkeypatch.setattr("escape_the_valley.save.save_game", lambda s: None)

        from escape_the_valley.backpack import BackpackManager, SettlementResult

        monkeypatch.setattr(
            BackpackManager, "settle",
            lambda self, st, loc: SettlementResult(
                success=False, message="testnet timeout",
            ),
        )
        monkeypatch.setattr(BackpackManager, "close", lambda self: None)

        result = runner.invoke(app, ["ledger", "settle"])
        assert result.exit_code == 1
        assert "hint:" in result.output
        assert "trail ledger reconcile" in result.output

    def test_enable_failure_prints_hint(self, monkeypatch):
        state = create_new_run(seed=5)  # backpack disabled
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        monkeypatch.setattr("escape_the_valley.save.save_game", lambda s: None)

        from escape_the_valley.backpack import BackpackManager, EnableResult

        monkeypatch.setattr(
            BackpackManager, "enable",
            lambda self, st: EnableResult(success=False, message="unreachable"),
        )
        monkeypatch.setattr(BackpackManager, "close", lambda self: None)

        result = runner.invoke(app, ["ledger", "enable"])
        assert result.exit_code == 1
        assert "hint:" in result.output
        assert "trail ledger enable" in result.output

    def test_parcel_send_failure_prints_hint(self, monkeypatch):
        state = _enabled_state()
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        monkeypatch.setattr("escape_the_valley.save.save_game", lambda s: None)

        from escape_the_valley.backpack import BackpackManager, SendResult

        monkeypatch.setattr(
            BackpackManager, "send_parcel",
            lambda self, st, r, sup, amt: SendResult(
                success=False, message="submit failed",
            ),
        )
        monkeypatch.setattr(BackpackManager, "close", lambda self: None)

        result = runner.invoke(
            app,
            ["parcel", "send", "rPT1Sjq2YGrBMTttX4GZHjKu9dyfzbpAYe", "food", "5"],
        )
        assert result.exit_code == 1
        assert "hint:" in result.output

    def test_settle_success_no_hint(self, monkeypatch):
        """A successful settle does not nag with a network hint."""
        state = _enabled_state()
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        monkeypatch.setattr("escape_the_valley.save.save_game", lambda s: None)

        from escape_the_valley.backpack import BackpackManager, SettlementResult

        monkeypatch.setattr(
            BackpackManager, "settle",
            lambda self, st, loc: SettlementResult(success=True, message="settled"),
        )
        monkeypatch.setattr(BackpackManager, "close", lambda self: None)

        result = runner.invoke(app, ["ledger", "settle"])
        assert result.exit_code == 0
        assert "hint:" not in result.output


class TestPostcardCommand:
    """FEAT-CLITUI-01: `trail postcard` prints the retelling and can save it."""

    def test_postcard_no_save_prints_ledger(self, monkeypatch):
        state = create_new_run(seed=5)
        state.game_over = True
        state.victory = True
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        result = runner.invoke(app, ["postcard"])
        assert result.exit_code == 0
        # The plain trail ledger header is present (no backpack → not a postcard).
        assert "TRAIL LEDGER" in result.output

    def test_postcard_no_save_does_not_write_file(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        state = create_new_run(seed=5)
        state.game_over = True
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        runner.invoke(app, ["postcard"])
        assert not (tmp_path / ".trail").exists() or not list(
            (tmp_path / ".trail").glob("postcard-*.txt")
        )

    def test_postcard_save_writes_file(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        state = create_new_run(seed=5)
        state.game_over = True
        state.victory = True
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        result = runner.invoke(app, ["postcard", "--save"])
        assert result.exit_code == 0
        out_file = tmp_path / ".trail" / f"postcard-{state.run_id}.txt"
        assert out_file.exists()
        assert "TRAIL LEDGER" in out_file.read_text(encoding="utf-8")
        assert "Saved to" in result.output

    def test_postcard_with_receipts_uses_xrpl_variant(self, tmp_path, monkeypatch):
        from escape_the_valley.backpack_models import SettlementRecord

        monkeypatch.chdir(tmp_path)
        state = _enabled_state()
        state.game_over = True
        state.victory = True
        state.backpack.settlements.append(
            SettlementRecord(
                day=3, location="Millford", status="settled",
                deltas={"food": -4}, txids=["ABCDEF0123456789"],
            ),
        )
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
        result = runner.invoke(app, ["postcard", "--save"])
        assert result.exit_code == 0
        # The receipted postcard variant names the on-ledger receipts.
        assert "RECEIPTS ON LEDGER" in result.output

    def test_postcard_no_save_no_game(self, monkeypatch):
        monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: None)
        result = runner.invoke(app, ["postcard"])
        assert result.exit_code == 0
        assert "No saved game" in result.output

    def test_write_postcard_file_helper(self, tmp_path, monkeypatch):
        from escape_the_valley.cli import write_postcard_file

        monkeypatch.chdir(tmp_path)
        state = create_new_run(seed=5)
        state.game_over = True
        path = write_postcard_file(state)
        assert path.exists()
        assert path.name == f"postcard-{state.run_id}.txt"
        # Trailing newline, plain text, no Rich markup tags.
        text = path.read_text(encoding="utf-8")
        assert text.endswith("\n")
        assert "[/" not in text


class _FakeResp:
    def __init__(self, status_code=200, models=None):
        self.status_code = status_code
        self._models = models or []

    def json(self):
        return {"models": [{"name": n} for n in self._models]}


class TestSelfCheckHints:
    """cli-tui-B-05: self-check gives actionable next steps."""

    def test_model_present_matches_tagged_name(self):
        from escape_the_valley.cli import _model_present

        assert _model_present("llama3.2", ["llama3.2:latest"]) is True
        assert _model_present("llama3.2", ["llama3.2"]) is True
        assert _model_present("llama3.2:latest", ["llama3.2:latest"]) is True
        assert _model_present("mistral", ["llama3.2:latest"]) is False

    def test_unreachable_prints_start_or_gm_off(self, monkeypatch):
        import httpx

        def _boom(*a, **k):
            raise httpx.ConnectError("nope")

        monkeypatch.setattr(httpx, "get", _boom)
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "not reachable" in result.output
        assert "ollama serve" in result.output
        assert "--gm-off" in result.output

    def test_reachable_warns_on_missing_model(self, monkeypatch):
        import httpx

        monkeypatch.setattr(
            httpx, "get",
            lambda *a, **k: _FakeResp(200, models=["mistral:latest"]),
        )
        result = runner.invoke(app, ["self-check", "--model", "llama3.2"])
        assert result.exit_code == 0
        assert "reachable" in result.output
        assert "not found" in result.output
        assert "ollama pull llama3.2" in result.output

    def test_reachable_with_model_no_warning(self, monkeypatch):
        import httpx

        monkeypatch.setattr(
            httpx, "get",
            lambda *a, **k: _FakeResp(200, models=["llama3.2:latest"]),
        )
        result = runner.invoke(app, ["self-check", "--model", "llama3.2"])
        assert result.exit_code == 0
        assert "reachable" in result.output
        assert "not found" not in result.output


def _stub_ollama_down(monkeypatch):
    """Keep self-check tests offline — Ollama is not the probe under test."""
    import httpx

    def _boom(*a, **k):
        raise httpx.ConnectError("offline")

    monkeypatch.setattr(httpx, "get", _boom)


class TestSelfCheckOptionalProbes:
    """F-6af2cd01: event-library, voice, xrpl, tui.tcss — non-failing probes."""

    def test_source_names_optional_probes(self):
        """inspect.getsource(self_check) must name the optional axes."""
        import inspect

        from escape_the_valley.cli import self_check

        src = inspect.getsource(self_check)
        assert "_HAS_XRPL" in src
        assert "VoiceBridge" in src
        assert "VoiceConfig" in src
        assert "voice extra" in src
        assert "tui.tcss" in src
        assert "build_event_library" in src
        assert "_resolve_css_path" in src

    def test_reports_event_library_and_optionals(self, monkeypatch):
        _stub_ollama_down(monkeypatch)
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "Event library:" in result.output
        assert "xrpl extra" in result.output
        assert "voice extra" in result.output
        assert "tui.tcss" in result.output

    def test_event_library_count_meets_floor(self, monkeypatch):
        import re

        from escape_the_valley.cli import _EVENT_LIBRARY_FLOOR

        _stub_ollama_down(monkeypatch)
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        # Worktree ships event_skeletons.json; a real load is the probe.
        match = re.search(r"Event library:.*?(\d+)\s+events", result.output)
        assert match is not None
        assert int(match.group(1)) >= _EVENT_LIBRARY_FLOOR

    def test_low_event_count_prints_degraded(self, monkeypatch):
        _stub_ollama_down(monkeypatch)
        monkeypatch.setattr(
            "escape_the_valley.events.build_event_library",
            lambda: [object()] * 60,
        )
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "60" in result.output
        assert "event_skeletons.json" in result.output

    def test_xrpl_absent_prints_extra_hint(self, monkeypatch):
        _stub_ollama_down(monkeypatch)
        monkeypatch.setattr("escape_the_valley.backpack._HAS_XRPL", False)
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "xrpl extra absent" in result.output
        assert "escape-the-valley[xrpl]" in result.output

    def test_xrpl_present_prints_present(self, monkeypatch):
        _stub_ollama_down(monkeypatch)
        monkeypatch.setattr("escape_the_valley.backpack._HAS_XRPL", True)
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "xrpl extra present" in result.output

    def test_voice_absent_prints_extra_hint(self, monkeypatch):
        class _Missing:
            def __init__(self, config=None):
                pass

            def status(self):
                return {
                    "installed": False,
                    "available": False,
                    "enabled": False,
                    "last_error": None,
                }

            def start(self):
                raise AssertionError("self-check must not start the voice worker")

        _stub_ollama_down(monkeypatch)
        monkeypatch.setattr("escape_the_valley.voice.VoiceBridge", _Missing)
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "voice extra absent" in result.output
        assert "escape-the-valley[voice]" in result.output

    def test_voice_installed_but_not_live(self, monkeypatch):
        class _Dead:
            started = False

            def __init__(self, config=None):
                pass

            def status(self):
                return {
                    "installed": True,
                    "available": False,
                    "enabled": False,
                    "last_error": "No audio player found",
                }

            def start(self):
                _Dead.started = True
                raise AssertionError("self-check must not start the voice worker")

        _stub_ollama_down(monkeypatch)
        monkeypatch.setattr("escape_the_valley.voice.VoiceBridge", _Dead)
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "voice extra present but not live" in result.output
        assert "No audio player found" in result.output
        assert _Dead.started is False

    def test_voice_live_does_not_start_worker(self, monkeypatch):
        class _Live:
            started = False

            def __init__(self, config=None):
                pass

            def status(self):
                return {
                    "installed": True,
                    "available": True,
                    "enabled": False,
                    "last_error": None,
                }

            def start(self):
                _Live.started = True
                return True

        _stub_ollama_down(monkeypatch)
        monkeypatch.setattr("escape_the_valley.voice.VoiceBridge", _Live)
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "voice extra present" in result.output
        assert "live" in result.output
        assert _Live.started is False

    def test_css_missing_prints_degraded(self, tmp_path, monkeypatch):
        missing = tmp_path / "no-such" / "tui.tcss"
        _stub_ollama_down(monkeypatch)
        monkeypatch.setattr(
            "escape_the_valley.tui_app._resolve_css_path",
            lambda: str(missing),
        )
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "tui.tcss" in result.output
        assert "missing" in result.output
        assert "TUI stylesheet missing" in result.output

    def test_css_present_when_file_exists(self, tmp_path, monkeypatch):
        css = tmp_path / "tui.tcss"
        css.write_text("Screen { background: black; }\n", encoding="utf-8")
        _stub_ollama_down(monkeypatch)
        monkeypatch.setattr(
            "escape_the_valley.tui_app._resolve_css_path",
            lambda: str(css),
        )
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "TUI stylesheet found" in result.output
        assert "tui.tcss" in result.output

    def test_self_check_does_not_write_save(self, tmp_path, monkeypatch):
        """self-check must not call save_game into CWD (or the launch dir)."""
        from pathlib import Path

        _stub_ollama_down(monkeypatch)
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        cwd = Path.cwd()
        assert not (cwd / ".trail" / "run.json").exists()
        assert not (tmp_path / ".trail" / "run.json").exists()
        assert not (tmp_path / "_cwd" / ".trail" / "run.json").exists()

    def test_degraded_optionals_still_exit_zero(self, tmp_path, monkeypatch):
        missing_css = tmp_path / "gone" / "tui.tcss"
        _stub_ollama_down(monkeypatch)
        monkeypatch.setattr("escape_the_valley.backpack._HAS_XRPL", False)
        monkeypatch.setattr(
            "escape_the_valley.events.build_event_library",
            lambda: [object()] * 60,
        )
        monkeypatch.setattr(
            "escape_the_valley.tui_app._resolve_css_path",
            lambda: str(missing_css),
        )

        class _Missing:
            def __init__(self, config=None):
                pass

            def status(self):
                return {
                    "installed": False,
                    "available": False,
                    "enabled": False,
                    "last_error": None,
                }

        monkeypatch.setattr("escape_the_valley.voice.VoiceBridge", _Missing)
        result = runner.invoke(app, ["self-check"])
        assert result.exit_code == 0
        assert "xrpl extra absent" in result.output
        assert "voice extra absent" in result.output
        assert "event_skeletons.json" in result.output
        assert "missing" in result.output


class TestLedgerProofCommand:
    def test_help_lists_proof(self):
        result = runner.invoke(app, ["ledger", "--help"])
        assert result.exit_code == 0
        assert "proof" in result.output

    def test_no_save_exits_one(self):
        result = runner.invoke(app, ["ledger", "proof"])
        assert result.exit_code == 1
        assert "No saved game" in result.output


class TestV131CliFixes:
    """v1.3.1 player-facing CLI fixes: H-1, M-1, and Issue #19."""

    def test_cli_run_stats_shows_nonzero_memory_cards(self, tmp_path, monkeypatch):
        """H-1: trail stats after a CLI GameEngine run shows non-zero memory cards."""
        import escape_the_valley.engine as engine_mod
        from escape_the_valley.engine import GameEngine
        from escape_the_valley.gm import GMConfig
        from escape_the_valley.save import load_game, save_game

        monkeypatch.chdir(tmp_path)
        monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
        monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)
        monkeypatch.setattr(engine_mod, "show_game_over", lambda *a, **k: None)
        monkeypatch.setattr(
            engine_mod, "show_event_scene", lambda title, narr, choices: choices[0]["id"]
        )
        monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)
        monkeypatch.setattr(engine_mod, "show_route_choice", lambda conns: conns[0][0])

        state = create_new_run(seed=42)
        state.supplies.food = 0  # triggers Starvation crisis card on first travel
        eng = GameEngine(state, GMConfig(enabled=False))
        eng._do_travel()
        save_game(eng.state, tmp_path)

        loaded = load_game(tmp_path)
        assert loaded is not None
        assert len(loaded.memory_cards) > 0

        result = runner.invoke(app, ["stats"])
        assert result.exit_code == 0
        assert "Memories:  0 cards" not in result.output
        assert f"Memories:  {len(loaded.memory_cards)} cards" in result.output

        json_res = runner.invoke(app, ["stats", "--json"])
        assert json_res.exit_code == 0
        payload = json.loads(json_res.output)
        assert payload["memory_cards"] == len(loaded.memory_cards)
        assert payload["memory_cards"] > 0

    def test_cli_run_trail_ledger_shows_death_days(self, monkeypatch):
        """H-1 & M-1: Trail Ledger shows death days and canonical cause of death for CLI runs."""
        import escape_the_valley.engine as engine_mod
        from escape_the_valley.engine import GameEngine
        from escape_the_valley.gm import GMConfig
        from escape_the_valley.ledger import build_trail_ledger

        monkeypatch.setattr(engine_mod, "show_message", lambda *a, **k: None)
        monkeypatch.setattr(engine_mod, "show_status", lambda *a, **k: None)
        monkeypatch.setattr(engine_mod, "show_game_over", lambda *a, **k: None)
        monkeypatch.setattr(
            engine_mod, "show_event_scene", lambda title, narr, choices: choices[0]["id"]
        )
        monkeypatch.setattr(engine_mod, "show_outcome", lambda *a, **k: None)
        monkeypatch.setattr(engine_mod, "show_route_choice", lambda conns: conns[0][0])

        state = create_new_run(seed=42)
        state.supplies.food = 0
        state.supplies.water = 0
        for m in state.party.members:
            m.health = 5

        eng = GameEngine(state, GMConfig(enabled=False))
        eng._do_travel()
        eng._check_game_over()

        assert state.game_over is True
        assert state.cause_of_death in {"Starvation", "Dehydration"}
        ledger_lines = build_trail_ledger(state)
        ledger_text = "\n".join(ledger_lines)
        for m in state.party.members:
            cause_lower = (m.death_cause or "unknown").lower()
            assert f"  {m.name} -- died of {cause_lower}, day {state.day}" in ledger_text

    def test_cli_new_piped_cp1252_reaches_camp_prompt(self, tmp_path):
        """Issue #19: trail new piped under cp1252 reaches camp prompt without crash."""
        import os
        import subprocess
        import sys

        env = dict(os.environ)
        env["PYTHONIOENCODING"] = "cp1252"
        env.pop("PYTHONUTF8", None)

        proc = subprocess.run(
            [sys.executable, "-m", "escape_the_valley", "new", "--seed", "42", "--gm-off"],
            input="Q\n",
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            cwd=str(tmp_path),
            env=env,
            timeout=30,
        )
        assert proc.returncode == 0, f"stderr:\n{proc.stderr}\nstdout:\n{proc.stdout}"
        assert "UnicodeEncodeError" not in proc.stderr
        assert "What do you do?" in proc.stdout


class _FakeMeta:
    def __init__(self, data: dict, multi: dict | None = None):
        self._data = data
        self._multi = multi or {}

    def get(self, key: str, default=None):
        return self._data.get(key, default)

    def get_all(self, key: str, default=None):
        return self._multi.get(key, default)


class _FakeDist:
    def __init__(
        self,
        name: str,
        version: str = "1.0.0",
        license_expr: str = "MIT",
        requires_dist: list[str] | None = None,
        project_urls: list[str] | None = None,
        homepage: str | None = None,
        files: list | None = None,
        top_level: str = "",
    ):
        data = {
            "Name": name,
            "Version": version,
            "License-Expression": license_expr,
        }
        if homepage is not None:
            data["Home-page"] = homepage
        multi = {}
        if requires_dist is not None:
            multi["Requires-Dist"] = requires_dist
        if project_urls is not None:
            multi["Project-URL"] = project_urls
        self.metadata = _FakeMeta(data, multi)
        self.files = files or []
        self._top_level = top_level

    def read_text(self, filename: str) -> str | None:
        if filename == "top_level.txt":
            return self._top_level
        return None


class TestThirdPartyNotices:
    """Issue #28: third-party license notices generator and CLI commands."""

    @staticmethod
    def _pin_matching_interpreter(monkeypatch):
        """Pin interpreter version/tag to SOURCES.json when running under CI matrix 3.11/3.13."""
        monkeypatch.setattr(
            "escape_the_valley.notices.sys.version",
            "3.12.13 (main, May 10 2026, 00:00:00)",
        )
        monkeypatch.setattr(
            "escape_the_valley.notices._read_interpreter_pbs_build_tag",
            lambda: "20260510",
        )

    def test_vendored_licenses_sha256_match_sources_json(self):
        """CI check: every vendored file under _licenses/ matches SOURCES.json SHA-256."""
        import escape_the_valley.notices as notices_mod
        from escape_the_valley.notices import verify_vendored_licenses_integrity

        # No hand-written fallback constants exist on the module.
        assert not hasattr(notices_mod, "_CPYTHON_INCORPORATED_SOFTWARE_NOTICES")
        assert not hasattr(notices_mod, "_CPYTHON_PSF_FALLBACK")
        assert not hasattr(notices_mod, "_GAME_MIT_LICENSE_FALLBACK")

        sources = verify_vendored_licenses_integrity()
        assert sources["uv_version"] == "0.11.18"
        assert sources["python_version"] == "3.12.13"
        assert sources["pbs_build_tag"] == "20260510"
        assert set(sources["targets"]) == {
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
        }
        for target_name, target_info in sources["targets"].items():
            vendored_names = {
                entry["filename"]
                for entry in target_info["files"]
            }
            assert "LICENSE.openssl-1.1.txt" in vendored_names, target_name
            assert "LICENSE.openssl-3.txt" in vendored_names, target_name
            assert len(target_info["archive_license_files"]) == 19, target_name
        darwin_sys_libs = {
            item["name"]
            for item in sources["targets"]["aarch64-apple-darwin"]["system_libraries"]
        }
        assert darwin_sys_libs == {"libedit", "ncurses"}

    def test_gen_third_party_notices_includes_game_cpython_and_rich(
        self, tmp_path, monkeypatch
    ):
        """Generator includes game license, verbatim CPython + pbs texts, and rich."""
        import sys

        from escape_the_valley.notices import (
            generate_third_party_notices,
            write_third_party_notices,
        )

        self._pin_matching_interpreter(monkeypatch)
        out_path = write_third_party_notices(tmp_path / "THIRD_PARTY_NOTICES.txt")
        text = out_path.read_text(encoding="utf-8")
        py_ver = sys.version.split()[0]

        assert "Escape the Valley" in text
        assert "MIT License" in text
        assert "Copyright (c) 2026 mcp-tool-shop" in text
        assert "Name: Python (CPython Runtime and Standard Library)" in text
        assert f"Version: {py_ver}" in text
        assert "PYTHON SOFTWARE FOUNDATION LICENSE" in text
        assert "Licenses and Acknowledgements for Incorporated Software" in text
        assert "Mersenne Twister" in text
        assert "SipHash24" in text
        # Complete verbatim texts required by F1:
        # 1. OpenSSL full Apache-2.0 text:
        assert "Apache License\n                           Version 2.0, January 2004" in text
        assert "END OF TERMS AND CONDITIONS" in text
        # 2. bzip2 including liability paragraph:
        assert "Julian R Seward" in text
        assert (
            "IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY\n"
            "DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL"
        ) in text
        # 3. mpdecimal including liability paragraph:
        assert "Stefan Krah" in text
        assert (
            "IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE\n"
            "FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL"
        ) in text
        for c_lib in (
            "OpenSSL",
            "libffi",
            "zlib",
            "expat",
            "bzip2",
            "liblzma",
            "SQLite",
            "mpdecimal",
        ):
            assert c_lib in text
        # Every target's vendored set generates cleanly.
        for target in (
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
        ):
            target_text = generate_third_party_notices(target=target)
            assert f"Target: {target}" in target_text
            assert "END OF TERMS AND CONDITIONS" in target_text
        # CPython section appears right after the game's MIT license (before the Summary).
        game_idx = text.index("Copyright (c) 2026 mcp-tool-shop")
        cpython_idx = text.index("Name: Python (CPython Runtime and Standard Library)")
        summary_idx = text.index("Summary of Included Third-Party Distributions")
        assert game_idx < cpython_idx < summary_idx
        assert "Name: rich" in text
        assert "Will McGugan" in text
        assert "https://pypi.org/project/certifi/" in text
        assert "https://github.com/certifi/python-certifi" in text
        assert "site-packages" not in text

    def test_missing_cpython_license_file_fails_loudly(self, monkeypatch):
        """F2: missing CPython LICENSE file raises FileNotFoundError instead of partial fallback."""
        import pytest

        from escape_the_valley.notices import generate_third_party_notices

        self._pin_matching_interpreter(monkeypatch)
        monkeypatch.setattr(
            "escape_the_valley.notices._locate_cpython_license_file",
            lambda: None,
        )
        with pytest.raises(FileNotFoundError, match="CPython LICENSE.txt not found"):
            generate_third_party_notices()

    def test_interpreter_pbs_tag_version_and_target_match_checks(self, monkeypatch):
        """R&D addition A: generator refuses when pbs tag, version, or target mismatches."""
        import pytest

        from escape_the_valley.notices import generate_third_party_notices

        self._pin_matching_interpreter(monkeypatch)

        # 1. Mismatched pbs build tag refuses:
        monkeypatch.setattr(
            "escape_the_valley.notices._read_interpreter_pbs_build_tag",
            lambda: "20990101",
        )
        with pytest.raises(RuntimeError, match="build tag '20990101' does not match"):
            generate_third_party_notices()

        # 2. Mismatched CPython version refuses:
        monkeypatch.setattr(
            "escape_the_valley.notices._read_interpreter_pbs_build_tag",
            lambda: "20260510",
        )
        monkeypatch.setattr(
            "escape_the_valley.notices.sys.version",
            "3.13.13 (main, May 10 2026, 00:00:00)",
        )
        with pytest.raises(
            RuntimeError, match="CPython version '3.13.13' does not match"
        ):
            generate_third_party_notices()

        # 3. Mismatched target refuses:
        self._pin_matching_interpreter(monkeypatch)
        with pytest.raises(
            RuntimeError,
            match="does not cover target 'riscv64-unknown-linux-gnu'",
        ):
            generate_third_party_notices(target="riscv64-unknown-linux-gnu")

    def test_mpl_source_line_generated_for_fake_distribution(self, monkeypatch):
        """MPL/LGPL/GPL distributions receive a Source: line with PyPI + upstream repo URL."""
        from escape_the_valley.notices import generate_third_party_notices

        self._pin_matching_interpreter(monkeypatch)
        fake_mpl = _FakeDist(
            name="fake-mpl-pkg",
            version="2.4.0",
            license_expr="MPL-2.0",
            project_urls=["Source, https://github.com/example/fake-mpl-pkg"],
        )
        fake_mit = _FakeDist(
            name="fake-mit-pkg",
            version="1.0.0",
            license_expr="MIT",
            project_urls=["Source, https://github.com/example/fake-mit-pkg"],
        )
        text = generate_third_party_notices(distributions=[fake_mpl, fake_mit])
        assert (
            "Source: fake-mpl-pkg==2.4.0 — "
            "https://pypi.org/project/fake-mpl-pkg/2.4.0/#files | "
            "Upstream: https://github.com/example/fake-mpl-pkg"
        ) in text
        assert "Upstream: https://github.com/example/fake-mit-pkg" in text

    def test_types_stub_packages_excluded_and_packaging_not_excluded(self, monkeypatch):
        """Stub-only `types-*` dists are excluded; `packaging` is not in EXCLUDED_BUILD_TOOLS."""
        from escape_the_valley.notices import (
            EXCLUDED_BUILD_TOOLS,
            generate_third_party_notices,
        )

        self._pin_matching_interpreter(monkeypatch)
        assert "packaging" not in EXCLUDED_BUILD_TOOLS

        fake_root = _FakeDist(
            name="escape-the-valley",
            version="1.3.1",
            license_expr="MIT",
            requires_dist=[
                "Deprecated>=1.2.14",
                "types-Deprecated>=1.2.14",
                "types-requests",
            ],
        )
        fake_dep = _FakeDist(name="Deprecated", version="1.3.1", license_expr="MIT")
        fake_stub1 = _FakeDist(
            name="types-Deprecated", version="1.3.1", license_expr="Apache-2.0"
        )
        fake_stub2 = _FakeDist(
            name="types-requests", version="2.31.0", license_expr="Apache-2.0"
        )
        text = generate_third_party_notices(
            distributions=[fake_root, fake_dep, fake_stub1, fake_stub2]
        )
        assert "Name: Deprecated" in text
        assert "types-Deprecated" not in text
        assert "types-requests" not in text

    def test_frozen_meipass_bundled_file_found(self, tmp_path, monkeypatch):
        """With sys.frozen=True and sys._MEIPASS set, bundled notices under _MEIPASS resolve."""
        from escape_the_valley.notices import (
            get_third_party_notices_text,
            resolve_bundled_notices_path,
        )

        pkg_dir = tmp_path / "escape_the_valley"
        pkg_dir.mkdir(parents=True)
        bundled = pkg_dir / "THIRD_PARTY_NOTICES.txt"
        marker = "FROZEN_MEIPASS_SENTINEL_131\nName: rich\n"
        bundled.write_text(marker, encoding="utf-8")

        monkeypatch.setattr("escape_the_valley.notices.sys.frozen", True, raising=False)
        monkeypatch.setattr(
            "escape_the_valley.notices.sys._MEIPASS", str(tmp_path), raising=False
        )

        resolved = resolve_bundled_notices_path()
        assert resolved == bundled
        assert get_third_party_notices_text() == marker

        res = runner.invoke(app, ["licenses"])
        assert res.exit_code == 0
        assert res.output == marker

    def test_frozen_meipass_missing_file_raises_filenotfound(self, tmp_path, monkeypatch):
        """With sys.frozen=True and empty _MEIPASS, missing file raises FileNotFoundError."""
        import pytest

        from escape_the_valley.notices import (
            get_third_party_notices_text,
            resolve_bundled_notices_path,
        )

        monkeypatch.setattr("escape_the_valley.notices.sys.frozen", True, raising=False)
        monkeypatch.setattr(
            "escape_the_valley.notices.sys._MEIPASS", str(tmp_path), raising=False
        )

        assert resolve_bundled_notices_path() is None
        with pytest.raises(FileNotFoundError, match="Bundled THIRD_PARTY_NOTICES.txt"):
            get_third_party_notices_text()

    def test_verify_toc_against_notices_pass_and_fail(self, tmp_path, monkeypatch):
        """TOC check verifies exact two-way match and prune_notices_to_toc drops unbundled dists."""
        import pytest

        from escape_the_valley.notices import (
            generate_third_party_notices,
            prune_notices_to_toc,
            verify_toc_against_notices,
        )

        self._pin_matching_interpreter(monkeypatch)
        build_dir = tmp_path / "build" / "escape-the-valley"
        build_dir.mkdir(parents=True)
        toc_file = build_dir / "PYZ-00.toc"
        toc_file.write_text(
            "[\n"
            "  ('rich.console', '/v/site-packages/rich/console.py', 'PYMODULE'),\n"
            "  ('Crypto.Cipher', '/v/site-packages/Crypto/Cipher/__init__.py', 'PYMODULE'),\n"
            "]\n",
            encoding="utf-8",
        )

        dists = [
            _FakeDist(
                name="rich",
                top_level="rich\n",
                requires_dist=[
                    "pycryptodome",
                    "Deprecated",
                    "mdit-py-plugins",
                    "wrapt",
                ],
            ),
            _FakeDist(name="pycryptodome", top_level="Crypto\n"),
            _FakeDist(name="Deprecated", top_level="deprecated\n"),
            _FakeDist(name="mdit-py-plugins", top_level="mdit_py_plugins\n"),
            _FakeDist(name="wrapt", top_level="wrapt\n"),
        ]

        # Runtime pip walk keeps all 5 (including Deprecated, mdit-py-plugins, wrapt):
        unpruned = tmp_path / "THIRD_PARTY_NOTICES_UNPRUNED.txt"
        unpruned.write_text(
            generate_third_party_notices(distributions=dists),
            encoding="utf-8",
        )
        assert "Name: Deprecated" in unpruned.read_text(encoding="utf-8")
        assert "Name: mdit-py-plugins" in unpruned.read_text(encoding="utf-8")
        assert "Name: wrapt" in unpruned.read_text(encoding="utf-8")

        # Two-way TOC check fails when unbundled dists remain in notices:
        with pytest.raises(
            RuntimeError, match="deprecated, mdit-py-plugins, wrapt"
        ):
            verify_toc_against_notices(build_dir, unpruned, distributions=dists)

        # Pruning to TOC keeps only rich and pycryptodome (plus Python/PyInstaller):
        pruned_path = tmp_path / "THIRD_PARTY_NOTICES.txt"
        pruned_dists = prune_notices_to_toc(
            build_dir, pruned_path, distributions=dists
        )
        assert pruned_dists == ["pycryptodome", "rich"]
        pruned_text = pruned_path.read_text(encoding="utf-8")
        assert "Name: Python (CPython Runtime and Standard Library)" in pruned_text
        assert "Name: rich" in pruned_text
        assert "Name: pycryptodome" in pruned_text
        assert "Name: Deprecated" not in pruned_text
        assert "Name: mdit-py-plugins" not in pruned_text
        assert "Name: wrapt" not in pruned_text

        verified = verify_toc_against_notices(
            build_dir, pruned_path, distributions=dists
        )
        assert verified == ["pycryptodome", "rich"]

        # Two-way TOC check also fails when a bundled dist is missing from notices:
        notices_missing = tmp_path / "THIRD_PARTY_NOTICES_INCOMPLETE.txt"
        notices_missing.write_text("Name: rich\nVersion: 15.0.0\n", encoding="utf-8")
        with pytest.raises(RuntimeError, match="pycryptodome"):
            verify_toc_against_notices(
                build_dir, notices_missing, distributions=dists
            )

    def test_licenses_command_prints_bundled_file(self, tmp_path, monkeypatch):
        """`trail licenses` and `trail --licenses` print the bundled file when present."""
        bundled = tmp_path / "THIRD_PARTY_NOTICES.txt"
        marker = "BUNDLED_NOTICES_SENTINEL_131\nName: rich\n"
        bundled.write_text(marker, encoding="utf-8")

        monkeypatch.setattr(
            "escape_the_valley.notices.resolve_bundled_notices_path",
            lambda: bundled,
        )

        res_cmd = runner.invoke(app, ["licenses"])
        assert res_cmd.exit_code == 0
        assert res_cmd.output == marker

        res_flag = runner.invoke(app, ["--licenses"])
        assert res_flag.exit_code == 0
        assert res_flag.output == marker

    def test_licenses_command_generates_runtime_fallback(self, monkeypatch):
        """G1: pip install (no bundled file) runs `trail licenses` on unpatched interpreter."""
        import subprocess
        import sys

        from escape_the_valley.notices import get_third_party_notices_text

        monkeypatch.setattr(
            "escape_the_valley.notices.resolve_bundled_notices_path",
            lambda: None,
        )
        monkeypatch.setattr(
            "escape_the_valley.notices.sys.frozen",
            False,
            raising=False,
        )

        # 1. Real unpatched interpreter via CliRunner:
        res_cmd = runner.invoke(app, ["licenses"])
        assert res_cmd.exit_code == 0
        assert "Escape the Valley" in res_cmd.output
        assert "MIT License" in res_cmd.output
        assert "Name: rich" in res_cmd.output
        assert "Will McGugan" in res_cmd.output
        assert (
            "Note: CPython and python-build-standalone license texts "
            "apply to the standalone release binaries only."
        ) in res_cmd.output

        res_flag = runner.invoke(app, ["--licenses"])
        assert res_flag.exit_code == 0
        assert "Escape the Valley" in res_flag.output
        assert "Name: rich" in res_flag.output

        # 2. Real unpatched interpreter via subprocess (`licenses` & `--licenses`):
        for arg in ("licenses", "--licenses"):
            proc = subprocess.run(
                [sys.executable, "-m", "escape_the_valley", arg],
                capture_output=True,
                text=True,
                encoding="utf-8",
                timeout=30,
            )
            assert proc.returncode == 0, f"stderr:\n{proc.stderr}"
            assert "Escape the Valley" in proc.stdout
            assert "Name: rich" in proc.stdout

        # 3. Even on Python 3.13 / unknown OS target, runtime pip fallback never raises:
        monkeypatch.setattr(
            "escape_the_valley.notices.sys.version",
            "3.13.99 (main, Oct 23 2026, 00:00:00)",
        )
        monkeypatch.setattr(
            "escape_the_valley.notices._detect_target_triple",
            lambda: "riscv64-unknown-freebsd",
        )
        fallback_text = get_third_party_notices_text()
        assert "Escape the Valley" in fallback_text
        assert "Name: rich" in fallback_text
        assert "apply to the standalone release binaries only" in fallback_text

    def test_verify_toc_rejects_excluded_unbundled_extensions(
        self, tmp_path, monkeypatch
    ):
        """G2: --verify-toc fails if excluded_unbundled_extensions entry is in the TOC."""
        import subprocess
        import sys
        from pathlib import Path

        import pytest

        from escape_the_valley.notices import (
            generate_third_party_notices,
            verify_toc_against_notices,
        )

        self._pin_matching_interpreter(monkeypatch)
        build_dir = tmp_path / "build" / "escape-the-valley"
        build_dir.mkdir(parents=True)
        toc_file = build_dir / "Analysis-00.toc"
        toc_file.write_text(
            "[\n"
            "  ('rich.console', '/v/site-packages/rich/console.py', 'PYMODULE'),\n"
            "]\n",
            encoding="utf-8",
        )

        dists = [_FakeDist(name="rich", top_level="rich\n")]
        notices_path = tmp_path / "THIRD_PARTY_NOTICES.txt"
        notices_path.write_text(
            generate_third_party_notices(distributions=dists),
            encoding="utf-8",
        )

        # Clean TOC passes:
        assert verify_toc_against_notices(
            build_dir, notices_path, distributions=dists
        ) == ["rich"]

        # TOC containing _tkinter.cpython-312-x86_64-linux-gnu.so fails:
        tk_so = "_tkinter.cpython-312-x86_64-linux-gnu.so"
        toc_file.write_text(
            "[\n"
            "  ('rich.console', '/v/site-packages/rich/console.py', 'PYMODULE'),\n"
            f"  ('lib-dynload/{tk_so}', '/python/lib-dynload/{tk_so}', 'EXTENSION'),\n"
            "]\n",
            encoding="utf-8",
        )
        with pytest.raises(RuntimeError, match="_tkinter"):
            verify_toc_against_notices(build_dir, notices_path, distributions=dists)

        # Also fails via scripts/gen_third_party_notices.py --verify-toc:
        script_path = (
            Path(__file__).resolve().parents[1]
            / "scripts"
            / "gen_third_party_notices.py"
        )
        proc = subprocess.run(
            [
                sys.executable,
                str(script_path),
                "--verify-toc",
                str(build_dir),
                str(notices_path),
            ],
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=30,
        )
        assert proc.returncode != 0
        assert "_tkinter" in (proc.stderr + proc.stdout)

        # TOC containing _dbm also fails (tested against Linux target where _dbm is listed):
        dbm_so = "_dbm.cpython-312-x86_64-linux-gnu.so"
        toc_file.write_text(
            "[\n"
            "  ('rich.console', '/v/site-packages/rich/console.py', 'PYMODULE'),\n"
            f"  ('lib-dynload/{dbm_so}', '/python/lib-dynload/{dbm_so}', 'EXTENSION'),\n"
            "]\n",
            encoding="utf-8",
        )
        with pytest.raises(RuntimeError, match="_dbm"):
            verify_toc_against_notices(
                build_dir,
                notices_path,
                distributions=dists,
                target="x86_64-unknown-linux-gnu",
            )

        # Preservation test (#31.3): Windows (.pyd) and darwin (.so) excluded-extension
        # vectors are also rejected by verify_toc_against_notices:
        win_pyd = "_tkinter.pyd"
        toc_file.write_text(
            "[\n"
            "  ('rich.console', 'C:\\\\v\\\\site-packages\\\\rich\\\\console.py', 'PYMODULE'),\n"
            f"  ('{win_pyd}', 'C:\\\\uv-python\\\\DLLs\\\\{win_pyd}', 'EXTENSION'),\n"
            "]\n",
            encoding="utf-8",
        )
        with pytest.raises(RuntimeError, match="_tkinter"):
            verify_toc_against_notices(
                build_dir,
                notices_path,
                distributions=dists,
                target="x86_64-pc-windows-msvc",
            )

        darwin_so = "_tkinter.cpython-312-darwin.so"
        toc_file.write_text(
            "[\n"
            "  ('rich.console', '/v/site-packages/rich/console.py', 'PYMODULE'),\n"
            f"  ('lib-dynload/{darwin_so}', "
            f"'/opt/uv-python/lib/python3.12/lib-dynload/{darwin_so}', 'EXTENSION'),\n"
            "]\n",
            encoding="utf-8",
        )
        with pytest.raises(RuntimeError, match="_tkinter"):
            verify_toc_against_notices(
                build_dir,
                notices_path,
                distributions=dists,
                target="aarch64-apple-darwin",
            )

    def test_n1_n2_lf_normalization_no_cr_and_byte_for_byte_trail_licenses(
        self, tmp_path, monkeypatch
    ):
        """N1 & N2: every license text is normalised to LF before hashing and
        writing; THIRD_PARTY_NOTICES*.txt is written with newline='\\n' (0 '\\r'
        bytes); and `trail licenses` / `trail --licenses` output equals the
        bundled file byte for byte even on Windows."""
        import hashlib
        import json
        import subprocess
        import sys
        from pathlib import Path

        from escape_the_valley.notices import (
            prune_notices_to_toc,
            write_third_party_notices,
        )

        repo_root = Path(__file__).resolve().parents[1]
        self._pin_matching_interpreter(monkeypatch)

        # 1. Craft a distribution whose license file on disk has CRLF line endings:
        crlf_lic_bytes = (
            b"MIT License\r\n\r\nCopyright (c) 2026 CRLF Test\r\n"
            b"Permission is hereby granted.\r\n"
        )
        lf_lic_bytes = crlf_lic_bytes.replace(b"\r\n", b"\n")
        expected_lf_sha = hashlib.sha256(lf_lic_bytes).hexdigest()
        unexpected_crlf_sha = hashlib.sha256(crlf_lic_bytes).hexdigest()
        assert expected_lf_sha != unexpected_crlf_sha

        disk_lic = tmp_path / "site-packages" / "crlf_pkg-1.0.0.dist-info" / "LICENSE.txt"
        disk_lic.parent.mkdir(parents=True, exist_ok=True)
        disk_lic.write_bytes(crlf_lic_bytes)
        assert b"\r\n" in disk_lic.read_bytes()

        class _FakePkgPath:
            def __init__(self, rel: str, target_file: Path):
                self._rel = rel
                self._target_file = target_file

            def __str__(self) -> str:
                return self._rel

            def locate(self) -> Path:
                return self._target_file

        dist = _FakeDist(
            name="crlf-pkg",
            version="1.0.0",
            license_expr="MIT",
            top_level="crlf_pkg\n",
            files=[_FakePkgPath("crlf_pkg-1.0.0.dist-info/LICENSE.txt", disk_lic)],
        )

        # 2. Write THIRD_PARTY_NOTICES.txt and check LF hash + zero '\r' bytes:
        out_path = tmp_path / "THIRD_PARTY_NOTICES.txt"
        write_third_party_notices(
            out_path,
            distributions=[dist],
            release=True,
            target="x86_64-pc-windows-msvc",
        )
        raw_written = out_path.read_bytes()
        assert b"\r" not in raw_written, "Generated notices must contain zero '\\r' bytes"
        written_text = raw_written.decode("utf-8")
        assert f"SHA256: {expected_lf_sha}" in written_text
        assert unexpected_crlf_sha not in written_text

        # Game LICENSE header SHA256 must match SOURCES.json game_license.sha256 (LF):
        sources = json.loads(
            (repo_root / "src" / "escape_the_valley" / "_licenses" / "SOURCES.json").read_text(
                encoding="utf-8"
            )
        )
        lf_game_lic_sha = hashlib.sha256(
            (repo_root / "LICENSE").read_bytes().replace(b"\r\n", b"\n").replace(b"\r", b"\n")
        ).hexdigest()
        assert sources["game_license"]["sha256"] == lf_game_lic_sha
        assert f"SHA256: {lf_game_lic_sha}" in written_text

        # Every SHA256: <hex> line in THIRD_PARTY_NOTICES.txt across all 3 targets
        # must equal sha256((text_block + "\n").encode("utf-8")):
        import re

        sep = "=" * 80
        for tname in (
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
        ):
            t_path = tmp_path / f"THIRD_PARTY_NOTICES-{tname}.txt"
            write_third_party_notices(
                t_path, distributions=[dist], release=True, target=tname
            )
            t_raw = t_path.read_bytes()
            assert b"\r" not in t_raw
            t_lines = t_raw.decode("utf-8").split("\n")
            # Check game LICENSE header block:
            assert t_lines[3] == f"SHA256: {lf_game_lic_sha}"
            hdr_end = t_lines.index(sep, 5)
            hdr_text = "\n".join(t_lines[5:hdr_end]).strip()
            assert (
                hashlib.sha256((hdr_text + "\n").encode("utf-8")).hexdigest()
                == lf_game_lic_sha
            )
            # Check every '--- ... ---' block:
            heads = [
                idx
                for idx in range(len(t_lines) - 2)
                if re.fullmatch(r"--- (.+) ---", t_lines[idx])
                and t_lines[idx + 1].startswith("Source: ")
                and re.fullmatch(r"SHA256: [0-9a-f]{64}", t_lines[idx + 2])
            ]
            assert len(heads) >= 13
            for h_idx, start_line in enumerate(heads):
                limit = heads[h_idx + 1] if h_idx + 1 < len(heads) else len(t_lines)
                end_line = start_line + 3
                while end_line < limit and t_lines[end_line] != sep:
                    end_line += 1
                line_sha = t_lines[start_line + 2][8:]
                block_text = "\n".join(t_lines[start_line + 3 : end_line]).strip()
                computed_sha = hashlib.sha256(
                    (block_text + "\n").encode("utf-8")
                ).hexdigest()
                assert line_sha == computed_sha, (
                    f"{tname} block {t_lines[start_line]}: "
                    f"SHA256 line {line_sha} != sha256(text_block + '\\n') {computed_sha}"
                )

        # 3. prune_notices_to_toc also writes with newline='\n' and zero '\r' bytes:
        build_dir = tmp_path / "build"
        build_dir.mkdir()
        (build_dir / "Analysis-00.toc").write_text(
            "[('crlf_pkg', '/sp/site-packages/crlf_pkg/__init__.py', 'PYMODULE')]\n",
            encoding="utf-8",
        )
        prune_notices_to_toc(
            build_dir,
            out_path,
            distributions=[dist],
            release=True,
            target="x86_64-pc-windows-msvc",
        )
        pruned_bytes = out_path.read_bytes()
        assert b"\r" not in pruned_bytes

        # 4. Subprocess `trail licenses` and `trail --licenses` with bundled file
        #    must match `pruned_bytes` byte-for-byte (no Windows CRLF translation):
        for flag in ("licenses", "--licenses"):
            code = (
                "import pathlib, escape_the_valley.notices as n, escape_the_valley.cli as c; "
                f"n.resolve_bundled_notices_path = lambda: pathlib.Path({str(out_path)!r}); "
                f"c.app(args=[{flag!r}], prog_name='trail')"
            )
            proc = subprocess.run(
                [sys.executable, "-c", code],
                capture_output=True,
                timeout=30,
            )
            assert proc.returncode == 0, proc.stderr.decode("utf-8", errors="replace")
            assert b"\r" not in proc.stdout
            assert proc.stdout == pruned_bytes

    def test_gen_third_party_notices_prune_toc_honours_release_flag(
        self, tmp_path, monkeypatch
    ):
        """#31.1: `gen_third_party_notices.py --prune-toc` without `--release`
        must pass `release=False` (not hardcoded `release=True`)."""
        import runpy
        import sys
        from pathlib import Path

        import pytest

        import escape_the_valley.notices as notices_mod

        recorded: list[tuple[bool]] = []

        def _spy_prune(toc_dir, notices_path, *, release=False, **kwargs):
            recorded.append((release,))
            Path(notices_path).write_text("pruned\n", encoding="utf-8")
            return ["rich"]

        monkeypatch.setattr(notices_mod, "prune_notices_to_toc", _spy_prune)
        script_path = (
            Path(__file__).resolve().parents[1] / "scripts" / "gen_third_party_notices.py"
        )

        # Without --release -> release=False:
        monkeypatch.setattr(
            sys,
            "argv",
            [
                str(script_path),
                "--prune-toc",
                str(tmp_path / "build"),
                str(tmp_path / "THIRD_PARTY_NOTICES.txt"),
            ],
        )
        with pytest.raises(SystemExit) as exc_info:
            runpy.run_path(str(script_path), run_name="__main__")
        assert exc_info.value.code == 0
        assert recorded == [(False,)]

        # With --release -> release=True:
        recorded.clear()
        monkeypatch.setattr(
            sys,
            "argv",
            [
                str(script_path),
                "--release",
                "--prune-toc",
                str(tmp_path / "build"),
                str(tmp_path / "THIRD_PARTY_NOTICES.txt"),
            ],
        )
        with pytest.raises(SystemExit) as exc_info2:
            runpy.run_path(str(script_path), run_name="__main__")
        assert exc_info2.value.code == 0
        assert recorded == [(True,)]

    def test_verify_pbs_archive_accounting_pass_and_fail(self, tmp_path):
        """#31.2: verify_pbs_archive_accounting inspects the archive's
        python/licenses/ and python/PYTHON.json and fails if SOURCES.json does
        not account for every license file in files ∪ system_libraries ∪
        excluded_unbundled_extensions; also runnable via
        `scripts/gen_third_party_notices.py --verify-pbs-archive`."""
        import io
        import json
        import subprocess
        import sys
        import tarfile
        from pathlib import Path

        import pytest

        import escape_the_valley.notices as notices_mod

        repo_root = Path(__file__).resolve().parents[1]
        sources = json.loads(
            (repo_root / "src" / "escape_the_valley" / "_licenses" / "SOURCES.json").read_text(
                encoding="utf-8"
            )
        )
        target = "x86_64-unknown-linux-gnu"
        tinfo = sources["targets"][target]
        lic_dir = repo_root / "src" / "escape_the_valley" / "_licenses"

        def _build_tar(
            extra_files: dict[str, bytes] | None = None,
            python_json_obj: dict | None = None,
        ) -> Path:
            tar_path = tmp_path / f"pbs-{len(list(tmp_path.iterdir()))}.tar"
            with tarfile.open(tar_path, "w") as tf:
                # Populate all accounted archive_license_files:
                vendored_by_archive = {
                    e["archive_path"]: e["vendored_path"] for e in tinfo["files"]
                }
                for arch_path in tinfo["archive_license_files"]:
                    if arch_path in vendored_by_archive:
                        data = (lic_dir / vendored_by_archive[arch_path]).read_bytes()
                    else:
                        data = b"dummy license text\n"
                    info = tarfile.TarInfo(name=arch_path)
                    info.size = len(data)
                    tf.addfile(info, io.BytesIO(data))
                if extra_files:
                    for name, data in extra_files.items():
                        info = tarfile.TarInfo(name=name)
                        info.size = len(data)
                        tf.addfile(info, io.BytesIO(data))
                pj = python_json_obj or {
                    "python_license_path": "licenses/LICENSE.cpython.txt",
                    "build_info": {
                        "core": {"license_path": "licenses/LICENSE.cpython.txt"},
                        "extensions": {
                            "_bz2": [{"license_path": "licenses/LICENSE.bzip2.txt"}],
                        },
                    },
                }
                pj_bytes = json.dumps(pj).encode("utf-8")
                info = tarfile.TarInfo(name="python/PYTHON.json")
                info.size = len(pj_bytes)
                tf.addfile(info, io.BytesIO(pj_bytes))
            return tar_path

        good_tar = _build_tar()
        bad_tar = _build_tar(extra_files={"python/licenses/LICENSE.unaccounted_foo.txt": b"foo\n"})

        # 1. Runnable by hand via scripts/gen_third_party_notices.py --verify-pbs-archive:
        script_path = repo_root / "scripts" / "gen_third_party_notices.py"
        proc_ok = subprocess.run(
            [
                sys.executable,
                str(script_path),
                "--verify-pbs-archive",
                "--target",
                target,
                "--archive-path",
                str(good_tar),
                "--no-verify-archive-sha256",
            ],
            cwd=tmp_path,
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=30,
        )
        assert proc_ok.returncode == 0, proc_ok.stderr
        assert "pbs-archive-check" in proc_ok.stdout

        proc_fail = subprocess.run(
            [
                sys.executable,
                str(script_path),
                "--verify-pbs-archive",
                "--target",
                target,
                "--archive-path",
                str(bad_tar),
                "--no-verify-archive-sha256",
            ],
            cwd=tmp_path,
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=30,
        )
        assert proc_fail.returncode != 0
        assert "LICENSE.unaccounted_foo.txt" in (proc_fail.stderr + proc_fail.stdout)

        # 2. Direct function checks:
        verify_pbs_archive_accounting = getattr(
            notices_mod, "verify_pbs_archive_accounting", None
        )
        assert verify_pbs_archive_accounting is not None
        verified = verify_pbs_archive_accounting(
            target=target, archive_path=good_tar, verify_archive_sha256=False
        )
        assert target in verified

        with pytest.raises(RuntimeError, match="LICENSE.unaccounted_foo.txt"):
            verify_pbs_archive_accounting(
                target=target, archive_path=bad_tar, verify_archive_sha256=False
            )

        bad_pj_tar = _build_tar(
            python_json_obj={
                "build_info": {
                    "extensions": {
                        "_mystery": [{"license_path": "licenses/LICENSE.mystery_ext.txt"}]
                    }
                }
            }
        )
        with pytest.raises(RuntimeError, match="LICENSE.mystery_ext.txt"):
            verify_pbs_archive_accounting(
                target=target, archive_path=bad_pj_tar, verify_archive_sha256=False
            )


    def test_verify_bundle_no_direct_url_pass_and_fail(self, tmp_path):
        """G3: --verify-bundle fails when *.dist-info/direct_url.json is present."""
        import pytest

        from escape_the_valley.notices import verify_bundle_no_direct_url

        fake_bin = tmp_path / "escape-the-valley-1.3.1-linux-x64"
        fake_bin.write_bytes(b"fake")

        clean_entries = [
            "escape_the_valley/THIRD_PARTY_NOTICES.txt",
            "escape_the_valley-1.3.1.dist-info/METADATA",
            "escape_the_valley-1.3.1.dist-info/WHEEL",
            "rich-15.0.0.dist-info/METADATA",
        ]
        assert (
            verify_bundle_no_direct_url(fake_bin, archive_entries=clean_entries)
            == sorted(clean_entries)
        )

        leaked_entries = [
            *clean_entries,
            "escape_the_valley-1.3.1.dist-info/direct_url.json",
        ]
        with pytest.raises(RuntimeError, match="direct_url.json"):
            verify_bundle_no_direct_url(fake_bin, archive_entries=leaked_entries)

    def test_verify_bundle_sysconfigdata_home_paths_pass_and_fail(
        self, tmp_path, monkeypatch
    ):
        """G1: --verify-bundle reads PYZ _sysconfigdata* and rejects home paths in any case."""
        import marshal
        import sys
        import types

        import pytest

        from escape_the_valley.notices import verify_bundle_no_direct_url

        fake_bin = tmp_path / "escape-the-valley-1.3.1-linux-x64"
        fake_bin.write_bytes(b"fake")

        def _make_sysconfig_pyc_body(prefix: str) -> bytes:
            src = (
                "# system configuration generated and used by the sysconfig module\n"
                "build_time_vars = {\n"
                "    'ABIFLAGS': '',\n"
                f"    'BINDIR': {f'{prefix}/bin'!r},\n"
                f"    'BINLIBDEST': {f'{prefix}/lib/python3.12'!r},\n"
                "    'CONFIG_ARGS': \"'--prefix=/install' '--with-openssl=/tools/deps'\",\n"
                f"    'DESTDIRS': {f'{prefix} {prefix}/lib {prefix}/lib/python3.12'!r},\n"
                f"    'INCLUDEDIR': {f'{prefix}/include'!r},\n"
                f"    'LIBDIR': {f'{prefix}/lib'!r},\n"
                f"    'exec_prefix': {prefix!r},\n"
                f"    'prefix': {prefix!r},\n"
                "}\n"
            )
            code = compile(src, "_sysconfigdata__linux_x86_64-linux-gnu.py", "exec")
            return marshal.dumps(code)

        class _FakePyzReader:
            def __init__(self, modules: dict[str, bytes]):
                self.toc = {k: (1, 0, len(v)) for k, v in modules.items()}
                self._modules = modules

            def extract(self, name: str, raw: bool = False) -> bytes | None:
                assert raw is True
                return self._modules.get(name)

        def _install_fake_carchive(
            carchive_entries: dict[str, tuple],
            pyz_modules: dict[str, bytes],
        ) -> None:
            class _FakeCArchiveReader:
                def __init__(self, path: str):
                    self.path = path
                    self.toc = carchive_entries

                def open_embedded_archive(self, name: str) -> _FakePyzReader:
                    assert name == "PYZ.pyz"
                    return _FakePyzReader(pyz_modules)

                def extract(self, name: str) -> bytes | None:
                    return None

            fake_readers = types.ModuleType("PyInstaller.archive.readers")
            fake_readers.CArchiveReader = _FakeCArchiveReader  # type: ignore[attr-defined]
            fake_archive = types.ModuleType("PyInstaller.archive")
            fake_archive.readers = fake_readers  # type: ignore[attr-defined]
            fake_pyi = types.ModuleType("PyInstaller")
            fake_pyi.archive = fake_archive  # type: ignore[attr-defined]
            monkeypatch.setitem(sys.modules, "PyInstaller", fake_pyi)
            monkeypatch.setitem(sys.modules, "PyInstaller.archive", fake_archive)
            monkeypatch.setitem(
                sys.modules, "PyInstaller.archive.readers", fake_readers
            )

        base_toc = {
            "PYZ.pyz": (0, 100, 100, 0, "z"),
            "escape_the_valley/THIRD_PARTY_NOTICES.txt": (100, 50, 50, 1, "x"),
            "escape_the_valley-1.3.1.dist-info/METADATA": (150, 50, 50, 1, "x"),
        }

        # 1. Clean neutral prefixes pass via PYZ reader (/opt/uv-python, C:\uv-python):
        clean_pyz = {
            "_sysconfigdata__linux_x86_64-linux-gnu": _make_sysconfig_pyc_body(
                "/opt/uv-python/cpython-3.12.13-linux-x86_64-gnu"
            ),
            "_sysconfigdata__darwin_darwin": _make_sysconfig_pyc_body(
                "/opt/uv-python/cpython-3.12.13-macos-aarch64-none"
            ),
            "_sysconfigdata__win_x64": _make_sysconfig_pyc_body(
                "C:\\uv-python\\cpython-3.12.13-windows-x86_64-none"
            ),
            "rich.console": marshal.dumps(compile("x = 1\n", "console.py", "exec")),
        }
        _install_fake_carchive(base_toc, clean_pyz)
        assert verify_bundle_no_direct_url(fake_bin) == sorted(base_toc.keys())

        # 2. Linux home path (/home/<x>/ and uppercase /HOME/<x>/) fails:
        for linux_prefix in (
            "/home/runner/work/_temp/uv-python-dir/cpython-3.12.13-linux-x86_64-gnu",
            "/HOME/Runner/.local/share/uv/python/cpython-3.12.13-linux-x86_64-gnu",
        ):
            _install_fake_carchive(
                base_toc,
                {
                    "_sysconfigdata__linux_x86_64-linux-gnu": _make_sysconfig_pyc_body(
                        linux_prefix
                    )
                },
            )
            with pytest.raises(
                RuntimeError,
                match=r"_sysconfigdata.*_sysconfigdata__linux_x86_64-linux-gnu",
            ):
                verify_bundle_no_direct_url(fake_bin)

        # 3. macOS home path (/Users/<x>/ and lowercase /users/<x>/) fails:
        mac_upper = "/".join(
            ("", "Users", "runner", "work", "_temp", "uv-python-dir", "cpython-3.12.13")
        )
        mac_lower = "/".join(
            ("", "users", "runner", ".local", "share", "uv", "python", "cpython-3.12.13")
        )
        for mac_prefix in (mac_upper, mac_lower):
            _install_fake_carchive(
                base_toc,
                {
                    "_sysconfigdata__darwin_darwin": _make_sysconfig_pyc_body(
                        mac_prefix
                    )
                },
            )
            with pytest.raises(
                RuntimeError,
                match=r"_sysconfigdata.*_sysconfigdata__darwin_darwin",
            ):
                verify_bundle_no_direct_url(fake_bin)

        # 4. Windows home path (C:\Users\<x>\ and lowercase c:\users\<x>\) fails:
        win_upper = "\\".join(
            ("C:", "Users", "runneradmin", "AppData", "Roaming", "uv", "python", "cpython-3.12.13")
        )
        win_lower = "\\".join(
            ("c:", "users", "runneradmin", "appdata", "roaming", "uv", "python", "cpython-3.12.13")
        )
        for win_prefix in (win_upper, win_lower):
            _install_fake_carchive(
                base_toc,
                {"_sysconfigdata__win32": _make_sysconfig_pyc_body(win_prefix)},
            )
            with pytest.raises(
                RuntimeError,
                match=r"_sysconfigdata.*_sysconfigdata__win32",
            ):
                verify_bundle_no_direct_url(fake_bin)

        # 5. Bundle carrying BOTH direct_url.json AND _sysconfigdata home path reports both:
        both_toc = {
            **base_toc,
            "xrpl_camp-1.5.0.dist-info/direct_url.json": (200, 50, 50, 1, "x"),
        }
        _install_fake_carchive(
            both_toc,
            {
                "_sysconfigdata__linux_x86_64-linux-gnu": _make_sysconfig_pyc_body(
                    "/home/runner/work/_temp/uv-python-dir/cpython-3.12.15-linux-x86_64-gnu"
                )
            },
        )
        with pytest.raises(RuntimeError) as exc_info:
            verify_bundle_no_direct_url(fake_bin)
        msg = str(exc_info.value)
        assert "direct_url.json" in msg
        assert "_sysconfigdata__linux_x86_64-linux-gnu" in msg
        assert "/home/runner/work/_temp/uv-python-dir/cpython-3.12.15-linux-x86_64-gnu/bin" in msg

    def test_seed_help_and_changelog_132_reproducibility_wording(self):
        """PR #40 fix-up 1 (#21, #41): `trail new --help` (`--seed` help) and the
        1.3.2 CHANGELOG section carry the qualified reproducibility wording, and
        the 1.3.2 CHANGELOG section states plainly that a run saved in 1.3.1
        continues differently after upgrading to 1.3.2."""
        import re
        from pathlib import Path

        repo_root = Path(__file__).resolve().parents[1]

        # 1. `--seed` help text on `trail new`:
        res = runner.invoke(app, ["new", "--help"], env={"COLUMNS": "200"})
        assert res.exit_code == 0
        normalized_help = re.sub(r"\s+", " ", res.output)
        assert (
            "RNG seed; the same seed and choices give the same run in this version"
            in normalized_help
        )
        assert "RNG seed for reproducible runs" not in normalized_help

        # 2. 1.3.2 CHANGELOG section (extracted the same way release.yml does):
        changelog = (repo_root / "CHANGELOG.md").read_text(encoding="utf-8")
        m = re.search(
            r"^## \[1\.3\.2\][^\n]*\n(.*?)(?=^## \[|\Z)",
            changelog,
            flags=re.MULTILINE | re.DOTALL,
        )
        assert m is not None, "Missing ## [1.3.2] section in CHANGELOG.md"
        section_132 = m.group(1)

        assert (
            "a run saved in 1.3.1 continues differently after upgrading to 1.3.2"
            in section_132
        )
        assert (
            "Within one version, the same seed and the same choices play the same run. "
            "Reloading a save at a fork can change the run until #41 is fixed."
            in section_132
        )
        assert (
            "Seed reproducibility is guaranteed within the same version"
            not in section_132
        )

