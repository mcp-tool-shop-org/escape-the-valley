#!/usr/bin/env bash
# release-binaries.yml build job (24926be shape: adds the G1 UV_PYTHON_INSTALL_DIR step), linux-x64 leg, step by step, in WSL.
# Usage (inside WSL): bash linux_build_final.sh <checkout> <out-dir>
# Each step writes <out-dir>/<NN>-<name>.log and .exit; steps continue after a failure.
set -u
src=$1; out=$2
mkdir -p "$out"
cd "$src" || exit 2
export UV_PYTHON_INSTALL_DIR=/opt/uv-python  # G1 step of release-binaries.yml at 24926be
export UV_CONCURRENT_INSTALLS=12 UV_CONCURRENT_BUILDS=12 UV_CONCURRENT_DOWNLOADS=12
TOOL_NAME=escape-the-valley
ENTRYPOINT=src/escape_the_valley/__main__.py
VERSION=1.3.1; TARGET=linux-x64; EXT=""; DATA_SEP=":"
step() { local name=$1; shift; { echo "+ $*"; "$@"; } > "$out/$name.log" 2>&1; local rc=$?; echo "$rc" > "$out/$name.exit"; echo "$name: exit $rc"; }
pyi() {
  uv run --no-sync pyinstaller "$@" --onefile --name "$TOOL_NAME" --console \
    --exclude-module tkinter --exclude-module _tkinter --exclude-module dbm --exclude-module _dbm \
    --collect-submodules rich --collect-submodules textual --collect-submodules xrpl \
    --collect-data textual --collect-data xrpl \
    --add-data "src/escape_the_valley/tui.tcss${DATA_SEP}escape_the_valley" \
    --add-data "THIRD_PARTY_NOTICES.txt${DATA_SEP}escape_the_valley" \
    --add-data "src/escape_the_valley/data/event_skeletons.json${DATA_SEP}escape_the_valley/data" \
    "$ENTRYPOINT"
}
OUTNAME="${TOOL_NAME}-${VERSION}-${TARGET}${EXT}"
git rev-parse HEAD > "$out/head.txt"; uv --version > "$out/uv-version.txt"
step 01-python-install uv python install 3.12.13
step 02-venv uv venv --python 3.12.13
step 03-install uv pip install ".[xrpl]" "pyinstaller>=6.9.0"
step 04-remove-direct-url uv run --no-sync python -c '
import pathlib, sysconfig
purelib = pathlib.Path(sysconfig.get_path("purelib"))
for p in purelib.glob("*.dist-info/direct_url.json"):
    p.unlink()
remaining = list(purelib.glob("*.dist-info/direct_url.json"))
if remaining:
    raise SystemExit(f"direct_url.json still present in venv: {remaining}")
print("Removed direct_url.json from build venv.")
'
step 05-gen-notices uv run --no-sync python scripts/gen_third_party_notices.py --release THIRD_PARTY_NOTICES.txt
cp THIRD_PARTY_NOTICES.txt "$out/notices-before-prune.txt"
step 06-pyinstaller-1 pyi
step 07-prune-toc uv run --no-sync python scripts/gen_third_party_notices.py --release --prune-toc build/escape-the-valley THIRD_PARTY_NOTICES.txt
step 08-pyinstaller-2 pyi --noconfirm
step 09-rename-copy bash -c "mv 'dist/${TOOL_NAME}${EXT}' 'dist/${OUTNAME}' && cp THIRD_PARTY_NOTICES.txt 'dist/THIRD_PARTY_NOTICES-${TARGET}.txt'"
step 10-verify-toc uv run --no-sync python scripts/gen_third_party_notices.py --verify-toc build/escape-the-valley THIRD_PARTY_NOTICES.txt
step 11-verify-bundle uv run --no-sync python scripts/gen_third_party_notices.py --verify-bundle "dist/$OUTNAME"
step 12-smoke uv run --no-sync python scripts/smoke_test_binary.py "dist/$OUTNAME"
uv run --no-sync python -c "import sys, PyInstaller; print(sys.version); print('PyInstaller', PyInstaller.__version__)" > "$out/versions.txt" 2>&1
(cd dist && sha256sum "$OUTNAME" "THIRD_PARTY_NOTICES-${TARGET}.txt" && stat -c '%n %s bytes' "$OUTNAME" "THIRD_PARTY_NOTICES-${TARGET}.txt") > "$out/dist-sha256.txt" 2>&1
file "dist/$OUTNAME" > "$out/file.txt" 2>&1
cp "dist/$OUTNAME" "dist/THIRD_PARTY_NOTICES-${TARGET}.txt" THIRD_PARTY_NOTICES.txt "$out/"
