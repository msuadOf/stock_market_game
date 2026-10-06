# E7-X1 跨 tick 投机预计算（本周期未实现，可行性分析收档）

## 假设与已证实部分

tick-overlap 探索（.tmp/perf-expl/tick-overlap/）已实证三前提：
1. rebase 重排到 queue_npc 前在 Rayon1 下逐字节等价（错误延后传播保持首错序）；
2. "rebase 后、queue 前"的 candidate 上投机运行下个 tick 的 read_only_start::prepare，与真实结果 2084/2084 逐字段等价（含跨自然日边界）；
3. commit 尾部窗口（queue ~766µs + swap ~367µs ≈ 1133µs）≥ prepare 长杆（~1020µs），理论上可全额隐藏，外推 ~+16%。

诊断参考：spec-probe-diagnostic.patch（仅插桩与 stash 对比，不含真投机）。

## 本周期未实现的原因：借用死结

投机 prepare（读 ledger/markets/accounts-cash-positions/setup/lifecycles）必须与 queue_npc（`&mut GameSession`，内部经 capture_decision_snapshot 只读 + stream 回调写 attention/experience/strategy/pending_npc）**同时**持有同一 GameSession 的 `&` 与 `&mut`——Rust 借用检查禁止，与语义上读写字段是否相交无关。已评估的绕行方案全部不可行或降级严重：

- `rayon::spawn` 要 `'static`，无法借局部 session；`rayon::scope` 可借但拆不开顶层互斥借用。
- 提前到 commit 后/protocol 外层启动：重叠窗口只有 protocol 串行 ~300µs + TickShadow::capture ~168µs（不足 1020µs 的一半），且玩家 intent 注入（`&mut`）会切断长借；预期只剩 ~+10%，且借用仍不可行（intent ingestion 在中间）。
- queue_npc 回调写缓冲后批量化：改变 NPC 决策/attention 的写入顺序，E5 同类合并已被证伪，语义风险高。
- prepare 拆分支各自 spawn：三个分支合计仍需整体 `&`，同样被 queue 的 `&mut` 挡住。

## 前置条件（下一个专门周期的设计输入）

要全额兑现 +16%，必须先做一次**NPC 决策管线借用解耦重构**：把 `queue_npc_for_next_tick` 的读写集显式化为字段组参数（写组：attention_scheduler/npc_attention/retail_experience/last_retail_decisions/pending_npc/ingress cursors/accounts.strategy 路径；读组：ledger/markets/accounts cash-positions/setup/lifecycles），使 commit 尾部可以在单函数体内 split borrow：`rayon::scope` 一支跑 queue（字段组 `&mut`），一支跑投机 prepare（字段组 `&`）。要点：
- capture_decision_snapshot 全链路签名从 `&GameSession` 改为读组引用；stream 回调的写从 `&mut GameSession` 收窄为写组引用。
- accounts 的 strategy（写）与 cash/positions（读）同页共存：依赖 AccountBook 页 Arc COW（replace_existing_shared_parallel）保证写不原地改页，读并发安全；需在重构中显式断言。
- 语义红线：NPC 决策顺序、attention 状态序、首错序、失败原子性全部不变（E5 教训）；重排 rebase 前置可并入同一批次。
- 估值：重构本身一个完整周期（含 TDD 与独立复核），投机实现+基准再一个周期；合计两周期换取 ~+16%（若届时结构未再漂移）。

## 判定

X1 本周期**不实现**：不做高语义风险的大签名重构赶工半成品。分析、参考 patch 与前置设计已收档供后续周期直接使用。
