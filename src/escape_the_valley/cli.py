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
    callouts: str = typer.Option(
        "verbose", "--callouts",
        help="Warning detail: verbose or minimal",
    ),
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
    callouts = callouts.lower()
    if callouts not in ("verbose", "minimal"):
        console.print("[red]--callouts must be 'verbose' or 'minimal'[/red]")
        raise typer.Exit(1)

    # Check for existing save
    if has_save():
        if not typer.confirm("A saved game exists. Start a new one? (old save will be lost)"):
            raise typer.Exit(0)

    # Create new run
    state = create_new_run(seed=seed, gm_profile=gm_profile, weirdness_level=weirdness)
    state.callout_level = callouts

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
def tui(
    seed: int | None = typer.Option(
        None, "--seed", "-s", help="RNG seed for new run",
    ),
    resume: bool = typer.Option(
        False, "--continue", help="Resume saved game",
    ),
    gm_off: bool = typer.Option(
        False, "--gm-off", help="Disable Ollama GM",
    ),
    model: str = typer.Option(
        "llama3.2", "--model", "-m", help="Ollama model name",
    ),
    voice: bool = typer.Option(
        False, "--voice", help="Enable voice narration",
    ),
    voice_pace: str = typer.Option(
        "normal", "--voice-pace",
        help="Voice pacing: fast, normal, slow",
    ),
    callouts: str = typer.Option(
        "verbose", "--callouts",
        help="Warning detail: verbose or minimal",
    ),
) -> None:
    """Launch the full-screen Textual UI."""
    from .gm import GMConfig
    from .step_engine import StepEngine
    from .tui_app import LedgerTrailApp

    if resume:
        state = load_game()
        if state is None:
            console.print(
                "[red]No saved game. "
                "Use 'trail tui' to start new.[/red]"
            )
            raise typer.Exit(1)
        if state.game_over:
            console.print(
                "[yellow]Run is over. "
                "Use 'trail tui' for a new one.[/yellow]"
            )
            raise typer.Exit(0)
    else:
        state = create_new_run(seed=seed)

    # Apply callout level
    callouts = callouts.lower()
    if callouts in ("verbose", "minimal"):
        state.callout_level = callouts

    gm_config = GMConfig(
        host=os.environ.get(
            "OLLAMA_HOST", "http://localhost:11434",
        ),
        model=model,
        enabled=not gm_off,
    )

    # Voice config (optional)
    voice_config = None
    if voice:
        from .voice import VoiceConfig, VoicePace

        try:
            pace = VoicePace(voice_pace.lower())
        except ValueError:
            console.print(
                f"[red]Unknown voice pace: {voice_pace}. "
                "Use fast, normal, or slow.[/red]"
            )
            raise typer.Exit(1) from None

        voice_config = VoiceConfig(
            enabled=True,
            pace=pace,
            profile=state.gm_profile.value,
        )

    engine = StepEngine(state, gm_config)
    LedgerTrailApp(engine=engine, voice_config=voice_config).run()


@app.command()
def version() -> None:
    """Show version."""
    console.print(f"Escape the Valley: Ledger Trail v{__version__}")


# ── Ledger subcommands ──────────────────────────────────────────

ledger_app = typer.Typer(name="ledger", help="Ledger Backpack commands")
app.add_typer(ledger_app)


