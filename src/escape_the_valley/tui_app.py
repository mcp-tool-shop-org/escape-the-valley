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

    supplies: dict[str, int] = field(default_factory=dict)

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

    # Ledger Backpack
    backpack_status: str = ""


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
        if s.backpack_status:
            lines.append("")
            lines.append(s.backpack_status)
        self.update("\n".join(lines))


class SuppliesPanel(Static):
    # Display keys that belong to the GEAR category (for visual grouping)
    _GEAR_KEYS = {"PART", "ROPE", "TOOL", "BOOT"}

    def update_from(self, s: FrameState) -> None:
        consumables = []
        gear = []
        for k, v in s.supplies.items():
            line = f"{k}: {v}"
            if k in self._GEAR_KEYS:
                gear.append(line)
            else:
                consumables.append(line)

        body = "[b]Supplies[/b]\n"
        if consumables:
            body += "\n".join(consumables)
        if gear:
            body += "\n\u2500\u2500\u2500\n"
            body += "\n".join(gear)
        self.update(body)


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
\u2022 L Toggle ledger menu
\u2022 V Toggle voice narration
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
        Binding("l", "toggle_ledger", "Ledger"),
        Binding("v", "toggle_voice", "Voice"),
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
    show_ledger: reactive[bool] = reactive(False)
    show_nudge: reactive[bool] = reactive(False)
    show_enable_flow: reactive[bool] = reactive(False)
    show_wallet_info: reactive[bool] = reactive(False)
    show_learn_more: reactive[bool] = reactive(False)
    show_send_parcel: reactive[bool] = reactive(False)
    show_parcel_notify: reactive[bool] = reactive(False)

    def __init__(
        self,
        engine=None,
        *,
        demo: bool = False,
        voice_config=None,
    ) -> None:
        super().__init__()
        self._engine = engine
        self._demo = demo
        self._frame = FrameState()
        self._voice_config = voice_config
        self._voice_bridge = None
        self._voice_enabled = False

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

            from .backpack_ui import (
                EnableFlowOverlay,
                LearnMoreOverlay,
                LedgerMenuOverlay,
                NudgeOverlay,
                ParcelNotification,
                SendParcelOverlay,
                WalletInfoOverlay,
            )

            yield LedgerMenuOverlay(id="ledger_menu")
            yield NudgeOverlay(id="nudge")
            yield EnableFlowOverlay(id="enable_flow")
            yield WalletInfoOverlay(id="wallet_info")
            yield LearnMoreOverlay(id="learn_more")
            yield SendParcelOverlay(id="send_parcel")
            yield ParcelNotification(id="parcel_notify")

        yield Footer()

    def on_mount(self) -> None:
        if self._engine:
            self._sync_frame()

        # Initialize voice bridge if configured
        if self._voice_config and self._voice_config.enabled:
            from .voice import VoiceBridge

            self._voice_bridge = VoiceBridge(self._voice_config)
            self._voice_enabled = self._voice_bridge.start()

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
        self.query_one("#ledger_menu").display = self.show_ledger
        self.query_one("#nudge").display = self.show_nudge
        self.query_one("#enable_flow").display = self.show_enable_flow
        self.query_one("#wallet_info").display = self.show_wallet_info
        self.query_one("#learn_more").display = self.show_learn_more
        self.query_one("#send_parcel").display = self.show_send_parcel
        self.query_one("#parcel_notify").display = self.show_parcel_notify

    def _after_step(self) -> None:
        """Sync frame, render, optionally narrate, and check nudge."""
        self._sync_frame()
        self._render_all()

        # Nudge: show once at first town if backpack not enabled/dismissed
        if self._engine:
            bp = self._engine.state.backpack
            if (
                not bp.enabled
                and not bp.nudge_dismissed
                and not bp.nudge_shown
            ):
                # Check if we just arrived at a town
                cur = None
                for n in self._engine.state.map_nodes:
                    if n.node_id == self._engine.state.location_id:
                        cur = n
                        break
                if cur and cur.is_town:
                    bp.nudge_shown = True
                    self.show_nudge = True
                    self._render_all()

        if self._voice_enabled and self._voice_bridge:
            from .narration import extract_narration

            events = extract_narration(
                self._engine.msgs,
                self._engine.phase.value,
                warnings=self._frame.warnings,
            )
            for event in events:
                self._voice_bridge.enqueue(event)

    def on_unmount(self) -> None:
        """Clean shutdown of voice worker."""
        if self._voice_bridge:
            self._voice_bridge.stop()

    def action_toggle_help(self) -> None:
        self.show_help = not self.show_help
        self._render_all()

    def action_toggle_journal(self) -> None:
        self.show_journal = not self.show_journal
        self._render_all()

    def action_toggle_voice(self) -> None:
        """Toggle voice narration on/off."""
        if self._voice_bridge is None:
            from .voice import VoiceBridge, VoiceConfig

            config = self._voice_config or VoiceConfig(enabled=True)
            config.enabled = True
            self._voice_bridge = VoiceBridge(config)
            self._voice_enabled = self._voice_bridge.start()
            if not self._voice_enabled:
                self.notify("Voice not available")
                return
            self.notify("Voice ON")
            return

        new_state = self._voice_bridge.toggle()
        self._voice_enabled = new_state
        self.notify("Voice ON" if new_state else "Voice OFF")

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
        self._after_step()

    def action_intent(self, intent_str: str) -> None:
        """Map hotkeys t/r/h/p to engine intents."""
        if not self._engine:
            return

        from .intent import GamePhase, IntentAction, PlayerIntent

        # If in EVENT or ROUTE phase, only CHOOSE is valid
        if self._engine.phase in (
            GamePhase.EVENT, GamePhase.ROUTE,
        ):
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
        self._after_step()

    # ── Ledger Backpack actions ────────────────────────────────────

    def action_toggle_ledger(self) -> None:
        """Toggle the ledger menu overlay."""
        self._close_all_overlays()
        self.show_ledger = not self.show_ledger
        if self.show_ledger and self._engine:
            from .backpack_ui import LedgerMenuOverlay

            self.query_one(
                "#ledger_menu", LedgerMenuOverlay,
            ).update_from_state(self._engine.state.backpack.enabled)
        self._render_all()

    def action_ledger_enable(self) -> None:
        """Start the backpack enable flow."""
        if not self._engine:
            return

        self._close_all_overlays()
        self.show_enable_flow = True
        self._render_all()

        from .backpack_ui import EnableFlowOverlay

        overlay = self.query_one("#enable_flow", EnableFlowOverlay)
        overlay.show_progress()

        from .backpack import BackpackManager

        mgr = BackpackManager()
        result = mgr.enable(self._engine.state)
        mgr.close()

        if result.success:
            overlay.show_success(result.wallet_address)
            self.notify("Ledger Backpack enabled")
        else:
            overlay.show_failure(result.message)

        self._sync_frame()
        self._render_all()

    def action_ledger_disable(self) -> None:
        """Disable the backpack."""
        if not self._engine:
            return

        from .backpack import BackpackManager

        mgr = BackpackManager()
        mgr.disable(self._engine.state)

        self._close_all_overlays()
        self._sync_frame()
        self._render_all()
        self.notify("Ledger Backpack disabled")

    def action_ledger_settle(self) -> None:
        """Manual settlement."""
        if not self._engine:
            return

        from .backpack import BackpackManager

        mgr = BackpackManager()
        cur = None
        for n in self._engine.state.map_nodes:
            if n.node_id == self._engine.state.location_id:
                cur = n
                break
        location = cur.name if cur else "Unknown"
        result = mgr.settle(self._engine.state, location)
        mgr.close()

        self.notify(result.message)
        self._sync_frame()
        self._render_all()

    def action_wallet_info(self) -> None:
        """Show wallet info overlay."""
        if not self._engine:
            return

        from .backpack import BackpackManager
        from .backpack_ui import WalletInfoOverlay

        mgr = BackpackManager()
        info = mgr.wallet_info(self._engine.state)

        self._close_all_overlays()
        self.show_wallet_info = True
        self.query_one(
            "#wallet_info", WalletInfoOverlay,
        ).update_from_info(info)
        self._render_all()

    def action_send_parcel(self) -> None:
        """Show the send parcel overlay with wallet + supply info."""
        if not self._engine:
            return

        bp = self._engine.state.backpack
        if not bp.enabled:
            self.notify("Enable backpack first (L → E)")
            return

        from .backpack_models import XRPL_TOKEN_MAP
        from .backpack_ui import SendParcelOverlay

        supplies = self._engine.state.supplies
        supply_lines = []
        for key in sorted(XRPL_TOKEN_MAP.keys()):
            amount = supplies.get(key)
            supply_lines.append(f"  {key}: {amount}")

        self._close_all_overlays()
        self.show_send_parcel = True
        self.query_one(
            "#send_parcel", SendParcelOverlay,
        ).show_form("\n".join(supply_lines))
        self._render_all()

    def action_show_parcel(self, parcel) -> None:
        """Show a parcel notification for accept/refuse."""
        from .backpack_ui import ParcelNotification

        contents = ", ".join(
            f"{v} {k}" for k, v in parcel.contents.items()
        )
        self._close_all_overlays()
        self.show_parcel_notify = True
        self._current_parcel = parcel
        self.query_one(
            "#parcel_notify", ParcelNotification,
        ).show_parcel(parcel.sender, contents)
        self._render_all()

    def action_accept_parcel(self) -> None:
        """Accept the currently shown parcel."""
        if not self._engine or not hasattr(self, "_current_parcel"):
            return

        from .backpack import BackpackManager

        mgr = BackpackManager()
        mgr.accept_parcel(self._current_parcel, self._engine.state)

        contents = ", ".join(
            f"+{v} {k}" for k, v in self._current_parcel.contents.items()
        )
        self.notify(f"Parcel accepted: {contents}")
        self._close_all_overlays()
        self._sync_frame()
        self._render_all()

    def action_refuse_parcel(self) -> None:
        """Refuse the currently shown parcel."""
        if not self._engine or not hasattr(self, "_current_parcel"):
            return

        from .backpack import BackpackManager

        mgr = BackpackManager()
        mgr.refuse_parcel(self._current_parcel)

        self.notify("Parcel refused")
        self._close_all_overlays()
        self._render_all()

    def action_learn_more(self) -> None:
        """Show learn more overlay."""
        self._close_all_overlays()
        self.show_learn_more = True
        self._render_all()

    def action_nudge_dismiss(self) -> None:
        """Dismiss the nudge — never show again."""
        if self._engine:
            self._engine.state.backpack.nudge_dismissed = True
        self.show_nudge = False
        self._render_all()

    def action_close_overlay(self) -> None:
        """Close any open overlay."""
        self._close_all_overlays()
        self._render_all()

    def _close_all_overlays(self) -> None:
        """Close all backpack overlays."""
        self.show_ledger = False
        self.show_nudge = False
        self.show_enable_flow = False
        self.show_wallet_info = False
        self.show_learn_more = False
        self.show_send_parcel = False
        self.show_parcel_notify = False

    def on_key(self, event) -> None:
        """Handle overlay keys and voice interrupt."""
        key = event.key

        # Escape closes any overlay
        if key == "escape":
            if any([
                self.show_ledger, self.show_nudge,
                self.show_enable_flow, self.show_wallet_info,
                self.show_learn_more, self.show_help,
                self.show_send_parcel, self.show_parcel_notify,
            ]):
                self._close_all_overlays()
                self.show_help = False
                self._render_all()
                event.prevent_default()
                return

        # Parcel notification keys
        if self.show_parcel_notify:
            if key == "a":
                self.action_accept_parcel()
                event.prevent_default()
                return
            if key == "r":
                self.action_refuse_parcel()
                event.prevent_default()
                return

        # Ledger menu keys (when menu is visible)
        if self.show_ledger:
            if key == "e":
                self.action_ledger_enable()
                event.prevent_default()
                return
            if key == "d":
                self.action_ledger_disable()
                event.prevent_default()
                return
            if key == "w":
                self.action_wallet_info()
                event.prevent_default()
                return
            if key == "p":
                self.action_send_parcel()
                event.prevent_default()
                return
            if key == "s":
                self.action_ledger_settle()
                event.prevent_default()
                return

        # Nudge keys
        if self.show_nudge:
            if key == "e":
                self.show_nudge = False
                self.action_ledger_enable()
                event.prevent_default()
                return
            if key == "n":
                self.action_nudge_dismiss()
                event.prevent_default()
                return

        # Learn more from nudge or ledger
        if self.show_nudge or self.show_ledger:
            if key == "l":
                self.action_learn_more()
                event.prevent_default()
                return

        # Voice interrupt
        if self._voice_enabled and self._voice_bridge:
            self._voice_bridge.interrupt()


if __name__ == "__main__":
    LedgerTrailApp().run()
