"""Terminal UX — Rich panels, menus, journal display."""

from __future__ import annotations

from rich import box
from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.text import Text

from .models import Condition, JournalEntry, Pace, RunState

console = Console()


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

        party_table.add_row(
            member.name if member.is_alive() else f"[dim strikethrough]{member.name}[/]",
            f"[{health_style}]{member.health}[/]" if member.is_alive() else "[dim]dead[/]",
            f"[{cond_style}]{member.condition.value}[/]" if member.is_alive() else "",
            ", ".join(t.value for t in member.traits) if member.is_alive() else "",
        )

    console.print(Panel(
        party_table,
        title=f"[bold]Party[/bold]  Morale: {state.party.morale}/100",
        border_style=(
            "green" if state.party.morale > 40
            else "yellow" if state.party.morale > 20
            else "red"
        ),
        box=box.ROUNDED,
    ))

    # Supplies + Wagon side by side
    supplies_table = Table(show_header=False, box=None)
    supplies_table.add_column(width=12)
    supplies_table.add_column(width=8, justify="right")

    s = state.supplies
    for name, val in [("Food", s.food), ("Water", s.water), ("Medicine", s.meds),
                       ("Ammo", s.ammo), ("Parts", s.parts)]:
        style = "white" if val > 5 else "yellow" if val > 0 else "red bold"
        supplies_table.add_row(name, f"[{style}]{val}[/]")

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


def show_outcome(title: str, narration: str, callout: str, deltas: dict) -> None:
    """Display the outcome of a choice."""
    text = narration
    if callout:
        text += f"\n\n[bold]{callout}[/bold]"

    if deltas:
        delta_parts = []
        for key, val in deltas.items():
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
        best = max(state.journal, key=lambda e: abs(sum(e.deltas.values())) if e.deltas else 0)
        if best.scene_title:
            summary += f"\n\nMost notable event: {best.scene_title}"

    console.print(Panel(summary, title=title_text, border_style="bold", box=box.DOUBLE))


def show_route_choice(connections: list[tuple[str, str, int]]) -> str:
    """Show route choice when at a branching node."""
    console.print("\n  [bold]The trail forks.[/bold]")
    for i, (_node_id, name, dist) in enumerate(connections):
        console.print(f"  [bold]{i + 1}[/bold]. {name} ({dist} miles)")

    while True:
        answer = console.input("\n[bold]Which way? [/bold]").strip()
        try:
            idx = int(answer) - 1
            if 0 <= idx < len(connections):
                return connections[idx][0]
        except ValueError:
            pass
        console.print(f"  [dim]Choose 1-{len(connections)}[/dim]")


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
