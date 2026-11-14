# escape-the-valley: how it works

Mapped at 2026-11-14 from commit fa6f51e by Atlas 1.26.0.

## What this is

10 parts, in Rust (86 files), Python (70 files), shell (13 files), CSS (2 files), JavaScript (2 files), TypeScript (2 files) and HTML (1 file). Work enters through 14 doors; CI and Release each reach 4 parts, and CI is followed because a pull request goes through it. It publishes to npm and PyPI. It deploys a site to GitHub Pages. People run escape-the-valley and trail. etv, etv_content_diff, etv_simulate, etv_turn_set, etv_turn_set_v2 and narrator_measure are commands built from rust/etv-core (nothing ships them).

## What changed since 2026-11-14 (e2bd674)

- rust/etv-core/tests/narrator_settings_diagnostics.rs is new and belongs to no part, so atlas check fails on it against the previous map.
- 1 file added.

## What comes in

1. **CI.** On a pull request; on a push touching 10 paths; on a `workflow_call` event; or by hand. Runs scripts/gen_third_party_notices.py, tests/, rust/etv-core/src/cloud_claude.rs and 49 more; checks scripts/, src/, rust/etv-core/src/bin/etv.rs and 6 more.
2. **Release.** When a tag matching `v*` is pushed; or by hand. Runs scripts/gen_third_party_notices.py, tests/, rust/etv-core/src/cloud_claude.rs and 49 more; checks scripts/, src/, rust/etv-core/src/bin/etv.rs and 6 more.
3. **Release Binaries.** When a release is published; when the workflow Release completes; or by hand. Runs scripts/check_glibc_floor.py, scripts/gen_third_party_notices.py and scripts/smoke_test_binary.py; builds src/escape_the_valley/__main__.py.
4. **Deploy site to GitHub Pages.** On a push to main touching 2 paths; or by hand. Runs site/astro.config.mjs and site/src/.
5. **Publish to PyPI.** When a release is published; when the workflow Release completes; or by hand. Checks src/escape_the_valley/.
6. **escape-the-valley** (a command people run). Runs bin/escape-the-valley.js.
7. **trail** (a command people run, from package.json). Runs bin/escape-the-valley.js.
8. **trail** (a command people run, from pyproject.toml). Runs src/escape_the_valley/cli.py.
9. **etv** (a command built from rust/etv-core, which nothing ships). Runs rust/etv-core/src/bin/etv.rs.
10. **etv_content_diff** (a command built from rust/etv-core, which nothing ships). Runs rust/etv-core/src/bin/etv_content_diff.rs.
11. **etv_simulate** (a command built from rust/etv-core, which nothing ships). Runs rust/etv-core/src/bin/etv_simulate.rs.
12. **etv_turn_set** (a command built from rust/etv-core, which nothing ships). Runs rust/etv-core/src/bin/etv_turn_set.rs.
13. **etv_turn_set_v2** (a command built from rust/etv-core, which nothing ships). Runs rust/etv-core/src/bin/etv_turn_set_v2.rs.
14. **narrator_measure** (a command built from rust/etv-core, which nothing ships). Runs rust/etv-core/src/bin/narrator_measure.rs.

## What happens through CI

1. The workflow runs scripts/gen_third_party_notices.py in scripts, tests/ in tests, rust/etv-core/src/cloud_claude.rs, rust/etv-core/src/engine.rs, rust/etv-core/src/events.rs, and 47 files in 7 more places; it checks scripts/ in scripts, src/ in src, rust/etv-core/src/bin/etv.rs, rust/etv-core/src/bin/etv_content_diff.rs, rust/etv-core/src/bin/etv_simulate.rs, and 4 files in 4 more places.
2. That reaches agents (1 file).
3. It uploads coverage to Codecov.

## Who reads the results

CI writes nothing this map can see.

## The other doors

**Release** runs scripts/gen_third_party_notices.py, tests/, rust/etv-core/src/cloud_claude.rs and 49 more, checks scripts/, src/, rust/etv-core/src/bin/etv.rs and 6 more, reaches agents, uploads coverage to Codecov, publishes to npm, and creates a GitHub release.

**Release Binaries** runs scripts/check_glibc_floor.py, scripts/gen_third_party_notices.py and scripts/smoke_test_binary.py, creates a GitHub release, and builds src/escape_the_valley/__main__.py into binaries for darwin-arm64, linux-x64 and win-x64 and uploads them to the release.

