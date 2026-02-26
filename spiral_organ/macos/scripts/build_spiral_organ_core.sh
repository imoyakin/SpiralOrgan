#!/bin/sh
set -eu

# Only enforce auto build for debug sessions (flutter run -d macos).
if [ "${CONFIGURATION:-}" != "Debug" ]; then
  exit 0
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found in PATH; cannot build spiral_organ_core" >&2
  exit 1
fi

WORKSPACE_ROOT="$(cd "$PROJECT_DIR/../.." && pwd)"

echo "[spiral_organ] building spiral_organ_core (debug) at $WORKSPACE_ROOT"
(
  cd "$WORKSPACE_ROOT"
  cargo build -p spiral_organ_core
)
