# PipelineSpec Schema Draft (v0.1)

## 目标

定义可编辑 Agent Pipeline 的统一配置契约，支持:

- 默认 pipeline 模板
- 用户覆盖与自定义节点
- lint 校验与安全约束

## 顶层结构

```yaml
api_version: "spiralorgan.pipeline/v0.1"
pipeline_id: "default-main"
description: "default orchestrator pipeline"
defaults:
  max_iterations: 20
  max_parallel_nodes: 4
  cancel_mode: "cooperative" # cooperative | hard
nodes: []
edges: []
hooks: []
```

## 节点结构

```yaml
id: "plan_01"
kind: "plan" # start|intake|recall|plan|dispatch|execute|verify|review|commit|end|custom
enabled: true
role_scope: ["designer"]
timeout_ms: 300000
retries:
  max_attempts: 2
  backoff_ms: 1200
approval:
  required: false
execution:
  mode: "sequential" # sequential | parallel
  allowed_tools: ["memory_recall", "kanban_update"]
inputs:
  from_context: ["target", "kanban", "memory"]
outputs:
  write_context: ["plan_result"]
on_error:
  strategy: "goto" # goto | fail | skip | retry
  next_node: "review_01"
```

## 连线结构

```yaml
from: "plan_01"
to: "dispatch_01"
priority: 100
when: "ctx.plan_result.ready == true"
```

## Hook 结构

```yaml
id: "sanitize_before_execute"
phase: "before_node" # before_node | after_node | before_pipeline | after_pipeline
target: "execute_01"
action: "policy.sanitize_context"
enabled: true
```

## JSON Schema 草案

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "spiralorgan.pipeline.v0.1",
  "type": "object",
  "required": ["api_version", "pipeline_id", "nodes", "edges"],
  "properties": {
    "api_version": { "const": "spiralorgan.pipeline/v0.1" },
    "pipeline_id": { "type": "string", "minLength": 1 },
    "description": { "type": "string" },
    "defaults": {
      "type": "object",
      "properties": {
        "max_iterations": { "type": "integer", "minimum": 1, "maximum": 1000 },
        "max_parallel_nodes": { "type": "integer", "minimum": 1, "maximum": 64 },
        "cancel_mode": { "enum": ["cooperative", "hard"] }
      },
      "additionalProperties": false
    },
    "nodes": {
      "type": "array",
      "minItems": 2,
      "items": { "$ref": "#/$defs/node" }
    },
    "edges": {
      "type": "array",
      "items": { "$ref": "#/$defs/edge" }
    },
    "hooks": {
      "type": "array",
      "items": { "$ref": "#/$defs/hook" }
    }
  },
  "$defs": {
    "node": {
      "type": "object",
      "required": ["id", "kind", "enabled"],
      "properties": {
        "id": { "type": "string", "pattern": "^[a-z0-9_\\-]+$" },
        "kind": {
          "enum": ["start", "intake", "recall", "plan", "dispatch", "execute", "verify", "review", "commit", "end", "custom"]
        },
        "enabled": { "type": "boolean" },
        "role_scope": { "type": "array", "items": { "type": "string" } },
        "timeout_ms": { "type": "integer", "minimum": 100, "maximum": 3600000 },
        "retries": {
          "type": "object",
          "properties": {
            "max_attempts": { "type": "integer", "minimum": 0, "maximum": 10 },
            "backoff_ms": { "type": "integer", "minimum": 0, "maximum": 60000 }
          },
          "additionalProperties": false
        },
        "approval": {
          "type": "object",
          "properties": {
            "required": { "type": "boolean" }
          },
          "additionalProperties": false
        },
        "execution": {
          "type": "object",
          "properties": {
            "mode": { "enum": ["sequential", "parallel"] },
            "allowed_tools": { "type": "array", "items": { "type": "string" } }
          },
          "additionalProperties": false
        },
        "inputs": { "type": "object" },
        "outputs": { "type": "object" },
        "on_error": {
          "type": "object",
          "properties": {
            "strategy": { "enum": ["goto", "fail", "skip", "retry"] },
            "next_node": { "type": "string" }
          },
          "additionalProperties": false
        }
      },
      "additionalProperties": false
    },
    "edge": {
      "type": "object",
      "required": ["from", "to"],
      "properties": {
        "from": { "type": "string" },
        "to": { "type": "string" },
        "priority": { "type": "integer", "minimum": 0, "maximum": 1000 },
        "when": { "type": "string" }
      },
      "additionalProperties": false
    },
    "hook": {
      "type": "object",
      "required": ["id", "phase", "target", "action"],
      "properties": {
        "id": { "type": "string" },
        "phase": { "enum": ["before_node", "after_node", "before_pipeline", "after_pipeline"] },
        "target": { "type": "string" },
        "action": { "type": "string" },
        "enabled": { "type": "boolean" }
      },
      "additionalProperties": false
    }
  },
  "additionalProperties": false
}
```

## Lint 规则

- 必须存在且仅存在一个 `start` 节点。
- 必须至少存在一个 `end` 节点。
- 不允许孤立节点。
- `on_error.next_node` 必须指向存在节点。
- `execution.mode = parallel` 的节点必须通过策略层检查。

## 与现有任务的对应

- 对应 `20-editable-agent-pipeline.md` 中 `PipelineSpec` schema 任务。

