# 现存单测断言强度审计（六失效模式扫描）

- 工作区：`.worktree/std-thread-model`（分支 exp/std-thread-model）
- 扫描面：`packages/engine/src/runtime/`（26 文件）+ `packages/engine/tests/`（197 文件），共 223 个 .rs
- 方法：重点文件全文逐行审读（drive_tests.rs 全文、group.rs 两个内嵌测试模块 sol_recheck1/3、runtime_flip_batch1-5、common/flip_helpers.rs）；其余文件用六模式 grep 过筛后对命中处逐一上下文复核
- 性质：只读审计，未修改任何源码/测试文件
- 置信标注：HIGH=模式确定成立且断言实质失效；MED=很可能空转/弱化但有缓解因素；LOW=弱但可接受或已登记

## 六模式命中总览

| 模式 | 命中数 | 其中高置信 |
|------|--------|-----------|
| a 恒真断言 | 6 | 4 |
| b 只计数不校验内容 | 4 | 2 |
| c 聚合口径漏检 | 3 | 0（均已登记） |
| d 期待值反构造 | 2 | 0（跨通道对拍，MEDIUM） |
| e Some/非空即过 | 4 | 1 |
| f fixture 巧合/断言空转 | 14 | 6 |
| 合计 | 33 | 13 处（合并同根因后 11 项） |

---

## 模式 a：恒真断言（6 处）

### a-1 [HIGH] equivalence_tests.rs:400-407 — 字面 `assert!(true)`
`batch8_final_verdict_flip_requires_integration_migration_batch` 以 `assert!(true, "批 8 终判…")` 收尾——零判别力的决策记录占位。修法：改为编译期 const 断言或移入 ADR 文档；若保留测试形态，至少断言两个前提的**可计算状态**（如迁移台账计数）而非 true。

### a-2/a-3 [HIGH] runtime_flip_batch1.rs:246-249、runtime_flip_batch4.rs:195-198 — 魔数容差吞掉断言
`assert!(total <= base + 5_670_000_000, "现金总量不异常增加")`。base 与被测会话同种子同构造，理论期望是精确 `total <= base`（费用只减不增）；容差 5.67e9 cents ≈ 整个流通市值（100k 股 × 1000 cents = 1e8 cents）的 56 倍——引擎内任何现实注入 bug 都在容差内恒过。修法：改 `assert!(total <= base)`（同种子下无随机性来源），或容差收敛到 fee 上界推导值。

### a-4 [HIGH] runtime_flip_batch5.rs:43 — `seq < u64::MAX` 近恒真
注释称"锚 seq 单调域"，实际断言 `s1.snapshot.seq < u64::MAX && s2.snapshot.seq < u64::MAX`——seq 到达 u64::MAX 现实不可达，两路径 seq 完全相同也绿。修法：改断言 `s1.snapshot.seq >= 30`（驱动 30 tick 后 seq 至少前进）或比对单路径内 seq 与 tick 的单调关系。

### a-5 [LOW] tests/diagnostics.rs:299-300 — `>= 0.0` 口径恒真
`mean_daily_turnover_rate_bps >= 0.0`、`maximum_drawdown_bps >= 0.0`——换手率/回撤按定义非负（幅度值）。作为 sanity 可留，建议改为与 completed_days 的上界关系（紧邻的 `max_zero_volume_streak <= completed_days` 是对的示范）。

### a-6 [LOW] tests/technical_memory/gold.rs:206 — `atr14…cents() >= 0`
ATR 定义非负；该行是 atr14 唯一内容断言（其余指标都有精确值）。建议按 fixture 手算精确 ATR 值（fixture 全 -50 步，可手算）。

## 模式 b：只计数不校验内容（4 处）

### b-1 [HIGH] runtime_flip_batch5.rs:113-130 — `batch5_diag_minute_counts` 零断言
整个测试只有 eprintln 诊断输出，无任何 assert——永远绿。修法：把注释里的期望（recent ≤16、minutes=37）转为断言，或加 `#[ignore]` 移出默认门禁。

