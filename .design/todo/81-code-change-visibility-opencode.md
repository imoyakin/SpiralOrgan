# T-11 Code Change Visibility (Ref OpenCode)

## 目标

给 Flutter 用户展示“代码变化”而不是只展示文本回复，借鉴 OpenCode 的 client/server 和 patch 文件接口经验。

## 可借鉴点 (来自 `.refer/opencode`)

- client/server 架构，前端只是客户端之一。
- 多项目/多会话模型:
  - `GET /project/:projectID/session`
  - `GET /project/:projectID/session/:sessionID/file`
- 文件内容接口支持 `raw` / `patch` 两种返回形态。

## SpiralOrgan 设计

- 变更来源:
  - git diff
  - 任务阶段产物
  - agent 生成 patch
- 展示形态:
  - 文件树级摘要
  - 行级 diff
  - patch 原文
  - 关联任务与审批记录

## API Contract (建议)

- `GET /project/:project_id/session/:session_id/file/status`
- `GET /project/:project_id/session/:session_id/file?path=...&view=raw|patch|diff`
- `GET /project/:project_id/session/:session_id/changes/summary`
- `POST /project/:project_id/session/:session_id/changes/ack`

## Flutter 端模块

- `ChangeListScreen`: 文件变化列表
- `DiffViewerWidget`: 行级比较视图
- `PatchRawWidget`: patch 文本视图
- `TaskTracePanel`: 关联任务轨迹与审计

## TODO

- [x] 定义变更数据模型 (`ChangedFile`, `Hunk`, `PatchMeta`)。
- [ ] 实现服务端 `raw|patch|diff` 统一接口。
- [ ] 实现 Flutter 行级 diff 组件。
- [ ] 实现“按任务过滤变化”与“按 agent 过滤变化”。
- [ ] 实现“确认已读”与“请求重做”动作。

## 验收标准

- 用户可从任务详情一键查看相关文件的完整变更。
- 每个变更可追溯到 agent、任务、审批决策。
- 变更视图可在移动端和桌面端都可用。
