# T-03 Role And Ghost System

## 目标

保留并强化“多角色协作 + `ghost.md` 激活差异”能力，使其成为系统核心而非装饰层。

## 范围

- 角色: 甲方、施工方、设计方、监理。
- Ghost: 每个角色支持独立 `ghost.md`、独立记忆空间、独立策略限制。

## 设计要点

- Role 与 Agent 解耦: 角色是职责，agent 是执行实体。
- Ghost 必载: 无 ghost 不允许激活 agent。
- Ghost 冲突处理: 优先级链 `session override > role default > system baseline`。
- Memory 隔离: ghost 级命名空间，避免跨角色污染。

## TODO

- [ ] 定义 `RoleProfile` 与 `GhostProfile` 数据模型。
- [ ] 定义 `ghost.md` 必填字段与校验规则。
- [ ] 实现 ghost 加载器与热更新机制。
- [ ] 实现角色到 agent 的绑定器与调度映射。
- [ ] 实现角色级权限覆盖与审计标注。
- [ ] 补充“同任务多角色分工”模板。

## 验收标准

- 同一任务可看到 4 角色的独立输出与审计轨迹。
- 更换 `ghost.md` 后行为差异可被重复验证。
- 不同角色 memory 检索不会串线。

## 风险与回退

- 风险: ghost 文档质量差导致行为漂移。
- 回退: 角色可回落到内建 baseline ghost。