### b-2 [MED] runtime_flip_batch2.rs:107-115 — 4 倍数量级比较
`rt.len() >= pipe.len()/4 && pipe.len() >= rt.len()/4`——注释称"无静默吞事件"，但 75% 事件丢失仍绿。修法：对逐事件类型计数比（AuctionTick/PriceTick 应逐 tick 恒等数量）。

### b-3 [MED] tests/auction.rs:1846-1862 — matched 分布计算后丢弃
`total_matched` 逐日算出后 `let _ = (codes, total_matched);`，只断言 `total_accepts > 0`（计数）。注释称"原断言 ≥2 保持在原用例"——本见证用例的撮合面（f.4=matched_at_completion）没有任何断言。修法：至少断言 `total_matched > 0`。

### b-4 [LOW] tests/company_decision_session/gold.rs:116 — `retail_styles.len() >= 2`
风格多样性只查数量不查内容（哪些风格、参数是否分化）。可加具体风格集合断言。

## 模式 c：聚合口径漏检（3 处，均已自登记）

### c-1 [LOW·已登记] common/flip_helpers.rs:184-191 — 账户+股聚合闭环
`(受理−撤销) ≤ touch` 的跨方向腿抵消漏洞已在注释里给出静态反例，并由 batch3#11（无成交 fixture 精确核销）+ closure_tests 反例三测承载。语义单测细化时应把 exact_closure 形态推广为通用 oracle。

### c-2 [LOW·已登记] drive_tests.rs:1149-1167 — 同键聚合下界
`traded（saturating 聚合）>= child_qty - remaining` 是下界断言（同键含其他子单成交），注释已声明。可细化：按 desk_lifecycle 的订单 serial 精确核销。

### c-3 [MED·已登记] drive_tests.rs:1091-1102 — (账户,方向) 跨计划聚合 filled
多计划同键合并累计与独立源对拍——同账户同方向两计划的 filled 互相挪移不可检（零成交计划又被显式跳过）。注释登记了合法形态；细化时建议按 plan_id 分键。

## 模式 d：期待值反构造（2 处，MEDIUM）

### d-1 [MED] drive_tests.rs:386-444 — `d3a2c_cost_basis_writes_back`
`expect_invested` 从 `driver.world_report_log()` 的 desk_trades 逐笔累计——期待值消费被测系统另一通道的输出。跨通道对拍能抓"组侧 vs 报表"漂移，但两通道共同漂移（如价格口径同错）不可检。注释已声明"独立推算"并做了 `expect_invested > 0` 非空前置。同构问题在 d-2。修法：金额类期待改为手算 oracle（注入价格 × 数量常量，如 batch4/sol7th 的 `gross - fees` 写法）。
（佐证：drive_tests.rs:810-817 `independent_allocation_cash` 注释"不反构造受理量——期待值不受被测输出影响"——Sol 整改 5 已在 allocation_cash 上修过同类问题，说明此模式在本代码库真实发生过。）

### d-2 [MED] drive_tests.rs:1024-1107 — `solmid_execution_leg_lifecycle`
expect 从 desk_trades 累计（同 d-1），且 maker/taker→方向映射口径依赖 desk.rs 实现约定（注释自认"此前写反，红证数据显红后修正"——映射本身就是被测物的一部分）。同修法。

## 模式 e：Some/非空即过（4 处）

### e-1 [HIGH，与 f 叠加] runtime_flip_batch2.rs:63-74 — AuctionCompleted 字段"域合法"
`if let Some(p) = clearing_price { assert!(p.cents() > 0) }`——clearing_price=None 时零检查；`matched_volume` 完全未断言；且无"AuctionCompleted 事件确实出现"前置，25 tick 内事件缺失则整个循环空转。修法：先断言 completed 非空，再对 Some/None 两分支分别断言（None ⇒ matched_volume==0；Some ⇒ 价在涨跌停带内、量>0）。

### e-2 [LOW-MED] tests/strategy.rs:1541-1548 — `factory_builds_retail` 只查 `s.is_some()`
不校验构建出的策略种类/参数。可断言 `matches!(s, Some(StrategyData::Retail(..)))` 及参数回读。

