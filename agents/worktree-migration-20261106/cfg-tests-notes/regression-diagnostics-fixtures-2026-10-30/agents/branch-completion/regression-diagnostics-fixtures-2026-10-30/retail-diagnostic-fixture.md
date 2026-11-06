# Retail 拒单诊断 fixture 修正记录

- 源 HEAD：`a1fc292cd3d9cb608125382a8c66bcad7e497d6f`；限定改动为 `packages/engine/src/session/pipeline/authoritative_tick_tests.rs`。
- 修复原因：`session/retail_lifecycle.rs` 按账户 `AccountKind::Retail` 归集生命周期诊断。仅向 `retail_experience` 插入 `AccountId(0)` 记录，不会把 Player 变成 Retail。
- 原 Player 用例继续验证公开 `Event::IntentRejected`、拒单原因、相位推进及后续 place/cancel 生命周期；现明确断言 Player 拒单不产生 Retail 诊断。
- 补充两个真实 Retail 名册账户的旧管线 handover 见证：首次 `step()` 前设置 tick 2 并同步市场模式，确保 staged handover 仍会被消费；由 `IngressReceiptCursors::receive` 接受意图，再通过 `NpcDecisionHandover` 交付；覆盖开盘前两种拒单和连续竞价 `InvalidQuantity` 零股拒单。Retail 到达率设为 0，避免自然意图干扰 fixture。
- 未运行 Cargo、metadata、rustc、Node 或 target 相关命令，遵守任务验证限制。已运行 `rustfmt --edition 2021 --check` 与 `git diff --check`，均通过。
