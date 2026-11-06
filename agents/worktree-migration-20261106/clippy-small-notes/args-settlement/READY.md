# Settlement 参数重构冻结记录

- 日期：2026-10-30
- 基线：`50d493701aa0fbb7d62563855a84dc07c89c29b0`
- 分支：`codex/clippy-cfg-args-settlement-20261030`
- Worktree：`.worktree/clippy-cfg-args-settlement-2026-10-30`
- 状态：READY；等待独立复核。实现者未自认语义/门禁通过。

## 文件范围

1. `packages/engine/src/session/pipeline/account_settlement.rs`
   - 新增借用字段原样的 `SettlementReadContext<'a>` 与测试专用 `SettlementWriteContext<'a>`。
   - `prepare_settlement_transaction_with_beliefs` 与 `apply_settlement_transaction_with_beliefs` 收拢相关参数；测试准备/采纳顺序未调换。
   - `apply_prepared_settlement_transaction` 与 `test_experience_moment` 限定 `cfg(test)`。
2. `packages/engine/src/session/pipeline/stock_execution_transaction.rs`
   - `compose_stock_execution_transaction_for_test` 复用 `SettlementReadContext`。
   - 测试简参入口维持原调用面；`test_experience_moment` 限定 `cfg(test)`。
   - 删除全仓检索无调用的私有 `invariant` helper。
3. `packages/engine/src/session/pipeline/continuous_tick_finalizer.rs`
   - 正式调用点只把原四个借用装入 context。
4. `packages/engine/src/session/pipeline/session_execution_transaction.rs`
   - 测试组合调用点复用同一 context。

## 验证与限制

- `rustfmt --edition 2024` 已对上述四个 Rust 文件执行。
- `git diff --check` 通过。
- 按派工要求未运行 Cargo、Clippy、构建或测试。
- 尚未由未实施者审查完整 diff；交易制度输入、单位及行为未主动改动。
- 冻结时 `git diff --binary | sha256sum`：`56ecb43b773d05b6126b351b683eb2e638a92aeaebda997d2cd1f6d511f4b57a`
