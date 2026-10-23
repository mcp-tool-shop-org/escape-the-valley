#!/usr/bin/env bash
# Run release-binaries.yml's build job for the linux-x64 leg, step by step, in WSL.
# Usage (inside WSL): bash linux_build.sh <checkout> <out-dir>
# Each step writes <out-dir>/<NN>-<name>.log and .exit. Steps keep going after a failure,
# so one red step doesn't hide the later ones (the workflow itself would stop there).
# uv's concurrency is capped at 12 (half of WSL's 24 cores).
set -u
src=$1; out=$2
mkdir -p "$out"
cd "$src" || exit 2
export UV_CONCURRENT_INSTALLS=12 UV_CONCURRENT_BUILDS=12 UV_CONCURRENT_DOWNLOADS=12
TOOL_NAME=escape-the-valley
ENTRYPOINT=src/escape_the_valley/__main__.py
VERSION=1.3.1            # REF_NAME v1.3.1 -> VERSION=${REF_NAME#v}
TARGET=linux-x64; EXT=""
DATA_SEP=":"             # runner.os != Windows

step() {  # step <NN-name> <command...>
  local name=$1; shift
  { echo "+ $*"; "$@"; } > "$out/$name.log" 2>&1
  local rc=$?
  echo "$rc" > "$out/$name.exit"
  echo "$name: exit $rc"
}

git rev-parse HEAD > "$out/head.txt"
step 01-python-install uv python install 3.12
# The workflow's bare `uv venv` gets 3.12 on a fresh runner. Pinned here because this
# machine also has other managed Pythons, and a bare `uv venv` would pick the newest.
step 02-venv uv venv -p 3.12
step 03-install uv pip install ".[xrpl]" "pyinstaller>=6.9.0"
step 04-gen-notices uv run --no-sync python scripts/gen_third_party_notices.py THIRD_PARTY_NOTICES.txt
step 05-pyinstaller uv run --no-sync pyinstaller --onefile --name "$TOOL_NAME" --console \
  --collect-submodules rich \
  --collect-submodules textual \
  --collect-submodules xrpl \
  --collect-data textual \
  --collect-data xrpl \
  --add-data "src/escape_the_valley/tui.tcss${DATA_SEP}escape_the_valley" \
  --add-data "THIRD_PARTY_NOTICES.txt${DATA_SEP}escape_the_valley" \
  --add-data "src/escape_the_valley/data/event_skeletons.json${DATA_SEP}escape_the_valley/data" \
  "$ENTRYPOINT"
OUTNAME="${TOOL_NAME}-${VERSION}-${TARGET}${EXT}"
step 06-rename-copy bash -c "mv 'dist/${TOOL_NAME}${EXT}' 'dist/${OUTNAME}' && cp THIRD_PARTY_NOTICES.txt 'dist/THIRD_PARTY_NOTICES-${TARGET}.txt' && ls -l dist"
step 07-verify-toc uv run --no-sync python scripts/gen_third_party_notices.py --verify-toc build/escape-the-valley THIRD_PARTY_NOTICES.txt
step 08-smoke uv run --no-sync python scripts/smoke_test_binary.py "dist/$OUTNAME"
uv run --no-sync python -c "import sys, PyInstaller; print(sys.version); print('PyInstaller', PyInstaller.__version__)" > "$out/versions.txt" 2>&1
(cd dist && sha256sum "$OUTNAME" "THIRD_PARTY_NOTICES-${TARGET}.txt" && stat -c '%n %s bytes' "$OUTNAME" "THIRD_PARTY_NOTICES-${TARGET}.txt") > "$out/dist-sha256.txt" 2>&1
file "dist/$OUTNAME" > "$out/file.txt" 2>&1
