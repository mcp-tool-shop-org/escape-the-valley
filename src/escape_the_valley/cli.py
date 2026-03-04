"""CLI entry point — Typer commands for Escape the Valley."""

from __future__ import annotations

import os

import typer
from rich.console import Console

from . import __version__
from .engine import GameEngine
from .gm import GMConfig
from .models import GMProfile
from .save import has_save, load_game
from .ui import show_journal, show_status, show_title_screen
from .worldgen import create_new_run

app = typer.Typer(
    name="trail",
    help="Escape the Valley: Ledger Trail — Oregon Trail-style survival game",
    no_args_is_help=True,
)
console = Console()


@app.command()
def new(
    seed: int | None = typer.Option(None, "--seed", "-s", help="RNG seed for reproducible runs"),
    profile: str = typer.Option(
        "fireside",
        "--gm-profile",
        "-p",
        help="GM personality: chronicler, fireside, lantern",
    ),
    weirdness: int = typer.Option(2, "--weirdness", "-w", help="Weirdness level 0-3"),
    gm_off: bool = typer.Option(False, "--gm-off", help="Disable Ollama GM (deterministic only)"),
    model: str = typer.Option("llama3.2", "--model", "-m", help="Ollama model name"),
) -> None:
    """Start a new run."""
    show_title_screen()

    # Parse profile
    try:
        gm_profile = GMProfile(profile.lower())
    except ValueError:
        console.print(
            f"[red]Unknown profile: {profile}. "
            "Use chronicler, fireside, or lantern.[/red]"
        )
        raise typer.Exit(1) from None

    weirdness = max(0, min(3, weirdness))

    # Check for existing save
    if has_save():
        if not typer.confirm("A saved game exists. Start a new one? (old save will be lost)"):
            raise typer.Exit(0)

    # Create new run
    state = create_new_run(seed=seed, gm_profile=gm_profile, weirdness_level=weirdness)

    console.print(f"  Run ID: [bold]{state.run_id}[/bold]")
    console.print(f"  Seed: [bold]{state.seed}[/bold]")
    console.print(f"  Profile: [bold]{gm_profile.value}[/bold]")
    console.print(f"  Weirdness: [bold]{weirdness}[/bold]")
    console.print(f"  Twists: [bold]{', '.join(t.value for t in state.twists)}[/bold]")
    console.print(f"  Party: [bold]{', '.join(m.name for m in state.party.members)}[/bold]")
    console.print()

    # Configure GM
    gm_config = GMConfig(
        host=os.environ.get("OLLAMA_HOST", "http://localhost:11434"),
        model=model,
        enabled=not gm_off,
    )

    engine = GameEngine(state, gm_config)
    engine.run()


@app.command()
def play(
    gm_off: bool = typer.Option(False, "--gm-off", help="Disable Ollama GM"),
    model: str = typer.Option("llama3.2", "--model", "-m", help="Ollama model name"),
) -> None:
    """Continue a saved run."""
    show_title_screen()

    state = load_game()
    if state is None:
        console.print("[red]No saved game found. Use 'trail new' to start one.[/red]")
        raise typer.Exit(1)

    if state.game_over:
        console.print("[yellow]This run is over. Use 'trail new' to start a fresh one.[/yellow]")
        raise typer.Exit(0)

    console.print(f"  Resuming run [bold]{state.run_id}[/bold] — Day {state.day}")
    console.print()

    gm_config = GMConfig(
        host=os.environ.get("OLLAMA_HOST", "http://localhost:11434"),
        model=model,
        enabled=not gm_off,
    )

    engine = GameEngine(state, gm_config)
    engine.run()


@app.command()
def status() -> None:
    """Show current party, wagon, and supplies."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    show_status(state)


@app.command()
def journal(
    limit: int = typer.Option(10, "--limit", "-n", help="Number of entries to show"),
) -> None:
    """Show recent journal entries."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    show_journal(state.journal, limit)


@app.command(name="self-check")
def self_check() -> None:
    """Check game environment health."""
    console.print("[bold]Escape the Valley — Self Check[/bold]\n")
    console.print(f"  Version: {__version__}")

    # Check save directory
    if has_save():
        state = load_game()
        if state:
            console.print(f"  [green]Save found:[/green] Run {state.run_id}, Day {state.day}")
        else:
            console.print("  [yellow]Save file exists but could not be loaded.[/yellow]")
    else:
        console.print("  [dim]No saved game.[/dim]")

    # Check Ollama
    host = os.environ.get("OLLAMA_HOST", "http://localhost:11434")
    GMConfig(host=host)
    try:
        import httpx
        resp = httpx.get(f"{host}/api/tags", timeout=5)
        if resp.status_code == 200:
            models = resp.json().get("models", [])
            model_names = [m.get("name", "?") for m in models[:5]]
            console.print(f"  [green]Ollama reachable[/green] at {host}")
            console.print(f"  Models: {', '.join(model_names)}")
        else:
            console.print(f"  [yellow]Ollama returned {resp.status_code}[/yellow]")
    except Exception:
        console.print(f"  [red]Ollama not reachable[/red] at {host}")

    console.print()


@app.command()
def tui() -> None:
    """Launch the Textual UI (full-screen terminal app)."""
    from .tui_app import LedgerTrailApp

    LedgerTrailApp().run()


@app.command()
def version() -> None:
    """Show version."""
    console.print(f"Escape the Valley: Ledger Trail v{__version__}")


if __name__ == "__main__":
    app()
