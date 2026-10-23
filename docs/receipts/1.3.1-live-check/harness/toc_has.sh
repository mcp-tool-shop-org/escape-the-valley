#!/usr/bin/env bash
# Is each named top-level module in the PyInstaller build's TOCs? Usage: bash toc_has.sh <build-dir> <module>...
cd "$1" || exit 2; shift
for m in "$@"; do
  printf '%-16s PYZ-00.toc: %4s   Analysis-00.toc site-packages: %4s\n' "$m" \
    "$(grep -o "'${m}[.']" PYZ-00.toc | wc -l)" "$(grep -c "site-packages/${m}" Analysis-00.toc)"
done
