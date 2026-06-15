"""Cross-family GM panel simulator — drive full GM-ON runs across model families.

Runs full GM-ON game runs against DIFFERENT Ollama models (local AND cloud) and
measures how the v1.1 GM integration holds up across families: streaming,
tone-lint, fallback-never-bricks, and graded endings. The engine decides
mechanics (a deterministic, maintenance-aware survival policy picks every
choice); the model only narrates. So for a fixed seed the *survival* outcome
(days, survivors, ending tier, cause) is IDENTICAL across models — only the
prose varies. The harness asserts that.

Design constraints (cloud latency is the hard one):
  - A single generate_scene on a cold cloud model was measured at ~230s, and a
    full run makes 60-120 GM calls, so 100 full cloud runs is INFEASIBLE. This
    script is therefore fully configurable in scale, background-friendly, writes
    its report INCREMENTALLY (each run appended to <out>/runs.jsonl as it
    finishes, so a long or interrupted sweep is never lost), enforces a per-run
    wall-clock timeout, and ships SMALL defaults.

A GM/model failure NEVER crashes the harness: the engine's own fallback handles
a GM that returns None, and any unexpected exception is caught + recorded with
bricked=True (which, on a healthy build, must always be False).

Standards compliance (workflow-standards.md):
  EXTERNAL_VERIFIER=2   the deterministic engine grades the ending; the narrating
                        model never decides mechanics, and the determinism
                        cross-check runs the SAME seed on a DIFFERENT model family
                        and asserts the mechanical outcome is byte-identical, so a
                        model cannot silently change survival. (Not 3: the GM's
                        prose itself is not adversarially graded by a second model
                        here — tone-lint is the in-engine check.)
  PIN_PER_STEP=2        each run pins (model, seed, profile, weirdness, gm_timeout)
                        and the engine is seed + GM-off-deterministic on mechanics;
                        every run's full config is recorded in the JSONL row so a
                        row is replayable. (Not 3: model sampling temperature is
                        fixed in gm.py at 0.7 but narration is not seeded, by design
                        — only mechanics must replay.)
  ANDON_AUTHORITY=2     a determinism cross-check mismatch is reported as a hard
                        FAIL in the report; a per-run timeout aborts that run's loop
                        without poisoning the sweep.
  NAMED_COMPENSATORS    no irreversible action — this harness only reads models and
                        writes local report files. Undo = delete <out>. No network
                        writes, no publishes, no ledger. skip: nothing to compensate.
  DECOMPOSE_BY_SECRETS=2 the per-run driver (_run_one), the timing wrapper
                        (_TimedGM), the determinism check, and the aggregator are
                        separate units; the volatile part (which models, what scale)
                        is all argparse, isolated from the stable run/aggregate core.
  UNCERTAINTY_GATED_HUMANS=1 small defaults + explicit --runs/--models gate the
                        cost on an operator decision rather than auto-scaling; no
                        contrastive in-run human checkpoint (it is an unattended
                        batch tool by design).
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
import time
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

# Make src/ and the repo root importable regardless of CWD (mirrors the other
# scripts in this directory: agents.skilled lives at the repo root, the package
# lives under src/).
_REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(_REPO_ROOT))
sys.path.insert(0, str(_REPO_ROOT / "src"))

from agents.skilled import choose_intent, reset_step_counter  # noqa: E402
from escape_the_valley.gm import GMClient, GMConfig  # noqa: E402
from escape_the_valley.intent import GamePhase  # noqa: E402
from escape_the_valley.models import GMProfile  # noqa: E402
from escape_the_valley.step_engine import StepEngine  # noqa: E402
from escape_the_valley.worldgen import create_new_run  # noqa: E402

# Mechanical-outcome dimensions that MUST match across models for a fixed seed.
# Narration is free to vary; these are the engine's deterministic verdict.
_OUTCOME_KEYS = ("days", "survivors", "ending_tier", "cause")

# Small, sane default panel: one fast local model + one cloud model. Round-robin
# across them. Operators override with --models for a wider or cloud-only sweep.
_DEFAULT_MODELS = "hermes3:8b,glm-4.6:cloud"


# ── Per-run result ───────────────────────────────────────────────────


@dataclass
class RunResult:
    """One full GM-ON run's measured outcome + GM reliability delta."""

    model: str
    seed: int
    profile: str
    weirdness: int
    # Mechanical outcome (must be identical across models for a fixed seed).
    days: int
    survivors: int
    party_size: int
    distance: int
    ending_tier: str
    cause: str
    # GM reliability — per-run delta of GMClient.stats.
    gm_calls: int
    gm_stats_delta: dict[str, int]
    mean_latency_s: float
    max_latency_s: float
    bricked: bool
    timed_out: bool
    error: str = ""
    sample_scene: str = ""
    epilogue: str = ""

    def outcome_tuple(self) -> tuple:
        return (self.days, self.survivors, self.ending_tier, self.cause)

    def to_row(self) -> dict:
        return {
            "model": self.model,
            "seed": self.seed,
            "profile": self.profile,
            "weirdness": self.weirdness,
            "days": self.days,
            "survivors": self.survivors,
            "party_size": self.party_size,
            "distance": self.distance,
            "ending_tier": self.ending_tier,
            "cause": self.cause,
            "gm_calls": self.gm_calls,
            "gm_stats_delta": self.gm_stats_delta,
            "mean_latency_s": round(self.mean_latency_s, 3),
            "max_latency_s": round(self.max_latency_s, 3),
            "bricked": self.bricked,
            "timed_out": self.timed_out,
            "error": self.error,
            "sample_scene": self.sample_scene,
            "epilogue": self.epilogue,
        }


