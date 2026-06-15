"""CLI entry point — Typer commands for Escape the Valley."""

from __future__ import annotations

import os
import re

import typer
from rich.console import Console

from . import __version__
from .engine import GameEngine
from .gm import GMConfig
from .models import GMProfile
from .save import has_save, load_game
from .ui import show_journal, show_status, show_title_screen
from .worldgen import create_new_run


def _version_string() -> str:
    """Single source of truth for the machine-parseable version line.

    Reused by --version, the `version` command, and `self-check` so automation
    sees one consistent string instead of three divergent ones (cli-tui-006).
    """
    return f"trail {__version__}"


def _version_callback(value: bool) -> None:
    if value:
        typer.echo(_version_string())
        raise typer.Exit()


# Classic-address shape: leading 'r', base58 alphabet, 25-35 chars total.
# Used as a fallback when xrpl-py is not installed (cli-tui-002).
_CLASSIC_ADDR_RE = re.compile(
    r"^r[1-9A-HJ-NP-Za-km-z]{24,34}$"
)


def _is_valid_recipient(address: str) -> bool:
    """Validate an XRPL classic address without a network call.

    Prefers xrpl-py's checksum-aware validator (guarded behind the same
    availability check the manager uses); falls back to a base58 shape regex
    so validation still happens when xrpl-py is absent (cli-tui-002).
    """
    if not address:
        return False

    from .backpack import _HAS_XRPL

    if _HAS_XRPL:
        from xrpl.core.addresscodec import is_valid_classic_address

        return is_valid_classic_address(address)

    return bool(_CLASSIC_ADDR_RE.match(address))


app = typer.Typer(
    name="trail",
    help="Escape the Valley: Ledger Trail — Oregon Trail-style survival game",
    no_args_is_help=True,
)
console = Console()


def _network_hint(action: str) -> None:
    """Structured next-step hint after a ledger/parcel round-trip fails.

    cli-tui-B-04: a bare red error leaves the player stuck. These XRPL calls
    can stall on a slow/unreachable testnet; on failure we name the likely
    cause and the exact command to recover, so a transient outage is
    actionable rather than a dead end.
    """
    console.print(
        "[dim]hint: the XRPL testnet may be slow or unreachable. "
        f"{action}[/dim]"
    )


def _run_with_spinner(message: str, fn):
    """Run a blocking network round-trip under a Rich status spinner.

    cli-tui-B-04: every ledger/parcel call can block for tens of seconds on
    the testnet. The spinner gives the player visible feedback that the CLI
    is working, not hung. Any exception is re-raised to the caller after the
    spinner closes so the command can render its own structured error.
    """
    with console.status(message, spinner="dots"):
        return fn()


@app.callback(invoke_without_command=True)
def main_callback(
    _version: bool = typer.Option(
        False, "--version", "-V", callback=_version_callback, is_eager=True,
        help="Show version and exit.",
    ),
) -> None:
    """Escape the Valley: Ledger Trail — Oregon Trail-style survival game."""


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


def _model_present(configured: str, available: list[str]) -> bool:
    """True if the configured game model is among Ollama's models.

    Ollama reports tagged names ('llama3.2:latest'); a bare configured name
    ('llama3.2') should match the untagged form too (cli-tui-B-05).
    """
    if configured in available:
        return True
    base = configured.split(":", 1)[0]
    return any(
        name == base or name.split(":", 1)[0] == base
        for name in available
    )


