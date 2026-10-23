# escape-the-valley 1.3.2 npm release check on Windows (Git Bash): the item 11 check, then self-check, a short
# GM-off run, and `licenses` in a real console (with the 1.3.1 package as the N2 control).
set -u
R=$1; out=$R/win; H=$(dirname "$0"); PY=${PY:-python}
bash "$H/npm_release_check.sh" "$out" 1.3.2 win
export npm_config_cache="$(cygpath -w "$out/npm-cache")" npm_config_prefix="$(cygpath -w "$out/npm-prefix")" LOCALAPPDATA="$(cygpath -w "$out/launcher-cache")" PYTHONIOENCODING=utf-8
pkg=@mcptoolshop/escape-the-valley@1.3.2
cd "$out/cwd"
echo "npx --yes $pkg self-check   (the caches above)" > "$out/08-self-check.cmd"
npx --yes $pkg self-check > "$out/08-self-check.out" 2> "$out/08-self-check.err"; echo $? > "$out/08-self-check.exit"
mkdir -p "$out/gmoff" && cd "$out/gmoff"
echo "cli_drive.py travel, 12 answers: npx --yes $pkg new --seed 42 --gm-off   (cwd: its own folder)" > "$out/09-gm-off-run.cmd"
"$PY" -I "$H/cli_drive.py" "$out/09-gm-off-run.log" 12 travel -- npx.cmd --yes $pkg new --seed 42 --gm-off > "$out/09-gm-off-run.driver.txt" 2>&1; echo $? > "$out/09-gm-off-run.exit"
cd "$out/cwd"
"$PY" -I "$H/console_capture.py" "$out/10-console-licenses" 800 32000 -- npx.cmd --yes $pkg licenses
# The N2 control: the released 1.3.1 package, through the caches of its own item 11 check.
c131=$(dirname "$R")/npm-131-release-20261023T0459Z/win
( export npm_config_cache="$(cygpath -w "$c131/npm-cache")" npm_config_prefix="$(cygpath -w "$c131/npm-prefix")" LOCALAPPDATA="$(cygpath -w "$c131/launcher-cache")"
  "$PY" -I "$H/console_capture.py" "$out/11-console-licenses-131-control" 800 32000 -- npx.cmd --yes @mcptoolshop/escape-the-valley@1.3.1 licenses )
"$PY" -I "$H/line_endings.py" "$out/THIRD_PARTY_NOTICES-win-x64.txt" "$out/03-licenses.out" "$out/04-licenses-flag.out" --rows "$out/10-console-licenses.rows.txt" > "$out/12-line-endings.txt" 2>&1
"$PY" -I "$H/line_endings.py" "$c131/THIRD_PARTY_NOTICES-win-x64.txt" "$c131/03-licenses.out" --rows "$out/11-console-licenses-131-control.rows.txt" > "$out/13-line-endings-131-control.txt" 2>&1
echo DONE