# ── GM timing wrapper ────────────────────────────────────────────────


class _TimedGM:
    """Wrap a GMClient instance to time generate_scene/outcome/epilogue calls.

    The engine calls ``engine.gm.generate_scene`` / ``generate_outcome``
    internally; we wrap those bound methods on the instance to record per-call
    wall-clock latency and capture the first non-empty scene narration as a
    sample. The wrapper is transparent — it returns exactly what the underlying
    method returns (including None on a GM failure), so fallback-never-bricks is
    untouched.
    """

    def __init__(self, gm: GMClient):
        self.gm = gm
        self.latencies: list[float] = []
        self.sample_scene = ""
        self._orig_scene = gm.generate_scene
        self._orig_outcome = gm.generate_outcome
        self._orig_epilogue = gm.generate_epilogue
        gm.generate_scene = self._timed_scene  # type: ignore[method-assign]
        gm.generate_outcome = self._timed_outcome  # type: ignore[method-assign]
        gm.generate_epilogue = self._timed_epilogue  # type: ignore[method-assign]

    def _time(self, fn, *args, **kwargs):
        t0 = time.perf_counter()
        try:
            return fn(*args, **kwargs)
        finally:
            self.latencies.append(time.perf_counter() - t0)

    def _timed_scene(self, *args, **kwargs):
        result = self._time(self._orig_scene, *args, **kwargs)
        if result is not None and not self.sample_scene:
            narration = (getattr(result, "narration", "") or "").strip()
            if narration:
                self.sample_scene = narration
        return result

    def _timed_outcome(self, *args, **kwargs):
        return self._time(self._orig_outcome, *args, **kwargs)

    def _timed_epilogue(self, *args, **kwargs):
        return self._time(self._orig_epilogue, *args, **kwargs)


# ── Single run ───────────────────────────────────────────────────────


