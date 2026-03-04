"""Escape the Valley: Ledger Trail — Textual UI scaffold.

Run:
    trail tui
    # or: python -m escape_the_valley.tui_app

Keys:
    t travel | r rest | h hunt | p repair
    1-4 choose option
    j/k scroll narration | J toggle journal drawer
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
    """Renderable snapshot the engine hands to the UI. No engine internals."""

    # Left column
    day: int = 3
    location: str = "Old Switchback"
    next_stop: str = "Grey Ford"
    weather: str = "Fog, cold wind"
    biome: str = "Forest"
    pace: str = "Steady"
    wagon: str = "Wagon: 71% \u2022 Animals: 82%"
    party_summary: str = "Party: 4 \u2022 Sick: 1 \u2022 Injured: 0"

    supplies: dict[str, int] = field(default_factory=lambda: {
        "FOOD": 38,
        "WATR": 41,
        "MEDS": 5,
        "AMMO": 18,
        "PART": 2,
    })

    # Center column
    route_ascii: str = (
        "  [You]\n"
        "    |\n"
        " Old Switchback\n"
        "    |\n"
        "  Grey Ford (next)\n"
        "    |\n"
        "  Split Pines\n"
        "   / \\\n"
        " East Ridge   Hollow Market\n"
    )
    narration: str = (
        "Fog settles in layers between the trees. The trail is "
        "familiar for a moment\u2014then it isn\u2019t. Your boots find old "
        "ruts, and the wagon follows as if it remembers.\n\n"
        "Somewhere ahead, water moves over stone. The sound "
        "carries oddly, as though the forest is listening for "
        "a second note."
    )

    # Right column
    party_detail: list[str] = field(default_factory=lambda: [
        "Mara \u2014 83% (tired)",
        "Oren \u2014 76% (sick)",
        "Ilya \u2014 91% (well)",
        "Jun  \u2014 88% (well)",
    ])
    warnings: list[str] = field(default_factory=lambda: [
        "Low PART (2) \u2014 repairs limited",
        "Oren is sick \u2014 consider rest",
    ])

    # Bottom bar (event / choices)
    prompt_title: str = "At camp"
    prompt_text: str = "Night fog thickens. What do you do?"
    choices: list[Choice] = field(default_factory=lambda: [
        Choice("A", "Travel", "Risk mishap in fog", "More WATR"),
        Choice("B", "Rest", "Safer, costs time", "FOOD/WATR"),
        Choice("C", "Hunt", "May fail or injure", "Costs AMMO"),
        Choice("D", "Repair", "May restore wagon", "Costs PART"),
    ])

    # Journal (full log)
    journal: list[str] = field(default_factory=lambda: [
        "Day 3 \u2014 Fog rolled in after dusk. The fire struggled.",
        "A bell rang once in the trees. No one admitted to "
        "carrying it.",
        "You decided to wait for morning. The horses did not "
        "settle.",
    ])


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
            choice_lines.append(f"[b]{c.id}[/b]) {c.label}{hint_txt}")

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
\u2022 j/k Scroll narration (when focused)
\u2022 J Toggle journal drawer
\u2022 q Quit

Notes:
\u2022 This is a UI scaffold with fake state.
\u2022 The engine will later supply FrameState snapshots.
\u2022 The table (engine) decides outcomes; the GM narrates.
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

    def __init__(self) -> None:
        super().__init__()
        self._frame = FrameState()

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)

        with Grid(id="main"):
            # Left column
            with Vertical(id="left"):
                yield StatusPanel(id="status")
                yield Rule()
                yield SuppliesPanel(id="supplies")

            # Center column
            with Vertical(id="center"):
                yield MapPanel(id="map")
                yield Rule()
                yield NarrationPanel(id="narration")

            # Right column
            with Vertical(id="right"):
                yield PartyPanel(id="party")

        yield EventBar(id="eventbar")

        # Overlays / drawers
        with Container(id="overlays"):
            yield HelpOverlay(id="help")
            yield JournalDrawer(id="journal")

        yield Footer()

    def on_mount(self) -> None:
        self._render_all()

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
        """Fake state mutations to prove UI wiring. Replace with
        engine.step(intent) later."""
        s = self._frame

        label = next(
            (c.label for c in s.choices if c.id == choice_id),
            choice_id,
        )
        s.journal.append(f"Chose {choice_id}: {label}")

        if choice_id == "A":
            s.day += 1
            s.narration = (
                "You move into the fog. The world narrows to "
                "lantern-light and hoof-sound."
            )
            s.supplies["WATR"] = max(0, s.supplies["WATR"] - 2)
            s.supplies["FOOD"] = max(0, s.supplies["FOOD"] - 1)
            s.prompt_title = "On the trail"
            s.prompt_text = (
                "The road bends. Something watches from "
                "the treeline."
            )
        elif choice_id == "B":
            s.day += 1
            s.narration = (
                "You rest. The sick one breathes easier. "
                "The fog does not."
            )
            s.supplies["WATR"] = max(0, s.supplies["WATR"] - 1)
            s.supplies["FOOD"] = max(0, s.supplies["FOOD"] - 1)
            s.prompt_title = "Morning camp"
            s.prompt_text = (
                "Ash in the firepit. Quiet woods. "
                "Decisions remain."
            )
        elif choice_id == "C":
            s.narration = (
                "You hunt the margins of the dark. "
                "The forest gives, but not freely."
            )
            s.supplies["AMMO"] = max(0, s.supplies["AMMO"] - 1)
            s.supplies["FOOD"] += 2
            s.prompt_title = "After hunting"
            s.prompt_text = "You return with meat and questions."
        elif choice_id == "D":
            s.narration = (
                "You work the axle by lantern-light. "
                "Metal complains, then settles."
            )
            if s.supplies["PART"] > 0:
                s.supplies["PART"] -= 1
            s.prompt_title = "Repairs"
            s.prompt_text = "The wagon holds\u2014for now."

        self._render_all()

    def action_intent(self, intent: str) -> None:
        """Map hotkeys t/r/h/p to choice IDs."""
        mapping = {
            "TRAVEL": "A",
            "REST": "B",
            "HUNT": "C",
            "REPAIR": "D",
        }
        cid = mapping.get(intent)
        if cid:
            self.action_choose(cid)


if __name__ == "__main__":
    LedgerTrailApp().run()
