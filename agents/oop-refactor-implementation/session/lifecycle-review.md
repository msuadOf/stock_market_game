# Session lifecycle 独立复核

日期：2026-10-03。独立复核者 canonical 身份为 `/root/implement_session/review_lifecycle`，未实施本批生产代码；baseline 为 `b89afb3346743a4b4fccf26c9ac9ff108595f696`。本轮只读源码与 diff，仅写入本记录及本人签署的 lifecycle source manifest；未运行 Cargo、产品测试、Git 写操作或全仓 fmt，未派 subagent。

## 范围与依据

已阅读全文复核 baseline 至当前工作区的 `session/execution.rs`、`session/execution/**`、`session/plan_execution.rs`、`session/plan_execution/**`、`session/protocol/**`、`session/candles.rs`、`session/attention.rs` 全部改动。辅助核对 `session.rs` 的构造、save/restore、candidate clone、`hash.rs`、`pipeline/auction_day_end.rs` 的 checked child 接线与局部 parents/pending 提交、`adaptive_plan_chain.rs` 的真实接受/receipt 转换，以及 attention capture/enqueue callers；辅助文件不代替其独立 owner 的完整复核。

authority 正文为 [action-index.md](../../oop-refactor-audit/challenge-2026-10-03/action-index.md) 的 `session-N01`、`session-N02`、`engine-session-02-A03`、`session-R2-N01`，不是旧索引的摘要。工程依据为 [principles.md](../../../docs/principles.md)、[architecture.md](../../../docs/architecture.md)、[open-questions.md](../../../docs/open-questions.md)；领域依据为 [trading-rules.md](../../../docs/trading-rules.md)、ADR-0011、ADR-0015、ADR-0018 的现有契约与明确简化边界。本批没有修改交易制度，未重新访问官方网页；官方规则引用与适用日期沿用 trading-rules 的已登记核对，不能称为 2026-10-03 重新核验。

## 三项门禁

1. **大 A 语义：静态通过。** `ParentOrderPlan` 只接受匹配 side/order id 的真实 settlement；受理与撤单不增加 `filled_qty`，linked 完成仍交 PlanBook。连续 assert/expect 的失败次序、竞价 checked 错误字符串与先检查后修改保持，50 股 Buy 余量不补足为额外买入；收盘不复制连续簿 child。数量仍为股、Money 仍为分，T+1、申报单位、费用、撤单时段与优先级没有迁入新对象或变更。CandleBook 保留首笔真实成交替换昨收零量占位、无成交日不成为 traded sample、checked 统计和完整历史；对外 save/snapshot 仍是两个原字段，child 剩余量仍从 live order 唯一重建。AttentionScheduler 只持候选 heap，不复制个体 RNG 或将到期当成实际观察；hash 排序与 restore 重建仍接原事实。ProtocolState/cursor 只组合未发布批次 rollback 与同 tick facts，不改变 wire payload、Event seq、自然日/交易日或公开日终存档时点。
2. **需求必要与最小范围：静态通过。** 四个 receiver 分别拥有原来分散的关联字段或 lifecycle，未新增第二份市场/账户/计划 authority、通用 manager、继承层次或存档迁移。ProtocolState 使用原 clone_for_tick_shadow、共享 history/Arc 和独立 RefCell 内容，测试注入 flag 保留在外层；只读 Deref 保留，没有 DerefMut。已有 CausalCollector、CommittableSessionState、TradingPlan/BeliefParticipant getter 调整是其他分组的真实接线，不在本记录宣称其完整实现通过。
3. **边界、跨层与复杂度：修复后静态通过。** 新测试固定了 matching fill、linked/unlinked 完成、双 Option 非法状态、连续/竞价不同错误边界、首成交/no-trade/overflow、stale heap、checkpoint published cache/candidates 与 cursor 续号。原 continuation 断言没有削弱。下表三项初审发现已由实施 agent 修复并再次独立阅读复核；实际测试执行由上层统一承担，本轮静态检查不等于测试通过。

