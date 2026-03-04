"""Escape the Valley: Ledger Trail — Textual TUI.

Run:
    trail tui           # new game, GM off
    trail tui --continue # resume saved game
    # or: python -m escape_the_valley.tui_app

Keys:
    t travel | r rest | h hunt | p repair
    1-4 choose option
    J toggle journal drawer
    ? help | q quit
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Literal

from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Container, Grid, Vertical
from textual.reactive import reactive
from textual.widgets import Footer, Header, Markdown, Rule, Static

# ── FrameState: the engine-to-UI contract ──────────────────────────

ChoiceId = Literal["A", "B", "C", "D"]


@dataclass
class Choice:
    id: ChoiceId
    label: str
    risk_hint: str = ""
    cost_hint: str = ""


@dataclass
class FrameState:
    """Renderable snapshot. No engine internals leak into this."""

    # Left column
    day: int = 1
    location: str = ""
    next_stop: str = ""
    weather: str = ""
    biome: str = ""
    pace: str = "Steady"
    wagon: str = ""
    party_summary: str = ""

    supplies: dict[str, int] = field(default_factory=lambda: {
        "FOOD": 0, "WATR": 0, "MEDS": 0, "AMMO": 0, "PART": 0,
    })

    # Center column
    route_ascii: str = ""
    narration: str = ""

    # Right column
    party_detail: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    # Bottom bar
    prompt_title: str = "Camp"
    prompt_text: str = "What will you do?"
    choices: list[Choice] = field(default_factory=list)

    # Journal
    journal: list[str] = field(default_factory=list)


# ── Widgets ─────────────────────────────────────────────────────────


class StatusPanel(Static):
    def update_from(self, s: FrameState) -> None:
        lines = [
            f"[b]Day {s.day}[/b]  \u2022  {s.location}",
            f"Next: {s.next_stop}",
            f"{s.weather}  \u2022  {s.biome}",
            f"Pace: {s.pace}",
            "",
            s.party_summary,
            s.wagon,
        ]
        self.update("\n".join(lines))


class SuppliesPanel(Static):
    def update_from(self, s: FrameState) -> None:
        items = [f"{k}: {v}" for k, v in s.supplies.items()]
        self.update("[b]Supplies[/b]\n" + "\n".join(items))


class MapPanel(Static):
    def update_from(self, s: FrameState) -> None:
        self.update("[b]Route[/b]\n" + s.route_ascii)


class NarrationPanel(Markdown):
    def update_from(self, s: FrameState) -> None:
        self.update(s.narration)


class PartyPanel(Static):
    def update_from(self, s: FrameState) -> None:
        body = "[b]Party[/b]\n" + "\n".join(s.party_detail)
        if s.warnings:
            body += "\n\n[b]Warnings[/b]\n"
            body += "\n".join(f"\u2022 {w}" for w in s.warnings)
        self.update(body)


class EventBar(Static):
    """Bottom prompt + choices."""

    def update_from(self, s: FrameState) -> None:
        choice_lines = []
        for c in s.choices:
            hints = []
            if c.risk_hint:
                hints.append(f"risk: {c.risk_hint}")
            if c.cost_hint:
                hints.append(f"cost: {c.cost_hint}")
            hint_txt = f"  ({'; '.join(hints)})" if hints else ""
            choice_lines.append(
                f"[b]{c.id}[/b]) {c.label}{hint_txt}"
            )

        text = (
            f"[b]{s.prompt_title}[/b]\n"
            f"{s.prompt_text}\n\n"
            + "\n".join(choice_lines)
            + "\n\n[i]Choose 1\u20134. Actions: t/r/h/p. "
            "Journal: J. Help: ?[/i]"
        )
        self.update(text)


class JournalDrawer(Static):
    """Toggle-able journal panel (right side drawer)."""

    def update_from(self, s: FrameState) -> None:
        lines = "\n".join(f"- {entry}" for entry in s.journal)
        self.update("[b]Journal[/b]\n" + lines)


HELP_TEXT = """\
[b]Escape the Valley: Ledger Trail[/b]

Keys:
\u2022 t Travel    \u2022 r Rest    \u2022 h Hunt    \u2022 p Repair
\u2022 1\u20134 Choose option (A\u2013D)
\u2022 J Toggle journal drawer
\u2022 q Quit

