# Batch 179 独立复核

## 来源核验

按计划读取三份历史复核材料至 EOF；其 SHA-256 与计划值相符，行数亦一致：

- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-foundation-01-recheck2.md`：36 行，SHA-256 `0fd5e5f56925599e4d09b018678e1bb588c76f0a03591ee04fdeab26af6f6185`。
- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-foundation-01.md`：33 行，SHA-256 `b250894f6bfaf96f8f1cc76ea273caa6492779d69274b75f06336009157eb1f2`。
- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-foundation-02-delta-evidence.md`：49 行，SHA-256 `dd5088151ff7bfa5de20265b8f0fe66880ac06090cfcaffa423156c8d005cbb9`。

以上三份审计材料不属于基线提交 `43b1aa5`（该提交中路径不存在），因此只能作为历史材料读取，不能当作基线中已批准、已实施或已核销的工作证据。

## 基线代码定位

基线中相关职责与调用点可定位如下：

- `Account` 的交易结算实现及 `apply_buy` / `apply_sell` 位于 `packages/engine/src/account.rs`；提交分派也在该文件。`AccountBook::get_mut` 位于 `packages/engine/src/session/account_book.rs`，在返回可变账户前调用 page 的 `invalidate()`。基线内 `session.rs`、`decision_chain.rs`、`failure_tests.rs`、`account_settlement_tests.rs`、`account_validation_tests.rs`、`auction_refactor_tests.rs`、`decision_resources_tests.rs`、`decision_snapshot_capture_tests.rs` 等有账户初始化或可变访问消费者。
- NPC 策略候选由 `session/pipeline/npc_decisions.rs` 构造 `NpcStrategyUpdate::Replace`，由 `session/pipeline/npc_state_projection.rs` 消费并替换账户策略。此处是 A03 封装审计必须覆盖的生产写入消费者；仍需保留经 `AccountBook::get_mut` 访问所带来的 page 缓存失效。
- 日历政策内容摘要由 `calendar/policy/validation.rs::compute_content_digest` 实现，`calendar/policy/mod.rs` 构造时调用，验证路径也会重算。基线摘要包含交易所、年份、citation ID、闭市区间和 fallback digest；不包含 `source_digest`。这确认历史材料所述的摘要覆盖缺口在基线代码中可见，但本批没有调查其威胁模型、修复承诺或官方规则影响。
- `StoredStrategy::production_validated` 定义于 `account/strategy.rs`，NPC 决策生产路径调用它；这是策略替换值的产生方，不是 Account 状态的最终写入点。

## 结论与边界

历史材料记录了两类结构候选边界（Account 封装及其调用方迁移、calendar policy digest 完整性）和针对 engine-foundation-02 的历史全文复核。基线源码定位确认上述当前 owner/producer/consumer 路径存在；但这些材料是复核报告而非正式需求、ADR 或实施清单，不能单凭它们认定存在漏做的已批准承诺。A02 预计算提取、A03 封装、digest 缺口以及过户费注释均不得误报为本批已实施。

本批没有重新全文审查 Account、calendar policy、engine-foundation-02 全部源文件，也没有核验官方交易所规则或运行验证。A 股资金结算语义不因此获得新的背书；如将历史候选转为实现任务，仍需按正式规则和 TDD 单独核验。本结论仅确认来源完整性、历史证据的基线边界及可定位的基线代码 owner/callers/consumers。
