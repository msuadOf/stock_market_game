# Batch 194 独立核读

## 读取完整性

- 按计划逐篇读至 EOF；三份材料的路径、SHA-256 与行数均和 `scan-plan.json` 一致。
- baseline `43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 中不包含这三份 review 文件；因此它们只作为当前可见的历史记录，不视为 baseline 已批准承诺或代码证据。两个被记录涉及的产品源码 `packages/engine/src/session/protocol/civil/session.rs` 与 `packages/engine/src/session/plan_chain_candidates/adaptive.rs` 存在于 baseline。
- 对照 `docs/principles.md`：本批仅为对象归属与事务边界审查，没有修改或提出 A 股撮合、申报、结算、T+1、费率或单位语义；不需要将纯结构审查包装为交易规则变更。

## 现行归属与调用方

- adaptive candidate 生命周期符号 `yield_adaptive_candidates_with_root_wait`、`operation_blocked`、`resume_adaptive_candidates` 归属 `PlanChainOperationBatch`，位于 `packages/engine/src/session/plan_chain_candidates/adaptive.rs`。当前生产调用从 `packages/engine/src/session/pipeline/adaptive_plan_chain.rs` 的 `yield_adaptive_candidates_with_root_wait` 及 `resume_adaptive_candidates` 路径进入；`decision_chain.rs` 和 `adaptive_tests.rs` 也有 resume 调用。审查记录最终结论是保留现有 owner，没有要求 OOP action。
- `ProtocolSession` 私有 `ProtocolState` 持有 `GameSession`、intraday history、fact cursor、day-end save、published runtime 和 pending save candidates；`ProtocolCheckpoint` 持有 checkpoint state。`checkpoint`、`rollback`、`step_frame`、`end_civil_day_update`、`save_candidate` 继续归属 `ProtocolSession`，位于 `packages/engine/src/session/protocol/civil/session.rs`。生产中的 `step_frame`、`end_civil_day_update` 通过该 session 自身的 checkpoint/rollback 管理事务；`tick_batch` 经 `tick_batch_delta` 更新 published runtime。`save_candidate` 和 checkpoint/rollback 的其它直接调用当前可见于同文件测试。`Deref<Target = GameSession>` 提供只读委托，旧记录指出 `player_working_orders` 通过此处访问 `GameSession` 查询。
- manager delta 对裸 `GameSession::end_civil_day_update` 与外层 `ProtocolSession::end_civil_day_update` 的事务边界作了区分；最终 retention 记录保留该限制，同时明确所有 action 为空、没有发生状态迁移。它报告的自由调度和失败时不外发部分帧覆盖待补是历史审查披露的验证限制，不构成已批准实现承诺。

## 核查结论

未发现可确认的已批准承诺遗漏或历史核销错误。manager delta 曾指出 8 条 `retained_symbols` 带有伪 `to` 目标；后续 `engine-session-06-retention-final.md` 明确核验这些字段已被移除、owner 和理由仍保留，且 actions 为空。因此这项材料问题已由历史材料自身闭环，没有遗留实现动作。`engine-session-05.md` 同样明确限定为只读候选调查及保留现有 owner。

1. **大 A 语义：通过。** 所涉对象与事务说明没有定义或改变交易规则，也没有把协议序号 / 候选 identity 写成交易优先级。
2. **承诺必要性与范围：通过。** 历史结论保持为 retention，未支持新增 owner、窄方法或代码迁移。
3. **遗漏、跨层漂移与复杂度：未发现应升级项。** 记录明示未覆盖行为与未运行测试；本批不将它们改写成既有批准要求。以上结论限于这三份材料及现行对应符号归属，未重新验证历史审查所述完整行为测试覆盖。

本批未运行测试、未修改产品文件、未执行 Git 写操作。
