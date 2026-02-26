# T-04 Editable Agent Pipeline

## 目标

把 agent loop 从“固定代码逻辑”升级为“可编辑 pipeline”:

- 默认使用官方 pipeline。
- 用户可基于 DSL/JSON 修改节点、条件、分支、并行策略。

## 默认 Pipeline (V1)

1. Intake: 读取 target 与上下文。
2. Recall: 检索记忆与历史看板。
3. Plan: 生成/更新 Todo 与任务分解。
4. Dispatch: 分配给多角色 agent。
5. Execute: 工具执行。
6. Verify: 监理校验与事实对齐。
7. Review: 风险动作审批。
8. Commit: 回写任务状态、记忆、审计。
9. Loop: 进入下一轮或结束。

## 用户可编辑能力

- 节点增删改。
- 分支条件表达式。
- 并行/串行策略。
- 失败重试与回退节点。
- 节点前后 hook。

## TODO

- [x] 设计 `PipelineSpec` schema (nodes, edges, conditions, retries, hooks)。
- [ ] 实现 `pipeline lint` 校验器。
- [x] 实现默认模板 `pipeline.default.toml`。
- [x] 实现用户覆盖层 `pipeline.local.toml`。
- [ ] 实现运行时编排器 (支持暂停/恢复/取消)。
- [ ] 输出节点级 trace 与可视化数据。

## 验收标准

- 在不改核心代码前提下，用户可新增至少 2 个自定义节点。
- pipeline 配置错误能被 lint 阶段拦截。
- 失败后可从指定节点恢复，不必整轮重跑。

## 风险与回退

- 风险: 用户 pipeline 设计不当导致死循环。
- 回退: 一键恢复 `pipeline.default.toml`。
