#!/bin/sh
set -eu

# Keep behavior aligned with debug development runs.
if [ "${CONFIGURATION:-}" != "Debug" ]; then
  exit 0
fi

WORKSPACE_ROOT="$(cd "$PROJECT_DIR/../.." && pwd)"
CORE_LIB_SRC="$WORKSPACE_ROOT/target/debug/libspiral_organ_core.dylib"
APP_FRAMEWORKS_DIR="$TARGET_BUILD_DIR/$WRAPPER_NAME/Contents/Frameworks"
CORE_LIB_DST="$APP_FRAMEWORKS_DIR/libspiral_organ_core.dylib"

if [ ! -f "$CORE_LIB_SRC" ]; then
  echo "error: missing $CORE_LIB_SRC; run cargo build -p spiral_organ_core first" >&2
  exit 1
fi

mkdir -p "$APP_FRAMEWORKS_DIR"
cp -f "$CORE_LIB_SRC" "$CORE_LIB_DST"

if [ "${CODE_SIGNING_ALLOWED:-NO}" = "YES" ] && [ -n "${EXPANDED_CODE_SIGN_IDENTITY:-}" ]; then
  /usr/bin/codesign --force --sign "$EXPANDED_CODE_SIGN_IDENTITY" --timestamp=none "$CORE_LIB_DST"
fi

echo "[spiral_organ] embedded core dylib -> $CORE_LIB_DST"
