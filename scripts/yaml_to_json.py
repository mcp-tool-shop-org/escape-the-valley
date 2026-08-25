"""One-time conversion: parse the simple YAML event files to JSON.

F-c79caa11: none of the five source `event skeletons*.txt` files this script
reads are tracked in this repository (confirmed via `git ls-files`) -- they
were the one-time inputs for the original conversion and were never
committed. `src/escape_the_valley/data/event_skeletons.json` (the script's
output) is now the canonical, curated source of truth and is edited
directly. Running this script today would find all five sources missing and
(without the guard below) would have silently overwritten that file with an
empty list. The guard + backup below make an accidental run safe; a future
cleanup pass may want to delete this script outright now that its inputs are
gone, but that is a separate, larger decision than closing the immediate
data-loss hole.
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def parse_yaml_events(text: str) -> list[dict]:
    """Parse the simple YAML event format used in event skeleton files."""
    events = []
    current_event = None
    current_choice = None
    current_profile = None
    current_list_key = None

    for raw_line in text.splitlines():
        line = raw_line.rstrip()
        if not line or line.startswith("events:"):
            continue

        # Count leading spaces
        stripped = line.lstrip()
        indent = len(line) - len(stripped)

        # Event-level (indent 2): "  - id: ..."
        if indent == 2 and stripped.startswith("- id:"):
            if current_event is not None:
                if current_choice and current_profile:
                    current_choice["engine_effect_profile"] = current_profile
                if current_choice:
                    current_event.setdefault("choices", []).append(current_choice)
                events.append(current_event)
            current_event = {}
            current_choice = None
            current_profile = None
            current_list_key = None
            val = stripped[len("- id:"):].strip().strip('"')
            current_event["id"] = val
            continue

        # Event field (indent 4): "    title: ..."
        if indent == 4 and current_event is not None:
            # Check for list start or key-value
            if stripped.startswith("- "):
                # List item under current_list_key
                if current_list_key:
                    val = stripped[2:].strip().strip('"')
                    current_event.setdefault(current_list_key, []).append(val)
                continue

            if ":" in stripped:
                key, _, val = stripped.partition(":")
                key = key.strip()
                val = val.strip().strip('"')

                if key == "tags":
                    current_list_key = "tags"
                    current_event["tags"] = []
                elif key == "preconditions":
                    current_list_key = "preconditions"
                    current_event["preconditions"] = []
                elif key == "choices":
                    current_list_key = None
                    current_event["choices"] = []
                elif key == "weirdness_band":
                    current_list_key = None
                    current_event["weirdness_band"] = int(val)
                elif key == "narration_seed":
                    current_list_key = None
                    current_event["narration_seed"] = val
                elif key == "title":
                    current_list_key = None
                    current_event["title"] = val
                else:
                    current_list_key = None
                    current_event[key] = val
            continue

        # Tags list item (indent 6 under tags): "      - river"
        if indent == 6 and stripped.startswith("- ") and current_list_key == "tags":
            val = stripped[2:].strip().strip('"')
            current_event.setdefault("tags", []).append(val)
            continue

        # Choice start (indent 6): "      - label: ..."
        if indent == 6 and stripped.startswith("- label:"):
            if current_choice:
                if current_profile:
                    current_choice["engine_effect_profile"] = current_profile
                current_event.setdefault("choices", []).append(current_choice)
            current_choice = {}
            current_profile = None
            val = stripped[len("- label:"):].strip().strip('"')
            current_choice["label"] = val
            continue

        # Choice field (indent 8): "        intent_action: FORD"
        if indent == 8 and current_choice is not None:
            if ":" in stripped:
                key, _, val = stripped.partition(":")
                key = key.strip()
                val = val.strip().strip('"')
                if key == "intent_action":
                    current_choice["intent_action"] = val
                elif key == "engine_effect_profile":
                    current_profile = {}
                else:
                    # Could be profile field at this level
                    if current_profile is not None:
                        try:
                            current_profile[key] = int(val)
                        except ValueError:
                            try:
                                current_profile[key] = float(val)
                            except ValueError:
                                current_profile[key] = val
            continue

        # Engine effect profile fields (indent 10): "          time_days: 1"
        if indent == 10 and current_profile is not None:
            if ":" in stripped:
                key, _, val = stripped.partition(":")
                key = key.strip()
                val = val.strip()
                try:
                    current_profile[key] = int(val)
                except ValueError:
                    try:
                        current_profile[key] = float(val)
                    except ValueError:
                        current_profile[key] = val
            continue

    # Flush last event
    if current_event is not None:
        if current_choice and current_profile:
            current_choice["engine_effect_profile"] = current_profile
        if current_choice:
            current_event.setdefault("choices", []).append(current_choice)
        events.append(current_event)

    return events


def main():
    files = [
        ("f1", ROOT / "event skeletons.txt"),
        ("f2", ROOT / "event skeletons_2.txt"),
        ("f3", ROOT / "event skeletons_3.txt"),
        ("f4", ROOT / "event skeletons_4.txt"),
        ("f5", ROOT / "event skeletons_5.txt"),
    ]

    all_events = []
    for prefix, path in files:
        if not path.exists():
            print(f"  SKIP {path} (not found)")
            continue
        text = path.read_text(encoding="utf-8")
        events = parse_yaml_events(text)
        # Prefix IDs to avoid collisions
        for ev in events:
            ev["id"] = f"{prefix}_{ev['id']}"
        all_events.extend(events)
        print(f"  {prefix}: {len(events)} events from {path.name}")

    out = ROOT / "src" / "escape_the_valley" / "data" / "event_skeletons.json"

    # F-c79caa11: refuse to replace a populated, curated data file with a
    # near-empty result. This is a *conversion* script, not a truncation
    # tool -- a count this low almost always means the source .txt files are
    # missing (see the SKIP lines above), not that the game legitimately
    # shrank to a handful of events. Threshold is well below the real event
    # count but well above zero.
    min_expected_events = 100
    if len(all_events) < min_expected_events:
        raise SystemExit(
            f"refusing to write {len(all_events)} events over the existing "
            f"{out} -- source files missing? (need >= {min_expected_events} "
            "parsed events; see the SKIP lines above)"
        )

    if out.exists():
        backup = out.with_suffix(".json.bak")
        out.rename(backup)
        print(f"  backed up existing {out.name} -> {backup.name}")

    out.write_text(json.dumps(all_events, indent=2), encoding="utf-8")
    print(f"\nWrote {len(all_events)} events to {out}")


if __name__ == "__main__":
    main()
