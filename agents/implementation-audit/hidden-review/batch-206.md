# 独立审读：batch-206（owner 1）

## 基线与来源

- 目标 worktree：`.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。只写本批报告与 JSON；没有修改产品代码、运行测试/构建或执行 Git 写操作。
- 按指定来源逐篇连续读取到 EOF；核对行数与 SHA-256 均符合 scan-plan。来源章节族：frontend closure 元数据收尾、Session 01 修订复核、Session 02 第二轮复核。具体指纹见配套 JSON。
- 核对两工作区的 `AGENTS.md`、`docs/principles.md`，以及实现审计主账、`reaudit-engine.md`、候选核对与先前 hidden-review batch 040/064。范围是历史结论的当前源码复核，不替代 G/Q 全量再审。

## 来源结论与当前状态

1. **frontend closure 收尾：只能按元数据复核理解。** 来源明确把自身限制为 closure 文件、引用、hash 与 finding 定位的一致性核对，并说明没有重读源码、不证明实现正确、不替代大 A 语义及完整 diff 审查。其 web-07 两份旧收据 hash 抄写问题已有 binding-final 纠正；这条历史结论没有扩张为源码通过结论。本批不对 closure 所列 521 个文件作实现核销。
2. **Session 01 的 R1 限定仍正确。** `GameSession` 的可提交权威状态由 `CommittableSessionState` 聚合（`packages/engine/src/session.rs:1125` 起）；`enqueue_player_intent` 先校验玩家账户后写入权威 `pending_player`（`:2518-2532`），失败 tick 候选不会回滚此前接受的输入。`civil_clock_mut` 明确绕过同步守卫（`:2056-2059`）；而 `end_civil_day` 使用 checkpoint 并在事务错误时恢复（`:2073-2097`）。因此 shadow/P9 描述仅限市场 tick candidate publication，不能写成所有会话写入的全局事务边界。Web WASM 与 server/desktop actor 有真实 enqueue/daily-end 接口调用；Web worker 日终入口在 `apps/web/src/host/wasm-worker.ts:254`。
3. **Session 02 的存档及 owner 结论仍成立。** `SessionCandleBook` 当前持有完整历史与活动日 K（`packages/engine/src/session/candles.rs:90-110`），但 `SaveSnapshot` 仍单独投影 `daily_candles` 与 `active_daily_candles`（`session/snapshot.rs:215-227`），恢复也分别写回（`session.rs:2813-2820`）；内部聚合没有改变 SaveSlot 两字段契约或要求迁移。`AccountBook`/`AccountPage` 仍是账户页分组与 COW/cache 存储组合（`session/account_book.rs:18-30`），页/组是存储组织，不是撮合顺序。`SessionCandleBook` 已被此前审读确认是实施状态，旧候选 `proposed` 不能再作为待办。
4. **A05/A07 缺陷线索仍需与 OOP 候选分开。** 当前 `CivilClock::register_due` 以 `self.next_due_seq += 1` 推进 `u32` 序号（`session/civil_clock.rs:330-351`），未作 checked overflow；耗尽风险存在，但“拒绝分配 `u32::MAX`/预留哨兵”的新政策没有由现行 ADR 敲定。当前 `CompanyOperationsClockWiring::sync` 先修改 `mirrored`，再执行可能失败的 `clock.register_due(...)?`（`session/company_operations.rs:60-73`）；多项后项失败可令独立调用留下部分修改。生产日终 caller 在 `GameSession::end_civil_day` checkpoint 之内（`:2088-2097,2100-2109`），因此不能把 adapter 局部风险夸成宿主日终事务一定无法回滚；`install` 失败则发生于构造/接线。A05/A07 的修复建议属于行为/失败原子性边界，不是 OOP 抽取，不应在本批实施或计入 OOP 完成项。

## G/Q、决策与领域边界

- 对照 G01–G68/Q 主账及既有裁定：本批没有证据核销或重开任何 G/Q。对象抽取、状态聚合与旧 review 的“通过”都不能代替实际生产 caller/消费链审计；与状态/root 历史复制有关的 G16 仍按主账独立跟踪，不由 `CommittableSessionState` 或 `SessionCandleBook` 核销。
- 对照现行 ADR 与明确取代关系：ADR-0019 的范围/容量约束、ADR-0025 的日终持久化边界及 ADR-0027/0028 的宿主/发布决策没有要求改变本批内部 owner；ADR-0018 未被整体接受，不能将其 proposed 部分提升为实现要求。未发现后续 ADR 取代 SaveSlot candle 双字段或 P9 局部语义。
- 未提出交易制度新主张：本批仅核对会话状态、日 K 存档边界与自然日事务边界，没有改变沪深 A 股交易规则，也未进行交易所/中国结算官方规则查询。

## 结论

历史 closure 结论的元数据限定诚实；Session 01/02 中 P9 范围修订、SessionCandleBook 存档契约及对象保留结论与当前源码相符。候选实施状态需以当前源码更新；A05 序号耗尽与 A07 同步部分写入是独立风险线索，不能归为 OOP 抽取，也不能忽略日终外层 checkpoint 对可达路径的保护。无新 G/Q 核销、无 A 股语义变化。未运行测试/构建，不作运行验收声明。