@app.command(name="self-check")
def self_check(
    model: str = typer.Option(
        "llama3.2", "--model", "-m",
        help="Game model to verify is installed in Ollama",
    ),
) -> None:
    """Check game environment health."""
    console.print("[bold]Escape the Valley — Self Check[/bold]\n")
    console.print(f"  {_version_string()}")

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
    try:
        import httpx
        resp = httpx.get(f"{host}/api/tags", timeout=5)
        if resp.status_code == 200:
            models = resp.json().get("models", [])
            all_names = [m.get("name", "?") for m in models]
            console.print(f"  [green]Ollama reachable[/green] at {host}")
            console.print(f"  Models: {', '.join(all_names[:5])}")
            # cli-tui-B-05: reachable but the configured game model is missing
            # is a silent failure waiting to happen — name the exact pull.
            if not _model_present(model, all_names):
                console.print(
                    f"  [yellow]model '{model}' not found[/yellow] "
                    f"-- run: ollama pull {model}"
                )
        else:
            console.print(f"  [yellow]Ollama returned {resp.status_code}[/yellow]")
            console.print(
                "  [dim]hint: the GM is optional -- play with --gm-off[/dim]"
            )
    except Exception:
        # cli-tui-B-05: unreachable Ollama gets an actionable next step, not
        # just a red line. The GM is optional, so name the no-GM escape too.
        console.print(f"  [red]Ollama not reachable[/red] at {host}")
        console.print(
            "  [dim]hint: start Ollama (ollama serve) "
            "or play without the GM: trail tui --gm-off[/dim]"
        )

    console.print()