The engine decides outcomes. The GM narrates.
"""


class HelpOverlay(Static):
    def on_mount(self) -> None:
        self.update(HELP_TEXT)


# ── App ─────────────────────────────────────────────────────────────


class LedgerTrailApp(App):
    CSS_PATH = "tui.tcss"

    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("question_mark", "toggle_help", "Help"),
        Binding("shift+j", "toggle_journal", "Journal"),
        Binding("t", "intent('TRAVEL')", "Travel"),
        Binding("r", "intent('REST')", "Rest"),
        Binding("h", "intent('HUNT')", "Hunt"),
        Binding("p", "intent('REPAIR')", "Repair"),
        Binding("1", "choose('A')", "A"),
        Binding("2", "choose('B')", "B"),
        Binding("3", "choose('C')", "C"),
        Binding("4", "choose('D')", "D"),
    ]

    show_help: reactive[bool] = reactive(False)
    show_journal: reactive[bool] = reactive(False)

    def __init__(
        self,
        engine=None,
        *,
        demo: bool = False,
    ) -> None:
        super().__init__()
        self._engine = engine
        self._demo = demo
        self._frame = FrameState()

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)

        with Grid(id="main"):
            with Vertical(id="left"):
                yield StatusPanel(id="status")
                yield Rule()
                yield SuppliesPanel(id="supplies")

            with Vertical(id="center"):
                yield MapPanel(id="map")
                yield Rule()
                yield NarrationPanel(id="narration")

            with Vertical(id="right"):
                yield PartyPanel(id="party")

        yield EventBar(id="eventbar")

        with Container(id="overlays"):
            yield HelpOverlay(id="help")
            yield JournalDrawer(id="journal")

        yield Footer()

    def on_mount(self) -> None:
        if self._engine:
            self._sync_frame()
        self._render_all()

    def _sync_frame(self) -> None:
        """Pull a fresh FrameState from the engine."""
        from .adapter import state_to_frame

        self._frame = state_to_frame(self._engine)

    def _render_all(self) -> None:
        s = self._frame
        self.query_one("#status", StatusPanel).update_from(s)
        self.query_one("#supplies", SuppliesPanel).update_from(s)
        self.query_one("#map", MapPanel).update_from(s)
        self.query_one("#narration", NarrationPanel).update_from(s)
        self.query_one("#party", PartyPanel).update_from(s)
        self.query_one("#eventbar", EventBar).update_from(s)
        self.query_one("#journal", JournalDrawer).update_from(s)

        self.query_one("#help").display = self.show_help
        self.query_one("#journal").display = self.show_journal

    def action_toggle_help(self) -> None:
        self.show_help = not self.show_help
        self._render_all()

    def action_toggle_journal(self) -> None:
        self.show_journal = not self.show_journal
        self._render_all()

    def action_choose(self, choice_id: str) -> None:
        """Send CHOOSE intent to the engine."""
        if not self._engine:
            return

        from .intent import IntentAction, PlayerIntent

        intent = PlayerIntent(
            action=IntentAction.CHOOSE,
            choice_id=choice_id,
        )
        self._engine.step(intent)
        self._sync_frame()
        self._render_all()

    def action_intent(self, intent_str: str) -> None:
        """Map hotkeys t/r/h/p to engine intents."""
        if not self._engine:
            return

        from .intent import GamePhase, IntentAction, PlayerIntent

        # If in EVENT or ROUTE phase, only CHOOSE is valid
        if self._engine.phase in (
            GamePhase.EVENT, GamePhase.ROUTE,
        ):
            # Map t/r/h/p to A/B/C/D when in choice mode
            mapping = {
                "TRAVEL": "A",
                "REST": "B",
                "HUNT": "C",
                "REPAIR": "D",
            }
            cid = mapping.get(intent_str)
            if cid:
                self.action_choose(cid)
            return

        # CAMP phase — direct intent
        action_map = {
            "TRAVEL": IntentAction.TRAVEL,
            "REST": IntentAction.REST,
            "HUNT": IntentAction.HUNT,
            "REPAIR": IntentAction.REPAIR,
        }
        action = action_map.get(intent_str)
        if not action:
            return

        intent = PlayerIntent(action=action)
        self._engine.step(intent)
        self._sync_frame()
        self._render_all()


if __name__ == "__main__":
    LedgerTrailApp().run()
