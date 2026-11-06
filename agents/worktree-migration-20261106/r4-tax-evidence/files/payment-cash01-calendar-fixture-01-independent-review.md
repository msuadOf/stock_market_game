# cash01 Calendar fixture 窄独审

日期：2026-10-29。裁决：**APPROVED_TESTS_ONLY_SOURCE_ADMISSION**。未发现阻断问题，可交接最窄测试修正并进行必要重跑；不表示 cash01 或 Tax16 已通过。

本审未实施本改动，仅核单文件完整 diff、实际失败原件、Calendar API 与冻结绑定；只写本报告，未修改产品、未运行 Cargo、未读取 target 或计算产物大 hash。

## 冻结绑定

审查树：`/data1/<USER>/workplace/stock_market_game/.worktree/r4-tax-production-2026-10-29`。实际 HEAD 与冻结基线一致，为 `6fbcc324e3037e64082781bdaa56e1333d592366`。

[payment-cash01-calendar-fixture-01.patch](payment-cash01-calendar-fixture-01.patch) SHA-256 已核为 `12dd240796f23094b4de05ee779cf22a20b1bfba28e8e30f2abc0ea161bd0cdc`；完整 live diff 与该 patch 逐字节一致，`git diff --check` 无输出。对应 [fingerprints](payment-cash01-calendar-fixture-01-fingerprints.json) 只有 `packages/engine/src/session/ordinary_tax_day_end/payment_runtime_tests.rs`，文件 SHA-256 已核为 `71f794cbe2067e26e4e2bf463da8c72ce08fffb7b955c5af72def2fedad414c7`。

## 实际失败原件

本审亲读主仓 `agents/branch-completion/joint-tax16-2026-10-29/r4_cash_01_payable_credits_gross_and_activates_personal_income_once.stdout` 与 `.stderr`：单 case summary 为 `FAILED / 0 passed / 1 failed`，panic 在 `payment_runtime_tests.rs:259:10`，原恢复尾直接调用 `step()`，收到 `2030-06-05 全场休市（本局冻结日历判定）`，错误位置为 `GameSession::check_step_start::calendar_gate`。这是恢复尾 fixture 的日期错误，不能重新标作原付款现金目标红。

实际 `actual-tool-wait.json` 与 inventory 均绑定 session `31387`、WAIT exit `1`、chunk `c79d78`。原整批 `15 green + 1 failed` 的记录保留；本审没有重新审查其余15门，也不把此单文件源码准入改写为整批 green。2030农历事实行记录 DragonBoat 为 `2030-06-05`，与 Calendar 的模拟假日分支一致；此处遵循恢复局冻结政策，不冒充未来交易所官方休市公告。

## 完整差量与 API 核对

fixture 明确仅一只 Shanghai 股票，查询 `CalendarExchange::Sse` 与原证券一致。`restored.civil_clock().calendar()` 返回恢复局已冻结的 `TradingCalendar`；`next_trading_day(exchange, after)` 明确返回严格晚于 `after` 的第一交易日，覆盖越界时返回错误。此处以既有 `PAYMENT_DAY` 为 after，未新建默认 Calendar 或覆写冻结政策，现有可见性与返回类型均适用于本测试模块。

中间每个自然日先断言 SSE 休市，再调用真实 `end_civil_day()`，逐日核 `settled_date`、`next_date`，并断言 `tick/day` 不变；报告的次日断言也保证循环按自然日推进。到达实际下一交易日后仍调用真实 `step()` 和 `end_civil_day()`，没有跳过恢复后的市场及日结行为，没有直接设置 clock 或伪造 report。

原付款、税账、save/restore 前件全部不变；本审逐字节核对 `for (account, previous) in restored_before` 起的完整旧尾，cash、gross receipts、payments 相等断言及后续 case02..04均未改。只是将错误的硬编码“恢复后6月5日交易”改为读取真实冻结 Calendar 的前件；未删除、弱化或绕过原不重复支付要求。

## AGENTS 三问与准入范围

1. **大 A 语义：通过。** 休市日只推进 civil，开市日才推进 market；使用恢复局同一本政策，不新增交易制度或2030官方日期假设，不改分红或税务规则。
2. **必要性及最小范围：通过。** 单一测试文件修正实际暴露的日期前件；生产、Calendar 政策、付款逻辑与旧尾断言保持不变。
3. **边界及复杂度：通过源码窄审。** 显式核中间日休市、逐日推进及 tick/day不变，下一交易日仍经过真实市场；未发现额外复杂度或接口阻断。本片尚无实际编译或运行结果。

**最终结论：冻结 tests-only 片可接收。原 Tax16 FAIL 保留；修复后的必要重跑与实际独审仍须完成，不能提前汇报 cash01 或 Tax16 green。**