def _run_one(
    *,
    model: str,
    seed: int,
    profile: GMProfile,
    weirdness: int,
    max_steps: int,
    host: str,
    gm_timeout: float,
    per_run_timeout_s: float,
    stream: bool,
) -> RunResult:
    """Drive one full GM-ON run. Never raises — a brick is recorded, not thrown.

    The deterministic survival policy (agents.skilled) picks every engine choice;
    the model narrates each scene + outcome (engine-internal) and the epilogue
    (driven here). Returns a fully-populated RunResult even on GM failure or
    per-run timeout.
    """
    reset_step_counter()
    gm_config = GMConfig(
        host=host, model=model, timeout=gm_timeout, enabled=True,
    )
    # Mechanics are seed-deterministic and GM-independent; create the run with
    # the chosen profile/weirdness so the GM is asked to wear the right voice.
    state = create_new_run(
        seed=seed, gm_profile=profile, weirdness_level=weirdness,
    )
    party_size = len(state.party.members)

    bricked = False
    timed_out = False
    error = ""
    epilogue = ""
    timed: _TimedGM | None = None

    deadline = time.perf_counter() + per_run_timeout_s

    try:
        engine = StepEngine(state, gm_config=gm_config)
        timed = _TimedGM(engine.gm)
        # Snapshot GM stats BEFORE the run so we can report the per-run delta.
        stats_before = dict(engine.gm.stats)

        for _ in range(max_steps):
            if engine.phase == GamePhase.GAME_OVER:
                break
            if time.perf_counter() > deadline:
                timed_out = True
                break
            intent = choose_intent(state, engine)
            engine.step(intent)

        # Grade the ending (idempotent — returns the clean game-over ending if
        # one fired, else grades the timeout/abandon honestly). Never None.
        ending = engine.finalize_run(reason="timeout" if timed_out else "complete")

        # Narrate the epilogue (fallback-safe: NEVER None — deterministic floor
        # on any GM failure). Skip on timeout to respect the wall-clock budget.
        if not timed_out:
            try:
                epilogue = engine.gm.generate_epilogue(state, ending) or ""
            except Exception as exc:  # noqa: BLE001 — record, never brick
                epilogue = ""
                error = f"epilogue: {type(exc).__name__}: {exc}"

        stats_after = dict(engine.gm.stats)
        gm_stats_delta = {
            k: stats_after.get(k, 0) - stats_before.get(k, 0)
            for k in stats_after
        }
        # gm_calls = scene + outcome attempts only (epilogue attempts are part of
        # the stats delta too, but the engine's own gm_calls diagnostic counts
        # scene+outcome requests — report that for parity with cli-tui 'stats').
        gm_calls = engine.diagnostics.get("gm_calls", 0)

        survivors = sum(1 for m in state.party.members if m.is_alive())
        cause = (
            state.cause_of_death
            or ("victory" if state.victory else "timeout")
        )
        latencies = timed.latencies if timed else []
        result = RunResult(
            model=model,
            seed=seed,
            profile=profile.value,
            weirdness=weirdness,
            days=state.day,
            survivors=survivors,
            party_size=party_size,
            distance=state.distance_traveled,
            ending_tier=ending.tier,
            cause=cause,
            gm_calls=gm_calls,
            gm_stats_delta=gm_stats_delta,
            mean_latency_s=statistics.fmean(latencies) if latencies else 0.0,
            max_latency_s=max(latencies) if latencies else 0.0,
            bricked=False,
            timed_out=timed_out,
            error=error,
            sample_scene=timed.sample_scene if timed else "",
            epilogue=epilogue,
        )
    except Exception as exc:  # noqa: BLE001 — an escaped engine exception IS a brick
        bricked = True
        error = f"{type(exc).__name__}: {exc}"
        latencies = timed.latencies if timed else []
        result = RunResult(
            model=model,
            seed=seed,
            profile=profile.value,
            weirdness=weirdness,
            days=state.day,
            survivors=sum(1 for m in state.party.members if m.is_alive()),
            party_size=party_size,
            distance=state.distance_traveled,
            ending_tier="lost",
            cause=state.cause_of_death or "error",
            gm_calls=0,
            gm_stats_delta={},
            mean_latency_s=statistics.fmean(latencies) if latencies else 0.0,
            max_latency_s=max(latencies) if latencies else 0.0,
            bricked=bricked,
            timed_out=timed_out,
            error=error,
            sample_scene=timed.sample_scene if timed else "",
            epilogue="",
        )

    return result


