set -u
out=<scratch>/npm-132-release-20261023T1457Z/linux
bash <scratch>/_harness/npm_release_check.sh "$out" 1.3.2 linux ~/opt/node-v22.22.3-linux-x64/bin
rm -rf ~/etv-132-tag ~/etv-132-asset && git clone -q https://github.com/mcp-tool-shop-org/escape-the-valley ~/etv-132-tag && cd ~/etv-132-tag && git checkout -q v1.3.2 && git rev-parse HEAD > "$out/08-tag-head.txt"
UV_PYTHON_INSTALL_DIR=/opt/uv-python uv venv -q --python 3.12.13 .venv && uv pip install -q "pyinstaller>=6.9.0" > /dev/null 2>&1
mkdir -p ~/etv-132-asset && curl -fsSL -o ~/etv-132-asset/escape-the-valley-1.3.2-linux-x64 https://github.com/mcp-tool-shop-org/escape-the-valley/releases/download/v1.3.2/escape-the-valley-1.3.2-linux-x64
(cd ~/etv-132-asset && sha256sum escape-the-valley-1.3.2-linux-x64) > "$out/08-release-asset-sha256.txt"
for pair in "09-verify-bundle-release-asset:$HOME/etv-132-asset/escape-the-valley-1.3.2-linux-x64" "10-verify-bundle-launcher-cache:$out/launcher-cache/mcptoolshop/escape-the-valley/1.3.2/escape-the-valley-1.3.2-linux-x64"; do
  n=${pair%%:*}; b=${pair#*:}
  { echo "+ gen_third_party_notices.py --verify-bundle <$n>"; .venv/bin/python scripts/gen_third_party_notices.py --verify-bundle "$b"; } > "$out/$n.log" 2>&1; echo $? > "$out/$n.exit"
done
cp ~/etv-132-asset/escape-the-valley-1.3.2-linux-x64 "$out/"
H=<scratch>/_harness
export npm_config_cache="$out/npm-cache" npm_config_prefix="$out/npm-prefix" XDG_CACHE_HOME="$out/launcher-cache" PYTHONIOENCODING=utf-8 PATH="$HOME/opt/node-v22.22.3-linux-x64/bin:$PATH"
pkg=@mcptoolshop/escape-the-valley@1.3.2
cd "$out/cwd"
echo "npx --yes $pkg self-check   (the caches above)" > "$out/11-self-check.cmd"
npx --yes $pkg self-check > "$out/11-self-check.out" 2> "$out/11-self-check.err"; echo $? > "$out/11-self-check.exit"
mkdir -p "$out/gmoff" && cd "$out/gmoff"
echo "cli_drive.py travel, 12 answers: npx --yes $pkg new --seed 42 --gm-off   (cwd: its own folder)" > "$out/12-gm-off-run.cmd"
python3 -I "$H/cli_drive.py" "$out/12-gm-off-run.log" 12 travel -- npx --yes $pkg new --seed 42 --gm-off > "$out/12-gm-off-run.driver.txt" 2>&1; echo $? > "$out/12-gm-off-run.exit"
python3 -I "$H/line_endings.py" "$out/THIRD_PARTY_NOTICES-linux-x64.txt" "$out/03-licenses.out" "$out/04-licenses-flag.out" > "$out/13-line-endings.txt" 2>&1
echo DONE
