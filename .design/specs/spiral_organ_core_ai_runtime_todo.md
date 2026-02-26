# SpiralOrgan Core AI Runtime TODO

Last updated: 2026-02-25
Owner: Codex + User
Scope: `crates/spiral_organ_core` as runtime core, `spiral_organ` as UI adapter.

## Source Requirement (User Directive)

This document records the long-running requirement set:

1. Switch notification flow to EventChannel.
2. Notification path supports `HTTP + /kernel/ws` (and local bridge mode).
3. Provider-direct AI API calls should be first-class and default.
4. AI responses in user chat must stream back in real time.
5. Add sandboxed agent operations in a designated workspace folder:
   - create/edit/delete files
   - create/delete folders
   - execute scripts/commands
6. Execution should run on the host where `spiral_organ_core` is deployed.
7. Optionally support remote execution over SSH (when user provides SSH config).
8. Add Skills tab:
   - skill store placeholder
   - manage/load/create/select/activate skill
   - AI can read skill list and activate selected skills
9. Use OpenCode/OpenClaw/Codex design ideas where better.
10. Keep this as a long-running roadmap and continue across sessions.

## Current Status Snapshot

- Confirmed decisions (2026-02-25):
  - Skill format: `SKILL.md` standard.
  - Safety default: `ask-first`.
  - Notifications: pure push stream (no polling-first UX).
  - Provider default: `base_url` provider path as default dispatch.

- Done:
  - Local-client Codex dispatch emits streaming task events (`task.output.chunk` / `task.error.chunk`) during execution.
  - Core exposes `/kernel/ws`, and Flutter HTTP transport now consumes it.
  - macOS local bridge now exposes EventChannel push (`spiral_organ/events`) via Rust FFI subscribe/next/unsubscribe.
  - App shell now subscribes to push events and disables polling while push is connected.
  - Provider dispatch defaults to provider target and supports streamed OpenAI-like output events.
  - Core skill APIs exist: list/create/activate/deactivate/store placeholder.
  - Active skills are injected into task input context in role pipeline execution.
  - Flutter now has Skills end-to-end wiring:
    - `kernel_client` skill APIs (`list/create/activate/deactivate/store`)
    - dedicated `Skills` tab
    - create + activate/deactivate + refresh
    - store placeholder pane
  - Ask-first sandbox guardrail baseline is implemented in core:
    - `POST /kernel/sandbox/tools/execute`
    - `GET /kernel/sandbox/approvals`
    - `POST /kernel/sandbox/approvals/{approval_id}/approve`
    - `POST /kernel/sandbox/approvals/{approval_id}/reject`
    - enforced workspace-relative paths (rejects `..` traversal)
    - command allowlist enforcement from policy/default set
    - approval + execution lifecycle events are published to realtime stream
    - state persistence includes pending approvals
  - Flutter `kernel_client` now includes sandbox APIs/models for execute + approval queue actions.
  - Sandbox UI wiring is implemented in `AppShell`:
    - Skills tab includes sandbox action runner dialog
    - pending approval queue shows operation details
    - approve/reject actions call core approval APIs
    - realtime `tool.*` events trigger automatic queue refresh
  - SSH remote target support is wired end-to-end:
    - core endpoints: list/create/update/delete `runtime/ssh-targets`
    - dispatcher kind `ssh` is accepted in config/session routing
    - `ssh` dispatch streams stdout/stderr through `task.output.chunk` / `task.error.chunk`
    - SSH targets are persisted in core state and available to UI
    - Flutter runtime tab now supports SSH target add/edit/delete and dispatcher selection
  - Skills store now supports install flow:
    - curated store catalog is returned from core endpoint
    - install endpoint creates local skill records from store items
    - Flutter Skills tab can install store items and reflects installed/active status
  - AI-driven skill activation is implemented via controlled directive path:
    - active-skill prompt context documents directive protocol
    - runtime parses `@skill activate <id_or_name>` / `@skill deactivate <id_or_name>`
    - successful and failed directives are emitted as structured realtime events
- Pending:
  - Sandbox/tool-execution policy completion with richer risk tiers, quota controls, and stricter audit redaction.

## Epics and Work Breakdown

## Epic A - Event Push Architecture

- A1. Add Flutter `KernelEventStream` abstraction.
- A2. Implement HTTP transport stream subscriber (`/kernel/ws`).
- A3. Implement macOS EventChannel stream subscriber (`spiral_organ/events`).
- A4. Route sticky/task updates from event stream first, polling as fallback only.
- A5. Add dedupe by `event_id` and reconnect backoff.

Acceptance:
- Running tasks show incremental updates without frequent polling loops.
- macOS local bridge and HTTP server mode both receive push events.

## Epic B - Provider-First AI Runtime

- B1. Introduce provider-dispatch as default execution target for new sessions/projects.
- B2. Add streamed provider output events (`task.output.chunk`) similar to local-client dispatch.
- B3. Align stage output format so UI chat rendering is consistent across providers.
- B4. Preserve existing local-client mode as explicit opt-in fallback.

Acceptance:
- New task defaults to provider path unless project overrides.
- User sees streamed provider replies in chat within active task.

## Epic C - Sandbox and Tool Execution Model

- C1. Define policy model:
  - workspace root allowlist
  - read/write/exec controls
  - command allow/deny + approval mode
- C2. Implement sandboxed file ops API in core (safe path resolution, traversal prevention).
- C3. Implement sandboxed command runner with timeout/resource guards.
- C4. Integrate policy checks into role pipeline tool execution.
- C5. Add audit event trail per tool call.

Acceptance:
- Agent can modify files and run scripts only under configured sandbox scope.
- Violations are blocked and surfaced as structured events.

## Epic D - Remote SSH Execution Target

- D1. Add optional SSH target config (host/user/auth/working_dir).
- D2. Add remote executor that mirrors local sandbox policy semantics.
- D3. Stream stdout/stderr/events back through the same task event channel.
- D4. Add connectivity test and host fingerprint handling.

Acceptance:
- Same task contract works for local and SSH targets.
- Streamed task events remain uniform regardless of execution location.

## Epic E - Skills System

- E1. Add core skill schema and storage (`id`, `name`, `version`, `description`, `entry`, `policy`).
- E2. Add skill registry APIs (list/create/update/delete/activate/deactivate).
- E3. Add "Skill Store" placeholder endpoint and empty UI pane.
- E4. Inject active skills into prompt context (progressive disclosure style loading).
- E5. Let AI request skill activation through controlled action/event.

Acceptance:
- User can manage skills in UI.
- Active skills are available to pipeline runtime and visible in logs/events.

## Epic F - Continuation Protocol

- F1. Persist roadmap and session handoff summary in repo.
- F2. Add "resume prompt template" for opening a new chat with full context.
- F3. Maintain execution checklist and update progress each session.

Acceptance:
- Work can continue after context reset without losing requirements.

## Session Handoff Template

When opening a new conversation, inject:

1. Objective:
   - Continue `.design/specs/spiral_organ_core_ai_runtime_todo.md`.
2. Current completed items:
   - (list from this doc)
3. Current branch/files touched:
   - (fill per session)
4. Next concrete task:
   - (single next milestone, e.g. A1/A2)
5. Validation done/pending:
   - tests, analyze, runtime smoke

## Notes

- `bd` issue tracker sync is currently blocked in this environment because Dolt server binary is unavailable (`dolt: command not found`), so this markdown acts as durable task memory for now.