### e-3 [LOW·已登记] runtime/plans_tests.rs:254 — `rollback_hash.is_some()`
注释声明"哈希判据退役，完整性由成交/费用桶字段断言承载"——同函数确有 `volume_today==0`、`fee_sink==0` 精确断言。可接受。

### e-4 [LOW-MED] drive_tests.rs:2354-2357 — `d3b5` 只查 watchlist `contains_key`
注入的 attention 参数值（tick 0,0）未回读校验，且未断言其余域（price_memory/belief）不被注入覆盖。修法：断言 `stocks[&StockId(1)]` 的记录内容 + 其余域与 original 逐字段相等（original 目前 `let _ = original;` 弃置）。

## 模式 f：fixture 巧合/断言空转（14 处，重点）

### f-1 [HIGH] runtime_flip_batch1.rs:16-62 — 自带 setup `arrival_rate: 0.0` 使成交面测试空转
batch1 文件内置 `setup_1stock()` 与 flip_helpers 几乎相同，唯一关键差异 arrival_rate=0.0（flip_helpers.rs:35-42 注释权威解释："0.0=两路径都零下单"）。后果：
- batch1#8（trade_event_fields_wellformed，197-222）：无 NPC 买单 ⇒ 永无 Trade ⇒ for 循环空转，价格/maker≠taker 断言从未执行；
- batch1#9（cash_never_increases，226-251）：无订单 ⇒ 现金恒等，容差断言平凡通过（叠加 a-2）。
修法：batch1 复用 flip_helpers::setup_1stock（arrival=1.0），或注入对手单（batch4_t1 的写法）。

### f-2 [HIGH] runtime_flip_batch3.rs:50-80 — `batch3_pipeline_emits_order_canceled` 名实不符
测试名与注释都承诺"撤单事件存在"，断言却只有 `!events.is_empty()` + seq 单调——OrderCanceled 出现与否完全不影响结果（注释自认"arrival_rate=0 的 fixture 下旧管线不产订单事件"）。而 batch3#7（170-186）又声称"arrival 0.30 产单"并断言 `p_c > 0`——两处注释矛盾，说明 fixture 演进后本测试被改成永不针对其命名语义。修法：改用与 #7 相同断言（`p_c > 0`）或显式重命名并登记。

### f-3 [MED-HIGH] runtime_flip_batch3.rs:83-95 — cancel_fields_wellformed 固定 seed 202 无存在性前置
同文件 #1 需要 199..260 种子搜索才能找到撤单事件，#3 却固定 seed 202 且无 `cancels 非空` 前置——若 202 无撤单则字段断言（P1-9 域）空转。#5（accept, seed 204/30tick）与 #9（cancel account 域, seed 208）同构风险。修法：各加 `assert!(!target_events.is_empty())` 前置或复用种子搜索。

### f-4 [HIGH] drive_tests.rs:2415-2418 — `d5_runtime_step_installs_day_boundary` 默认静默跳过
`if !step_runtime_enabled() { return; }`——`ENGINE_STEP_RUNTIME` 缺省 0（runtime/mod.rs:98-110），默认测试运行中该测试**永远零断言绿**，DayBoundary/day 推进断言从未在常规门禁执行（CI 不设此 env 时即假覆盖）。修法：用 `#[ignore]` + 显式 env 分开跑，或改由子进程设 env 断言（batch5 flip_opt_in 的模式）。

### f-5 [HIGH] drive_tests.rs:1247-1264 — `solmid_day_end_drain_releases_child_tickets` 前置被弃置
`had_child`（日结前是否有在途子单）计算后 `let _ = had_child;`——无此前置则日终释放循环可空转（对照同文件 sol4 版本 1361-1368 有 `assert!(had_child, …)`，此处疑似整改中被删弱）。修法：恢复 `assert!(had_child)`。

### f-6 [HIGH] drive_tests.rs:2138-2147 — `d3b2` Abort 冲回段断言缺失
注释承诺"注入拒绝→回滚→Paused 消失"，`after`（回滚后 Paused 面）计算后 `let _ = after;`——"Paused 消失"从未断言。修法：`assert!(after.is_empty(), …)`。

