# T-10 Flutter Remote Kernel And Notify

## 目标

实现 Flutter 控制台连接远程 Rust 内核，实现“远程执行 + 任务完成通知 + 等待指令”。

## 设计背景

- 你希望 Flutter 作为可视化控制面。
- Rust 内核可先跑在服务器，后续可嵌入本机/端侧。
- 空闲时主动通知用户“已完成，等待下一步”。

## 架构草案

- Flutter App:
  - 会话管理
  - 任务面板
  - 通知中心
  - 变更查看入口
- Rust Kernel Server:
  - 项目执行 API
  - 编译部署 API
  - 会话/任务状态流
  - 审计与权限网关

## API Contract (最小集合)

- `POST /kernel/session/open`
- `POST /kernel/task/submit`
- `POST /kernel/task/abort`
- `GET /kernel/task/:id/status`
- `GET /kernel/task/:id/events`
- `POST /kernel/deploy`

事件推送:

- WebSocket: `task.progress`, `task.done`, `task.idle_waiting`, `task.error`

## 通知策略

- 触发条件:
  - `task.done`
  - `task.idle_waiting` (无待办)
  - `task.need_approval`
- 通知内容:
  - 任务摘要
  - 变更摘要
  - 下一步建议动作

## TODO

- [x] 定义 Flutter <-> Kernel 的认证协议 (token + pairing)。
- [ ] 实现任务状态事件流订阅。
- [ ] 实现系统通知桥接 (iOS/Android/Desktop)。
- [ ] 实现“空闲等待指令”状态和 UI 卡片。
- [ ] 实现通知点击回跳到任务详情页。

## 验收标准

- 远程任务执行与编译部署可在 Flutter 端观察全过程。
- 任务完成和空闲状态会触发可点击通知。
- 通知点击后可直接回到对应任务和变更详情。
