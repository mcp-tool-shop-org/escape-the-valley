"""#30 D2/D3: `trail parcel list` shows its status label and the parcel id,
and `trail parcel accept` takes a unique id prefix."""

from typer.testing import CliRunner

from escape_the_valley.backpack_models import ParcelRecord
from escape_the_valley.cli import app
from escape_the_valley.worldgen import create_new_run

runner = CliRunner()


def _state_with_parcels():
    state = create_new_run(seed=7)
    state.backpack.parcels = [
        ParcelRecord(
            parcel_id="abc123def", sender="rSENDERADDRESS000000000000000",
            contents={"food": 5}, day_received=3,
        ),
        ParcelRecord(
            parcel_id="abd999zzz", sender="rSENDERADDRESS000000000000000",
            contents={"water": 2}, day_received=4,
        ),
        ParcelRecord(
            parcel_id="zzz000", sender="rSENDERADDRESS000000000000000",
            contents={"parts": 1}, accepted=True, day_received=1,
        ),
    ]
    return state


def _patch(monkeypatch, state):
    saved = {"n": 0}

    def _save(s):
        saved["n"] += 1

    monkeypatch.setattr("escape_the_valley.cli.load_game", lambda: state)
    monkeypatch.setattr("escape_the_valley.save.save_game", _save)
    return saved


def test_parcel_list_keeps_status_label_and_shows_id(monkeypatch):
    _patch(monkeypatch, _state_with_parcels())
    result = runner.invoke(app, ["parcel", "list"])
    assert result.exit_code == 0
    assert "[pending]" in result.output
    assert "[accepted]" in result.output
    assert "abc123def" in result.output
    assert "abd999zzz" in result.output


def test_parcel_accept_by_unique_prefix(monkeypatch):
    state = _state_with_parcels()
    saved = _patch(monkeypatch, state)
    result = runner.invoke(app, ["parcel", "accept", "abc"])
    assert result.exit_code == 0, result.output
    assert state.backpack.parcels[0].accepted is True
    assert state.backpack.parcels[1].accepted is False
    assert saved["n"] == 1


def test_parcel_accept_ambiguous_prefix_is_refused(monkeypatch):
    state = _state_with_parcels()
    _patch(monkeypatch, state)
    result = runner.invoke(app, ["parcel", "accept", "ab"])
    assert result.exit_code == 1
    assert "matches 2 parcels" in result.output
    assert not state.backpack.parcels[0].accepted
    assert not state.backpack.parcels[1].accepted


def test_parcel_accept_unknown_or_accepted_prefix(monkeypatch):
    state = _state_with_parcels()
    _patch(monkeypatch, state)
    for arg in ("nope", "zzz"):
        result = runner.invoke(app, ["parcel", "accept", arg])
        assert result.exit_code == 1
        assert "not found" in result.output