# ── Determinism cross-check ──────────────────────────────────────────


@dataclass
class CrossCheck:
    seed: int
    model_a: str
    model_b: str
    outcome_a: tuple
    outcome_b: tuple
    identical: bool
    note: str = ""

    def to_dict(self) -> dict:
        return {
            "seed": self.seed,
            "model_a": self.model_a,
            "model_b": self.model_b,
            "outcome_a": dict(zip(_OUTCOME_KEYS, self.outcome_a, strict=True)),
            "outcome_b": dict(zip(_OUTCOME_KEYS, self.outcome_b, strict=True)),
            "identical": self.identical,
            "note": self.note,
        }


def _determinism_crosscheck(
    *,
    seed: int,
    models: list[str],
    profile: GMProfile,
    weirdness: int,
    max_steps: int,
    host: str,
    gm_timeout: float,
    per_run_timeout_s: float,
    stream: bool,
) -> CrossCheck:
    """Run the SAME seed on two models; assert the mechanical outcome matches.

    The narration will differ; the survival outcome (days, survivors, ending
    tier, cause) must be byte-identical because the engine — not the model —
    decides mechanics. If only one model is configured, runs the same model
    twice (still a valid determinism check: two independent GM-ON runs on one
    seed must agree mechanically).
    """
    model_a = models[0]
    model_b = models[1] if len(models) > 1 else models[0]

    def _outcome(model: str) -> tuple:
        r = _run_one(
            model=model, seed=seed, profile=profile, weirdness=weirdness,
            max_steps=max_steps, host=host, gm_timeout=gm_timeout,
            per_run_timeout_s=per_run_timeout_s, stream=stream,
        )
        return r.outcome_tuple(), r

    out_a, run_a = _outcome(model_a)
    out_b, run_b = _outcome(model_b)
    note = ""
    if run_a.timed_out or run_b.timed_out:
        note = "one or both runs hit the per-run timeout; comparison may be partial"
    if model_a == model_b:
        note = (note + "; " if note else "") + "only one model configured — ran it twice"
    return CrossCheck(
        seed=seed, model_a=model_a, model_b=model_b,
        outcome_a=out_a, outcome_b=out_b,
        identical=out_a == out_b, note=note,
    )


# ── Aggregation ──────────────────────────────────────────────────────