### f-7 [HIGH] runtime/equivalence_tests.rs:229-277 — `market_views_install_into_session_markets` 死脚手架
测试名与注释承诺"会话每股 last_price 与 runtime 收束点一致"，实际唯一断言是 `view.last_price > 0`；`snap`/`probe`/`installed`（None 字面量）全是弃置的死脚手架（264-271 行）。修法：从 session 快照读回每股 last_price 与 view 逐股 `assert_eq!`。

### f-8 [MED] runtime_flip_batch5.rs:46-53、101-111 — publication_ids 调用即弃
#5 名为"公司注册表在场"只 `let _ = restored.publication_ids();`；#10 名为"披露序列不破坏"两次调用即弃，且 `end_civil_day` 成败两可均绿（已注释为合法形态）。修法：断言 `publication_ids()` 非空或跨日结不变/递增。

### f-9 [MED] drive_tests.rs:730-734 — d3a3 或条件前置
`assert!(!plan_ids.is_empty() || !accepted.is_empty())` 允许"有计划但零受理"——后续报价对拍循环（752 行）空转。修法：改 `assert!(!accepted.is_empty())`（对拍循环的输入面）。

### f-10 [MED] tests/company_decision_session/gold.rs:44-58 — before/after 诊断弃置
`decision_chain_diagnostics()` 前后取值双双 `let _ =`——测试名里的"disclosures_publish"面只剩 civil_date 推进断言。修法：断言 after 的披露计数 ≥ before（或排期未到期时相等）。

### f-11 [MED] runtime/equivalence_tests.rs:400-413 — candle_projection 无内容断言
名为"writes_into_session_candle_book"，只断言 install_candles/history 读不失败，K 线簿内容（bar 数/OHLC 值）零校验。修法：驱动 N tick 后断言 session K 线条数与收盘价。

### f-12 [LOW-MED] runtime/bridge_tests.rs:460-483 — `world_day <= session.day()+1` 弱不等式
纵切日结测试主断言（MarketSessionOutOfSync）扎实，但该补充断言允许 world_day 任意落后；`day_before` 弃置。可收紧为等式推导值。

### f-13 [LOW] runtime_flip_batch1.rs:255-271 — #10 相位一致性只查存在
注释承诺"越窗后 CallAuction 不再出现"，断言只有"前 15 tick 有竞价事件"（存在性）；越窗后的缺席面未查（batch2#6 才补上）。建议 batch1#10 补 tick>16 无 CallAuction 断言或删除误导性注释。

### f-14 [LOW] runtime_flip_batch3.rs:226-228、runtime_flip_batch4.rs:143-147 — no-panic 韧性测试
`let _ = runtime_events(485, …)` 零断言——但注释与命名明示为韧性（no-panic）用例，属诚实登记。可保留。

---

## 附：扫描确认干净的区域

- `group.rs` 两个内嵌模块（sol_recheck1_tests / sol_recheck3_tests，4676-5324 行）：手工 fixture + 精确字面期待值（PURSE0/CHARGE 常量推导、 Completed 状态机、serial 5/77/88 身份关联），是六模式全部未命中的模范。
- `day_end_tests.rs`、`desk_semantics_tests.rs`、`facts_tests.rs`、`protocol_tests.rs`、`entities_tests.rs`、`decision_tests.rs`：grep 过筛未见命中，抽查上下文均有精确值断言或显式 Err 形态断言。
- flip_helpers 内嵌 closure_tests 三例（双撤单反例等）为模式 c 的正确防御示范。
- 大型金测（money/account/orderbook/industrial_accounting/bank_accounting 等）抽查为手算 oracle 精确断言，未发现六模式命中。

## 与"语义单测细化"批次的衔接建议

1. 高置信 11 项应在本批先修（多数是一行前置断言或复用现有 helper，不改生产代码）。
2. d-1/d-2 与 c-3 的对拍口径，正是"A 股语义从对账测试转独立手算 oracle"要替换的形态——fee/价格 oracle 已有手算范本（drive_tests sol7th:1656-1670、sol8th:2054-2057 的 `gross - 500 - 2` 写法）可直接推广。
3. batch1 整文件的 fixture 与 flip_helpers 分叉（arrival 0.0 vs 1.0）是历史遗留双源，建议合并消除。