**Deploy site to GitHub Pages** runs site/astro.config.mjs and site/src/, and deploys the site.

**Publish to PyPI** checks src/escape_the_valley/ and publishes to PyPI.

**escape-the-valley** (a command people run) runs bin/escape-the-valley.js.

**trail** (a command people run, from package.json) runs bin/escape-the-valley.js.

**trail** (a command people run, from pyproject.toml) runs src/escape_the_valley/cli.py.

**etv** (a command built from rust/etv-core, which nothing ships) runs rust/etv-core/src/bin/etv.rs.

**etv_content_diff** (a command built from rust/etv-core, which nothing ships) runs rust/etv-core/src/bin/etv_content_diff.rs.

**etv_simulate** (a command built from rust/etv-core, which nothing ships) runs rust/etv-core/src/bin/etv_simulate.rs.

**etv_turn_set** (a command built from rust/etv-core, which nothing ships) runs rust/etv-core/src/bin/etv_turn_set.rs.

**etv_turn_set_v2** (a command built from rust/etv-core, which nothing ships) runs rust/etv-core/src/bin/etv_turn_set_v2.rs.

**narrator_measure** (a command built from rust/etv-core, which nothing ships) runs rust/etv-core/src/bin/narrator_measure.rs.

## What breaks what

- **src** is imported by 2 parts (agents, scripts), and by 1 more only from tests; it sits on the path of 5 doors.
- **agents** is imported by 1 part (scripts), and by 1 more only from tests; it sits on the path of 2 doors.
- **scripts** is imported only from tests, by 1 part (tests), and sits on the path of 3 doors.
- **bin** is imported by no other part and sits on the path of 2 doors.
- **tests** is imported by no other part and sits on the path of 2 doors.

## What tends to change together

- **src/escape_the_valley/tui_app.py** and **tests/test_tui_smoke.py** changed together in 13 of 14 commits, and the tests part imports the src part.
- **src/escape_the_valley/cli.py** and **tests/test_cli_stats.py** changed together in 6 of 7 commits, and the tests part imports the src part.
- **src/escape_the_valley/engine.py** and **tests/test_step_engine.py** changed together in 14 of 18 commits, and the tests part imports the src part.
- **src/escape_the_valley/ui.py** and **tests/test_adapter.py** changed together in 7 of 9 commits, and the tests part imports the src part.
- **src/escape_the_valley/engine.py** and **src/escape_the_valley/step_engine.py** changed together in 13 of 17 commits, inside the src part.

8 files changed together with their own tests, as expected.

Confidence is low: fewer than 25 source files reach 10 revisions in the window.

Window: 180 days; a pair counts from 3 shared commits, since 12 source files reach 10 revisions; the floor rises to 10 when 25 do.

## What no test touches

- **bin** is imported by no test.

## Written but never read

Every written place has a reader.

## Helpers that look duplicated

No two parts export a helper that looks alike.

## Generated, never hand-edited

Every tracked place code writes here is edited by people too; see Hand-authored.

## Hand-authored

People write .github/, assets/, docs/, the repository root and site/; 2 writes with paths built at run time may land here.

- **rust/etv-core/notices/THIRD_PARTY_NOTICES.txt** is written by rust/tools/gen_notices.py from inputs this repository does not keep, and by people.
- **rust/etv-core/notices/notices_manifest.json** is written by rust/tools/gen_notices.py from inputs this repository does not keep, and by people.
- **src/escape_the_valley/data/event_skeletons.json** is written by scripts/yaml_to_json.py from inputs this repository does not keep, and by people.

## Where to start

.github/workflows/ci.yml → scripts/gen_third_party_notices.py → src/escape_the_valley/notices.py

Read those in order to follow one pull request end to end.

## What this map cannot see

- 1 import could not be resolved: `src/escape_the_valley/notices.py` imports a path built at run time.
- 2 writes and 6 reads use paths built at run time and are not named here.
- 4 writes and 20 reads go to a path their caller passes, not to this repository.
- 5 writes go to the directory the command is run in (.trail/) or a path their caller passes, not to this repository.
- 138 files belong to no part: rust/Cargo.lock, rust/Cargo.toml, rust/deny.toml and 135 more.
- Statistics confidence is low: fewer than 25 source files reach 10 revisions in the window.

Regenerate with `npx --yes @dogfood-lab/atlas map`.
