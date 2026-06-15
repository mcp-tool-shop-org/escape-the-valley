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

from textual import work
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Container, Grid, Vertical
from textual.reactive import reactive
from textual.widgets import Footer, Header, Input, Markdown, Rule, Static

# ── FrameState: the engine-to-UI contract ──────────────────────────

# A-D are the base camp actions; E/F/G are the conditional escape valves
# (Abandon Cargo / Desperate Repair / Hard Ration). They are reachable now
# (cli-tui-B-01).
ChoiceId = Literal["A", "B", "C", "D", "E", "F", "G"]


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

    # GM narration health (ENG-B-05 / gm-B-02 consumer). When the GM was asked
    # for narration but fell back to the engine's deterministic voice, the UI
    # shows a subtle, non-nagging signal so a degraded GM reads differently
    # from an intentionally terse one.
    gm_degraded: bool = False
    gm_degraded_reason: str = ""

    # ── End-of-run (EC-04 / FEAT-CLITUI-01) ─────────────────────────
    # Populated only when the run is over. The end screen renders the graded
    # ending (tier + facts), the GM epilogue, the run diagnostics, and the
    # trail ledger / XRPL postcard. None/empty while the run is live.
    game_over: bool = False
    victory: bool = False
    ending_tier: str = ""
    ending_headline: str = ""
    # Pre-rendered (label, value) rows for the facts table.
    ending_facts: list[tuple[str, str]] = field(default_factory=list)
    # The narrated epilogue (GM if available, deterministic floor otherwise).
    # Empty string means "not computed yet" (the worker is composing it).
    epilogue: str = ""
    # The trail ledger / XRPL postcard lines (read-only, from ledger.py).
    postcard_lines: list[str] = field(default_factory=list)
    # True when the postcard carries on-ledger receipts (backpack was used).
    is_postcard: bool = False
    # Run diagnostics (the data the CLI `stats` command computes).
    run_stats: list[tuple[str, str]] = field(default_factory=list)


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

    # gm-feat-01: progressive streaming. While a GM scene/outcome narration
    # arrives token-by-token on the worker thread, the app marshals each delta
    # onto the UI thread and feeds it here so the player watches the storyteller
    # write. stream_reset() opens a fresh buffer; stream_append() grows it and
    # repaints. The final _render_all() (after the step completes) overwrites
    # this with the fully-parsed FrameState narration, so a streamed scene and a
    # non-streamed one converge on identical end text.
    def stream_reset(self) -> None:
        self._stream_text = ""
        self.update("")

    def stream_append(self, delta: str) -> None:
        self._stream_text = getattr(self, "_stream_text", "") + delta
        self.update(self._stream_text)


class PartyPanel(Static):
    def update_from(self, s: FrameState) -> None:
        body = "[b]Party[/b]\n" + "\n".join(s.party_detail)
        if s.warnings:
            body += "\n\n[b]Warnings[/b]\n"
            body += "\n".join(f"\u2022 {w}" for w in s.warnings)
        self.update(body)


class EventBar(Static):
    """Bottom prompt + choices."""

    def update_from(
        self, s: FrameState, *, busy: bool = False, streaming: bool = False,
    ) -> None:
        if busy:
            # cli-tui-B-02: a step/ledger call is in flight on a worker thread.
            # Show a visible loading state so the UI never looks frozen, and
            # make it plain that input is paused.
            #
            # gm-feat-01: once the first narration token has arrived the state
            # shifts from 'thinking' to 'writing' \u2014 the storyteller is no longer
            # composing in silence, the words are landing in the panel. Choices
            # stay withheld until the step completes either way.
            if streaming:
                self.update(
                    "[b]The storyteller is writing...[/b]\n"
                    "[dim]Watch the trail above \u2014 keys resume in a "
                    "moment.[/dim]"
                )
            else:
                self.update(
                    "[b]The storyteller is thinking...[/b]\n"
                    "[dim]Working \u2014 keys are paused for a moment.[/dim]"
                )
            return

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

        # cli-tui-B-01 / B-08: the choose-prompt enumerates the *visible*
        # choices (so the conditional valves E/F/G are named), and the
        # persistent hint is now complete \u2014 every always-available key,
        # including ledger, voice, and quit.
        choice_letters = ", ".join(c.id for c in s.choices) if s.choices else ""
        pick_hint = (
            f"Choose {choice_letters} (number keys also work). "
            if choice_letters
            else ""
        )
        hint_line = (
            f"[i]{pick_hint}Actions: t/r/h/p. "
            "L ledger \u2022 V voice \u2022 J journal \u2022 ? help \u2022 q quit[/i]"
        )

        # gm-B-02 / ENG-B-05 consumer: a single subtle footer line when the GM
        # fell back to the engine's own voice. Not a popup, not repeated per
        # choice \u2014 just enough that a degraded GM reads differently from a
        # deliberately terse one. Never nags.
        degraded_line = ""
        if s.gm_degraded:
            degraded_line = (
                "\n[dim]\u2022 Narration: engine fallback "
                "(the storyteller is quiet)[/dim]"
            )

        body = "\n".join(choice_lines)
        text = (
            f"[b]{s.prompt_title}[/b]\n"
            f"{s.prompt_text}\n\n"
            + body
            + f"\n\n{hint_line}"
            + degraded_line
        )
        self.update(text)


