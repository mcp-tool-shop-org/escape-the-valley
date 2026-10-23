set -u
cd ~ && rm -rf etv-131-final && git clone -q https://github.com/mcp-tool-shop-org/escape-the-valley etv-131-final && cd etv-131-final && git checkout -q 7481d88f29cdb772cb5810f3c30c03ed4f0bf494 && git log --oneline -1
out=<scratch>/1.3.1-final-7481d88/linux
bash <scratch>/_harness/linux_build_final.sh ~/etv-131-final "$out"
echo DONE
