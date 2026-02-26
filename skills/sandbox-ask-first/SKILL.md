# Sandbox Ask-First

## Purpose

Use sandboxed operations safely under supervision.

## Rules

- Default to the smallest possible action.
- Prefer read-only exploration before writes.
- When a write/delete/run is needed, request approval with a clear reason.
- Keep commands short and reproducible.

## Recommended Tool Use

- File changes: write the smallest necessary file(s).
- Commands: run targeted commands (`rg`, `git status`, `flutter test`, `cargo test`) and use outputs to guide next steps.