class JournalDrawer(Static):
    """Toggle-able journal panel (right side drawer)."""

    def update_from(self, s: FrameState) -> None:
        lines = "\n".join(f"- {entry}" for entry in s.journal)
        self.update("[b]Journal[/b]\n" + lines)


class EndScreen(Static):
    """The end-of-run screen (EC-04 / FEAT-CLITUI-01).

    Replaces the old clinical cause-of-death line with an ending that feels like
    one: a tier-keyed banner, the graded result's facts, the GM-narrated
    epilogue (or its deterministic floor), the run diagnostics, and the trail
    ledger / XRPL postcard. Serious, not cute — period weight throughout.
    """

    # A short, period-appropriate caption per tier so the worst and best
    # endings read differently at a glance. Serious — never a score screen.
    _TIER_CAPTION = {
        "lost": "The valley kept its distance.",
        "pyrrhic": "Arrived — and counting the cost of arriving.",
        "weathered": "Worn thin, late, and still standing.",
        "triumphant": "Intact, on time, the vow unbroken.",
    }

    def update_from(self, s: FrameState) -> None:
        # The end screen only has content for a finished run; while the run is
        # live it stays hidden, so render nothing rather than a stale banner.
        if not s.game_over:
            self.update("")
            return

        lines: list[str] = []

        # Banner — victory or defeat, then the graded headline.
        if s.victory:
            lines.append("[b]THE VALLEY IS BEHIND YOU[/b]")
        else:
            lines.append("[b]THE TRAIL CLAIMS ANOTHER[/b]")
        caption = self._TIER_CAPTION.get(s.ending_tier, "")
        if caption:
            lines.append(f"[dim]{caption}[/dim]")
        if s.ending_headline:
            lines.append("")
            lines.append(s.ending_headline)

        # The epilogue — the storyteller's closing words. While the GM is still
        # composing it on the worker, show a quiet placeholder rather than a
        # blank gap.
        lines.append("")
        lines.append("─" * 30)
        if s.epilogue:
            lines.append(s.epilogue)
        else:
            lines.append("[dim]The storyteller gathers the last of it...[/dim]")
        lines.append("─" * 30)

        # The graded facts.
        if s.ending_facts:
            lines.append("")
            lines.append("[b]The reckoning[/b]")
            for label, value in s.ending_facts:
                lines.append(f"  {label}: {value}")

        # Run diagnostics (the CLI `stats` data).
        if s.run_stats:
            lines.append("")
            lines.append("[b]The run[/b]")
            for label, value in s.run_stats:
                lines.append(f"  {label}: {value}")

        # The trail ledger / XRPL postcard.
        if s.postcard_lines:
            lines.append("")
            heading = "[b]Postcard (on-ledger)[/b]" if s.is_postcard else "[b]Trail ledger[/b]"
            lines.append(heading)
            for ln in s.postcard_lines:
                lines.append(ln)

        lines.append("")
        if s.postcard_lines:
            lines.append(
                "[dim]Press C to copy this postcard to a file • "
                "q to close.[/dim]"
            )
        else:
            lines.append("[dim]Press q to close.[/dim]")

        self.update("\n".join(lines))


