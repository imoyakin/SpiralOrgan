# T-08 WorldDefine ECS Migration

## 目标

将当前早期 pipeline (参考 oneday_server / izanamiNexus) 逐步迁移为 WorldDefine ECS 管理模型。

## 迁移愿景

- 把 pipeline 的节点、任务、资源、策略都映射为 ECS 实体/组件/系统。
- 把执行过程可视化为“可运行世界”，支持调试、回放、仿真。
- 统一“流程执行”和“世界状态演化”。

## 当前到目标

- 当前: 传统服务式 pipeline 编排。
- 目标: ECS 驱动编排 + 可视化状态机 + 可回放。

## ECS 映射草案

- Entity:
  - `TaskEntity`
  - `AgentEntity`
  - `MemoryEntity`
  - `PolicyEntity`
  - `ChannelEntity`
- Component:
  - `StateComponent`
  - `PriorityComponent`
  - `RiskComponent`
  - `ContextComponent`
  - `ExecutionComponent`
- System:
  - `PlanningSystem`
  - `DispatchSystem`
  - `VerificationSystem`
  - `PolicyEnforcementSystem`
  - `PersistenceSystem`

## TODO

- [ ] 提炼最小 ECS 词表与映射规范。
- [ ] 实现 pipeline -> ECS 编译层 (过渡适配器)。
- [ ] 实现 ECS 运行态快照与回放。
- [ ] 实现 ECS 调试视图 (最小可视化)。
- [ ] 将默认 pipeline 迁移为 ECS 执行后端。

## 验收标准

- 同一任务可在“传统 pipeline”和“ECS pipeline”两后端运行并对比。
- ECS 后端可回放关键步骤与策略决策。
- 迁移不破坏角色系统与 ghost 差异化激活。

## 风险与回退

- 风险: ECS 抽象过重影响初期交付速度。
- 回退: 双后端并行维护，按模块逐步切换。

