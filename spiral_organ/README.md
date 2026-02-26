# Spiral Organ

Flutter control panel for `spiral_organ_core` kernel.

## Architecture

- `lib/app/`: app entry shell + multi-screen navigation.
- `lib/bloc/`: global and persistent settings state (`flutter_bloc` + `hydrated_bloc`).
- `lib/features/`: session/task/changes/notifications modules.
- `lib/core/`: kernel client and transport (macOS `MethodChannel` + HTTP fallback).
- `lib/widgets/`: reusable panel and JSON viewer widgets.

## What It Controls

- `POST /kernel/session/open`
- `POST /kernel/task/submit`
- `POST /kernel/task/abort`
- `GET /kernel/task/{task_id}/status`
- `GET /kernel/task/{task_id}/events`
- `POST /kernel/deploy`
- `GET /project/{project_id}/session/{session_id}/file/status`
- `GET /project/{project_id}/session/{session_id}/changes/summary`
- `POST /project/{project_id}/session/{session_id}/changes/ack`

## Run

1. From workspace root, start dev UI:

```bash
cd /Volumes/storage/project/babel/SpiralOrgan
just dev
```

`just dev` will:
- build `spiral_organ_core` (debug)
- run `flutter run -d macos` from `spiral_organ/`

2. On macOS, the app uses MethodChannel by default:

- Channel: `spiral_organ/core`
- Call shape matches HTTP contract: `method + path + body`
- The core dylib is embedded into the app bundle Frameworks directory during
  the Xcode debug build phase and loaded from there by default.

3. On non-macOS, it falls back to HTTP with Base URL `http://127.0.0.1:8787`.
