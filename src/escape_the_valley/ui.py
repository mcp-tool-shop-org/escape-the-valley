"""Terminal UX — Rich panels, menus, journal display."""

from __future__ import annotations

import os

from rich import box
from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

from .models import Condition, JournalEntry, Pace, RunState
from .resources import RESOURCE_CATALOG


def _no_color() -> bool:
    """Honor the NO_COLOR convention (https://no-color.org).

    cli-tui-B-06: when NO_COLOR is set (to anything), the player has asked for
    a monochrome terminal. We still need urgency to survive, which is why the
    danger cues below are plain text, not just color.
    """
    return os.environ.get("NO_COLOR") is not None


# Rich already auto-detects NO_COLOR, but we pin it explicitly so the behavior
# is deterministic and testable regardless of Rich version.
console = Console(no_color=_no_color())


def _supply_cue(val: int, warning_low: int | None = None) -> str:
    """Non-color urgency tag for a supply count (cli-tui-B-06 / F-c0b4e383).

    Returns a plain-text tag so a critically low resource is legible on a
    monochrome or colorblind read, where a red number alone is invisible.

    Thresholds come from ``ResourceDef.warning_low`` (0 is always CRITICAL).
    The one-arg form keeps the historical band of 5 so older call sites and
    tests stay stable; paint paths must pass the catalog value.
    """
    if val <= 0:
        return " (CRITICAL)"
    threshold = 5 if warning_low is None else warning_low
    if threshold > 0 and val <= threshold:
        return " (LOW)"
    return ""


def _health_cue(health: int, *, alive: bool = True) -> str:
    """Plain (!) marker for the CLI/TUI health band (cli-tui-B-06)."""
    if alive and health <= 30:
        return " (!)"
    return ""


def _wagon_cue(condition: int) -> str:
    """Plain wagon-condition tag, keyed off the same 15/30 bands as warnings."""
    if condition <= 15:
        return " (CRITICAL)"
    if condition < 30:
        return " (LOW)"
    return ""


def show_title_screen() -> None:
    """Display the game title."""
    title = Text()
    title.append("ESCAPE THE VALLEY", style="bold white")
    title.append("\n")
    title.append("Ledger Trail", style="dim")
    console.print(Panel(title, border_style="dim", box=box.DOUBLE))
    console.print()


def show_status(state: RunState) -> None:
    """Show the main camp status panel."""
    node = _find_node(state)
    location_name = node.name if node else "Unknown"
    biome = node.biome.value if node else "?"

    # Status header
    header = Table(show_header=False, box=None, padding=(0, 2))
    header.add_column(width=30)
    header.add_column(width=25)
    header.add_column(width=25)

    header.add_row(
        f"Day {state.day}  |  {state.time_of_day.value.title()}",
        f"Location: {location_name}",
        f"Biome: {biome}",
    )
    header.add_row(
        f"Pace: {state.wagon.pace.value}",
        f"Distance to next: {state.distance_remaining} mi",
        f"Traveled: {state.distance_traveled} mi",
    )
    console.print(Panel(
        header, title="[bold]Trail Status[/bold]",
        border_style="blue", box=box.ROUNDED,
    ))

    # Party
    party_table = Table(show_header=True, box=box.SIMPLE, header_style="bold")
    party_table.add_column("Name", width=15)
    party_table.add_column("Health", width=10)
    party_table.add_column("Condition", width=12)
    party_table.add_column("Traits", width=20)

    for member in state.party.members:
        health_style = "green" if member.health > 60 else "yellow" if member.health > 30 else "red"
        if not member.is_alive():
            health_style = "dim red"

        cond_style = "green" if member.condition == Condition.HEALTHY else "yellow"
        if not member.is_alive():
            cond_style = "dim"

        # cli-tui-B-06: critically low health gets a plain (!) marker so the
        # danger survives a monochrome read, not just a red number.
        health_cue = _health_cue(member.health, alive=member.is_alive())
        if member.is_alive():
            health_cell = f"[{health_style}]{member.health}{health_cue}[/]"
        else:
            health_cell = "[dim]dead[/]"

        party_table.add_row(
            member.name if member.is_alive() else f"[dim strikethrough]{member.name}[/]",
            health_cell,
            f"[{cond_style}]{member.condition.value}[/]" if member.is_alive() else "",
            ", ".join(t.value for t in member.traits) if member.is_alive() else "",
        )

    # cli-tui-B-06: a plain morale tag so low/critical morale reads without
    # relying on the panel border color alone.
    morale = state.party.morale
    morale_cue = (
        " (CRITICAL)" if morale <= 20
        else " (LOW)" if morale <= 40
        else ""
    )
    console.print(Panel(
        party_table,
        title=f"[bold]Party[/bold]  Morale: {morale}/100{morale_cue}",
        border_style=(
            "green" if morale > 40
            else "yellow" if morale > 20
            else "red"
        ),
        box=box.ROUNDED,
    ))

    # Supplies + Wagon side by side
    # F-c0b4e383: put the (LOW)/(CRITICAL) tag on the *name* column so a
    # width-80 force_terminal capture cannot wrap '0 (CRITICAL)' in the
    # 8-wide value cell to '(CRITI…'. Cue thresholds come from the catalog
    # (meds/parts warning_low=1), not a global 5.
    supplies_table = Table(show_header=False, box=None)
    supplies_table.add_column(width=20)
    supplies_table.add_column(width=5, justify="right")

    s = state.supplies
    for name, key in (
        ("Food", "food"),
        ("Water", "water"),
        ("Medicine", "meds"),
        ("Ammo", "ammo"),
        ("Parts", "parts"),
    ):
        val = s.get(key)
        warning_low = RESOURCE_CATALOG[key].warning_low
        cue = _supply_cue(val, warning_low)
        if val <= 0:
            style = "red bold"
        elif warning_low > 0 and val <= warning_low:
            style = "yellow"
        else:
            style = "white"
        supplies_table.add_row(f"{name}{cue}", f"[{style}]{val}[/]")

    wagon_text = (
        f"Condition: {_bar(state.wagon.condition)}\n"
        f"Animals:   {_bar(state.wagon.animals_health)}"
    )

    cols = Table(show_header=False, box=None, padding=(0, 3))
    cols.add_column(width=25)
    cols.add_column(width=35)
    cols.add_row(
        Panel(supplies_table, title="[bold]Supplies[/bold]", box=box.ROUNDED, border_style="cyan"),
        Panel(wagon_text, title="[bold]Wagon[/bold]", box=box.ROUNDED, border_style="cyan"),
    )
    console.print(cols)


