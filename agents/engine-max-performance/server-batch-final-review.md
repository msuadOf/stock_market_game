# Rust server 本批独立终审

复核日期：2026-10-06。复核者未参与实现。本次只复核前审固定清单、完整未提交源码 diff 与新增的 `ledger_paged_map.rs`、`server_max_speed_probe.rs`；不扩大范围，不重编译或重跑性能测量。

## 发现

未发现有效新问题。前审记录的 P3 测试证据缺口已闭合：`ordering_and_hash_projection_serde_match_btreemap_for_account_and_key_boundaries` 在 [ledger_paged_map.rs:308](/data1/baiyifan/workplace/stock_market_game/.worktree/engine-max-performance/packages/engine/src/session/pipeline/ledger_paged_map.rs:308) 对 JS safe integer domain 中的 canonical entry 数组执行成功的 `serde_json::to_vec(...).unwrap()` 字节比较；测试另外在 [ledger_paged_map.rs:296](/data1/baiyifan/workplace/stock_market_game/.worktree/engine-max-performance/packages/engine/src/session/pipeline/ledger_paged_map.rs:296) 全量比较 `iter`、`keys`、`values`，并以含 `u64::MAX` 的样本比较 owned iteration 与 `BTreeMap`。它在 [ledger_paged_map.rs:314](/data1/baiyifan/workplace/stock_market_game/.worktree/engine-max-performance/packages/engine/src/session/pipeline/ledger_paged_map.rs:314) 明确断言 unsafe u64 entry projection 被拒绝，在 [ledger_paged_map.rs:319](/data1/baiyifan/workplace/stock_market_game/.worktree/engine-max-performance/packages/engine/src/session/pipeline/ledger_paged_map.rs:319) 单独固定 JSON struct map key 不受支持的错误契约。原来把两个失败结果相等当成功 serde 对照的问题已经消除；修复没有改变生产 `Serialize`。

## 固定清单结论

1. **大 A 语义：通过。** 完整 diff 未改变适用价格时间优先、费用累计、资金与股份守恒、T+1、交易阶段或受理规则。分页只改变 ledger 行的内部组织；snapshot 仍调用原资源预留和费用 helper；checkpoint 的 `Arc<TickFrame>` 在 civil publication 时恢复原 transport frame；NPC 改动保留 worker 内独立 consumer 与宿主调用线程消费路径。结论依据为本次源码 diff、既有 `docs/trading-rules.md` 与前审列明的 ADR-0017、ADR-0018、ADR-0032。本批没有变更交易制度，因此本复核不声称重新完成现行规则的官方核验。
2. **必要性与最小范围：通过。** 改动限于 server/engine 热路径、对应边界覆盖、必要的 fixture 修复及生产 `SessionActor` Fastest 探针。新增分页映射按既有 `EnvelopeKey` 顺序输出，并通过 COW 保持共享状态隔离；未发现与目标无关的产品行为改动或新依赖。探针只测生产 actor 的 Fastest 提交路径，不包含 HTTP/WS 序列化或前端；它的倍率不能解释为真实 A 股日历速度。
3. **边界测试、跨层漂移与复杂度：通过。** P3 已关闭。其余固定清单中的 ledger 排序/删除/COW、rebase 费用审计及失败原子性、NPC 完成即受理和消费、intraday rollback、玩家与完整 snapshot 投影均未见新遗漏或语义漂移。新增探针逐帧检查 tick 连续性，达到请求数量后计入实际完整 batch 数量，未隐藏 overshoot；其证据边界见 [server_max_speed_probe.rs:38](/data1/baiyifan/workplace/stock_market_game/.worktree/engine-max-performance/apps/server/examples/server_max_speed_probe.rs:38)。

## 验证与报告边界

本次 reviewer 按要求只做静态复核，没有重编译、运行测试或测量性能。主 agent 报告最新 binary 的 `ledger_` filter 在 `timeout 9s`、`--test-threads=4` 下 56 passed、0 failed，包含修复后的 serde case。主 agent 另报告 `--test-threads=16` 的一次短测中，既有 `conservation_rows_execute_on_multiple_rayon_workers` 因调度偶发只由一个 worker 执行而失败；本轮没有改该测试，该失败未隐藏。现有测量约 103x；1000x 目标尚未完成。`save_contract` 的 35/36 失败已在独立 HEAD baseline 仅补 import 后复现，仍未解决；不能报告为全绿，也不将其归因于本轮改动。