@app.command()
def tui(
    seed: int | None = typer.Option(
        None, "--seed", "-s", help="RNG seed for new run",
    ),
    profile: str = typer.Option(
        "fireside",
        "--gm-profile",
        "-p",
        help="GM personality: chronicler, fireside, lantern",
    ),
    weirdness: int = typer.Option(
        2, "--weirdness", "-w", help="Weirdness level 0-3",
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
        # Parse + validate GM profile (mirrors `new`) so the documented
        # `trail tui --gm-profile <name>` works and a TUI player can pick a
        # grounded run (weirdness 0-1) per the D2 gate.
        try:
            gm_profile = GMProfile(profile.lower())
        except ValueError:
            console.print(
                f"[red]Unknown profile: {profile}. "
                "Use chronicler, fireside, or lantern.[/red]"
            )
            raise typer.Exit(1) from None

        weirdness = max(0, min(3, weirdness))
        state = create_new_run(
            seed=seed,
            gm_profile=gm_profile,
            weirdness_level=weirdness,
        )

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
    LedgerTrailApp(
        engine=engine, voice_config=voice_config, resumed=resume,
    ).run()


@app.command()
def version() -> None:
    """Show version."""
    # Human-friendly title, but the parseable token comes from the shared
    # helper so all three version surfaces agree (cli-tui-006).
    console.print(f"Escape the Valley: Ledger Trail ({_version_string()})")


@app.command()
def stats(
    json_output: bool = typer.Option(False, "--json", help="Output as JSON"),
) -> None:
    """Show run statistics from the current save."""
    state = load_game()
    if state is None:
        console.print("[dim]No saved game found.[/dim]")
        raise typer.Exit(0)

    alive = state.party.alive_count
    total = len(state.party.members)
    journal_count = len(state.journal)
    events_seen = len(state.recent_event_tags)
    memories = len(state.memory_cards)
    twists = len(state.twists)

    data = {
        "run_id": state.run_id,
        "seed": state.seed,
        "day": state.day,
        "time_of_day": (
            state.time_of_day.value
            if hasattr(state.time_of_day, "value")
            else str(state.time_of_day)
        ),
        "party_alive": alive,
        "party_total": total,
        "distance_traveled": state.distance_traveled,
        "distance_remaining": state.distance_remaining,
        "total_distance": state.total_distance,
        "game_over": state.game_over,
        "victory": state.victory,
        "gm_profile": (
            state.gm_profile.value
            if hasattr(state.gm_profile, "value")
            else str(state.gm_profile)
        ),
        "journal_entries": journal_count,
        "events_seen": events_seen,
        "memory_cards": memories,
        "twists": twists,
        "wagon_condition": state.wagon.condition,
    }

    if json_output:
        import json
        console.print(json.dumps(data, indent=2))
    else:
        console.print("[bold]Run Statistics[/bold]\n")
        console.print(f"  Run:       {state.run_id} (seed {state.seed})")
        console.print(f"  Day:       {data['day']} ({data['time_of_day']})")
        console.print(f"  Party:     {alive}/{total} alive")
        pct = (state.distance_traveled / state.total_distance * 100) if state.total_distance else 0
        console.print(f"  Distance:  {state.distance_traveled}/{state.total_distance} ({pct:.0f}%)")
        console.print(f"  Wagon:     {state.wagon.condition}% condition")
        console.print(f"  GM:        {data['gm_profile']}")
        console.print(f"  Journal:   {journal_count} entries")
        console.print(f"  Memories:  {memories} cards")
        console.print(f"  Twists:    {twists}")
        if state.game_over:
            outcome = "Victory!" if state.victory else f"Defeated: {state.cause_of_death}"
            console.print(f"  Outcome:   {outcome}")
        console.print()


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
    result = _run_with_spinner(
        "Enabling Ledger Backpack on XRPL Testnet...",
        lambda: mgr.enable(state),
    )
    mgr.close()

    if result.success:
        save_game(state)
        console.print(f"[green]{result.message}[/green]")
        console.print(f"  Wallet: {result.wallet_address}")
    else:
        console.print(f"[red]{result.message}[/red]")
        _network_hint("Try again at the next town: trail ledger enable")
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
    result = _run_with_spinner(
        f"Settling checkpoint at {location}...",
        lambda: mgr.settle(state, location),
    )
    mgr.close()

    if result.success:
        save_game(state)
        console.print(f"[green]{result.message}[/green]")
    else:
        # cli-tui-003: surface settlement failure via a non-zero exit so
        # automation can detect it (was exit 0 with only a red line).
        console.print(f"[red]{result.message}[/red]")
        _network_hint(
            "The checkpoint is queued; retry later: trail ledger reconcile"
        )
        raise typer.Exit(1)


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

    mgr = BackpackManager()
    _run_with_spinner(
        f"Retrying {pending_count} pending settlement(s)...",
        lambda: mgr._retry_pending(state),
    )
    mgr.close()

    remaining = len(state.backpack.pending_settlements)
    settled = pending_count - remaining
    save_game(state)

    if remaining == 0:
        console.print(f"[green]Reconciled {settled} checkpoint(s). All clear.[/green]")
    else:
        # cli-tui-003: settlements still pending after the retry is a failure
        # automation must see — exit non-zero (was exit 0 with a yellow line).
        console.print(
            f"[yellow]Settled {settled}, {remaining} still pending. "
            f"Network may be down — try again later.[/yellow]"
        )
        _network_hint("Run again when the testnet recovers: trail ledger reconcile")
        raise typer.Exit(1)


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

    # Validate the recipient address shape BEFORE any network round-trip
    # (cli-tui-002). A garbage address otherwise wastes a testnet submit and
    # comes back as an unstructured success=False.
    if not _is_valid_recipient(address):
        console.print(
            "[red]ERR_BAD_ADDRESS: '"
            f"{address}' is not a valid XRPL classic address.[/red]"
        )
        console.print(
            "[dim]hint: a classic address starts with 'r' and is 25-35 "
            "base58 chars. Get a friend's via: trail wallet share[/dim]"
        )
        raise typer.Exit(1)

    from .backpack import BackpackManager
    from .save import save_game

    mgr = BackpackManager()
    result = _run_with_spinner(
        f"Sending {amount} {supply} to {address[:8]}...",
        lambda: mgr.send_parcel(state, address, supply.lower(), amount),
    )
    mgr.close()

    if result.success:
        save_game(state)
        console.print(f"[green]{result.message}[/green]")
    else:
        console.print(f"[red]{result.message}[/red]")
        _network_hint(
            "Confirm the address with 'trail wallet share' and retry: "
            f"trail parcel send {address[:8]}... {supply} {amount}"
        )
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
