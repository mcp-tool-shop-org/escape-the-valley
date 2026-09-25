# escape-the-valley: how it works

Mapped at 2026-09-25 from commit fa2660c.

## What this is

10 parts, mostly Python (61 files), JavaScript (2) and TypeScript (2). Work enters through 8 doors; the busiest is CI, which reaches 3 parts. It publishes to npm and PyPI. People run escape-the-valley and trail.

## What changed since the last map

This is the first map.

## What comes in

1. **CI.** On a pull request touching 8 paths; on a push touching 8 paths; on a `workflow_call` event; or by hand. Runs tests/; checks src/.
2. **Release Binaries.** When a release is published; when the workflow Release completes; or by hand. Runs scripts/smoke_test_binary.py; builds src/escape_the_valley/__main__.py.
3. **Deploy site to GitHub Pages.** On a push to main touching 2 paths; or by hand. Runs site/astro.config.mjs and site/src/.
4. **Publish to PyPI.** When a release is published; when the workflow Release completes; or by hand. Checks src/escape_the_valley/.
5. **Release.** When a tag matching `v*` is pushed; or by hand. Runs no file this map can see.
6. **escape-the-valley** (a command people run). Runs bin/escape-the-valley.js.
7. **trail** (a command people run, from package.json). Runs bin/escape-the-valley.js.
8. **trail** (a command people run, from pyproject.toml). Runs src/escape_the_valley/cli.py.

## What happens through CI

1. The workflow runs tests/ in tests; it checks src/ in src.
2. That reaches agents (1 file).

## Who reads the results

CI writes nothing this map can see.

## The other doors

**Release Binaries** runs scripts/smoke_test_binary.py, creates a GitHub release, and builds src/escape_the_valley/__main__.py into binaries for darwin-arm64, linux-x64 and win-x64 and uploads them to the release.

**Deploy site to GitHub Pages** runs site/astro.config.mjs and site/src/, and deploys the site.

**Publish to PyPI** checks src/escape_the_valley/ and publishes to PyPI.

**Release** runs no file this map can see, publishes to npm, and creates a GitHub release.

**escape-the-valley** (a command people run) runs bin/escape-the-valley.js.

**trail** (a command people run, from package.json) runs bin/escape-the-valley.js.

**trail** (a command people run, from pyproject.toml) runs src/escape_the_valley/cli.py.

## What breaks what

- **src** is imported by 2 parts (agents, scripts), and by 1 more only from tests; it sits on the path of 4 doors.
- **agents** is imported by 1 part (scripts), and by 1 more only from tests; it sits on the path of 1 door.
- **bin** is imported by no other part and sits on the path of 2 doors.

## What tends to change together

- **src/escape_the_valley/tui_app.py** and **tests/test_tui_smoke.py** changed together in 12 of 13 commits, and the tests part imports the src part.
- **src/escape_the_valley/cli.py** and **tests/test_cli_stats.py** changed together in 6 of 7 commits, and the tests part imports the src part.
- **src/escape_the_valley/engine.py** and **tests/test_step_engine.py** changed together in 12 of 16 commits, and the tests part imports the src part.
- **src/escape_the_valley/ui.py** and **tests/test_adapter.py** changed together in 6 of 8 commits, and the tests part imports the src part.
- **src/escape_the_valley/engine.py** and **src/escape_the_valley/step_engine.py** changed together in 11 of 15 commits, inside the src part.

4 files changed together with their own tests, as expected.

Confidence is low: fewer than 20 source files reach 10 revisions in the window.

Window: 180 days; a pair counts from 3 shared commits, since 6 source files reach 10 revisions; the floor rises to 10 when 25 do.

## What no test touches

- **bin** is imported by no test.

## Written but never read

Every written place has a reader.

## Helpers that look duplicated

No two parts export a helper that looks alike.

## Generated, never hand-edited

- **src/escape_the_valley/data/event_skeletons.json** is written by scripts/yaml_to_json.py.

## Hand-authored

People write .github/, assets/, docs/, the repository root and site/; 2 writes with paths built at run time may land here.

## Where to start

src/escape_the_valley/cli.py

Read those in order to follow one run of trail end to end. This path follows trail (a command people run, from pyproject.toml) from its entry, since CI runs only tests.

## What this map cannot see

- 2 writes and 1 read use paths built at run time and are not named here.
- 3 writes and 2 reads go to a path their caller passes, not to this repository.
- 1 read goes to the directory the command is run in (.trail/), not to this repository.
- Statistics confidence is low: fewer than 20 source files reach 10 revisions in the window.

Regenerate with `npx --yes @dogfood-lab/atlas map`.
