# 三项目业务逻辑综合对照

## 1. 系统角色分工

- `gastown`: 工程流程编排与任务队列治理。
- `openclaw`: 多渠道消息网关与远程控制面。
- `opencode`: 编码执行引擎与会话化 API 平台。

## 2. 端到端能力对照图

```mermaid
flowchart LR
    Human[Human or Automation] --> GT[gastown orchestration]
    Human --> OC[openclaw gateway]
    Human --> OP[opencode execution]

    GT --> GTQ[bead convoy workflow]
    GTQ --> GTA[polecat workers]

    OC --> OCM[channel ingress and egress]
    OCM --> OCA[agent routing]

    OP --> OPS[session processor]
    OPS --> OPT[tool registry]

    GTA -. can invoke .-> OP
    OCA -. can invoke .-> OP
```

## 3. 业务抽象层统一视角

```mermaid
flowchart TB
    L1[Entry Layer] --> L2[Routing Layer]
    L2 --> L3[Execution Layer]
    L3 --> L4[Persistence Layer]
    L3 --> L5[Recovery and Policy Layer]

    L1 --> E1[gastown commands]
    L1 --> E2[openclaw commands and gateway]
    L1 --> E3[opencode cli and api]

    L2 --> R1[issue to rig]
    L2 --> R2[channel to agent session key]
    L2 --> R3[directory to project session]

    L3 --> X1[agent runtime and tmux]
    L3 --> X2[gateway handlers and outbound adapters]
    L3 --> X3[session processor and tools]

    L4 --> P1[beads and git]
    L4 --> P2[config and session store]
    L4 --> P3[sqlite project and session]

    L5 --> S1[daemon deacon patrol]
    L5 --> S2[scope auth and send policy]
    L5 --> S3[permission rules and retries]
```

## 4. 可协同模式建议

- 编排优先: 用 `gastown` 作为任务编排入口，`opencode` 作为执行引擎。
- 渠道优先: 用 `openclaw` 作为统一消息入口，把渠道指令路由到 `opencode` 会话执行。
- 运维闭环: 让 `gastown` 的 convoy 和 `openclaw/opencode` 的会话事件汇总到统一观察面。

## 5. 结论

三者并不是同类替代关系，而是可组合关系：

- `gastown` 管“任务编排与落地节奏”。
- `openclaw` 管“消息入口与控制平面”。
- `opencode` 管“会话执行与工具链能力”。