def show_event_scene(title: str, narration: str, choices: list[dict]) -> str:
    """Display an event scene and get player choice."""
    console.print()
    console.print(Panel(
        narration,
        title=f"[bold yellow]{title}[/bold yellow]",
        border_style="yellow",
        box=box.DOUBLE,
    ))

    for i, choice in enumerate(choices):
        choice_id = choice.get("id", chr(65 + i))
        label = choice.get("label", f"Option {choice_id}")
        risk = choice.get("risk_hint", "")
        cost = choice.get("cost_hint", "")

        hint = ""
        if risk or cost:
            parts = []
            if risk:
                parts.append(risk)
            if cost:
                parts.append(cost)
            hint = f" [dim]({'; '.join(parts)})[/dim]"

        console.print(f"  [bold]{choice_id}[/bold]. {label}{hint}")

    console.print()

    valid_ids = [c.get("id", chr(65 + i)) for i, c in enumerate(choices)]
    while True:
        answer = console.input("[bold]Your choice: [/bold]").strip().upper()
        if answer in valid_ids:
            return answer
        console.print(f"  [dim]Choose one of: {', '.join(valid_ids)}[/dim]")


def _is_numeric_delta(val: object) -> bool:
    """True for a delta value safe to do arithmetic on (cli-tui-008 guard).

    A corrupted/loaded save (an older schema, partial corruption, or a future
    engine change) can carry a non-numeric delta value. Shared by
    show_outcome and _delta_magnitude so both degrade the same way — skip the
    bad value — instead of raising.
    """
    return isinstance(val, (int, float))


def _delta_magnitude(deltas: dict) -> float:
    """Magnitude of a journal entry's deltas, ignoring non-numeric values.

    Sibling guard to cli-tui-008 (show_outcome): show_game_over ranks journal
    entries by ``abs(sum(deltas.values()))`` to find the "most notable event".
    The same non-numeric delta that show_outcome already defends against
    would raise an uncaught TypeError here too — at the worst possible
    moment, since this only runs once a run has ended and the player is about
    to see the summary screen (F-5ed15e0b).
    """
    if not deltas:
        return 0
    return abs(sum(v for v in deltas.values() if _is_numeric_delta(v)))


def show_outcome(title: str, narration: str, callout: str, deltas: dict) -> None:
    """Display the outcome of a choice."""
    text = narration
    if callout:
        text += f"\n\n[bold]{callout}[/bold]"

    if deltas:
        delta_parts = []
        for key, val in deltas.items():
            # Guard against a corrupted/loaded save with a non-numeric delta —
            # skip it rather than raise a raw TypeError to the player
            # (cli-tui-008).
            if not _is_numeric_delta(val):
                continue
            if val > 0:
                delta_parts.append(f"[green]+{val} {key}[/green]")
            elif val < 0:
                delta_parts.append(f"[red]{val} {key}[/red]")
        if delta_parts:
            text += f"\n\n{' | '.join(delta_parts)}"

    console.print(Panel(text, title=f"[bold]{title}[/bold]", border_style="dim", box=box.ROUNDED))


