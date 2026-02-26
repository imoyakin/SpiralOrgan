# agent_role_pipeline

Reusable role-based agent loop pipeline crate.

## What it provides

- YAML schema types for role pipeline (`RolePipelineSpec`)
- Lint rules (`lint_spec`)
- Async stage execution trait (`RoleStageExecutor`)
- Runtime runner (`RolePipelineRunner`)
- Built-in default template (`templates/pipeline.role.default.yaml`)

## Design

- Roles are modeled as explicit runtime entities (`scheduler`, `worker`, etc.)
- Role protocol supports `google_a2a` metadata as first-class field
- Dispatch target is independent from protocol metadata (`inherit` / `provider` / `local_client`)
- Scheduler stage can dynamically select worker pool and execute worker stage fan-out via `$selected_worker`
