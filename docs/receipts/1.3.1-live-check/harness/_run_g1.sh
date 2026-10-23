set -u
cd ~ && rm -rf etv-131-g1 && git clone -q https://github.com/mcp-tool-shop-org/escape-the-valley etv-131-g1 && cd etv-131-g1 && git checkout -q 24926be && git log --oneline -1
out=<scratch>/1.3.1-g1-24926be/linux
mkdir -p "$out"
{ ls -la /opt/uv-python; echo; grep -c '/home/' /opt/uv-python/cpython-3.12.13-linux-x86_64-gnu/lib/python3.12/_sysconfigdata__linux_x86_64-linux-gnu.py; } > "$out/00-uv-python-dir-before.txt" 2>&1
bash <scratch>/_harness/linux_build_g1.sh ~/etv-131-g1 "$out"
cd ~/etv-131-g1
neg() { local name=$1 bin=$2; { echo "+ gen_third_party_notices.py --verify-bundle $bin"; uv run --no-sync python scripts/gen_third_party_notices.py --verify-bundle "$bin"; } > "$out/$name.log" 2>&1; echo $? > "$out/$name.exit"; echo "$name: exit $(cat $out/$name.exit)"; }
neg 13-neg-verify-bundle-camp150 ~/camp-150-asset/xrpl-camp-1.5.0-linux-x64
neg 14-neg-verify-bundle-etv-7481d88-local <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64
sha256sum ~/camp-150-asset/xrpl-camp-1.5.0-linux-x64 <scratch>/1.3.1-final-7481d88/linux/escape-the-valley-1.3.1-linux-x64 > "$out/neg-controls-sha256.txt"
echo DONE
