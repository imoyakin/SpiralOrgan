# FeatureFlags And PolicyRules Schema Draft (v0.1)

## 目标

定义能力开关与策略治理的统一配置契约，保证:

- 能力可开关
- 风险可分级
- 审批可执行
- 审计可追踪

## 顶层结构

```yaml
api_version: "spiralorgan.policy/v0.1"
mode: "supervised" # readonly | supervised | full
features: {}
policy: {}
audit: {}
```

## FeatureFlags 草案

```yaml
features:
  channels:
    enabled: true
  plugins:
    enabled: true
  sandbox:
    enabled: true
    backend: "auto" # auto | docker | landlock | firejail | bubblewrap | none
  pipeline_edit:
    enabled: true
  token_distill:
    enabled: false
  vision_snapshot:
    enabled: false
  worlddefine_bridge:
    enabled: false
```

## PolicyRules 草案

```yaml
policy:
  workspace_only: true
  allowed_commands: ["git", "cargo", "npm", "pnpm", "ls", "cat", "rg"]
  forbidden_paths: ["/etc", "/root", "/proc", "/sys", "~/.ssh", "~/.gnupg", "~/.aws"]
  risk_matrix:
    shell_exec: "high"
    file_write_outside_workspace: "critical"
    browser_remote_control: "high"
    channel_bind: "medium"
  enforcement:
    low: "allow"      # allow | approve | deny
    medium: "approve"
    high: "approve"
    critical: "deny"
  approval:
    required_for: ["medium", "high"]
    channel_mode:
      cli: "interactive"
      telegram: "operator_only"
      webhook: "token_and_operator"
  rate_limits:
    per_user_per_minute: 20
    per_channel_per_minute: 120
    per_session_parallel_actions: 4
```

## JSON Schema 草案

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "spiralorgan.policy.v0.1",
  "type": "object",
  "required": ["api_version", "mode", "features", "policy"],
  "properties": {
    "api_version": { "const": "spiralorgan.policy/v0.1" },
    "mode": { "enum": ["readonly", "supervised", "full"] },
    "features": {
      "type": "object",
      "required": ["channels", "plugins", "sandbox", "pipeline_edit", "token_distill", "vision_snapshot", "worlddefine_bridge"],
      "properties": {
        "channels": { "$ref": "#/$defs/enabledObj" },
        "plugins": { "$ref": "#/$defs/enabledObj" },
        "sandbox": {
          "type": "object",
          "required": ["enabled", "backend"],
          "properties": {
            "enabled": { "type": "boolean" },
            "backend": { "enum": ["auto", "docker", "landlock", "firejail", "bubblewrap", "none"] }
          },
          "additionalProperties": false
        },
        "pipeline_edit": { "$ref": "#/$defs/enabledObj" },
        "token_distill": { "$ref": "#/$defs/enabledObj" },
        "vision_snapshot": { "$ref": "#/$defs/enabledObj" },
        "worlddefine_bridge": { "$ref": "#/$defs/enabledObj" }
      },
      "additionalProperties": false
    },
    "policy": {
      "type": "object",
      "required": ["workspace_only", "allowed_commands", "forbidden_paths", "enforcement"],
      "properties": {
        "workspace_only": { "type": "boolean" },
        "allowed_commands": { "type": "array", "items": { "type": "string" } },
        "forbidden_paths": { "type": "array", "items": { "type": "string" } },
        "risk_matrix": { "type": "object", "additionalProperties": { "enum": ["low", "medium", "high", "critical"] } },
        "enforcement": {
          "type": "object",
          "required": ["low", "medium", "high", "critical"],
          "properties": {
            "low": { "enum": ["allow", "approve", "deny"] },
            "medium": { "enum": ["allow", "approve", "deny"] },
            "high": { "enum": ["allow", "approve", "deny"] },
            "critical": { "enum": ["allow", "approve", "deny"] }
          },
          "additionalProperties": false
        },
        "approval": {
          "type": "object",
          "properties": {
            "required_for": { "type": "array", "items": { "enum": ["low", "medium", "high", "critical"] } },
            "channel_mode": { "type": "object", "additionalProperties": { "type": "string" } }
          },
          "additionalProperties": false
        },
        "rate_limits": {
          "type": "object",
          "properties": {
            "per_user_per_minute": { "type": "integer", "minimum": 1, "maximum": 10000 },
            "per_channel_per_minute": { "type": "integer", "minimum": 1, "maximum": 100000 },
            "per_session_parallel_actions": { "type": "integer", "minimum": 1, "maximum": 256 }
          },
          "additionalProperties": false
        }
      },
      "additionalProperties": false
    },
    "audit": {
      "type": "object",
      "properties": {
        "enabled": { "type": "boolean" },
        "redact_secrets": { "type": "boolean" },
        "export_format": { "enum": ["jsonl", "json", "csv"] }
      },
      "additionalProperties": false
    }
  },
  "$defs": {
    "enabledObj": {
      "type": "object",
      "required": ["enabled"],
      "properties": {
        "enabled": { "type": "boolean" }
      },
      "additionalProperties": false
    }
  },
  "additionalProperties": false
}
```

## 运行时冲突规则

- `mode = readonly` 时，`allowed_commands` 仅允许读命令白名单。
- `features.pipeline_edit.enabled = true` 但 `mode = readonly` 时，仅允许模拟执行，不落盘。
- `features.token_distill.enabled = true` 且 `audit.redact_secrets = false` 时拒绝启动。
- `features.sandbox.enabled = false` 时必须开启 `workspace_only = true` 和更严格命令白名单。

## 与现有任务的对应

- 对应 `30-feature-switch-and-policy.md` 中 `FeatureFlags` 和 `PolicyRules` schema 任务。

