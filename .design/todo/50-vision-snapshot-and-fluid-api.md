# T-07 Vision Snapshot And Fluid API

## 目标

为“非 token 优先”的未来推理路径预留接口:

- 支持代码快照/白纸黑字图像输入。
- 支持与传统 token 路径并行验证。
- 为“流体神经网络式处理”预留协议层。

## 设计原则

- 先做接口，不强绑具体模型实现。
- 先做双路推理对照，再决定默认路由。
- 图像输入必须有质量与来源校验。

## API 草案

- `vision.snapshot.ingest(image, metadata)`
- `vision.snapshot.segment(task_id)`
- `vision.snapshot.reason(task_id, mode)`
- `vision.snapshot.compare(task_id, token_result)`

说明:

- `mode` 可取 `token_assisted`、`vision_only`、`hybrid`。

## TODO

- [ ] 定义 `SnapshotEnvelope` 数据结构。
- [ ] 定义图像质量检查 (清晰度、裁切、分辨率)。
- [ ] 定义 OCR/结构解析与语义块映射。
- [ ] 定义双路对照评估指标 (一致性、正确率、耗时)。
- [ ] 预留 `fluid_engine` 插件接口，不绑定实现。

## 验收标准

- 可以对同一代码任务同时执行 token 路径与 snapshot 路径并比较。
- API 能在不改主循环的前提下替换具体视觉推理后端。
- 图像来源和处理链路可审计。

## 风险与回退

- 风险: 视觉路径在复杂代码场景中误读。
- 回退: 默认使用 token 主路，视觉路径仅作为实验分支。

