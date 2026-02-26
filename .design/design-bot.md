# design-bot

## 1. 文档定位

本文件是 `design.md` 的执行版蓝图，面向实现与运维，不替代原始设计愿景。

目标是把系统做成可持续运行的 AI 工程中枢:

- 围绕 `project/target` 持续推进 Kanban
- 多 agent 协作执行与审查
- 记忆可复用、可审计、可隔离
- 高风险能力默认受控

---

## 2. 核心原则

- 正确性和可维护性优先于性能。
- 外部状态优先: 看板、记忆、审批、日志都落地，不依赖模型短期记忆。
- 安全默认拒绝: 渠道、工具、文件、命令全部 deny-by-default。
- 架构可替换: provider/channel/memory/tool/runtime/security 都用 trait 抽象。
- 可中断可恢复: 任意任务必须支持暂停、取消、继续。

---

## 3. 总体架构

### 3.1 控制平面 (Rust)

- Orchestrator: 读取 target, 维护主循环, 调度多 agent
- Planner: 拆解需求为 Todo + Kanban 子任务
- Worker: 执行代码/脚本/UI/网络任务
- Supervisor: 验证结果与事实对齐
- Policy Engine: 权限、审批、风险评分、速率限制
- Extension Host: 加载插件、技能、渠道适配器

### 3.2 数据平面

- Structured Store: 任务、会话、审批、审计事件
- Memory Store:
  - Vector backend (Milvus 优先, trait 可替换)
  - Markdown evidence 文档 (标题/tag/摘要/来源/截图)
- Identity/Ghost Store:
  - `ghost.md` 必载
  - 每个 ghost 独立 memory 命名空间

### 3.3 交互平面

- Flutter UI: 运营面板与看板管理
- Flutter UI: 远程连接 Rust 内核服务器，执行/编译/部署项目
- Flutter UI: 任务完成与空闲等待指令通知中心
- Flutter UI: 代码变更查看 (file status + raw/patch/diff)
- Rust TUI: 工程态调试与值班
- Channels: Telegram/Discord/Slack/... 统一接入层

---

## 4. 关键领域对象

- `TargetSession`: 某次持续执行目标的生命周期容器
- `KanbanBoard`: 全局看板 + 项目子看板 + 依赖关系
- `TaskCard`: 状态、负责人、验收标准、证据链接
- `MemoryEntry`: UUID、tag、score、source、vector_ref、doc_ref
- `ApprovalRecord`: 风险动作审批链与决策
- `AuditEvent`: 安全与执行事件日志 (可检索、可导出)

---

## 5. 渠道接入设计 (参考 ZeroClaw 强化)

### 5.1 统一接入策略

- 每个渠道独立配置段: `channels_config.<channel>`
- 统一 sender allowlist 语义:
  - 空列表: 拒绝全部入站
  - `"*"`: 允许全部 (仅用于临时验证)
  - 显式列表: 精确匹配
- 不同渠道字段可不同，但语义一致:
  - `allowed_users`, `allowed_from`, `allowed_numbers`, `allowed_senders`, `allowed_contacts`, `allowed_pubkeys`

### 5.2 传输模式矩阵

- 轮询/WS 模式: 内网即可运行
- Webhook 模式: 必须公网 HTTPS 回调 + 签名校验
- 网关默认 `127.0.0.1` 绑定，公开监听需显式开启并通过隧道

### 5.3 渠道入网流程

1. 初始 deny-all 启动。
2. 用户首次消息触发“未授权提示 + 可复制绑定命令”。
3. 操作员在本机执行 bind，写入 allowlist。
4. 切回精确白名单，禁止长期 `"*"`。

### 5.4 会话隔离

- 私聊: 用户级独立记忆
- 群聊: 群级共享记忆
- 同 sender 可切换模型，但仅影响该 sender 的会话缓存

---

## 6. 插件与能力扩展 (参考 ZeroClaw trait + registry)

### 6.1 可插拔边界

- `Provider` (模型/推理服务)
- `Channel` (消息渠道)
- `Memory` (向量与文档存储)
- `Tool` (执行能力)
- `RuntimeAdapter` (native/docker/未来 wasm)
- `SecurityPolicy` (审计/路径/命令/审批)

### 6.2 插件包建议规范

- `PLUGIN.toml`: 元数据、版本、权限声明、依赖
- `SKILL.md`/`ghost.md`: 行为与提示词层约束
- 可选 `scripts/`、`assets/` 目录
- 安装前静态审计:
  - 禁止 symlink
  - 禁止脚本型危险载荷
  - 阻断高风险 shell payload
  - 阻断 markdown 越权链接

### 6.3 工具注册流程

1. 实现 `Tool` trait。
2. 注册到工具注册表 (agent/tool registry)。
3. 生成工具描述供 LLM 调用。
4. 在策略层声明风险等级与审批策略。

---

## 7. 沙箱与运行时隔离

### 7.1 运行时层

