# T-06 Token Stream Distillation

## 目标

在“统一 AI 执行”之外，引入 token 流接管能力，把系统升级为“模型提纯工具”。

## 核心思路

- 接管模型输入输出 token 流。
- 建立流式过滤、重写、压缩、对齐规则。
- 将有效片段沉淀为可复用蒸馏样本和策略。

## Distill Pipeline

1. Capture: 捕获 prompt/response/tool-call 流。
2. Normalize: 去噪、脱敏、标准化。
3. Classify: 按任务类型和质量标签打标。
4. Refine: 规则提纯与反模式过滤。
5. Persist: 样本与指标入库。
6. Replay: 用于后续训练或策略回放评估。

## TODO

- [ ] 定义 `TokenEvent` 统一事件格式。
- [ ] 定义脱敏与合规过滤规则。
- [ ] 定义质量标签体系 (correctness, cost, latency, safety)。
- [ ] 实现 `distill replay` 回放验证工具。
- [ ] 输出每轮提纯报告与可视化指标。

## 验收标准

- 可以对单次任务导出完整可回放 token 轨迹。
- 能自动生成“可用样本包”和“风险样本包”。
- 提纯策略修改后可通过回放验证收益。

## 风险与回退

- 风险: 捕获过量明文导致隐私风险。
- 回退: 默认只存脱敏摘要，原始流按策略可禁用。

