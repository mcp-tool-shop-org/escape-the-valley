# escape-the-valley: how it works

Mapped at 2026-10-23 from commit ffd90d1 by Atlas 1.24.0.

## What this is

10 parts, mostly Python (64 files), Rust (11), shell (10), CSS (2), JavaScript (2), TypeScript (2) and HTML (1). Work enters through 8 doors; CI and Release each reach 3 parts, and CI is followed because a pull request goes through it. It publishes to npm and PyPI. It deploys a site to GitHub Pages. People run escape-the-valley and trail.

## What changed since 2026-10-23 (ef9a702)

Nothing structural changed since 2026-10-23; 2 files changed content.

## What comes in

1. **CI.** On a pull request; on a push touching 10 paths; on a `workflow_call` event; or by hand. Runs rust/etv-core/src/engine.rs, rust/etv-core/src/events.rs, rust/etv-core/src/memory.rs and 6 more; checks src/, tests/ and rust/etv-core/src/lib.rs.
2. **Release.** When a tag matching `v*` is pushed; or by hand. Runs rust/etv-core/src/engine.rs, rust/etv-core/src/events.rs, rust/etv-core/src/memory.rs and 6 more; checks src/, tests/ and rust/etv-core/src/lib.rs.
3. **Release Binaries.** When a release is published; when the workflow Release completes; or by hand. Runs scripts/gen_third_party_notices.py and scripts/smoke_test_binary.py; builds src/escape_the_valley/__main__.py.
4. **Deploy site to GitHub Pages.** On a push to main touching 2 paths; or by hand. Runs site/astro.config.mjs and site/src/.
5. **Publish to PyPI.** When a release is published; when the workflow Release completes; or by hand. Checks src/escape_the_valley/.
6. **escape-the-valley** (a command people run). Runs bin/escape-the-valley.js.
7. **trail** (a command people run, from package.json). Runs bin/escape-the-valley.js.
8. **trail** (a command people run, from pyproject.toml). Runs src/escape_the_valley/cli.py.

## What happens through CI

1. The workflow runs rust/etv-core/src/engine.rs, rust/etv-core/src/events.rs, rust/etv-core/src/memory.rs, rust/etv-core/src/model.rs, rust/etv-core/src/physics.rs, and 4 files in 4 more places; it checks src/ in src, tests/ in tests and rust/etv-core/src/lib.rs.
2. That reaches agents (1 file).
3. It uploads coverage to Codecov.

## Who reads the results

CI writes nothing this map can see.

## The other doors

**Release** runs rust/etv-core/src/engine.rs, rust/etv-core/src/events.rs, rust/etv-core/src/memory.rs and 6 more, checks src/, tests/ and rust/etv-core/src/lib.rs, reaches agents, uploads coverage to Codecov, publishes to npm, and creates a GitHub release.

**Release Binaries** runs scripts/gen_third_party_notices.py and scripts/smoke_test_binary.py, creates a GitHub release, and builds src/escape_the_valley/__main__.py into binaries for darwin-arm64, linux-x64 and win-x64 and uploads them to the release.

**Deploy site to GitHub Pages** runs site/astro.config.mjs and site/src/, and deploys the site.

**Publish to PyPI** checks src/escape_the_valley/ and publishes to PyPI.

**escape-the-valley** (a command people run) runs bin/escape-the-valley.js.

**trail** (a command people run, from package.json) runs bin/escape-the-valley.js.

**trail** (a command people run, from pyproject.toml) runs src/escape_the_valley/cli.py.

## What breaks what

- **src** is imported by 2 parts (agents, scripts), and by 1 more only from tests; it sits on the path of 5 doors.
- **agents** is imported by 1 part (scripts), and by 1 more only from tests; it sits on the path of 2 doors.
- **bin** is imported by no other part and sits on the path of 2 doors.
- **tests** is imported by no other part and sits on the path of 2 doors.
- **src/escape_the_valley/data/event_skeletons.json** is written by scripts and read by rust/etv-core/src/worldgen.rs and src; a hand edit reaches every reader.

## What tends to change together

- **src/escape_the_valley/tui_app.py** and **tests/test_tui_smoke.py** changed together in 13 of 14 commits, and the tests part imports the src part.
- **src/escape_the_valley/cli.py** and **tests/test_cli_stats.py** changed together in 6 of 7 commits, and the tests part imports the src part.
- **src/escape_the_valley/ui.py** and **tests/test_adapter.py** changed together in 7 of 9 commits, and the tests part imports the src part.
- **src/escape_the_valley/engine.py** and **tests/test_step_engine.py** changed together in 13 of 17 commits, and the tests part imports the src part.
- **src/escape_the_valley/engine.py** and **src/escape_the_valley/step_engine.py** changed together in 12 of 16 commits, inside the src part.

8 files changed together with their own tests, as expected.

Confidence is low: fewer than 25 source files reach 10 revisions in the window.

Window: 180 days; a pair counts from 3 shared commits, since 8 source files reach 10 revisions; the floor rises to 10 when 25 do.

## What no test touches

- **bin** is imported by no test.

## Written but never read

Every written place has a reader.

## Helpers that look duplicated

No two parts export a helper that looks alike.

## Generated, never hand-edited

Every tracked place code writes here is edited by people too; see Hand-authored.

## Hand-authored

People write .github/, assets/, docs/, the repository root and site/; 1 write with a path built at run time may land here.

- **src/escape_the_valley/data/event_skeletons.json** is written by scripts/yaml_to_json.py from inputs this repository does not keep, and by people.

## Where to start

src/escape_the_valley/cli.py → src/escape_the_valley/save.py → src/escape_the_valley/models.py → src/escape_the_valley/worldgen.py → src/escape_the_valley/gm.py → src/escape_the_valley/voice.py → src/escape_the_valley/step_engine.py

Read those in order to follow one run of trail end to end. This path follows trail (a command people run, from pyproject.toml) from its entry, since CI runs only tests and checks.

## What this map cannot see

- 1 import could not be resolved: `src/escape_the_valley/notices.py` imports a path built at run time.
- 1 write and 7 reads use paths built at run time and are not named here.
- 4 writes and 18 reads go to a path their caller passes, not to this repository.
- 5 writes go to the directory the command is run in (.trail/) or a path their caller passes, not to this repository.
- 23 files belong to no part: rust/Cargo.lock, rust/Cargo.toml, rust/deny.toml and 20 more.
- Statistics confidence is low: fewer than 25 source files reach 10 revisions in the window.

Regenerate with `npx --yes @dogfood-lab/atlas map`.
