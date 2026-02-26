# SpiralOrgan TODO Index

本目录将你的“精神纲领”拆解为可执行任务，按阶段推进。

## 使用方式

- 每个主题独立文件，避免单文档失控。
- 每个文件包含: 目标、范围、TODO、验收、风险。
- 默认先完成 P0，再进入 P1/P2。

## 任务地图

| ID | 主题 | 优先级 | 文件 |
|---|---|---|---|
| T-01 | 精神纲领总述 | P0 | `01-manifesto.md` |
| T-02 | 架构原则与约束 | P0 | `02-architecture-principles.md` |
| T-03 | 角色系统与 ghost 激活差异 | P0 | `10-role-and-ghost-system.md` |
| T-04 | 可编辑 Agent Pipeline | P0 | `20-editable-agent-pipeline.md` |
| T-04A | PipelineSpec 配置契约草案 | P0 | `21-pipeline-spec-schema.md` |
| T-04B | Pipeline Lint 与错误码 | P0 | `22-pipeline-lint-and-error-codes.md` |
| T-05 | 能力开关与策略治理 | P0 | `30-feature-switch-and-policy.md` |
| T-05A | Feature/Policy 配置契约草案 | P0 | `31-feature-policy-schema.md` |
| T-10 | Flutter 远程内核与任务通知 | P0 | `80-flutter-remote-kernel-notify.md` |
| T-10A | Flutter/Kernel 认证与事件协议草案 | P0 | `.design/specs/kernel-auth-and-events.md` |
| T-11 | 代码变更可视化 (参考 opencode) | P0 | `81-code-change-visibility-opencode.md` |
| T-11A | 代码变更数据模型草案 | P0 | `.design/specs/change-model.md` |
| T-12 | 内核嵌入与 companion 模式 | P1 | `82-kernel-embed-companion-mode.md` |
| T-06 | Token 流接管与模型提纯 | P1 | `40-token-stream-distillation.md` |
| T-07 | 图像快照输入与流体神经 API 预留 | P1 | `50-vision-snapshot-and-fluid-api.md` |
| T-08 | WorldDefine ECS 引擎迁移路线 | P1 | `60-worlddefine-ecs-migration.md` |
| T-09 | 分期交付与质量门禁 | P0 | `70-delivery-phases-and-gates.md` |

## 阶段目标

- Phase A (P0): 先把“可运行且可控”的系统立起来。
- Phase B (P1): 打开提纯与视觉输入新能力。
- Phase C (P2): 迁移到 WorldDefine ECS 原生管理形态。

## 当前状态

- [ ] T-01
- [ ] T-02
- [ ] T-03
- [ ] T-04
- [x] T-04A
- [ ] T-04B
- [ ] T-05
- [x] T-05A
- [ ] T-10
- [x] T-10A
- [ ] T-11
- [x] T-11A
- [ ] T-12
- [ ] T-06
- [ ] T-07
- [ ] T-08
- [ ] T-09
