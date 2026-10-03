# 隐藏复核批次 125（owner=5）

## 来源与完整性

按指定范围全文读取至 EOF：`agents/oop-refactor-audit/chinese-localization/reviews/root-documents.md`、`session-a.md`、`session-b.md`。实测 SHA-256 与行数记录于配套 JSON。基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；基线到当前没有已跟踪文件差异。三篇来源都是旧中文化复核记录，作为关于当时文档与译文的证据，不视为当前实现已通过独立源码审核。

## 当前实现、调用方与领域边界

- `Account`/`Position` 当前仍归引擎账户域，账户范围明确为现金、持仓、成本、T+1 锁定及资金/持仓校验；它不拥有撮合、行情或策略实现（`packages/engine/src/account.rs:1-7`、`:114`、`:620`）。账户变更通过 `AccountBook::get_mut` 暴露，分页 COW 后使策略校验缓存失效；该簿由 session/pipeline 的候选状态使用（`packages/engine/src/session/account_book.rs:18-20`、`:124-134`）。这与 root-documents 记录中“账户状态归账户、簿负责分页和缓存失效”的关系一致。
- 收据结算位于 pipeline；`apply_receipt_settlements` 在候选账户上汇总并调用 `Account::apply_settlement`，再由 continuous tick finalizer 接续（`packages/engine/src/session/pipeline/settlement.rs:27-30`、`:111-131`；`continuous_tick_finalizer.rs:236`）。旧记录区分局部变更、补丁准备和安装责任，没有把 `Result` 误述为全链回滚保证；此界限仍符合当前代码。
- 当前 `GameSession` 的可提交字段收在 `CommittableSessionState`，候选 tick 由 `TickShadowPlan` 持有；`plan_tick` 捕获候选，continuous transaction 准备完整候选，准备成功后才由 `PreparedContinuousTick::commit` 安装（`packages/engine/src/session.rs:1135-1145`；`packages/engine/src/session/pipeline/mod.rs:194-204`；`continuous_tick_transaction.rs:76-98`）。这些实现与 session-a/b 对候选、P9 和失败边界的文字方向吻合，但本批没有把静态核对提升为行为验收。
- G/Q 主账复核：当前实现总账列有账户/结算 R07、session R10 等历史承诺映射；三篇来源没有将这些对象抽取候选宣称为实现缺口已修复。与 NPC 语义相关的 Q11 后续约束已由 ADR-0026 定义机构个体经验；Q12 已由 ADR-0024 关闭为允许资金池减少。未发现来源新增、错误关闭或覆盖本批关系的 G/Q。相关核对见 `agents/implementation-audit/implementation-audit-2026-10-02.md` §2、§3、§4，及 `docs/open-questions.md` Q11/Q12。
- 最新 ADR 为已接受的 ADR-0028（2026-10-03），主题为 tagged release、手动构建和静态 Pages，与三篇来源的领域对象及中文化审查无冲突；存档边界仍以 ADR-0025 为准。ADR-0026、0025 的旧审查描述不得覆盖其现行状态（`docs/decisions/0028-tagged-release-and-static-pages.md`、`0026-individual-institution-experience.md`、`0025-day-end-only-persistence.md`）。

## 结论

三篇历史 review 对当时所列中文化范围的限定总体清楚，未发现把中文化审查说成源码或交易规则重新认证的实质误导。当前实现抽查支持其账户 owner、候选提交及结算职责描述；未发现应由本批新增或核销的 G/Q，也未发现遗漏相关已接受 ADR 承诺。结论只限于三篇指定材料、上述调用链及治理核对；不代表 1,186 个文件源码全审、A 股官方规则重认证或运行时验收。未修改产品代码，未运行测试、构建或回归。