@ledger_app.command(name="status")
def ledger_status() -> None:
    """Show backpack status."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    from .backpack import BackpackManager

    mgr = BackpackManager()
    console.print(mgr.status_line(state))

    if state.backpack.enabled:
        info = mgr.wallet_info(state)
        console.print(f"  Address: {info.get('address_short', '?')}")
        console.print(f"  Settlements: {info.get('settlements', 0)}")
        console.print(f"  Pending: {info.get('pending', 0)}")


@ledger_app.command(name="enable")
def ledger_enable() -> None:
    """Enable the Ledger Backpack."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    if state.backpack.enabled:
        console.print("[yellow]Backpack already enabled.[/yellow]")
        raise typer.Exit(0)

    from .backpack import BackpackManager
    from .save import save_game

    mgr = BackpackManager()
    console.print("Enabling Ledger Backpack on XRPL Testnet...")
    result = mgr.enable(state)
    mgr.close()

    if result.success:
        save_game(state)
        console.print(f"[green]{result.message}[/green]")
        console.print(f"  Wallet: {result.wallet_address}")
    else:
        console.print(f"[red]{result.message}[/red]")
        raise typer.Exit(1)


@ledger_app.command(name="disable")
def ledger_disable() -> None:
    """Disable the Ledger Backpack."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    if not state.backpack.enabled:
        console.print("[yellow]Backpack already disabled.[/yellow]")
        raise typer.Exit(0)

    from .backpack import BackpackManager
    from .save import save_game

    mgr = BackpackManager()
    mgr.disable(state)
    save_game(state)
    console.print("Ledger Backpack disabled. Wallet kept for re-enable.")


@ledger_app.command(name="settle")
def ledger_settle() -> None:
    """Manually settle a checkpoint."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    if not state.backpack.enabled:
        console.print("[yellow]Backpack not enabled.[/yellow]")
        raise typer.Exit(1)

    from .backpack import BackpackManager
    from .save import save_game

    # Find current location name
    location = "Unknown"
    for n in state.map_nodes:
        if n.node_id == state.location_id:
            location = n.name
            break

    mgr = BackpackManager()
    result = mgr.settle(state, location)
    mgr.close()

    if result.success:
        save_game(state)
        console.print(f"[green]{result.message}[/green]")
    else:
        console.print(f"[red]{result.message}[/red]")


@ledger_app.command(name="reconcile")
def ledger_reconcile() -> None:
    """Retry all pending settlements from network failures."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    if not state.backpack.enabled:
        console.print("[yellow]Backpack not enabled.[/yellow]")
        raise typer.Exit(1)

    pending_count = len(state.backpack.pending_settlements)
    if pending_count == 0:
        console.print("[green]Nothing to reconcile. All checkpoints settled.[/green]")
        raise typer.Exit(0)

    from .backpack import BackpackManager
    from .save import save_game

    console.print(f"Retrying {pending_count} pending settlement(s)...")
    mgr = BackpackManager()
    mgr._retry_pending(state)
    mgr.close()

    remaining = len(state.backpack.pending_settlements)
    settled = pending_count - remaining
    save_game(state)

    if remaining == 0:
        console.print(f"[green]Reconciled {settled} checkpoint(s). All clear.[/green]")
    else:
        console.print(
            f"[yellow]Settled {settled}, {remaining} still pending. "
            f"Network may be down — try again later.[/yellow]"
        )


@ledger_app.command(name="wallet")
def ledger_wallet() -> None:
    """Show wallet info."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    from .backpack import BackpackManager

    mgr = BackpackManager()
    info = mgr.wallet_info(state)

    if info.get("status") == "No wallet":
        console.print("[dim]No wallet. Enable backpack first.[/dim]")
        raise typer.Exit(0)

    console.print(f"  Address: {info.get('address', '?')}")
    console.print(f"  Issuer: {info.get('issuer', '?')}")
    console.print(f"  Trust lines: {'Yes' if info.get('trust_lines') else 'No'}")
    console.print(f"  Settlements: {info.get('settlements', 0)}")
    console.print(f"  Pending: {info.get('pending', 0)}")

    balances = info.get("balances", {})
    if balances:
        console.print("  Balances:")
        for code, amount in balances.items():
            console.print(f"    {code}: {amount}")


# ── Parcel subcommands ──────────────────────────────────────────

parcel_app = typer.Typer(name="parcel", help="Parcel commands")
app.add_typer(parcel_app)


