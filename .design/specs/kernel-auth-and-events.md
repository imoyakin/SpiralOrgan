# Kernel Auth And Events Spec (v0.1)

## 目标

定义 Flutter 客户端与 Rust Kernel 的认证和事件推送协议。

## 认证流程

1. 客户端请求配对码:
   - `POST /kernel/auth/pair/request`
2. 服务端生成短时 `pair_code` 和 `pair_id`。
3. 操作员在可信端确认配对:
   - `POST /kernel/auth/pair/confirm`
4. 客户端兑换会话令牌:
   - `POST /kernel/auth/token/exchange`
5. 获取:
   - `access_token` (短期)
   - `refresh_token` (长期)

## Token 规则

- `access_token` 默认 30 分钟。
- `refresh_token` 默认 7 天。
- 绑定设备指纹与项目范围。
- 风险动作需二次确认 token。

## 事件总线

WebSocket:

- `GET /kernel/ws?token=<access_token>`

SSE (降级):

- `GET /kernel/events?token=<access_token>`

## 事件类型

- `task.progress`
- `task.done`
- `task.idle_waiting`
- `task.need_approval`
- `task.error`
- `build.started`
- `build.finished`
- `deploy.started`
- `deploy.finished`

## 事件结构

```json
{
  "event_id": "evt_01",
  "event_type": "task.done",
  "session_id": "sess_01",
  "task_id": "task_88",
  "timestamp": "2026-02-22T10:00:00Z",
  "payload": {
    "summary": "all checks passed",
    "changed_files": 12
  }
}
```

## 安全约束

- token 必须支持撤销与过期轮转。
- websocket 连接断开后重连要带最后事件游标。
- `task.need_approval` 事件必须带审批上下文，不带敏感密钥。

