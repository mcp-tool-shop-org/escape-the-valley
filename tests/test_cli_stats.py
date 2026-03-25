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
