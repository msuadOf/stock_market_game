# CivilClock 回归修复

## 范围与依据

本项仅修改 `packages/engine/tests/civil_clock.rs` 的两个过时场景，不改生产日历、撮合、策略、费用、T+1、经营或存档契约。
自然日经营与披露以 `docs/simulation-calendar.md` §4、§7 和 `session.rs` 的 `end_civil_day_after_session_check`、`record_civil_day_events` 为依据：休市日允许真实公司披露，披露先于唯一日期推进；不产生市场事件。2030 春节日期仍使用显式模拟日历，不冒称未来官方安排。
ADR-0024 不保证任意开局都有成交，ADR-0023 要求成交必须由真实订单撮合产生。测试需要自行构造有真实对手盘的情景，不能改生产模型保证流动性。

## 失败与修复

- `closed_civil_day_emits_one_date_advance_without_market_events` 误把“恰好一个日期推进”写成“总事件恰好一个”。原场景实际发布三条合法经营公告，然后推进日期。现精确验证唯一 `CivilDateAdvanced` 位于末尾、日期与日历状态正确、所有 seq 连续且最终游标一致；其余事件只允许真实公开库可查询的同公司当日 18:00 披露。原禁止 Trade、PriceTick、DayBoundary 和 tick/day 零推进检查保留。
- `closed_days_accrue_without_trading` 的通用 600101 公司在原价格下没有自然卖盘。仅扩大股本仍失败，因为通用公司的开局财务按股本同比推导。现使用默认虚构发行人 000812 的股份规模及 ST 主板类别，600 分报价、20 个具有个体策略的机构，让自然卖单与玩家 `Highest` 的100股买单通过正常入口撮合。没有写入 NPC 委托、改账户股份/现金或伪造成交；增加真实非自成交 Trade 的100股校验。当天不可卖、翌交易边界解锁、休市主 RNG/注意力 RNG/行情不变、经营到期恰好一次及恢复重放断言保留。

## 验证证据

- 原 `ff5f424` 编译产物 `civil_clock-bc0c1c1c50fbed9c`：10 项通过、上述 2 项失败，1.31 秒，外部 10000ms deadline、16 test threads。没有以此局部复现冒称全回归基线。
- 首次新产物 `civil_clock-222fe9f3d522390e`：11 项通过，单改股本的成交场景仍失败。曾误调用旧 hash，其输出仍为10绿2红；此调用不计入新代码验证。
- 最终 Cargo 定向编译 `civil_clock` 与 `company_event_contract`：默认 features `[]`，`CARGO_BUILD_JOBS=32`、`RAYON_NUM_THREADS=32`，绝对 `CARGO_TARGET_DIR` 为仓库 `.tmp/build-cache/full-regression`；由进程外 `runBoundedCommand` 监督 300000ms 编译期限。
- 最终 `civil_clock-222fe9f3d522390e`：12/12 通过，2.25 秒；`node scripts/run-with-deadline.mjs 10000 -- <binary> --test-threads=16`。实际 Cargo artifact 来源为 `.tmp/main-regression-2026-10-05/civil-company-build.jsonl`，短测日志为同目录 `civil-final-green.log`。
- Cargo 编译会产出不同 hash，不能根据旧测试文件名推断新产物。其他 agent 在并发执行此前产物的查漏批次时本项曾编译，因此该查漏不作为不可变历史基线，主控已获通知。

## 独立复核

非作者 `review_web_civil_server` 完整读取958行测试、全部 diff、模拟日历原文、ADR-0023／0024／0031及权威日结事件生成实现，结论：通过。改动保留当日买入不可卖、交易日界解锁和休市市场状态冻结的既有边界；合法限价请求与自然 NPC 卖单走真实撮合，新增100股非自成交事件校验，不通过写入账户或伪造成交保证流动性。日期测试允许经营披露符合自然日政策，仍严格限制事件类别，新增公开库真实性、连续 seq、唯一且末尾日期推进；不是删除市场事件禁止断言来掩盖失败。2030假日继续明确模拟，未引入新的现实制度解释或 Money 兼容。此次独立复核未启动 Cargo 或完整回归，12项绿色证据是上节作者实际运行记录。

额外复核主任务的 `collect-rust-regression-failures.mjs`：其逐子进程使用 `runBoundedCommand` 的真实终止／清理监督，不只是 Promise timeout；64个独立 case 多进程并行、每个至多10000ms，并受剩余共享预算限制。但 inventory 的来源 digest 不能证明并发重编译时 binary 未变化，且本体 timer 仍依赖自身事件循环，执行该诊断批次必须外加300000ms进程外监督。该脚本只能用于收集修复线索，不能替代不可变来源的正式完整回归证据。

完整回归由主控统一执行，本项未提交或推送。