@dataclass
class ModelAgg:
    model: str
    runs: int = 0
    gm_calls: int = 0
    attempts: int = 0
    successes: int = 0
    tone_rejects: int = 0
    json_rejects: int = 0
    fallbacks: int = 0  # runs where the engine fell back at least once (None scene/outcome)
    bricks: int = 0
    timeouts: int = 0
    latencies: list[float] = field(default_factory=list)
    max_latency: float = 0.0
    tiers: Counter = field(default_factory=Counter)
    samples: list[str] = field(default_factory=list)

    def add(self, r: RunResult) -> None:
        self.runs += 1
        self.gm_calls += r.gm_calls
        self.attempts += r.gm_stats_delta.get("attempts", 0)
        self.successes += r.gm_stats_delta.get("successes", 0)
        self.tone_rejects += r.gm_stats_delta.get("tone_rejects", 0)
        self.json_rejects += r.gm_stats_delta.get("json_rejects", 0)
        # An engine fallback = a scene/outcome attempt that did not become a
        # success (timeouts, connect errors, repeated json/tone rejects). We
        # approximate the run-level fallback signal from attempts vs successes.
        if r.gm_stats_delta.get("attempts", 0) > r.gm_stats_delta.get("successes", 0):
            self.fallbacks += 1
        if r.bricked:
            self.bricks += 1
        if r.timed_out:
            self.timeouts += 1
        if r.mean_latency_s:
            self.latencies.append(r.mean_latency_s)
        self.max_latency = max(self.max_latency, r.max_latency_s)
        self.tiers[r.ending_tier] += 1
        for text in (r.sample_scene, r.epilogue):
            if text and len(self.samples) < 3:
                self.samples.append(text)

    @property
    def success_rate(self) -> float:
        return self.successes / self.attempts if self.attempts else 0.0

    @property
    def tone_reject_rate(self) -> float:
        return self.tone_rejects / self.attempts if self.attempts else 0.0

    @property
    def fallback_rate(self) -> float:
        return self.fallbacks / self.runs if self.runs else 0.0

    @property
    def mean_latency(self) -> float:
        return statistics.fmean(self.latencies) if self.latencies else 0.0

    def to_dict(self) -> dict:
        return {
            "model": self.model,
            "runs": self.runs,
            "gm_calls": self.gm_calls,
            "attempts": self.attempts,
            "successes": self.successes,
            "success_rate": round(self.success_rate, 4),
            "tone_reject_rate": round(self.tone_reject_rate, 4),
            "json_rejects": self.json_rejects,
            "fallback_rate": round(self.fallback_rate, 4),
            "bricks": self.bricks,
            "timeouts": self.timeouts,
            "mean_latency_s": round(self.mean_latency, 3),
            "max_latency_s": round(self.max_latency, 3),
            "ending_tiers": dict(self.tiers),
            "samples": self.samples,
        }


def _aggregate(results: list[RunResult]) -> dict[str, ModelAgg]:
    aggs: dict[str, ModelAgg] = {}
    for r in results:
        aggs.setdefault(r.model, ModelAgg(model=r.model)).add(r)
    return aggs


def _write_report(
    out_dir: Path,
    aggs: dict[str, ModelAgg],
    crosscheck: CrossCheck | None,
    config: dict,
) -> None:
    report = {
        "config": config,
        "total_bricks": sum(a.bricks for a in aggs.values()),
        "determinism_crosscheck": crosscheck.to_dict() if crosscheck else None,
        "models": [a.to_dict() for a in aggs.values()],
    }
    (out_dir / "report.json").write_text(
        json.dumps(report, indent=2), encoding="utf-8",
    )
    (out_dir / "report.md").write_text(
        _render_markdown(aggs, crosscheck, config, report["total_bricks"]),
        encoding="utf-8",
    )


def _render_markdown(
    aggs: dict[str, ModelAgg],
    crosscheck: CrossCheck | None,
    config: dict,
    total_bricks: int,
) -> str:
    lines = [
        "# Cross-Family GM Panel Simulator — Report",
        "",
        f"- **Models:** {', '.join(config['models'])}",
        f"- **Runs:** {config['runs']}  (max-steps {config['max_steps']}, "
        f"seed-base {config['seed_base']})",
        f"- **Profile / weirdness:** {config['profile']} / {config['weirdness']}",
        f"- **GM timeout:** {config['gm_timeout']}s  "
        f"**per-run timeout:** {config['per_run_timeout_sec']}s  "
        f"**stream:** {config['stream']}",
        f"- **Total bricks:** {total_bricks}  "
        f"({'PASS — fallback never bricks' if total_bricks == 0 else 'FAIL'})",
        "",
    ]
    if crosscheck is not None:
        verdict = "PASS" if crosscheck.identical else "FAIL"
        lines += [
            "## Determinism cross-check",
            "",
            f"Same seed `{crosscheck.seed}` on `{crosscheck.model_a}` vs "
            f"`{crosscheck.model_b}` — mechanical outcome must be identical "
            "(only narration may vary).",
            "",
            f"- **Outcome A:** {dict(zip(_OUTCOME_KEYS, crosscheck.outcome_a, strict=True))}",
            f"- **Outcome B:** {dict(zip(_OUTCOME_KEYS, crosscheck.outcome_b, strict=True))}",
            f"- **Verdict:** {verdict}",
        ]
        if crosscheck.note:
            lines.append(f"- _Note: {crosscheck.note}_")
        lines.append("")
    lines += [
        "## Per-model summary",
        "",
        "| Model | Runs | GM calls | Success | Tone-reject | Fallback | "
        "Mean lat (s) | Max lat (s) | Bricks | Timeouts |",
        "|-------|-----:|---------:|--------:|------------:|---------:|"
        "-------------:|------------:|-------:|---------:|",
    ]
    for a in aggs.values():
        lines.append(
            f"| {a.model} | {a.runs} | {a.gm_calls} | "
            f"{a.success_rate * 100:.1f}% | {a.tone_reject_rate * 100:.1f}% | "
            f"{a.fallback_rate * 100:.1f}% | {a.mean_latency:.2f} | "
            f"{a.max_latency:.2f} | {a.bricks} | {a.timeouts} |"
        )
    lines.append("")
    for a in aggs.values():
        lines += [
            f"### {a.model}",
            "",
            f"- Ending-tier distribution: {dict(a.tiers)}",
            "",
            "Sample narrations:",
        ]
        if a.samples:
            for s in a.samples[:3]:
                snippet = s if len(s) <= 400 else s[:397] + "..."
                lines.append(f"> {snippet}")
                lines.append("")
        else:
            lines.append("> (none captured — GM produced no usable narration)")
            lines.append("")
    return "\n".join(lines)