HELP_TEXT = """\
[b]Escape the Valley: Ledger Trail[/b]

Keys:
\u2022 t Travel    \u2022 r Rest    \u2022 h Hunt    \u2022 p Repair
\u2022 1\u20137 Choose option (A\u2013G); letters a\u2013g also work
\u2022 E/F/G are last-resort moves (Abandon Cargo, Desperate
  Repair, Hard Ration) \u2014 they appear only when things are dire
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
        # cli-tui-B-01: the escape valves are reachable. 5/6/7 map to the
        # conditional E/F/G choices; e/f/g are handled in on_key (so they don't
        # collide with the ledger/nudge overlay letters) when no overlay is up.
        Binding("5", "choose('E')", "E"),
        Binding("6", "choose('F')", "F"),
        Binding("7", "choose('G')", "G"),
    ]

    show_help: reactive[bool] = reactive(False)
    show_journal: reactive[bool] = reactive(False)
    # EC-04 / FEAT-CLITUI-01: the end-of-run screen. Shown once the engine
    # transitions to GAME_OVER; covers the play surface with the graded ending.
    show_end: reactive[bool] = reactive(False)
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
        resumed: bool = False,
    ) -> None:
        super().__init__()
        self._engine = engine
        self._demo = demo
        self._frame = FrameState()
        self._voice_config = voice_config
        self._voice_bridge = None
        self._voice_enabled = False
        # gm-B-06 consumer: latch so a runtime voice failure is announced to
        # the player exactly once, not on every subsequent step/tick.
        self._voice_failure_notified = False
        # cli-tui-B-09: True when this app was launched via `--continue`, so
        # on_mount can confirm which run + day the player picked back up.
        self._resumed = resumed
        # cli-tui-B-02: True while a blocking step()/ledger call runs on a
        # worker thread. Gameplay + ledger hotkeys are ignored while in flight
        # so queued keypresses don't pile up and double-step the engine.
        self._in_flight = False
        # gm-feat-01: streaming render. _streaming flips True on the FIRST
        # narration token of a step, so the event bar can drop the 'thinking'
        # state and the narration panel can begin showing prose as it is
        # written. The GM-off path and the deterministic fallback path never set
        # a token sink, so they render instantly with no streaming (no
        # regression). Set by _run_step, consumed by _on_narration_token.
        self._streaming = False
        self._gm_wrapped = False
        # The live token sink for the current step (None = no streaming this
        # step). Set by _arm_stream, cleared by _finish_step. Initialized here
        # so the GM wrapper and _finish_step can always read it safely.
        self._token_sink = None
        # EC-04: the narrated epilogue once computed (empty until then). Held on
        # the app so a frame re-sync after game-over re-applies it.
        self._epilogue_text = ""

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
            # EC-04 / FEAT-CLITUI-01: the full-screen end-of-run screen.
            yield EndScreen(id="end_screen")

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
            # cli-tui-B-03: a real input so the send-parcel overlay is no
            # longer a dead end. Shown/hidden together with #send_parcel.
            # can_focus starts False so the hidden input doesn't steal initial
            # focus from the gameplay key bindings; it's enabled when shown.
            parcel_input = Input(
                placeholder="<address> <supply> <amount>  (Esc to cancel)",
                id="parcel_input",
            )
            parcel_input.can_focus = False
            yield parcel_input
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

        # EC-04: if a finished run was loaded (the CLI normally refuses this,
        # but be robust), raise the end screen straight away.
        if self._engine and self._engine.state.game_over and not self.show_end:
            self._begin_end_screen()
            return

        # cli-tui-B-09: confirm a resumed run so the player knows the save
        # loaded and where they are, with a pointer to the help overlay.
        if self._resumed and self._engine:
            st = self._engine.state
            self.notify(
                f"Resumed run {st.run_id} -- Day {st.day}. Press ? for keys.",
            )

    def _sync_frame(self) -> None:
        """Pull a fresh FrameState from the engine."""
        from .adapter import state_to_frame

        self._frame = state_to_frame(self._engine)
        # EC-04: re-apply an already-computed epilogue. _sync_frame rebuilds the
        # frame from scratch (the adapter leaves epilogue empty on purpose,
        # since it can block), so without this a re-sync after the epilogue
        # landed would blank it back to the placeholder.
        epilogue = getattr(self, "_epilogue_text", "")
        if epilogue and self._frame.game_over:
            self._frame.epilogue = epilogue

    def _save(self) -> None:
        """Persist the current run to disk after a backpack/parcel mutation.

        Ledger actions don't route through StepEngine.step()'s autosave, so a
        wallet created via the ledger menu (or any settle/parcel change) would
        be lost on quit without this explicit write (cli-tui-001).
        """
        if self._engine:
            from .save import save_game

            save_game(self._engine.state)

    def _render_all(self) -> None:
        s = self._frame
        self.query_one("#status", StatusPanel).update_from(s)
        self.query_one("#supplies", SuppliesPanel).update_from(s)
        self.query_one("#map", MapPanel).update_from(s)
        self.query_one("#narration", NarrationPanel).update_from(s)
        self.query_one("#party", PartyPanel).update_from(s)
        # cli-tui-B-02: while a worker is running, the event bar shows a
        # loading state instead of the choices.
        self.query_one("#eventbar", EventBar).update_from(s, busy=self._in_flight)
        self.query_one("#journal", JournalDrawer).update_from(s)
        # EC-04 / FEAT-CLITUI-01: keep the end screen's content fresh (the GM
        # epilogue lands asynchronously, so this re-renders when it arrives).
        self.query_one("#end_screen", EndScreen).update_from(s)

        self.query_one("#help").display = self.show_help
        self.query_one("#journal").display = self.show_journal
        self.query_one("#end_screen").display = self.show_end
        self.query_one("#ledger_menu").display = self.show_ledger
        self.query_one("#nudge").display = self.show_nudge
        self.query_one("#enable_flow").display = self.show_enable_flow
        self.query_one("#wallet_info").display = self.show_wallet_info
        self.query_one("#learn_more").display = self.show_learn_more
        self.query_one("#send_parcel").display = self.show_send_parcel
        # cli-tui-B-03: the parcel input rides with the send-parcel overlay.
        # It is only focusable while shown, so it never steals focus from the
        # gameplay keys when hidden.
        parcel_input = self.query_one("#parcel_input", Input)
        parcel_input.display = self.show_send_parcel
        parcel_input.can_focus = self.show_send_parcel
        if not self.show_send_parcel and self.focused is parcel_input:
            # Release focus so gameplay key bindings work again once the
            # overlay closes.
            try:
                self.set_focus(None)
            except Exception:
                pass
        self.query_one("#parcel_notify").display = self.show_parcel_notify

    def _after_step(self) -> None:
        """Sync frame, render, optionally narrate, and check nudge."""
        self._sync_frame()
        self._render_all()

        # EC-04 / FEAT-CLITUI-01: the run just ended — raise the end screen and
        # kick off the (fallback-safe) GM epilogue. Done before the nudge check
        # so a death never pops the backpack nudge over the ending.
        if (
            self._engine
            and self._engine.state.game_over
            and not self.show_end
        ):
            self._begin_end_screen()
            return

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

        # gm-B-06 consumer: voice synth/playback runs on the bridge's own
        # worker thread, so an infra failure (no audio player, engine raised)
        # surfaces asynchronously — often a tick or two after the enqueue that
        # triggered it. Re-check the bridge status every step so the player is
        # told the storyteller went quiet, instead of a silent stale ON state.
        self._check_voice_health()

    def _check_voice_health(self) -> None:
        """If the voice bridge failed at runtime, notify once and disable.

        gm-B-06 consumer for VoiceBridge.status(): when voice was on for the
        player but the bridge self-disabled on a runtime audio failure
        (status()['available'] is False with a last_error), surface the reason
        a single time and flip the UI's _voice_enabled False so the footer /
        toggle state stop claiming voice is on.
        """
        bridge = self._voice_bridge
        if bridge is None or self._voice_failure_notified:
            return
        # Only act on a genuine runtime failure: the bridge reports a reason
        # and is no longer available. (A bridge that was simply never started,
        # or toggled off cleanly, has no last_error.)
        status = bridge.status()
        if status["available"] or not status["last_error"]:
            return
        self._voice_failure_notified = True
        self._voice_enabled = False
        self.notify(f"Voice unavailable - {status['last_error']}")
        # Reflect the quieted state in the event bar / footer immediately.
        self._render_all()

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
        """Resolve a visible choice (A-G) to an intent and step the engine.

        In CAMP phase, the conditional escape valves E/F/G map to their own
        IntentActions via the shared camp_choices() table (cli-tui-B-01); a
        valve letter the gate hasn't opened resolves to nothing and is
        ignored. In EVENT/ROUTE phase the engine consumes the raw CHOOSE id.
        """
        if not self._engine or self._in_flight:
            return

        from .intent import GamePhase, IntentAction, PlayerIntent

        if self._engine.phase == GamePhase.CAMP:
            from .adapter import camp_choice_intent

            action = camp_choice_intent(self._engine.state, choice_id)
            if action is None:
                return
            intent = PlayerIntent(action=action)
        else:
            intent = PlayerIntent(
                action=IntentAction.CHOOSE,
                choice_id=choice_id,
            )
        self._run_step(intent)

    def action_intent(self, intent_str: str) -> None:
        """Map hotkeys t/r/h/p to engine intents."""
        if not self._engine or self._in_flight:
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

        self._run_step(PlayerIntent(action=action))

    # ── Worker-driven stepping (cli-tui-B-02) ──────────────────────

    def _run_step(self, intent) -> None:
        """Dispatch one engine step onto a worker thread.

        step() can block for tens of seconds on a GM/network call. Running it
        on the Textual UI thread froze the whole interface (cli-tui-B-02).
        Here we flip into a visible 'thinking' state, ignore further hotkeys,
        and hand the blocking call to a thread worker. The GM-off path is
        identical in behavior — it just returns fast.

        gm-feat-01: before the step runs we arm the GM streaming sink so the
        scene/outcome narration is shown progressively (see
        _install_gm_stream_wrapper / _on_narration_token). The sink is a no-op
        whenever the GM is disabled or falls back, so the GM-off and fallback
        paths render instantly with no streaming.
        """
        # If there is no live App event loop (unit tests calling the action
        # directly), run synchronously so existing call sites keep working.
        if not self._has_worker_runtime():
            self._engine.step(intent)
            self._after_step()
            return

        self._in_flight = True
        self._streaming = False
        self._arm_stream()
        self._render_all()
        self._step_worker(intent)

    def _arm_stream(self) -> None:
        """Arm the per-step GM token sink (gm-feat-01).

        Installs (once) a thin wrapper around the engine's GMClient
        scene/outcome generators that injects our on_token callback, then sets
        the live sink for this step. When the GM is disabled, unreachable, or
        produces nothing usable, no token ever flows and the narration is
        rendered in one shot by the post-step _render_all — so the fallback and
        GM-off paths are unchanged.
        """
        self._install_gm_stream_wrapper()
        self._token_sink = self._on_narration_token

    def _install_gm_stream_wrapper(self) -> None:
        """Wrap the engine's GMClient generators to inject on_token (idempotent).

        gm-feat-01: the StepEngine calls gm.generate_scene/outcome without an
        on_token callback. Rather than reach into the engine (out of cli-tui
        ownership), we wrap the GMClient instance the engine already holds so
        that — only when a live token sink is set for the current step — the
        streaming code path in gm.py fires. With no sink set the wrappers pass
        on_token=None, i.e. the exact non-streamed round-trip. The wrap is a
        plain delegation, so retries / tone-lint / fallback-never-bricks all
        live untouched in gm.py.
        """
        gm = getattr(self._engine, "gm", None)
        if gm is None or self._gm_wrapped:
            return

        orig_scene = gm.generate_scene
        orig_outcome = gm.generate_outcome

        def scene(*args, **kwargs):
            kwargs.setdefault("on_token", self._token_sink)
            return orig_scene(*args, **kwargs)

        def outcome(*args, **kwargs):
            kwargs.setdefault("on_token", self._token_sink)
            return orig_outcome(*args, **kwargs)

        gm.generate_scene = scene
        gm.generate_outcome = outcome
        self._gm_wrapped = True

    def _on_narration_token(self, delta: str) -> None:
        """GM streaming callback — runs on the WORKER thread (gm-feat-01).

        Every widget mutation must be marshalled back onto the UI thread, so we
        hand the delta to _apply_narration_token via call_from_thread. The first
        token of a step flips _streaming True, which lets the event bar drop the
        'thinking…' line and the narration panel begin showing prose. A buggy
        renderer can never brick generation: gm._safe_emit already swallows any
        exception this callback raises.
        """
        if not delta:
            return
        try:
            self.call_from_thread(self._apply_narration_token, delta)
        except Exception:
            # No live loop (defensive): drop the delta; the final render still
            # paints the complete narration.
            pass

    def _apply_narration_token(self, delta: str) -> None:
        """UI-thread half of streaming: grow the narration panel (gm-feat-01)."""
        first = not self._streaming
        self._streaming = True
        panel = self.query_one("#narration", NarrationPanel)
        if first:
            # First token: leave 'thinking' behind and open a fresh buffer.
            panel.stream_reset()
            # Repaint the event bar so the busy 'thinking' line yields to the
            # streaming state (the choices are still withheld until completion).
            self.query_one("#eventbar", EventBar).update_from(
                self._frame, busy=True, streaming=True,
            )
        panel.stream_append(delta)

    # ── End-of-run screen (EC-04 / FEAT-CLITUI-01) ─────────────────

    def _begin_end_screen(self) -> None:
        """Raise the end screen and compute the GM epilogue (fallback-safe).

        The graded ending, postcard, and diagnostics are already on the frame
        (populated by the adapter) and render instantly. The GM epilogue can
        block on a network round-trip, so it is computed on a worker thread and
        filled in by _finish_epilogue. generate_epilogue NEVER returns None — it
        falls back to a deterministic floor — so a dead/disabled GM still yields
        a real ending, just without the narrated dressing.
        """
        self._close_all_overlays()
        self.show_end = True
        self._render_all()

        ending = self._engine.state.ending
        if ending is None:
            # Defensive: recompute deterministically so the worker always has an
            # EndingResult to narrate (the adapter does the same).
            from .step_engine import compute_ending

            ending = compute_ending(self._engine.state)

        if not self._has_worker_runtime():
            self._epilogue_blocking(ending)
            return
        self._epilogue_worker(ending)

    def _epilogue_blocking(self, ending) -> None:
        """Synchronous epilogue (unit tests / no live loop)."""
        text = self._compute_epilogue(ending)
        self._finish_epilogue(text)

    @work(thread=True, exclusive=True, group="epilogue")
    def _epilogue_worker(self, ending) -> None:
        text = self._compute_epilogue(ending)
        self.call_from_thread(self._finish_epilogue, text)

    def _compute_epilogue(self, ending) -> str:
        """Call the (fallback-safe) GM epilogue API; never raises, never None.

        gm.generate_epilogue computes a deterministic floor first and only
        dresses it up if the GM is reachable, so this is safe to call with the
        GM off. Any unexpected error still degrades to the deterministic floor
        rather than leaving the end screen blank.
        """
        gm = getattr(self._engine, "gm", None)
        if gm is None:
            from .gm import build_deterministic_epilogue

            return build_deterministic_epilogue(self._engine.state, ending)
        try:
            return gm.generate_epilogue(self._engine.state, ending)
        except Exception:
            from .gm import build_deterministic_epilogue

            return build_deterministic_epilogue(self._engine.state, ending)

    def _finish_epilogue(self, text: str) -> None:
        """UI-thread completion: store the epilogue and repaint the end screen."""
        self._epilogue_text = text
        self._frame.epilogue = text
        self._render_all()

    def _has_worker_runtime(self) -> bool:
        """True only when a real Textual event loop is driving this App.

        Unit tests build the App without run()/run_test() and call actions
        directly; in that case run_worker/call_from_thread have no loop to
        target, so we fall back to a synchronous step.
        """
        try:
            return bool(self.is_running)
        except Exception:
            return False

    # _in_flight invariant (read before touching any @work worker below):
    #   * exclusive=True is PER-GROUP. The "step" group (gameplay step) and the
    #     "ledger" group (enable/settle/wallet_info/send_parcel) are SEPARATE
    #     groups, so Textual will happily run one of each concurrently — its
    #     exclusivity only cancels a prior worker in the SAME group.
    #   * The cross-action guard (a gameplay step must not race a ledger call,
    #     and vice versa) is therefore NOT provided by exclusive=True. It is the
    #     single self._in_flight flag, checked at the top of every dispatching
    #     action handler and cleared in each _finish_* completion.
    #   * That flag is safe as a plain bool ONLY because every action handler
    #     runs on the single-threaded Textual event loop, so the read-check and
    #     the write (self._in_flight = True) are never interleaved across
    #     actions. The workers themselves only ever flip it back to False via
    #     call_from_thread, i.e. marshalled back onto that same loop.
    @work(thread=True, exclusive=True, group="step")
    def _step_worker(self, intent) -> None:
        """Thread worker: the actual blocking engine step + frame sync."""
        self._engine.step(intent)
        # Marshal all widget/state mutations back onto the UI thread.
        self.call_from_thread(self._finish_step)

    def _finish_step(self) -> None:
        """UI-thread completion: clear busy state, sync, render, narrate."""
        self._in_flight = False
        # gm-feat-01: disarm the streaming sink and clear the flag so the next
        # step starts clean. _after_step's _render_all repaints the narration
        # panel from the fully-parsed FrameState, overwriting any streamed
        # partial with the final text (streamed and non-streamed converge).
        self._token_sink = None
        self._streaming = False
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
        """Start the backpack enable flow.

        cli-tui-B-02 / ledger-B01: enabling creates testnet wallets, which can
        block for 30-60s. Run that round-trip on a worker thread with the
        enable-flow overlay's progress state visible, so the UI never freezes.
        """
        if not self._engine or self._in_flight:
            return

        self._close_all_overlays()
        self.show_enable_flow = True
        self._render_all()

        from .backpack_ui import EnableFlowOverlay

        overlay = self.query_one("#enable_flow", EnableFlowOverlay)
        overlay.show_progress()

        if not self._has_worker_runtime():
            # Synchronous fallback (unit tests / no live loop).
            self._enable_blocking()
            return

        self._in_flight = True
        self._enable_worker()

    def _enable_blocking(self):
        """The blocking enable round-trip. Returns the EnableResult."""
        from .backpack import BackpackManager

        mgr = BackpackManager()
        result = mgr.enable(self._engine.state)
        mgr.close()
        if result.success:
            self._save()
        self._finish_enable(result)
        return result

    @work(thread=True, exclusive=True, group="ledger")
    def _enable_worker(self) -> None:
        from .backpack import BackpackManager

        mgr = BackpackManager()
        result = mgr.enable(self._engine.state)
        mgr.close()
        if result.success:
            self._save()
        self.call_from_thread(self._finish_enable, result)

    def _finish_enable(self, result) -> None:
        """UI-thread completion for the enable flow."""
        self._in_flight = False
        from .backpack_ui import EnableFlowOverlay

        overlay = self.query_one("#enable_flow", EnableFlowOverlay)
        if result.success:
            overlay.show_success(result.wallet_address)
            self.notify("Ledger Backpack enabled")
        else:
            overlay.show_failure(result.message)
        self._sync_frame()
        self._render_all()

    def action_ledger_disable(self) -> None:
        """Disable the backpack."""
        # Serialize against any in-flight worker: disable mutates state and
        # _save()s synchronously, so racing it with a step/ledger worker could
        # persist a torn snapshot. The _in_flight flag is the cross-action
        # guard (see the worker-invariant note above).
        if not self._engine or self._in_flight:
            return

        from .backpack import BackpackManager

        mgr = BackpackManager()
        mgr.disable(self._engine.state)
        self._save()

        self._close_all_overlays()
        self._sync_frame()
        self._render_all()
        self.notify("Ledger Backpack disabled")

    def action_ledger_settle(self) -> None:
        """Manual settlement.

        cli-tui-B-02 / ledger-B01: settlement submits to the testnet and can
        block; run it on a worker thread with the busy state visible.
        """
        if not self._engine or self._in_flight:
            return

        cur = None
        for n in self._engine.state.map_nodes:
            if n.node_id == self._engine.state.location_id:
                cur = n
                break
        location = cur.name if cur else "Unknown"

        if not self._has_worker_runtime():
            self._settle_blocking(location)
            return

        self._in_flight = True
        self._close_all_overlays()
        self._render_all()
        self._settle_worker(location)

    def _settle_blocking(self, location: str):
        from .backpack import BackpackManager

        mgr = BackpackManager()
        result = mgr.settle(self._engine.state, location)
        mgr.close()
        self._save()
        self._finish_settle(result)
        return result

    @work(thread=True, exclusive=True, group="ledger")
    def _settle_worker(self, location: str) -> None:
        from .backpack import BackpackManager

        mgr = BackpackManager()
        result = mgr.settle(self._engine.state, location)
        mgr.close()
        self._save()
        self.call_from_thread(self._finish_settle, result)

    def _finish_settle(self, result) -> None:
        self._in_flight = False
        self.notify(result.message)
        self._sync_frame()
        self._render_all()

    def action_wallet_info(self) -> None:
        """Show wallet info overlay.

        cli-tui-B-02 / ledger-B01: wallet_info reads balances from the testnet
        and can block; run it on a worker with a loading state.
        """
        if not self._engine or self._in_flight:
            return

        if not self._has_worker_runtime():
            self._wallet_info_blocking()
            return

        self._in_flight = True
        self._close_all_overlays()
        self.show_wallet_info = True
        from .backpack_ui import WalletInfoOverlay

        self.query_one("#wallet_info", WalletInfoOverlay).update(
            "[b]Wallet Info[/b]\n\nReading the ledger..."
        )
        self._render_all()
        self._wallet_info_worker()

    def _wallet_info_blocking(self):
        from .backpack import BackpackManager

        mgr = BackpackManager()
        info = mgr.wallet_info(self._engine.state)
        self._finish_wallet_info(info)
        return info

    @work(thread=True, exclusive=True, group="ledger")
    def _wallet_info_worker(self) -> None:
        from .backpack import BackpackManager

        mgr = BackpackManager()
        info = mgr.wallet_info(self._engine.state)
        self.call_from_thread(self._finish_wallet_info, info)

    def _finish_wallet_info(self, info) -> None:
        self._in_flight = False
        from .backpack_ui import WalletInfoOverlay

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
        from .resources import RESOURCE_CATALOG

        supplies = self._engine.state.supplies
        supply_lines = []
        for key in sorted(XRPL_TOKEN_MAP.keys()):
            amount = supplies.get(key)
            # Show the 4-char display label (FOOD/WATR/...) like SuppliesPanel,
            # not the raw lowercase game key (cli-tui-007).
            rdef = RESOURCE_CATALOG.get(key)
            label = rdef.display if rdef else key.upper()
            supply_lines.append(f"  {label}: {amount}")

        self._close_all_overlays()
        self.show_send_parcel = True
        self.query_one(
            "#send_parcel", SendParcelOverlay,
        ).show_form("\n".join(supply_lines))
        self._render_all()

        # cli-tui-B-03: clear and focus the real input so the player can type
        # the parcel command. (No-op without a live DOM, e.g. unit tests.)
        try:
            inp = self.query_one("#parcel_input", Input)
            inp.value = ""
            inp.focus()
        except Exception:
            pass

    def on_input_submitted(self, event) -> None:
        """Parse + dispatch the send-parcel command (cli-tui-B-03).

        Honest replacement for the dead-end overlay: the player types
        '<address> <supply> <amount>' (or 'cancel'), we validate the address
        with the same shape check the CLI uses, then send + persist on a
        worker thread and report success/failure on the overlay.
        """
        if event.input.id != "parcel_input":
            return
        if not self.show_send_parcel or not self._engine:
            return

        raw = (event.value or "").strip()
        event.input.value = ""

        if not raw or raw.lower() == "cancel":
            self.show_send_parcel = False
            self._render_all()
            return

        from .backpack_ui import SendParcelOverlay

        overlay = self.query_one("#send_parcel", SendParcelOverlay)

        parts = raw.split()
        if len(parts) != 3:
            overlay.show_failure(
                "Expected: <address> <supply> <amount>  "
                "(e.g. rPT1Sjq... food 10)"
            )
            self._render_all()
            return

        address, supply, amount_str = parts
        try:
            amount = int(amount_str)
        except ValueError:
            overlay.show_failure(f"'{amount_str}' is not a whole number.")
            self._render_all()
            return
        if amount <= 0:
            overlay.show_failure("Amount must be a positive whole number.")
            self._render_all()
            return

        # Same address shape check the CLI uses (cli-tui-002), so a bad
        # address is caught before any testnet round-trip.
        from .cli import _is_valid_recipient

        if not _is_valid_recipient(address):
            overlay.show_failure(
                f"'{address}' is not a valid XRPL classic address "
                "(starts with 'r', 25-35 base58 chars)."
            )
            self._render_all()
            return

        supply = supply.lower()
        if self._in_flight:
            return
        if not self._has_worker_runtime():
            self._send_parcel_blocking(address, supply, amount)
            return
        self._in_flight = True
        overlay.update("[b]Send Parcel[/b]\n\nSending parcel...")
        self._render_all()
        self._send_parcel_worker(address, supply, amount)

    def _send_parcel_blocking(self, address: str, supply: str, amount: int):
        from .backpack import BackpackManager

        mgr = BackpackManager()
        result = mgr.send_parcel(self._engine.state, address, supply, amount)
        mgr.close()
        if result.success:
            self._save()
        self._finish_send_parcel(result)
        return result

    @work(thread=True, exclusive=True, group="ledger")
    def _send_parcel_worker(
        self, address: str, supply: str, amount: int,
    ) -> None:
        from .backpack import BackpackManager

        mgr = BackpackManager()
        result = mgr.send_parcel(self._engine.state, address, supply, amount)
        mgr.close()
        if result.success:
            self._save()
        self.call_from_thread(self._finish_send_parcel, result)

    def _finish_send_parcel(self, result) -> None:
        self._in_flight = False
        from .backpack_ui import SendParcelOverlay

        overlay = self.query_one("#send_parcel", SendParcelOverlay)
        if result.success:
            overlay.show_success(result.message)
            self.notify("Parcel sent")
        else:
            overlay.show_failure(result.message)
        self._sync_frame()
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
        # Serialize against any in-flight worker (accept mutates state +
        # _save()s synchronously) — see the worker-invariant note above.
        if (
            not self._engine
            or self._in_flight
            or not hasattr(self, "_current_parcel")
        ):
            return

        from .backpack import BackpackManager

        mgr = BackpackManager()
        mgr.accept_parcel(self._current_parcel, self._engine.state)
        self._save()

        contents = ", ".join(
            f"+{v} {k}" for k, v in self._current_parcel.contents.items()
        )
        self.notify(f"Parcel accepted: {contents}")
        self._close_all_overlays()
        self._sync_frame()
        self._render_all()

    def action_refuse_parcel(self) -> None:
        """Refuse the currently shown parcel."""
        # Serialize against any in-flight worker (refuse mutates state +
        # _save()s synchronously) — see the worker-invariant note above.
        if (
            not self._engine
            or self._in_flight
            or not hasattr(self, "_current_parcel")
        ):
            return

        from .backpack import BackpackManager

        mgr = BackpackManager()
        mgr.refuse_parcel(self._current_parcel)
        self._save()

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
        # Serialize against any in-flight worker: dismiss flips a flag and
        # _save()s synchronously — see the worker-invariant note above.
        if self._in_flight:
            return
        if self._engine:
            self._engine.state.backpack.nudge_dismissed = True
            self._save()
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

        # cli-tui-B-02: while a step/ledger worker is running, swallow the
        # on_key-routed gameplay/overlay letters so queued keypresses can't
        # pile up and double-act. Escape (to close an overlay) still works.
        if self._in_flight and key != "escape":
            no_overlay = not any([
                self.show_ledger, self.show_nudge,
                self.show_enable_flow, self.show_wallet_info,
                self.show_learn_more, self.show_help,
                self.show_send_parcel, self.show_parcel_notify,
            ])
            if no_overlay and key in ("e", "f", "g"):
                event.prevent_default()
                return

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

        # cli-tui-B-01: e/f/g pick the conditional escape valves when no
        # overlay is open. (5/6/7 are bound globally; the letters are routed
        # here so they don't collide with the ledger/nudge overlay letters.)
        no_overlay = not any([
            self.show_ledger, self.show_nudge,
            self.show_enable_flow, self.show_wallet_info,
            self.show_learn_more, self.show_help,
            self.show_send_parcel, self.show_parcel_notify,
        ])
        if no_overlay and key in ("e", "f", "g"):
            self.action_choose(key.upper())
            event.prevent_default()
            return

        # Voice interrupt
        if self._voice_enabled and self._voice_bridge:
            self._voice_bridge.interrupt()


if __name__ == "__main__":
    LedgerTrailApp().run()
