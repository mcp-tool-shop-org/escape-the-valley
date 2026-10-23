#!/usr/bin/env bash
# Negative control for --verify-toc: drop one distribution's "Name:" line from a copy of the
# notices and confirm the check fails on the real build TOC.
# Usage (inside WSL): bash verify_toc_negative.sh <checkout> <out-dir> <dist-name>
set -u
cd "$1" || exit 2; out=$2; dist=$3
sed "/^Name: ${dist}\$/d" THIRD_PARTY_NOTICES.txt > "/tmp/notices-no-${dist}.txt"
{
  echo "+ sed '/^Name: ${dist}\$/d' THIRD_PARTY_NOTICES.txt > /tmp/notices-no-${dist}.txt"
  echo "  (Name: ${dist} lines before: $(grep -c "^Name: ${dist}\$" THIRD_PARTY_NOTICES.txt), after: $(grep -c "^Name: ${dist}\$" "/tmp/notices-no-${dist}.txt"))"
  echo "+ uv run --no-sync python scripts/gen_third_party_notices.py --verify-toc build/escape-the-valley /tmp/notices-no-${dist}.txt"
  uv run --no-sync python scripts/gen_third_party_notices.py --verify-toc build/escape-the-valley "/tmp/notices-no-${dist}.txt"
  echo "exit $?"
} > "$out/09-verify-toc-negative-${dist}.log" 2>&1
cat "$out/09-verify-toc-negative-${dist}.log"
