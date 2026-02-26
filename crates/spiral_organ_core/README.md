# spiral_organ_core

`spiral_organ_core` 是 SpiralOrgan 的 Rust runtime。当前主执行路径已切到「角色化 pipeline」：

- `agent_role_pipeline` crate 提供独立、可复用的 role pipeline 类型、lint、runner 与 trait。
- `spiral_organ_core` 在 `submit_task` 时加载 YAML pipeline，并按 scheduler -> worker -> verify 流程执行。

## 分层映射

参考 `.research` 中三项目综合逻辑:

- 编排层 (`gastown` 对应): `Orchestrator`, `Planner`, `Worker`, `Supervisor`, `PipelineEngine`
- 网关层 (`openclaw` 对应): `ChannelAdapter`, `NotificationSink`, `PolicyEngine`
- 执行层 (`opencode` 对应): `SessionEngine`, `Provider`, `Tool`, `ChangeStore`

新增：

- 角色化编排层：`agent_role_pipeline`（独立 crate，可被 `oneday_server` 复用）

## 目录

- `src/domain.rs`: 领域模型
- `../agent_role_pipeline`: 角色化 pipeline 抽象（可独立复用）
- `src/traits/`: 核心 trait 边界（按功能拆分）
  - `execution.rs`: Provider/Tool/SessionEngine/RuntimeAdapter
  - `gateway.rs`: ChannelAdapter/NotificationSink
  - `orchestration.rs`: Planner/Worker/Supervisor/Orchestrator/PipelineEngine
  - `policy.rs`: PolicyEngine
  - `storage.rs`: MemoryStore/GhostRegistry/ChangeStore
- `src/implementations/in_memory.rs`: 内存型参考实现
- `src/implementations/noop.rs`: Noop/基线实现
- `src/services/`: 参照 `.research` 的编排/网关/执行服务骨架
- `src/error.rs`: 统一错误类型

## 运行

在 `SpiralOrgan` 根目录执行：

```bash
cargo run -p spiral_organ_core -- help
cargo run -p spiral_organ_core -- run
cargo run -p spiral_organ_core -- tui
```

- `run`: 执行一次 demo orchestrator + execution + gateway 流程
- `tui`: 启动 `iocraft` fullscreen 终端 UI（`Ctrl+C` 退出）

## 下一步建议

- 增加 `pipeline lint` 真实实现，按 `.design/todo/22-pipeline-lint-and-error-codes.md` 错误码落地。
- 增加 `policy validate`，对接 `.design/specs/policy.schema.json`。
- 增加 `kernel-api` 与 `change-api` 的 axum 路由骨架。
