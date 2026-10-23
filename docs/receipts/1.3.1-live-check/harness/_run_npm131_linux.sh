set -u
out=<scratch>/npm-131-release-20261023T0459Z/linux
bash <scratch>/_harness/npm_release_check.sh "$out" 1.3.1 linux ~/opt/node-v22.22.3-linux-x64/bin
rm -rf ~/etv-131-tag ~/etv-131-asset && git clone -q https://github.com/mcp-tool-shop-org/escape-the-valley ~/etv-131-tag && cd ~/etv-131-tag && git checkout -q v1.3.1 && git rev-parse HEAD > "$out/08-tag-head.txt"
UV_PYTHON_INSTALL_DIR=/opt/uv-python uv venv -q --python 3.12.13 .venv && uv pip install -q "pyinstaller>=6.9.0" > /dev/null 2>&1
mkdir -p ~/etv-131-asset && curl -fsSL -o ~/etv-131-asset/escape-the-valley-1.3.1-linux-x64 https://github.com/mcp-tool-shop-org/escape-the-valley/releases/download/v1.3.1/escape-the-valley-1.3.1-linux-x64
(cd ~/etv-131-asset && sha256sum escape-the-valley-1.3.1-linux-x64) > "$out/08-release-asset-sha256.txt"
for pair in "09-verify-bundle-release-asset:$HOME/etv-131-asset/escape-the-valley-1.3.1-linux-x64" "10-verify-bundle-launcher-cache:$out/launcher-cache/mcptoolshop/escape-the-valley/1.3.1/escape-the-valley-1.3.1-linux-x64"; do
  n=${pair%%:*}; b=${pair#*:}
  { echo "+ gen_third_party_notices.py --verify-bundle <$n>"; .venv/bin/python scripts/gen_third_party_notices.py --verify-bundle "$b"; } > "$out/$n.log" 2>&1; echo $? > "$out/$n.exit"
done
cp ~/etv-131-asset/escape-the-valley-1.3.1-linux-x64 "$out/"
echo DONE
