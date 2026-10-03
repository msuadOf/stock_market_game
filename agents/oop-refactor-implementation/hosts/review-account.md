# Account caller 独立复核

## 范围与证据

- reviewer：未实施本批迁移的独立 subagent；复核日期：2026-10-03。
- baseline：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 完整审阅上述 baseline 到当前工作区的四文件 diff，以及四文件当前全文：`packages/engine/tests/account.rs`、`packages/engine/tests/strategy_state.rs`、`packages/engine/tests/company_opening/isolation.rs`、`packages/engine/tests/company_scenarios/controlled_experience.rs`。
- 已读 `AGENTS.md`、`docs/principles.md`、`docs/trading-rules.md`、`docs/open-questions.md`、`docs/architecture.md`、ADR-0006、ADR-0011、ADR-0024 与实施记录；按调用链检查当前 Account getter、Position factory、grant_position、apply_buy_settlement，并对照 baseline 对应结算逻辑。
- 仅执行只读源码检查与短 Python 文本比较；未执行 Cargo、编译、Rust 测试或任何长验收，不声称运行验证通过。

## 三项门禁结论

1. **大 A 语义：通过本批静态复核。** 现金仍以 Money 的整数分表达，qty / t1_locked 仍为股数，invested_cents / recovered_cents 仍为原始成交额分。没有修改费用、成交量、板块、申报单位、T+1 或公司与投资者的资金边界。领域依据沿用 `docs/trading-rules.md` 的官方来源及核对日期：2026 版沪深规则自 2026-07-06 实施，相关内容在 2026-09-22 / 25 / 27 核对；ADR-0011 的逐户风险与 ADR-0024 的禁止补钱边界均保留。本批没有引入新的制度解释，因此未重新访问官方网页；不能把本次静态等价复核称为全领域现行规则重新验收。
2. **必要性与最小范围：通过。** 四文件仅适配 Account / Position 私有字段的 API 迁移：getter、Position::from_restored_parts 与已有 grant_position 的等价调用。没有新增依赖、生产业务功能、DTO 迁移或额外抽象。
3. **边界、跨层漂移与复杂度：通过本批静态复核。** 原测试函数名称一致；按 getter 与 strategy.as_ref 借用形式归一化后，四文件全部 assert! / assert_eq! / assert_ne! 的完整表达式及顺序一致，数量依次为 77 / 13 / 12 / 2。没有删除、弱化断言。整数舍入、负成本、零持仓、部分锁定、当日买入锁定、现金不足、费用不足原子性、超卖及溢出边界保留。未发现本批新增边界遗漏或跨层语义漂移。

## 重点核对

- `tests/account.rs:252` 的 `grant_position(code, u32::MAX, Money::ZERO)` 与原直接插入的 Position 完全等价。`src/account.rs:224` 计算 `0 × u32::MAX = 0`，`src/money.rs:86` 用 checked_mul 的 i64 股数转换，不会提前溢出；随后仅插入持仓，不修改 cash，得到 qty=u32::MAX、t1_locked=0、invested_cents=0、recovered_cents=0，cash 仍是 i64::MAX。它是故意制造计算边界的 fixture，不是绕过 session 规则的新委托。
- 随后的 `apply_buy(price=1分, qty=1, t1_enabled=false)` 仍先完成合法交易参数、费用及现金检查，再在 `src/account.rs:310` 的 `position_qty_add` 失败；零投入避免 `invested_cents_add` 先失败，零锁定与 false 保持原 T+1 条件。修改现金与持仓位于全部 checked 计算之后，原 MoneyErr、现金不变与 qty 不变三断言均保留。原断言未细分 MoneyErr 内的 op，本次也没有将其弱化。
- Position::from_restored_parts 的四参数逐项对应原字段，当前 factory 不引入校验、舍入或默认值，保留原 fixture 可表达的零成本、负成本及零持仓边界。
- Account::strategy() 返回原 StoredStrategy 的共享引用，替换 strategy.as_ref() 保留 production_state 与 round-trip 参数断言。
- `company_opening/isolation.rs:67` 的 account_states 仍逐户读取现金与全部四个持仓事实；完整存档字节、证券类别、交易所、总股本、流通盘及公司资金独立断言未改。
- `controlled_experience.rs:95` 仍比较两个运行后 Account 的同一现金值；owned experience 与 decision trace 差异断言未改。
- `tests/account.rs` 中 SelfView / PositionView 的公开 DTO 字段未误改。旁查 `tests/extraction_replay.rs:213` 使用 PositionSnap，`src/session/snapshot.rs:25` 的 DTO 字段仍公开，不应当按 Account / Position getter 迁移。

## 发现及修复复核

- 发现一项实施记录准确性问题：原 `account-callers.md:10` 的“account_book.rs 不存在”未限定 tests 路径，而 `packages/engine/src/session/account_book.rs` 存在。已通知父 agent，父 agent 将记录改为 `packages/engine/tests/account_book.rs` 不存在；复读修正后记录，路径限定正确，该发现已关闭。四文件源码未发现需修复事项。
- 本结论只覆盖指定四文件 caller 迁移及上述记录修正，不代替 Account 生产实现、其他 worker diff 或上层运行门禁的独立验收。

## 审查时文件 SHA-256

| 文件 | SHA-256 |
| --- | --- |
| `packages/engine/tests/account.rs` | `ce1d8a0c78419cca409f638f473e57438d8a0fed19ffa2cc7b5a59ebe65dd4c6` |
| `packages/engine/tests/strategy_state.rs` | `00015946426523303fa372e9a62d7e564c1119c63190f93c4aed2d245056ddb2` |
| `packages/engine/tests/company_opening/isolation.rs` | `718be88ea7639e018b4a62e240b4e18384dd3715a3a80a05b4ac8c86f36b7066` |
| `packages/engine/tests/company_scenarios/controlled_experience.rs` | `fefef52a815a21aba4ee10b5e9b974c19b7e48246e73123440156bf5ea4a0ef2` |
