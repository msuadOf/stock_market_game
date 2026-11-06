# stock_stream 参数结构化实现交接

状态：READY，等待未参与实现者独立复核；本记录不代表复核通过。

## 工作区

- 仓库：`<REPO_ROOT>`
- worktree：`<REPO_ROOT>/.worktree/clippy-cfg-args-stream-2026-10-30`
- 分支：`codex/clippy-cfg-args-stream-20261030`
- 基线：`50d493701aa0fbb7d62563855a84dc07c89c29b0`

## 修改范围

- `packages/engine/src/session/pipeline/stock_stream.rs`
  - 以 `StockStreamInitialBatch` 保持 `initial operations` 与 `expected` 成对传递；驱动内仍按原先顺序安装两组 coordinator 初始队列。
  - 以 `StockTaskInput` 聚合每股任务私有的 `code/shard/operations/expected/feedback`。
  - 以 `StockTaskDelivery` 聚合执行所需的 compute、completion sender、notification sender、派发线程捕获的 bindings 与可选 `spawn_slot`。
  - worker spawn 和协调者内联执行路径使用相同的两组任务值。未改变 spawn 位置、绑定捕获点、完成 payload 发送、完成通知及 `spawn_slot` 显式 drop 顺序。
- `packages/engine/src/session/pipeline/market_tick_transaction.rs`
  - 唯一生产调用点构造 `StockStreamInitialBatch`。

## 证据

- 父任务提供的原始诊断文件：
  `<REPO_ROOT>/agents/performance-delivery/formal-runtime-sync-2026-10-30/clippy-50d493.stderr`
  `<REPO_ROOT>/agents/performance-delivery/formal-runtime-sync-2026-10-30/clippy-50d493-diagnostics.json`
- 原始 Clippy 报告列出 `drive_stock_stream_with_feedback` 8 个参数和 `execute_stock_task` 10 个参数，Clippy 上限为 7。
- 已运行 `git diff --check`，通过。
- 已运行受限 Python 源码结构检查，确认驱动和执行体参数分组、通知调用仍在 `drop(spawn_slot)` 之前、两个旧式执行调用均不存在。
- 未运行 Cargo、Clippy、Rust 测试或构建；父级设定统一排窗且此任务明确禁止 Cargo/target 操作。
- `rustfmt --edition 2024` 先前造成的无关格式变化已全部撤销；最终差异限于上述两份 Rust 源码。

## 冻结材料与指纹

- 完整二进制补丁：`agents/branch-completion/clippy-ci-2026-10-30/sol-cfg-implementation/args-stream/args-stream.patch`
- 补丁行数：193
- 补丁 SHA-256：`b918e6745ad5cb9d516ef6498eeeb8512feeb63d60b325b3e8d8498c1c0bb2ea`
- `stock_stream.rs` SHA-256：`267d29aec7d19c441912afc4b5977b335fb9c43c2b33df3b74cb7e5f3a39b44f`
- `market_tick_transaction.rs` SHA-256：`5524a47102ba1da4be341645dee3ffcefa9d468bb077d8e28bc93348d6439107`

完整复核请基于此 worktree 的 `git diff 50d493701aa0fbb7d62563855a84dc07c89c29b0`；不得将 READY 或实现者自检视作大 A 语义独立门禁已通过。