- `runtime.kind = native | docker`
- Docker 模式默认:
  - `network = none`
  - `read_only_rootfs = true`
  - `memory_limit_mb` / `cpu_limit`
  - workspace 挂载白名单校验

### 7.2 安全后端抽象

- `security.sandbox.backend = auto | landlock | firejail | bubblewrap | docker | none`
- 运行时自动探测最佳后端，失败降级到应用层防护，不可 silent fail

### 7.3 应用层兜底 (即使无 OS sandbox 也必须开启)

- workspace scope 校验
- forbidden path 阻断
- command allowlist
- 参数注入防护

---

## 8. 安全基线配置

### 8.1 网关安全

- 默认 localhost 绑定
- pairing code 首次认证
- webhook Bearer token 与幂等键支持
- 公网暴露需显式配置与 tunnel

### 8.2 最小权限

- `workspace_only = true` 默认开启
- `allowed_commands` 显式白名单
- `forbidden_paths` 内置系统敏感目录
- 高风险工具走审批器

### 8.3 密钥与敏感信息

- secrets 默认加密落盘
- 日志输出进行 credential scrub
- 渠道回调签名校验优先开启

### 8.4 可观测与审计

- 关键事件: llm_request/response/tool_call/approval/denied/cancelled
- 支持 JSON/CSV 导出，便于后续 SIEM 对接

---

## 9. Agent Loop 合同 (实现级)

### 9.1 循环不变量

- 每轮开始前 `history` 包含当前完整上下文 (system + user + tool results)
- 退出条件:
  - 无工具调用 -> 返回最终文本
  - 达到 `max_tool_iterations`
  - 收到 cancellation token

### 9.2 单轮执行步骤

1. 读取 cancellation token。
2. 进行 provider capability 检查 (如 vision 能力)。
3. 发起 LLM 请求并解析响应。
4. 解析多格式工具调用 (native/json/xml/markdown 等)。
5. 进行工具调用去重 (name + args 签名)。
6. 审批判定:
   - 有审批要求 -> 顺序执行
   - 无审批要求且多调用 -> 并行执行
7. 回写 tool results 到 history。
8. 若无工具调用，流式输出最终答案并结束。

### 9.3 建议配置项

- `max_tool_iterations`
- `max_history_messages`
- `history_compaction.enabled`
- `history_compaction.keep_recent`
- `approval.mode = readonly | supervised | full`
- `channels.<name>.interrupt_on_new_message`

---

## 10. ZeroClaw Agent Loop vs 当前设计

### 10.1 相同点

- 都是“目标驱动 + 多轮循环”而非单轮对话。
- 都强调工具执行与结果回写。
- 都把记忆与上下文外部化，支持持续运行。
- 都有角色协作思路 (worker/supervisor 类分工)。

### 10.2 你当前设计尚缺的实现细节

- 缺少显式 loop invariant 与退出条件合同。
- 缺少 cancel token 的全链路中断协议。
- 缺少多格式 tool-call 解析与异常恢复策略。
- 缺少“重复工具调用去重”与幂等策略。
- 缺少“审批要求下禁止并行”的硬规则。
- 缺少 history compaction 触发阈值和摘要约束。
- 缺少敏感信息 scrub 与结构化 runtime trace 事件规范。

### 10.3 对你设计的具体升级建议

1. 增加 `run_tool_call_loop` 的接口合同文档与测试矩阵。
2. 把审批、并行、取消做成三条独立策略，不和工具实现耦合。
3. 先实现应用层安全基线，再逐步引入 OS sandbox 后端。
4. 渠道统一采用 deny-by-default + operator bind 流程。
5. 插件安装默认执行静态安全审计，再允许启用。

### 10.4 Flutter 与远程内核补充

1. 采用 client/server 结构: Flutter 是控制客户端，Rust 是执行内核。
2. 任务结束且无待办时，主动推送“完成并等待指示”通知。
3. 参考 opencode 的多项目会话与 `raw|patch` 文件接口，提供变更可视化。
4. 预留 `remote` 与 `embedded` 双模式，后续支持桌宠/截图/自动化。

---

## 11. 分期实施 (更新)

- Phase 1:
  - Kanban + 基础 agent loop + 记忆双写
  - 最小安全基线 (allowlist/path/command/secrets)
  - Telegram 首通 + deny-by-default
- Phase 2:
  - 审批器 + 并行执行策略 + cancel token
  - 历史压缩与可观测事件
  - Docker runtime 与资源限制
- Phase 3:
  - 插件市场与签名/审计机制
  - 多渠道统一治理台
  - pipeline 可视化编排

---

## 12. 验收标准

- 单个 target 可持续运行并可安全中断恢复。
- 任意高风险动作都有审批与审计轨迹。
- 渠道默认拒绝，显式授权后才可执行高权限动作。
- 插件/工具新增不破坏主循环契约。
- agent loop 在异常输入下可恢复，不出现失控死循环。
