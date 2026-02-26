# Specs Directory

本目录存放可执行配置样例，供后续 Rust/Flutter 实现直接读取。

## 文件说明

- `pipeline.default.toml`
  - 默认主流程模板
  - 对应 `T-04`

- `pipeline.role.default.yaml`
  - 角色化 agent loop 默认模板（scheduler + worker pool）
  - 供 `agent_role_pipeline` crate 与 `spiral_organ_core` runtime 直接加载

- `pipeline.local.example.toml`
  - 用户本地覆盖示例
  - 对应 `T-04`

- `policy.default.toml`
  - 安全默认策略模板
  - 对应 `T-05`

- `policy.dev.toml`
  - 开发模式策略模板
  - 对应 `T-05`

- `kernel-auth-and-events.md`
  - Flutter 与 Kernel 认证和事件协议
  - 对应 `T-10`

- `change-model.md`
  - 代码变更模型与 raw/patch/diff 返回契约
  - 对应 `T-11`

- `pipeline.schema.json`
  - Pipeline 配置 JSON Schema
  - 对应 `T-04A`

- `policy.schema.json`
  - Feature/Policy 配置 JSON Schema
  - 对应 `T-05A`

- `kernel-api.openapi.yaml`
  - Flutter 连接远程 Rust Kernel 的 API 草案
  - 对应 `T-10`

- `change-api.openapi.yaml`
  - 代码变更展示 API 草案
  - 对应 `T-11`

## 约束

- 配置格式以 `21-pipeline-spec-schema.md` 和 `31-feature-policy-schema.md` 为准。
- 所有模板变更都应通过 `pipeline lint` 与 `policy validate`（待实现）。
