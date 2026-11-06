# ShutdownFailure 布局与保真红测

本轮仅新增测试源码，没有修改生产实现，也没有运行 Cargo、构建或测试命令。

- 新增 `shutdown_failure_stays_within_the_error_layout_budget`，要求 `size_of::<ShutdownFailure>() <= 128`；现有内联 `Option<ShutdownCompletion>` 基线预期超过上限，预期失败原因是布局断言，而不是编译错误。
- 加强真实 2 组、2 交易部的 `active_shutdown_rejects_worker_errors_and_keeps_every_real_join_failure`：真实失序消息导致 Drain 失败后，继续逐项核对 primary/secondary worker 错误与四个 join 结果，并断言 serde JSON 完整包含 primary、secondary、`completion` 计数及 worker `Result` 详情；不预设或截断失败上下文。
- 精确测试过滤器：`shutdown_failure_stays_within_the_error_layout_budget`、`active_shutdown_rejects_worker_errors_and_keeps_every_real_join_failure`（各自作为 Cargo test filter；按任务要求未执行以验证）。
- WASM wrapper 现有真实 Registry 测试覆盖未知/重复 handle 及撤销失败；该层构造的失败 `completion` 为 `None`，没有 native owned worker 的真实 partial completion 可复用，因此本轮不改 wrapper 测试或实现。

允许的静态检查：`rustfmt packages/engine/src/runtime/world/shutdown_completion/tests.rs`、`git diff --check`，均已执行且通过。未执行 Cargo 命令，以上红灯是按当前生产字段布局推断的预期，尚未由测试运行确认。