## 有效发现与修复复核

| 编号 | 初审发现 | 影响与要求 | 状态 |
| --- | --- | --- | --- |
| L1 | `execution.rs` 的 `replace_active_child` 与 test fixture 调用 `replace_child_facts`，全 engine 源没有方法定义。 | 生产代码编译失败；补齐内部双字段受控写入口，保持原事实和顺序。 | 已修复并静态复核关闭：私有方法按原事实配对赋值，restore 接缝也只写同一 id 与重建 remaining；未新增校验或改变失败顺序。 |
| L2 | `plan_execution/continuation_tests.rs::owned_plan_sync_late_failure_keeps_pending_and_parent_unchanged` 漏迁 `first.account/direction/target/opinion/confidence_bp/urgency/horizon_trading_days/created_trading_day/plan_id`。 | TradingPlan 已 `pub(in crate::plans)`，测试越界读字段无法编译；改用同名 getter，fixture 及原断言保持。 | 已修复并静态复核关闭：全部改为同名 getter，opinion() 返回同一 Copy 值；原 fixture 与断言保持，plan_execution raw read 自扫无漏项。 |
| L3 | 未找到 `materialize_parent_order_intents` 的未关联母单反向替换短矩阵。 | session-N01 authority 明列反向替换；TradingPlan decision-chain reversal 是另一入口。补旧 Buy partial fill 至新 Sell 固定目标，确认 target/fill/child/limit/expiry 重建且不继承旧 child。 | 已补并静态复核关闭：`parent_transition_tests::unlinked_parent_reverse_rebuilds_execution_from_the_new_target` 经生产 materialize、submission 与结算成功 typed fill fixture 建立 Buy400/fill40/child60，再带旧 Buy working 转 Sell800；断言 fill0/child200/limit980/新 expiry、双 active None 与唯一 Sell200 intent。这是 adapter fixture，未冒称运行完整撮合或账户结算。 |

另已给上层提示：`pipeline/continuous_tick_transaction_tests.rs` 还有 TradingPlan raw reads，例如 active_child_order_id/filled_qty；该文件由其他分组收尾，本记录不代替其编译验收。

修复后的指定完整范围再次执行 `git diff --check`，静态通过。最终检查范围包含 18 个有变更文件（1490 insertions、502 deletions，仅用作当前 diff 范围见证，不作为完成证据）。L1/L2/L3 均静态关闭，无剩余阻断发现；三项独立静态门禁通过。尚未执行红灯或绿灯，不宣称 TDD 运行证据、产品测试或整批验收通过；上层仍需完成统一编译、短测和跨组 caller 的独立验收。

## 本人最终版本签署

由 `/root/implement_session/review_lifecycle` 本人再次只读核对上述全部最终 diff，内容与本人上一轮修复复核的 18 个文件一致，L1/L2/L3 关闭结论保持；本范围无新增源码文件，无需补新增文件全文复核。辅助 caller 的限定核对范围保持上文所述，没有把其他分组的完整复核划入本人范围。

逐文件最终 SHA 由本人计算并写入 [lifecycle-source-manifest.json](lifecycle-source-manifest.json)，18 项全部匹配 [source-manifest.json](source-manifest.json) 对应条目。本人清单 SHA-256 为 `262ec00edfaf0f911d88f0fc0e202b23c87cf6f527815bedad842081614e9af8`；核对时 source-manifest SHA-256 为 `e410f344003cbe6ebb5c3fc323a98b244054890b7dbee5d85af4dfb90135989c`；本范围 baseline 完整 diff SHA-256 为 `a15c84939fbacc069df3d00ae10c2e9e4ddf5d7555619d87033e27e3cc85daa0`。签署绑定上述实际源码内容，不由 manager 代刷；后续源码变化使对应签署失效。

此次仅补身份与版本证据，未改源码、未运行或要求新增测试。此前的未测试限制保留；本人仅签署静态复核，不将其他执行者的集中编译或测试记录写成亲自运行结果。
