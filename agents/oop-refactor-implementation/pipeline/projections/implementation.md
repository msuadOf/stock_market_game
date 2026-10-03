# Projection 与 capture receiver 迁移记录

状态：四个指定动作的实现与 caller 迁移已落地；编译、产品测试和独立复核待 root 统一执行，不能据此宣称完成验收。

## 范围与依据

- `engine-pipeline-10-A01`：`ExperienceUpdateMode::InstitutionalFacts(ExperienceMoment)` 绑定机构必需时钟，移除独立 `Option<ExperienceMoment>` 与内部 `expect`。
- `pipeline-R2-N11`：`AccountFillProjection` 负责单账户临时经验、`running_qty`、机构 `running_positions` 和事件的累计。
- `pipeline-R2-N05`：`CapturedExperienceObservation::{NoExperience,Captured}` 绑定同一次观察的 experience、risk inputs 与 SelfView positions。
- `pipeline-R2-N13`：`SelfViewCashReservations` 负责 reserved/replaceable 的 Money 累计与 sub→add 的可用观察现金计算。

动作依据是本主题 `assigned-actions.json` 对应四项，以及 `agents/oop-refactor-audit/challenge-2026-10-03/action-index.md` 与 `relationships.md`。已阅读 AGENTS、principles、testing、architecture、open-questions、ADR-0017 顶部现行修订和 ADR-0018 §7。

本批没有交易制度变更。Money 仍是分，Position/Fill 数量仍是股；T+1 可卖数仍使用 `Position::sellable`，SelfView 可替换现金不变成本轮 P1 资源预算。依据是上述已决 ADR 与原行为契约；没有重新查询或宣称重新认证现实制度。

## 所有权与生产 caller

| 动作 | owner 方法 | 实际 caller | 保持的边界 |
| --- | --- | --- | --- |
| 10-A01 | `ExperienceUpdateMode::InstitutionalFacts(moment)` | `project_institutional_receipts` → `project_receipts` → `AccountFillProjection::apply_order` | 完整 moment 原样传入 dated fill；Retail 分支无 moment |
| N11 | `AccountFillProjection::new/apply_order/finish` | `project_receipts` 中 Rayon account job | 全部 job 的即时 Result 错误先检查；随后才检查 deferred `final_position_error`；watchlist prune 不提前短路 |
| N05 | `CapturedExperienceObservation::capture/consume` | `capture_decision_snapshot_in_place` 中每账户 job | 先观察，再构造 SelfView，再算 risk，最后导出 strategy；非 Retail 有 experience 时仍有 SelfView positions 且无 retail risk |
| N13 | `SelfViewCashReservations::record/available_cash` | `build_self_view_for` 的 continuous/auction 扫描 | record 先 reserved 后 replaceable；阶段/window 判断及各 Money error location 保持在 caller |

两个公开到上层的 receipt 投影入口签名保持；`DecisionSnapshot` / `DecisionAccountInput` 的 API 不改。`CapturedAccountObservation` 只是 consume 的具名输出，不存档、不形成第二份权威状态。

同时承接父协调者的跨组封装迁移：本文件簇的 GameSession 私有字段经 `state` 访问；Account/Position 使用 getter 和 fixture API。机构的临时 Position 只保留原 qty/cost 累积含义，使用 `Position::from_restored_parts` 重建，未调用真实 Settlement、未释放 T+1。

容器迁移补核对：capture 的 daily history 存在性读取改为 `state.candle_book.histories()`；生产 scheduled 候选逐项通过 `state.attention_scheduler.enqueue` 写入，tests 的 clear/enqueue/iter 一并迁移。按实际 `CommittableSessionState` 字段表逐个核对 `shadow/source/session/restored` 的访问，没有仅以 `session` 单一 receiver 搜索。本文件簇没有 TradingPlan 访问。

## 行为覆盖

新增 13 个 characterization tests，均先于本批 receiver 实现写入。现有行为在原实现已经存在，未宣称观察到新增测试红灯；按父协调者指令，本子任务不执行 cargo 或产品测试。已有测试保留所有断言，仅迁私有字段访问。

- capture：双持仓 equity、position peak、market minute 与 T+1 同源；watchlist prune 保留持仓并只删过额未持仓项目；非 Retail 有 experience 的现有合法组合；缺市场数据与 equity overflow 保留源会话；SelfView→risk→strategy 错误先后；混合 continuous/auction 买卖单在五个阶段/window 下只回加可撤部分。
- receipt projection：平均成交价整分截断而 gross 不变；多股票 Fill 后按 Settlement 持仓修剪 watchlist，未触及持仓 experience 保持；机构清仓缺实际费用历史返回 `InconsistentFeedback` 且不返回 patch；超额卖出与零数量 Fill 维持 typed error。
- snapshot：非 Retail 的 experience 可以存在，而 retail risk / behavior market 不必存在。
- 复用既有 `account_settlement_tests`：机构买入→部分卖出→清仓的实际费用净盈亏、显式 moment、幂等、错误原子性；没有在投影测试重复实现真实 Settlement。

精准短测 filters（预构建 engine lib test executable；默认 features 即可，单命令仍受 10000ms deadline，harness/Rayon 并发预算由 root 统一记录）：

```text
session::pipeline::retail_projection_tests::
session::pipeline::retail_projection_persistence_tests::
session::pipeline::decision_snapshot_capture_tests::
session::pipeline::decision_snapshot_tests::
session::pipeline::account_settlement_tests::institution_fill_updates_only_belief_book_trade_facts_and_is_idempotent
session::pipeline::account_settlement_tests::institutional_exit_profit_requires_net_cash_after_actual_fees
session::pipeline::account_settlement_tests::institutional_fill_preserves_same_order_failure_and_rejects_unknown_fee_history_atomically
```

已执行的静态检查：六个编辑的 Rust 文件以 `rustfmt --edition 2021 --config skip_children=true` 精确格式化成功；本文件簇 `git diff --check` 成功。未运行 Git 写操作、全仓 fmt、cargo、产品测试或额外 subagent。

## 全文读取记录

本次改动前读至 EOF：`retail_projection.rs`、`retail_projection_tests.rs`、`retail_projection_persistence_tests.rs`、`decision_snapshot_capture.rs`、`decision_snapshot_capture_tests.rs`、`decision_snapshot_tests.rs`、`decision_snapshot.rs`。用于跨层核对的 Account/Position、observation、experience、account_settlement_tests 与 session fixture 是相关片段读取，不宣称全文读完这些未编辑文件。

## 待核对

root 编译与短测仍须提供真实结果；独立 reviewer 须检查完整 diff，特别核对 job error/deferred FinalPosition 优先级、capture 风险构造时点、机构 moment 和实际费用。尚未获得这些证据。