def show_action_menu() -> str:
    """Show the main action menu and get choice."""
    console.print()
    actions = [
        ("1", "Travel"),
        ("2", "Rest"),
        ("3", "Hunt"),
        ("4", "Repair wagon"),
        ("5", "Check supplies"),
        ("6", "Change pace"),
        ("7", "View journal"),
        ("Q", "Quit and save"),
    ]

    for key, label in actions:
        console.print(f"  [bold]{key}[/bold]. {label}")

    console.print()
    valid = {a[0] for a in actions}
    while True:
        answer = console.input("[bold]What do you do? [/bold]").strip().upper()
        if answer in valid:
            return answer
        console.print(f"  [dim]Choose: {', '.join(sorted(valid))}[/dim]")


def show_pace_menu(current: Pace) -> Pace:
    """Show pace selection menu."""
    console.print(f"\n  Current pace: [bold]{current.value}[/bold]")
    paces = [
        ("1", Pace.SLOW, "Slow — less consumption, less distance, fewer breakdowns"),
        ("2", Pace.STEADY, "Steady — balanced"),
        ("3", Pace.HARD, "Hard — more distance, more consumption, more breakdowns"),
    ]
    for key, pace, desc in paces:
        marker = " [bold green]<[/bold green]" if pace == current else ""
        console.print(f"  [bold]{key}[/bold]. {desc}{marker}")

    while True:
        answer = console.input("\n[bold]Set pace: [/bold]").strip()
        for key, pace, _ in paces:
            if answer == key:
                return pace
        console.print("  [dim]Choose 1, 2, or 3[/dim]")


def show_journal(entries: list[JournalEntry], limit: int = 10) -> None:
    """Display recent journal entries."""
    if not entries:
        console.print("  [dim]No journal entries yet.[/dim]")
        return

    recent = entries[-limit:]
    for entry in recent:
        console.print(f"\n  [bold]Day {entry.day}[/bold] — {entry.location}")
        console.print(f"  [yellow]{entry.scene_title}[/yellow]")
        if entry.narration:
            console.print(f"  {entry.narration[:200]}{'...' if len(entry.narration) > 200 else ''}")
        if entry.choice_made:
            console.print(f"  [dim]Choice: {entry.choice_made}[/dim]")
        if entry.outcome:
            console.print(f"  {entry.outcome[:200]}{'...' if len(entry.outcome) > 200 else ''}")
    console.print()


def show_game_over(state: RunState) -> None:
    """Display end-of-run summary."""
    console.print()
    if state.victory:
        title_text = "[bold green]YOU ESCAPED THE VALLEY[/bold green]"
    else:
        title_text = "[bold red]THE TRAIL CLAIMS ANOTHER[/bold red]"

    summary = f"Days traveled: {state.day}\n"
    summary += f"Distance covered: {state.distance_traveled} miles\n"
    summary += f"Survivors: {state.party.alive_count}/{len(state.party.members)}\n"

    if state.cause_of_death and not state.victory:
        summary += f"\n{state.cause_of_death}\n"

    summary += f"\nSeed: {state.seed}  |  Run ID: {state.run_id}"
    summary += f"\nProfile: {state.gm_profile.value}"

    if state.journal:
        # F-5ed15e0b: ranks by _delta_magnitude (the same non-numeric-safe
        # guard as show_outcome above) instead of raw abs(sum(...)) — a
        # corrupted/loaded save with a non-numeric delta must not crash the
        # end-of-run screen, the one screen the player can least afford to
        # lose.
        best = max(state.journal, key=lambda e: _delta_magnitude(e.deltas))
        if best.scene_title:
            summary += f"\n\nMost notable event: {best.scene_title}"

    console.print(Panel(summary, title=title_text, border_style="bold", box=box.DOUBLE))


def show_route_choice(connections: list[tuple[str, str, int]]) -> str:
    """Show route choice when at a branching node.

    Offered ids are letters (A, B, C, ...) matching the engine/TUI CHOOSE
    contract. The prompt and the keys the player may press are the same
    document — do not claim 1-N.
    """
    console.print("\n  [bold]The trail forks.[/bold]")
    letters: list[str] = []
    by_key: dict[str, str] = {}
    for i, (node_id, name, dist) in enumerate(connections):
        letter = chr(65 + i)  # A, B, C, ...
        letters.append(letter)
        by_key[letter] = node_id
        console.print(f"  [bold]{letter}[/bold]. {name} ({dist} miles)")

    while True:
        answer = console.input("\n[bold]Which way? [/bold]").strip().upper()
        if answer in by_key:
            return by_key[answer]
        console.print(f"  [dim]Choose {', '.join(letters)}[/dim]")


def show_message(msg: str, style: str = "") -> None:
    """Show a simple message."""
    if style:
        console.print(f"  [{style}]{msg}[/{style}]")
    else:
        console.print(f"  {msg}")


def _bar(value: int, width: int = 20) -> str:
    """Create a simple text progress bar."""
    filled = int(value / 100 * width)
    empty = width - filled
    color = "green" if value > 60 else "yellow" if value > 30 else "red"
    return f"[{color}]{'█' * filled}{'░' * empty}[/{color}] {value}%"


def _find_node(state: RunState):
    for node in state.map_nodes:
        if node.node_id == state.location_id:
            return node
    return None
