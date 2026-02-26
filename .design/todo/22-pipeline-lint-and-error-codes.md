# T-04B Pipeline Lint And Error Codes

## 目标

定义 Pipeline 配置的静态校验器，保证用户可编辑但不失控。

## 校验分级

- `E*` Error: 阻断启动。
- `W*` Warning: 允许启动但提示风险。

## Error Codes

- `E100`: `api_version` 不匹配。
- `E101`: 缺少 `start` 节点。
- `E102`: 缺少 `end` 节点。
- `E103`: 多个 `start` 节点。
- `E104`: 节点 ID 重复。
- `E105`: edge 指向不存在节点。
- `E106`: 存在孤立节点。
- `E107`: `on_error.next_node` 不存在。
- `E108`: 形成无退出死循环。
- `E109`: 并行节点超过 `max_parallel_nodes`。
- `E110`: 并行节点包含高风险工具且无审批。
- `E111`: 非 `custom` 节点使用未知 `kind`。
- `E112`: timeout 超出策略上限。

## Warning Codes

- `W200`: 使用 `"*"` 风险配置。
- `W201`: 节点 `retry` 次数过高。
- `W202`: 单节点工具权限过宽。
- `W203`: 缺少关键审计 hook。
- `W204`: 条件表达式复杂度过高。

## TODO

- [ ] 实现 `pipeline lint` CLI 命令。
- [ ] 输出结构化错误 (json/text 双格式)。
- [ ] 在 CI 增加 lint gate。
- [ ] 在 UI 显示定位信息 (node/edge/hook)。

## 验收标准

- 错误能准确定位到配置路径。
- `E*` 必阻断，`W*` 可继续但有提示。
- 支持 `--strict` 模式把 `W*` 升级为阻断。

