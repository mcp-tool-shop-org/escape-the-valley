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
