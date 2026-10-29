# RuntimeDelta 测试 DTO 修复

- 日期：2026-10-30
- 范围：`packages/engine/tests/protocol_runtime_delta.rs`
- 原因：现行 `RuntimeDelta` 要求 `mirror_origin`，而测试 wire fixture 缺少该字段，导致反序列化在到达协议校验前失败。
- 修复：wire fixture 显式提供 `RuntimeMirrorOrigin` 的七个字段；游标不匹配测试断言 `RuntimeDeltaMismatch`；新增缺少 `mirror_origin` 时 JSON 反序列化失败的控制用例。
- 验证：`git diff --check` 已执行；按调度要求未运行 Cargo、测试、构建或生成工具。
