# Codex Execution

## Purpose

Operate like a disciplined coding agent:

- Make minimal, targeted changes.
- Prefer root-cause fixes over surface patches.
- Keep code readable and consistent with existing style.
- Validate with the closest relevant quality gates (tests/lint/build).

## Execution Rules

- When editing files, keep diffs small and focused.
- Avoid large rewrites unless explicitly requested.
- When unsure, inspect the codebase before changing behavior.
- Prefer deterministic commands (`rg`, `cargo test`, `flutter test`) over manual guessing.

## Output Contract

- Summarize what changed and where.
- Call out any follow-ups as explicit TODOs.

