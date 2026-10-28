# C2-C 首批 fixture 与行为断言清单

2026-10-26。写入基线为 `d7a26186d40ec38ffce901f48a935917b5d29641`；本轮无生产文件修改、无 Cargo、无 rustc 编译、无测试运行。下表是已经写出的断言，**不是红绿结果**。

## 初始合法性与后续观测

`packages/engine/tests/common/mixed_calendar_c2c_fixture.rs` 使用真实 `GameSession::new → save → restore`，只在市场尚未运行的初始 SaveSlot 冻结 synthetic 2030 年休市覆盖。覆盖包含 Jan1，以保持默认生成的 360 根前史日期合法；两所差异不代表真实 2030 公告。StockId 每次从 catalog 取得，setup 顺序置换不会更改其实际身份。

`initial_mixed_fixture_restores_with_frozen_policy_and_catalog` 必须先独立成功，之后才把目标断言的失败归因于 C2-C。该正控检查双所 × setup 顺序、`tick/day=0/0`、当前 civil date、union IntradayTrading、完整 policy 保留、按代码分配的 catalog 与 360 根前史。

后续 fixture 不调用 mixed 跨日 SaveSlot restore，不要求 C4 尚未交付的严格恢复能力。市场观察取实际 `step()` 结果、snapshot、价格样本长度、分钟样本和带健康校验的分钟/retained 查询。每次 `step()` 必须成功后才读观测；失败则保留真实 StepFatal，不借读取残留数据冒称目标行为已验证。

## 已新增的九个 case

| case | 输入与 oracle | 运行状态 |
|---|---|---|
| initial_mixed_fixture_restores_with_frozen_policy_and_catalog | 初始真实 restore，双所 × setup 顺序；policy/catalog/前史合法 | 未编译、未运行 |
| closed_first_tick_emits_no_price_fact_while_open_stock_progresses | 2 tick 日首拍；open 有 PriceTick，closed 零 PriceTick，共享 tick=1 | 未编译、未运行 |
| closed_first_tick_preserves_valuation_without_active_daily_candle | closed market 快照逐字段不变；open active K，closed 无 active K | 未编译、未运行 |
| closed_first_tick_adds_neither_price_history_nor_market_minute | SZSE closed 反向 fixture；open 有完成分钟，closed 价格与分钟样本不增 | 未编译、未运行 |
| mixed_day_end_archives_only_open_stock_and_reports_only_its_final_candle | 两拍；shared day=1，open 361/closed 360 根，DayBoundary 只含 open | 未编译、未运行 |
| closed_stock_produces_no_auction_facts_in_open_or_closing_window | 12/3/3 日；open 两次 AuctionCompleted，closed 无指示价和清算事实 | 未编译、未运行 |
| closed_current_minute_query_stays_closed_during_shared_market_tick | 真实首拍后健康分钟查询；closed/空 bars，open Continuous/无成交空 bars | 未编译、未运行 |
| retained_closed_and_open_no_trades_remain_distinct_after_civil_commit | 两拍 + 真实 civil commit；closed 无终 K，open NoTrades 有终 K | 未编译、未运行 |
| first_reopen_uses_stock_minute_zero_while_other_exchange_starts_day_one | 内存运行 Jan2→Jan3；首次复市分钟首键0，另一所240；根数361/362 | 未编译、未运行 |

这九项没有验证 PlanId/T+1、双时钟账户单调、G×N 尾组、typed ExchangeClosed、Abort/shutdown、INFO/D1 或实际 WASM；这些仍须后续同一完整生产分支闭合，不能把本清单外推为完成。

## 资源与来源约束

- 当前没有可借用的本轮 main 同源 normal engine rlib。mainwriter 本轮只构建 `cargo test -p engine --lib --no-run`，产物是 test executable，不是正常库。
- 不用 C1a 旧红阶段 rlib 为当前生产 C2-C 背书；不从 test executable 假造 engine 库，也不自行开额外 Cargo build。
- 后续实际编译须记录依赖完整路径、原始源码/构建证据与 SHA；新增测试源码和依赖库分别绑定，不把只重新编译 integration test 描述成重建 engine。
- 预构建后先独立运行初始 restore 正控，再以8并发运行互相隔离 case；每 case 与整条命令各受进程外10000ms deadline。需要的构建窗口由 root 统一排，构建与执行耗时分开。
- 已运行的准备检查仅为两个新 Rust 文件 rustfmt 与 `git diff --check`；它们成功不表示 Rust typecheck 或领域测试成功。

## 官方依据核对

本轮重读归档 SSE §2.4.1、SZSE §2.3.1，并实算原件 SHA：

- `sse-trading-rules.docx`：`fc922c433438b2636cb631eab25cca405209712acbb6aaded768c45456ff8888`。
- `szse-trading-rules.pdf`：`9b66f8b0db70f84a25ef1ccb4ee2351001724e408117552d75f6d8993483c586`。

两所均按各自公告休市；归档规则适用日期沿已批设计为 2026-07-06，SSE §11.9 已直接重读。共享 union、240 分钟与缩短 fixture 是既有已批游戏模型，不在本片另定真实制度。