@parcel_app.command(name="send")
def parcel_send(
    address: str = typer.Argument(help="Recipient XRPL wallet address"),
    supply: str = typer.Argument(help="Supply type: food, water, meds, ammo, parts"),
    amount: int = typer.Argument(help="Amount to send"),
) -> None:
    """Send supplies to another traveler via XRPL."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    if not state.backpack.enabled:
        console.print("[red]Ledger Backpack not enabled. Run: trail ledger enable[/red]")
        raise typer.Exit(1)

    from .backpack import BackpackManager
    from .save import save_game

    mgr = BackpackManager()
    console.print(f"Sending {amount} {supply} to {address[:8]}...")
    result = mgr.send_parcel(state, address, supply.lower(), amount)
    mgr.close()

    if result.success:
        save_game(state)
        console.print(f"[green]{result.message}[/green]")
    else:
        console.print(f"[red]{result.message}[/red]")
        raise typer.Exit(1)


@parcel_app.command(name="list")
def parcel_list() -> None:
    """List received parcels."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    if not state.backpack.parcels:
        console.print("[dim]No parcels received.[/dim]")
        return

    for p in state.backpack.parcels:
        refused = p.parcel_id.startswith("refused:")
        if p.accepted:
            status = "accepted"
        elif refused:
            status = "refused"
        else:
            status = "pending"
        contents = ", ".join(f"{v} {k}" for k, v in p.contents.items())
        sender_short = p.sender[:8] + "..." if len(p.sender) > 12 else p.sender
        console.print(
            f"  [{status}] From {sender_short}: {contents} "
            f"(day {p.day_received})"
        )


@parcel_app.command(name="accept")
def parcel_accept(
    parcel_id: str = typer.Argument(help="Parcel ID to accept"),
) -> None:
    """Accept a pending parcel."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    parcel = None
    for p in state.backpack.parcels:
        if p.parcel_id == parcel_id and not p.accepted:
            parcel = p
            break

    if parcel is None:
        console.print("[red]Parcel not found or already accepted.[/red]")
        raise typer.Exit(1)

    from .backpack import BackpackManager
    from .save import save_game

    mgr = BackpackManager()
    mgr.accept_parcel(parcel, state)
    save_game(state)

    contents = ", ".join(f"+{v} {k}" for k, v in parcel.contents.items())
    console.print(f"[green]Parcel accepted: {contents}[/green]")


@parcel_app.command(name="sent")
def parcel_sent() -> None:
    """List parcels you've sent to other travelers."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    sent = state.backpack.sent_parcels
    if not sent:
        console.print("[dim]No parcels sent yet.[/dim]")
        return

    for sp in sent:
        addr_short = sp.recipient[:8] + "..." if len(sp.recipient) > 12 else sp.recipient
        txid_short = sp.txid[:12] + "..." if sp.txid else "pending"
        console.print(
            f"  To {addr_short}: {sp.amount} {sp.supply} "
            f"(day {sp.day_sent}) [{txid_short}]"
        )


# ── Wallet subcommands ────────────────────────────────────────

wallet_app = typer.Typer(name="wallet", help="Wallet commands")
app.add_typer(wallet_app)


@wallet_app.command(name="share")
def wallet_share() -> None:
    """Print your wallet address for trading with other travelers."""
    state = load_game()
    if state is None:
        console.print("[red]No saved game found.[/red]")
        raise typer.Exit(1)

    addr = state.backpack.wallet_address
    if not addr:
        console.print("[red]No wallet. Enable backpack first: trail ledger enable[/red]")
        raise typer.Exit(1)

    console.print()
    console.print("[bold]Your Trail Address[/bold]")
    console.print(f"  {addr}")
    console.print()
    console.print("[dim]Share this with another traveler so they can send you supplies.[/dim]")
    console.print(f"[dim]They run: trail parcel send {addr} food 10[/dim]")


if __name__ == "__main__":
    app()