# ── CLI ──────────────────────────────────────────────────────────────


def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description=(
            "Cross-family GM panel simulator — full GM-ON runs across Ollama "
            "model families (local + cloud), measuring streaming/tone/"
            "fallback/endings. Engine decides mechanics; the model only narrates."
        ),
    )
    p.add_argument(
        "--models", type=str, default=_DEFAULT_MODELS,
        help=f"comma-separated model list, round-robin (default: {_DEFAULT_MODELS})",
    )
    p.add_argument(
        "--runs", type=int, default=4,
        help="total runs, round-robin across models (default: 4)",
    )
    p.add_argument(
        "--max-steps", type=int, default=600,
        help="max engine steps per run (default: 600)",
    )
    p.add_argument(
        "--seed-base", type=int, default=7000,
        help="first seed; run i uses seed-base + i (default: 7000)",
    )
    p.add_argument(
        "--profile", type=str, default="fireside",
        choices=[gp.value for gp in GMProfile],
        help="GM profile / voice (default: fireside)",
    )
    p.add_argument(
        "--weirdness", type=int, default=2, choices=[0, 1, 2, 3],
        help="weirdness level 0-3 (default: 2; uncanny gates at >=2)",
    )
    stream_grp = p.add_mutually_exclusive_group()
    stream_grp.add_argument(
        "--stream", dest="stream", action="store_true",
        help="(reserved) stream narration tokens",
    )
    stream_grp.add_argument(
        "--no-stream", dest="stream", action="store_false",
        help="do not stream (default)",
    )
    p.set_defaults(stream=False)
    p.add_argument(
        "--out", type=str, default=".trail/gm_panel",
        help="report output directory (default: .trail/gm_panel)",
    )
    p.add_argument(
        "--host", type=str, default="http://localhost:11434",
        help="Ollama host (default: http://localhost:11434)",
    )
    p.add_argument(
        "--gm-timeout", type=float, default=240.0,
        help="per-GM-call HTTP timeout in seconds (default: 240 — cloud-tolerant)",
    )
    p.add_argument(
        "--per-run-timeout-sec", type=float, default=1800.0,
        help="per-run wall-clock budget in seconds (default: 1800)",
    )
    p.add_argument(
        "--no-crosscheck", action="store_true",
        help="skip the determinism cross-check (saves 2 runs)",
    )
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)
    models = [m.strip() for m in args.models.split(",") if m.strip()]
    if not models:
        print("[ERROR] no models supplied via --models", file=sys.stderr)
        return 2
    profile = GMProfile(args.profile)

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    runs_jsonl = out_dir / "runs.jsonl"
    # Fresh sweep — start the incremental log clean. (Append-as-we-go below.)
    runs_jsonl.write_text("", encoding="utf-8")

    config = {
        "models": models,
        "runs": args.runs,
        "max_steps": args.max_steps,
        "seed_base": args.seed_base,
        "profile": profile.value,
        "weirdness": args.weirdness,
        "stream": args.stream,
        "gm_timeout": args.gm_timeout,
        "per_run_timeout_sec": args.per_run_timeout_sec,
        "host": args.host,
    }

    print(f"GM panel: {len(models)} model(s), {args.runs} run(s), "
          f"round-robin. Out: {out_dir}")
    print(f"Models: {', '.join(models)}")
    print(f"Per-run timeout {args.per_run_timeout_sec}s, "
          f"GM call timeout {args.gm_timeout}s\n")

    crosscheck: CrossCheck | None = None
    if not args.no_crosscheck:
        print(f"Determinism cross-check on seed {args.seed_base} "
              f"(same mechanics across models)...")
        t0 = time.perf_counter()
        crosscheck = _determinism_crosscheck(
            seed=args.seed_base, models=models, profile=profile,
            weirdness=args.weirdness, max_steps=args.max_steps,
            host=args.host, gm_timeout=args.gm_timeout,
            per_run_timeout_s=args.per_run_timeout_sec, stream=args.stream,
        )
        verdict = "PASS" if crosscheck.identical else "FAIL"
        print(f"  [{verdict}] A={crosscheck.outcome_a} B={crosscheck.outcome_b} "
              f"({time.perf_counter() - t0:.0f}s)")
        if crosscheck.note:
            print(f"  note: {crosscheck.note}")
        print()

    results: list[RunResult] = []
    for i in range(args.runs):
        model = models[i % len(models)]
        seed = args.seed_base + i
        print(f"[{i + 1}/{args.runs}] model={model} seed={seed} ...", flush=True)
        t0 = time.perf_counter()
        result = _run_one(
            model=model, seed=seed, profile=profile,
            weirdness=args.weirdness, max_steps=args.max_steps,
            host=args.host, gm_timeout=args.gm_timeout,
            per_run_timeout_s=args.per_run_timeout_sec, stream=args.stream,
        )
        results.append(result)
        # INCREMENTAL: append this run to the JSONL the instant it finishes, so
        # a long/interrupted sweep is never lost.
        with runs_jsonl.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(result.to_row()) + "\n")
        flags = []
        if result.bricked:
            flags.append("BRICKED")
        if result.timed_out:
            flags.append("TIMEOUT")
        flag_str = (" [" + ",".join(flags) + "]") if flags else ""
        print(
            f"    days={result.days} survivors={result.survivors}/"
            f"{result.party_size} tier={result.ending_tier} "
            f"cause={result.cause} gm_calls={result.gm_calls} "
            f"mean_lat={result.mean_latency_s:.2f}s "
            f"({time.perf_counter() - t0:.0f}s){flag_str}",
            flush=True,
        )

    aggs = _aggregate(results)
    _write_report(out_dir, aggs, crosscheck, config)

    total_bricks = sum(a.bricks for a in aggs.values())
    print(f"\nReport written to {out_dir / 'report.md'} / report.json")
    print(f"Per-run rows: {runs_jsonl}")
    print(f"Total bricks: {total_bricks} "
          f"({'PASS' if total_bricks == 0 else 'FAIL'})")
    if crosscheck is not None:
        print(f"Determinism cross-check: "
              f"{'PASS' if crosscheck.identical else 'FAIL'}")

    # Non-zero exit on a brick or a determinism mismatch — ANDON.
    if total_bricks > 0:
        return 1
    if crosscheck is not None and not crosscheck.identical:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
