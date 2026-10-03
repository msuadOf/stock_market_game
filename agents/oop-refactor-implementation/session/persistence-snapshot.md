# session 持久化与快照 owner 实施记录

日期：2026-10-03。范围仅包括权威 [action-index](../../oop-refactor-audit/challenge-2026-10-03/action-index.md) 的 session-R2-N02、session-R2-N06、session-R2-N07、session-R2-N10，含 optional 正文。

## 已实施

- `OpeningFigures`：原 `Figures` 的开局事实与两种账套投影归于同一 owner；新增 `for_stock` 仅搬迁既有代码 + 股本精确匹配。保留 registry 条件债务行、industrial 无条件债务行、应收插入位置、科目顺序和元到分换算。极小股本的不平衡账套仍显式失败。
- `SaveValidationContext`：只读借用 `SaveSlot`、已校验 `GameConfig`，拥有本轮已派生的交易时钟、股票集合、发行人集合和 NPC 数量。交易日范围校验仍紧跟 schema/setup，market/account/market-minute 的 checked 校验仍在原顺序执行，之后组合 context 供后续主流程及 personal/plan 校验共享。恢复后的 `GameSession` 订单校验独立保留，不加入 optional `GameSession` 借用。
- `CumulativeFeeAuditV2`：拥有 side、filled_value、charged；nominal 计算与累计分量校验由 owner 承担。`validate` 返回 `Result<bool, SessionError>`，与拟议 `Result<(), SessionError>` 不同：外层必须维持带原 envelope key 的失败文字，bool 可在不新增身份字段、不新增包装错误类型的情况下保留此边界。父 agent 已确认该最小签名调整。卖方累计分量仅约束于 nominal 和成交额上界，不重建逐笔扣取优先级。
- `LiveOrderReservations`：由单次 snapshot 临时拥有派生总额；continuous/auction 同实例汇总，`take_for` 同时移出现金与分股数量。合法无订单账户仍显示零值，checked 累计的原 invariant panic 文字保持。
- 所属源码及测试中的 `GameSession` 字段访问迁至 `session.state`；Snapshot 日 K 投影使用 `state.candle_book.histories()/active()`。所有 SaveSlot/SaveRuntimeV2/JSON/TS 字段保持原样。

## 依据与语义边界

已阅读 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md` 与 ADR-0009、ADR-0014、ADR-0017、ADR-0019、ADR-0025；源码包括 company_assembly、persistence、persistence/v2、v2_tests、snapshot。交易语义按现有 `docs/trading-rules.md` 及 ADR-0017 保留。没有新增或修改交易制度，不将 envelope 或 snapshot 预留描述为交易所/中国结算真实清算。此记录不声称重新联网核验了官方规则；既有费用表证据的访问失败限制仍按正式文档保留。

T+1、board lot、odd-lot、价格/证券类别、累计 minimum commission、买卖双方 transfer fee、卖方 stamp tax 与卖方实收封顶简化均未改变。候选类型不增加持久化事实、不推断资产交易来源、不修补可编辑存档。

## 短测试交接

新增 tests 先于对应 owner 实现写入；本 agent 按父级分工未运行 cargo/产品测试，未取得 red/green 运行结果，不声称 TDD 运行闭环完成。根 agent 统一执行编译与以下 filters：

- `opening_figures_`：3 cases；双投影矩阵、默认 code+shares 匹配、默认金额读取与极小股本失败。
- `save_validation_context_`：2 cases；call auction / PreOpen / continuous / closing auction / 日切边界、共享集合/时钟、首错类别与文字。
- `cumulative_fee_audit_v2_`：2 cases；买方各分量须等于 nominal，零/负 filled_value 边界。
- `snapshot_reservations_`：1 case；continuous/auction 买卖单、部分成交剩余数量、同账户跨证券股份汇总。
- `live_order_reservations_`：3 cases；账户隔离、移出后零值、现金/股份累加显式溢出。

既有 `persistence::v2_tests` 继续覆盖卖方 path-dependent 实收、恢复原子性、完整 serde 形状、未知字段拒绝；根 agent 按短 fixture 和 10 秒边界选择运行。

## 本 agent 验证与未完成门禁

- 精确 `rustfmt --edition 2021 --config skip_children=true`：仅上述 5 个 Rust 文件，成功。
- 对上述文件 `git diff --check`：成功。
- 仅执行 Git 只读 diff，无 Git 写入操作，无新依赖，无全仓 fmt。
- 编译、短测试及未实施者的独立完整 diff 复核由根 agent 统一安排；未完成前不宣称整批完成。

## leaf 独立复核发现后的 caller 修正

- ES01-A01 的 `poison` 不属于 session state；`capture_rejects_poisoned_session_without_emitting_a_dto` 已修回 `session.poison`。
- `validate_plan_contract` 的 `TradingPlan` account/code/target/filled_qty 与 linked account/code 已迁移同名 getter；ParentOrderPlan / SaveSlot DTO 仍保留字段访问与原条件顺序。
- 配合 domain A03，`restore_runtime_v2` 使用 `Account::restore_strategy`；运行时 Account strategy/cash/kind/positions 与 Snapshot Position 读取迁移 getter。SaveSnapshot 的可编辑 DTO cash/positions 修改不改。
- 修正后精确 rustfmt 与所属 diff whitespace 检查成功；未运行 cargo，复核者与根 agent 已收到待复核通知。
