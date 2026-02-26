# Code Change Data Model Spec (v0.1)

## 目标

定义代码变更展示的数据模型，支持 Flutter 端 `raw|patch|diff` 三视图。

## ChangedFile

```json
{
  "path": "src/app.rs",
  "status": "modified",
  "additions": 12,
  "deletions": 4,
  "agent_id": "worker_01",
  "task_id": "task_88",
  "approval_id": "apr_11"
}
```

## Hunk

```json
{
  "old_start": 20,
  "old_count": 5,
  "new_start": 20,
  "new_count": 9,
  "header": "@@ fn run_loop @@",
  "lines": [
    { "type": "context", "text": "let mut i = 0;" },
    { "type": "remove", "text": "while true {" },
    { "type": "add", "text": "while i < max_iterations {" }
  ]
}
```

## PatchMeta

```json
{
  "patch_id": "pch_1001",
  "project_id": "spiralorgan",
  "session_id": "sess_01",
  "task_id": "task_88",
  "generated_by": "worker_01",
  "generated_at": "2026-02-22T10:00:00Z",
  "base_commit": "abc123",
  "head_commit": "def456",
  "files": ["src/app.rs", "Cargo.toml"]
}
```

## 视图接口返回

- `view=raw`:
  - `content`: 文件全文
- `view=patch`:
  - `content`: unified patch 字符串
- `view=diff`:
  - `content`: 结构化 hunks

## 过滤建议

- `task_id`
- `agent_id`
- `path_prefix`
- `status`

