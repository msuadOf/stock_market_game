# 批次 181：engine-foundation-03 历史复核记录

## 范围与方法

审读目标是三份历史复核材料，按各自完整原文顺序读至 EOF；原文行数、SHA-256 与完成状态见同目录 `batch-181.json`。本次另读目标 worktree 的 `AGENTS.md`、`docs/principles.md`，以 HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 的源码行号核对相应结论，并检查同主题的 OOP 清单、模块说明、调用路径、G/Q 总账及后续 ADR。没有修改产品代码，也未运行测试、构建或 Git 写操作。

## 报告沿革及复核

- `engine-foundation-03-recheck.md`（17 行）结论未通过，指出 `verification_evidence.rs` 的 inventory 漏列 `UpdateStreamProjector::default` 和 `Sha256Provider::digest_hex`。后继 `recheck2` 已核对两符号；当前 machine inventory 分别列于 `agents/oop-refactor-audit/chinese-localization/before/exhaustive/items/engine-foundation-03.json:602`、`:640`，模块说明亦记录其分别委托 `new`、作为显式注入摘要边界（`modules/engine-foundation-03.md:11`）。**旧阻断已修复，当前源码与核销清单一致**；没有因此产生新对象或职责拆分候选。
- `engine-foundation-03-recheck2.md`（19 行）结论未通过，指出 `module_relationships` 的 “atomic consistency” 会误导为错误路径回滚保证。当前 items 的 OrderBook owner 描述已明确成功路径协调索引、失败可保留簿内部分变更（`items/engine-foundation-03.json:308-325`）；模块说明同样限定成功路径且明示错误不回滚（`modules/engine-foundation-03.md:9`）。**旧阻断已有明确修订，当前措辞未再承诺原子性**。
- `engine-foundation-03-delta-evidence.md`（29 行）报告“通过”，并附 unit-002/unit-044 短 delta 复核。其所述普通撮合可能部分变更、Market 的 `last_price` 只在 OrderBook 成功后更新、delta / filled restore 不具有 candidate restore 的事务保证，均与现行目标源码相符。该报告同时指出 public 写口等剩余 API 风险；当前行号和可见性需按本基线区分：`Market::set_last_price`/`set_last_close` 在目标 `market.rs:270-277` 是 crate-private fixture setter（不是 public API），`record_filled_order` 在 `market.rs:419-425` 也是 crate-private。旧 delta 关于它们“public” 的描述**不适用于 43b1aa5 基线，不能照抄为现状**。`OrderBook::restore_filled_orders` 在 `orderbook.rs:503-508` 是 public API，但其逐条写入失败语义仍应与 restore-resting 区分。

## 当前源码证据与调用边界

- `OrderBook::place` 进入 `place_inner`（`packages/engine/src/orderbook.rs:338-350`）。撮合循环先计算累计成交价值，再由 `BookState::apply_maker_fill` 更新/移除 maker（`orderbook.rs:424-438`；`orderbook/book_state.rs:117-149`）；清仓分支的 filled-index 插入仍可失败。剩余新单路径在递增 `next_seq` 后调用 `insert_resting`（`orderbook.rs:457-465`）。这些顺序支持“Err 可能留下先前簿变更”，不支持原子回滚推断。
- `OrderBook::restore_resting_orders` 在 candidate 上重建并全部检查后才替换原簿（`orderbook.rs:543-577`）。相对地，`apply_changes` 在前态游标检查后直接把逐项变更交给状态对象（`orderbook.rs:593-601`），filled restore 直接转交状态对象（`orderbook.rs:503-508`；`orderbook/book_state.rs:97-108`）。不得把前者局部提交保证扩展到后两者。
- `Market::place_inner` 先检查涨跌停，再调用簿；簿错误 `?` 提前返回，成功后才更新末笔成交价（`packages/engine/src/market.rs:295-319`）。生产调用包含连续撮合的 `continuous_matching.rs:480`，session 执行及 pipeline 的 Market/候选 delta 路径另见 `session.rs:3261`、`session/pipeline/adaptive_plan_chain.rs:717`、`initial_candidate_round_tests.rs:431`。这是行为边界的调用证据，不意味着本审读穷尽了所有 caller。
- Evidence projector 和 digest trait 当前定义为 `verification_evidence.rs:634-649`、`:1407-1409`；生产模块内部有 `new`/`project_update` 使用（`:737-741`），外部代表性 engine test 也触及公开类型（`packages/engine/tests/verification_evidence.rs:9-10`）。`Default::default` 和 trait method 都是合法需归档的符号，但没有依据要求将其提取为新的领域对象。

## G/Q、ADR 与范围判定

本主题是既有 engine owner 的 OOP 归档/保留建议，不是实现缺口核销。总账 `agents/implementation-audit/implementation-audit-2026-10-02.md:27` 记录 G01–G68 的缺口口径；早先 batch-020 明确三份此类 OOP 报告未映射到具体 G/Q 编号。因此本批不为 `OrderBook` 局部错误原子性或 evidence 符号清单新造 G 编号，也不以对象提取替代任何 G01–G68 验证。Q11 已由 ADR-0026 补充决定，Q12 已由 ADR-0024 解决；两者与本主题无关。

后续 ADR-0023 至 ADR-0028 分别收敛合成行情/撮合范围、资金池、日终持久化、机构行为假设、宿主部署及发布；未发现它们取代本批的 OrderBook 错误语义或 evidence owner 边界。它们也没有授权通过 OOP 重构改变撮合优先级、成交、持久化或 evidence 契约。就当前材料而言，正确处理是保留已记录的 owner/边界及错误顺序；发现的行为缺口仍须独立提出实现需求、测试和复核，不因这批重构审阅而视作已修复。

## 结论

两项旧阻断（evidence inventory 漏项、OrderBook “atomic consistency”措辞）在后续材料中已修正；delta 的普通撮合局部变更与提交边界结论仍成立。delta 曾称 Market setter 与 record-filled 写口为 public，但当前目标基线可见性已改变，相关旧风险描述须视为过时。没有发现本批材料支持的新 OOP 提取候选；保留现有实现边界，不核销实现缺口，不作交易规则变更结论。
