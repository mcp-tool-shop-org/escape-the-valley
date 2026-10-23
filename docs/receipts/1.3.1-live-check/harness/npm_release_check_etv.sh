#!/usr/bin/env bash
# escape-the-valley npm release check: npx through a fresh npm cache, npm prefix and launcher cache, then
# `licenses` compared with the release's THIRD_PARTY_NOTICES-<target>.txt, then the cached binary run directly
# as the argv[0] control.
# Usage: bash npm_release_check.sh <out-dir> <version> <win|linux> [node-bin-dir]
#   win:   Git Bash on Windows (launcher cache = LOCALAPPDATA)
#   linux: inside WSL, `bash -l` (launcher cache = XDG_CACHE_HOME)
set -u
out=$1; ver=$2; os=$3; nodebin=${4:-}
[ -n "$nodebin" ] && export PATH="$nodebin:$PATH"
pkg="@mcptoolshop/escape-the-valley@${ver}"
mkdir -p "$out"/{npm-cache,npm-prefix,launcher-cache,cwd}
cd "$out/cwd"
{ echo "node $(node --version), npm $(npm --version)"; uname -sm; } > "$out/node-version.txt"
if [ "$os" = win ]; then
  target=win-x64; ext=.exe; w() { cygpath -w "$1"; }
  export LOCALAPPDATA="$(w "$out/launcher-cache")"
else
  target=linux-x64; ext=""; w() { echo "$1"; }
  export XDG_CACHE_HOME="$out/launcher-cache"
fi
export npm_config_cache="$(w "$out/npm-cache")" npm_config_prefix="$(w "$out/npm-prefix")" PYTHONIOENCODING=utf-8
run() {
  local n=$1; shift
  echo "npx --yes ${pkg} $*   (fresh npm_config_cache, npm_config_prefix, launcher cache)" > "$out/$n.cmd"
  npx --yes "$pkg" "$@" > "$out/$n.out" 2> "$out/$n.err"
  echo $? > "$out/$n.exit"
}
run 01-version --version
run 02-help --help
run 03-licenses licenses
run 04-licenses-flag --licenses
(cd "$out/launcher-cache" && find . -type f) > "$out/05-cache-files.txt"
bin="$out/launcher-cache/mcptoolshop/escape-the-valley/${ver}/escape-the-valley-${ver}-${target}${ext}"
(cd "$(dirname "$bin")" && sha256sum "$(basename "$bin")") > "$out/05-cached-sha256.txt"
curl -fsSL -o "$out/THIRD_PARTY_NOTICES-${target}.txt" "https://github.com/mcp-tool-shop-org/escape-the-valley/releases/download/v${ver}/THIRD_PARTY_NOTICES-${target}.txt"
{ for f in 03-licenses.out 04-licenses-flag.out "THIRD_PARTY_NOTICES-${target}.txt"; do (cd "$out" && sha256sum "$f" && wc -c < "$f"); done
  cmp "$out/03-licenses.out" "$out/THIRD_PARTY_NOTICES-${target}.txt" && echo "cmp licenses vs release notices: identical"
  cmp "$out/04-licenses-flag.out" "$out/THIRD_PARTY_NOTICES-${target}.txt" && echo "cmp --licenses vs release notices: identical"; } > "$out/06-licenses-vs-release.txt" 2>&1
echo "control: run the cached binary directly, without the launcher's argv0" > "$out/07-direct-help.cmd"
"$bin" --help > "$out/07-direct-help.out" 2> "$out/07-direct-help.err"; echo $? > "$out/07-direct-help.exit"
