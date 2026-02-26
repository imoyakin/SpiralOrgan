# T-05 Feature Switch And Policy

## 目标

把你提到的所有“增强能力”做成可配置开关，并挂到统一策略层。

## 开关清单

- `features.channels.enabled`
- `features.plugins.enabled`
- `features.sandbox.enabled`
- `features.pipeline_edit.enabled`
- `features.token_distill.enabled`
- `features.vision_snapshot.enabled`
- `features.worlddefine_bridge.enabled`

## 策略层清单

- 风险分级: low/medium/high/critical。
- 执行模式: readonly/supervised/full。
- 审批规则: 高风险必须 HITL。
- 速率限制: channel/user/session 三层。
- 路径与命令白名单。

## TODO

- [x] 定义 `FeatureFlags` 配置模型。
- [x] 定义 `PolicyRules` 配置模型。
- [x] 产出官方配置模板 (`policy.default.toml`/`policy.dev.toml`)。
- [ ] 实现启动时配置校验和冲突检测。
- [ ] 实现运行时热更新与回滚。
- [ ] 为每个能力开关加审计事件。

## 验收标准

- 任一能力可动态开关，且行为变化可审计。
- 高风险功能在 `supervised` 下必须审批才能通过。
- 配置冲突时系统拒绝启动并给出可读错误。

## 风险与回退

- 风险: 开关组合过多导致配置复杂度上升。
- 回退: 提供官方“安全默认配置”与“开发配置”两套模板。
