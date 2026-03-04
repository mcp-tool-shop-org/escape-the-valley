#!/usr/bin/env bash
# Verify script — runs lint + tests. Exit 0 means shippable.
set -e

echo "=== Escape the Valley — Verify ==="

echo "Lint..."
ruff check src/ tests/

echo "Tests..."
pytest --tb=short -q

echo "=== All checks passed ==="
