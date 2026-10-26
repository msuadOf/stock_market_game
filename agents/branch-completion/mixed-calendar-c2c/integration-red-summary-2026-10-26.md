# C2-C 首批真实行为红

2026-10-26。准备提交机械 rebase 后为 `77bd566da405ae2a9f17d6b8e30170f5bd6e1260`，基线 `6ac167e1d98d1a56b1307822f311d0277d624003`，无冲突。本轮没有修改生产文件。

## 源码、构建与运行绑定

- root 密封 normal engine rlib：`.tmp/main-current-normal-rlib-2026-10-26/artifacts/libengine-e024975623ca4b70.rlib`，SHA-256 `9bee5bf8bb46234eaaeb075f82d38dde14ae2c4e9169664669f1627ed88f1480`，768433622 bytes；实际重算一致。
- 核对 root `engine-source-before.json` 的826项输入，8线程比对本树均一致。本树新增 integration test 不属于原 engine 编译，分别绑定两份测试源码 SHA。
- 本轮 rustc 只编译 integration test，真实 engine 复用上述库；codegen8、lld8、debuginfo0，外部进程树10000ms；实际 exit0、1.001625秒。
- test binary：`.tmp/mixed-calendar-c2c/integration-red-6ac167e1`，49534504 bytes，SHA-256 `99f40845745fcda323d10af1dfa28c5ef20bf9050d6d36ba054dfbf0e4fb2060`。
- 初始 restore 正控独立运行通过，wrapper exit0、3.025318秒。它先证明合法初始 policy/catalog/前史能真实恢复，再执行其余行为测试。
- 其余8项按8并发独立进程、各 libtest 1 thread 运行；每 case 与整批均进程外10000ms。整批约0.91秒，各case约0.84–0.90秒，没有超时。

原件均在 [integration-red](integration-red/)：`compile-plan/result.json`、`initial-plan/results.json`、`behavior-plan/results.json` 及各case完整 stdout/stderr。两份测试源码编译前后、运行前后 SHA 相同。

## 实际断言与局限

| case | 真实首个失败断言 |
|---|---|
| closed_first_tick_emits_no_price_fact_while_open_stock_progresses | 第54行，closed 股生成 PriceTick |
| closed_first_tick_preserves_valuation_without_active_daily_candle | 第74行，closed 股补出 active K；先前估值保持与open active正控通过 |
| closed_first_tick_adds_neither_price_history_nor_market_minute | 第96行，closed价格样本1，应0；后续分钟不增断言尚未到达 |
| mixed_day_end_archives_only_open_stock_and_reports_only_its_final_candle | 第121行，closed终K361，应360；后续DayBoundary断言尚未到达 |
| closed_stock_produces_no_auction_facts_in_open_or_closing_window | 第161行，closed清算2，应0；后续指示价断言尚未到达 |
| closed_current_minute_query_stays_closed_during_shared_market_tick | 第181行，closed.trading 为true |
| retained_closed_and_open_no_trades_remain_distinct_after_civil_commit | 第215行，closed被标NoTrades，应Closed；后续bars/终K断言尚未到达 |
| first_reopen_uses_stock_minute_zero_while_other_exchange_starts_day_one | 第249行，首次复市分钟首键240，应0；后续另一所240/终K361与362尚未到达 |

上述8项均为成功编译并真实执行后的领域断言红，不是缺API/类型错误。`run-with-deadline.mjs` CLI 对失败返回 **wrapper exit1**；各原始 stderr 另含 `bounded command exited with 101`，但本轮 JSON 没有直接子进程退出码字段，不能把该wrapper1冒记为直接raw101。未为补排版重复运行，旧原件不改写。

编译脚本曾实际核826项；先前给协调者即时消息中的“825?”猜测不属于证据，以上原件为准。运行脚本说明中的“实际child结果”应按此处限定解释为其 `subprocess.run` 直接启动的deadline CLI结果，不冒称最内层Rust退出码。

本批只证明目标缺陷存在；没有生产绿、没有native/WASM整体闭合或独审通过。下一步在已授权独占纯模块范围实施窄 `validate_for_stock` 的真实TDD；中央接线继续等待INFO最终交窗。
