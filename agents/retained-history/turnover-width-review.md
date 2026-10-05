# 累计成交额升宽独立复核

## 范围与结论

非作者复核本批 `DailyTradeStats.turnover_cents`、分钟历史共用编码、VWAP 分子、诊断累计额的 `u128` 升宽，以及 Server、Tauri、WASM 和 Web 的相应输入解析。已读根协作规则、工程原则、ADR-0031 与 ADR-0023。多人身份、月报、交易所日历等共享工作区中的其他改动不属于本复核。

源码复核未发现剩余阻断项；Web 定向短测及 fresh host42 的六项 Rust 新用例、相邻 continuous counter gate 均通过。本批累计成交额升宽的源码与定向单测门禁通过；不声明完整回归或三宿主运行矩阵完成。

## 大 A 语义与必要性

成交额仍为成交价格（分／股）乘实际成交股数（股）的逐笔累加；分时均价仍为当日累计成交额除当日累计成交股数，保持精确分子、分母，到图表边界才转近似数值。开局虚拟前史不伪造真实成交统计。此批不改变申报数量、价格时间优先、T+1、费用、余额及现金来源，不引入新的交易制度，因此不以任何外部行情或市场统计校准充当依据。

多笔单笔金额合法的成交可因资金重复流转使累计额超过 `u64`，累计成交额不是单账户余额；只提升累计统计到 `u128` 为必要修复。`Money` 保持 `i64` 分，`AccountingAmount`、股数、笔数、ID 的既有范围保持不变。字符串契约仍严格，拒绝数值、负数、前导零及超出 `u128` 的值，不增加版本或兼容路径。

## 失败原子性与跨层

`SessionCandleBook` 在本地副本完成 OHLC、volume、累计额及笔数更新后才插回；`DailyTradeStats::record_trade` 在两项 checked 运算都完成后才赋值，避免后一个溢出留下前一个更新。continuous finalizer 在 candidate 中预检相同边界，其测试仍以 `u128::MAX` 检验 typed fatal 不提交。未通过默认值或吞错掩盖非法价格及溢出。

Rust 与 Web 共用规范十进制非负分字符串语义。Server/Tauri/WASM 单点 VWAP 解析改为同一 Rust helper，curve 的 serde 编码同步；Web 日 K 协议、存档、VWAP 和图表边界同步校验，不把累计额交给 `parseMoney` 收窄。diagnostics 的事件累计、日 K 汇总及 combined causal 累计同步提升，验证投影仍输出十进制字符串。

## 发现及复核结果

1. 原 `protocol-parse.test.ts` 将 `18446744073709551616` 当作累计额非法值，升宽后该值合法。独立执行捕获真实失败，作者改为 `u128::MAX + 1`；证券总股本的 `u64` 越界负例保留。再执行 17 项全部通过。
2. ADR-0031 原段落承诺累计额保持 `u64`，与新代码冲突。作者在原类型边界段融合说明 `u128` 累计额及其上限，同时保留 `Money` 与其他字段范围；增量复核通过。
3. 搜索发现 `escrow-verification-contracts.mjs` 的旧 `validateProjectedIntegers` helper 内部把 `turnover_cents` 归为 `u64`。完整调用核查表明该 helper 只有定义及自身递归，现行导出校验器没有调用它。因此这是潜在过时 guard，不是已证明的现行验收阻断；不能据此声称正式证据必然失败，也不建议为本批突然启用该死路径而改变密封证据验收范围。

## 实际证据

- 亲读 `.tmp/retained-turnover/cumulative-red.log`：旧 `u64` 实现四笔累计超过范围时 panic，1 项真实红。
- 亲读 `.tmp/retained-turnover/wire-red.log`：旧编码接受 `"00"`，严格契约测试真实红。
- 独立运行 `node scripts/run-with-deadline.mjs 10000 -- node --experimental-strip-types --test --test-timeout=10000 --test-isolation=none --test-concurrency=8 apps/web/src/host/intraday-average.test.ts apps/web/src/host/protocol-parse.test.ts`：首次 16/17 通过；修正负例后再次 17/17 通过，执行约 0.16 秒。没有执行 Cargo、完整回归或提交。
- 主任务统一 fresh host41 Engine 后，亲读 `.tmp/retained-turnover/host41-*.log` 中六个 exact 用例：`four_legal_trades_preserve_turnover_above_u64`、`daily_turnover_wire_requires_canonical_decimal_strings`、`turnover_and_count_overflow_leave_candle_unchanged`、`maximum_u128_turnover_roundtrips_as_string`、`average_retains_wide_aggregate_and_strict_wire`、`seed_projection_preserves_aggregate_turnover_above_u64`，每项实际执行 1 个 case 且通过；诊断用例 0.19 秒，其余显示 0.00 秒。
- 相邻 `candle_counter_overflow_returns_fatal_without_committing_the_tick` 的 helper 原以元旦休市创建交易测试，委托在测试目标前即被休市规则拒绝。作者将共同 fixture 明确设为 `2030-01-02`，本增量只有日期选择及累计额故障注入值改为 `u128::MAX`；volume、count、typed fatal、authority hash 不变与 outbox 清空等全部断言保留。该修正不改变生产日历、不绕过休市交易禁令，符合其测试连续竞价失败原子性的目的。主任务随后以 fresh host42 Engine `engine-de08d335cdbb058d` 重跑，亲读 `.tmp/retained-turnover/host42-*.log`：上述六项及此相邻 gate 各实际执行 1 个 case、全部通过，相邻 gate 1.15 秒。主任务报告已以 `--list` 确认用例，整批外部 10000ms、每进程 9000ms、8 进程并发且 `RAYON_NUM_THREADS=4`；本复核未自行执行新的 Cargo 或三宿主 runtime matrix。
