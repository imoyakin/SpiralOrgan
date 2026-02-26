# T-12 Kernel Embed And Companion Mode

## 目标

为未来“Rust 内核直接集成到 Flutter 客户端”预留双形态运行能力:

- Remote Mode: 连接远程服务器内核
- Embedded Mode: 本地内核随客户端运行

并支持桌宠、截图、自动化任务等能力扩展。

## 运行模式

- `remote`:
  - Flutter 连接远程 Rust API
  - 适合重负载编译和集中运维
- `embedded`:
  - Rust Core 作为本地 sidecar/lib
  - 适合离线桌面、桌宠、自动化伴随

## Companion 能力

- Desktop pet UI (ghost persona)
- Screenshot ingestion
- Automated UI macro flow
- Local notification and action shortcuts

## TODO

- [ ] 定义 `KernelConnectionMode` 配置。
- [ ] 定义 remote/embedded 统一抽象层。
- [ ] 定义桌宠能力开关和权限边界。
- [ ] 定义截图输入到 pipeline 的标准事件。
- [ ] 定义自动化动作的审批与回放机制。

## 验收标准

- 相同 Flutter UI 能无缝切换 remote/embedded 模式。
- companion 能力可独立开关并受策略层约束。
- 自动化动作有完整审计和回放记录。

