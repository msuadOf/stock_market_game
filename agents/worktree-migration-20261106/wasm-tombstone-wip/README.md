# WASM tombstone 未完成原型归档

来源工作树：`.worktree/main-merge-2026-10-25`；来源 HEAD：
`bd799496f3f5a361f88343fbec164e536b64b6a4`。这里保存该工作树的未提交内容，
供后续迁移与审查，不参与编译。

- `wasm_tombstone.rs.txt` 和 `wasm_tombstone_tests.rs.txt` 分别是来源
  `packages/engine/src/wasm_tombstone.rs` 与 `wasm_tombstone_tests.rs` 的原始字节副本。
- `lib.rs.txt` 是来源 `packages/engine/src/lib.rs` 的原始字节副本；其相对上述 HEAD
  的差异是添加 `wasm_tombstone` 模块声明。后续恢复时应先比对目标基线与上下文，
  再审查并迁移该差异及两个源码副本。

这只是未接入产品的原型：panic hook、身份 admission、死亡唤醒和 join 仲裁均未接线；
没有产品验收，不能据此宣称生产功能完成。归档本身不改变交易规则或运行行为；
后续接入前仍须独立验证语义、边界、测试和宿主集成。
