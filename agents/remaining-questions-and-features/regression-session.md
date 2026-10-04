# Session 集成回归修复

## 问题与修复边界

本轮完整回归的 Session 集成用例中，费用、成交额、守恒、部分成交与 FIFO 的失败共享同一原因：合法订单夹具修改了 Retail 持仓，却只调用旧的 `initialize_holding`，没有登记现行双时钟 `feedback` 持仓生命周期。真实卖出被 `NoActiveEntry` 守卫拒绝。

两处开局持仓夹具改用 `initialize_holding_dated`，日期取各自 `setup.start_date`，`market_minute` 与 `trading_day` 均为 0。初始化股份仍是已分配持仓，不伪造买入成交，不修改生产策略、撮合、费用、T+1 或经历守卫。

普通小样本完整交易日验证在独立 4 worker 执行中仍超过 10000ms。其代表性 fixture 改为 3 tick、沪深两只股票；保留所有账户、逐 tick 收据/成交/资产对账、同 seed 初始化确定性、注意力无损 JSON、恢复一致性及恰好一次日结检查。5 股票及 300 tick 的大规模显式长压力用例保持原样，普通测试不借长期限逃避 10 秒门禁。

这里的 `GameSession.save/restore` 是引擎内存验证 checkpoint，不是用户日内持久化接口；公共 `CivilSession.save/restore` 仍遵循 ADR-0025 日级存档限制，不添加任何兼容或存档版本。

## 验证证据

- Red：来自本轮密封 inventory 的 `session-a6ae8594325ba2b6`。`continuous_multi_fill_charges_one_minimum_commission_per_account_batch` 在 0.39 秒真实失败，错误为没有 active holding epoch；小样本完整日用例在 `RAYON_NUM_THREADS=4` 的单独执行中耗尽 10000ms deadline。
- Green：root 统一编译日志 `repair-engine-build.jsonl` 给出的新 `session-98b18f1928a6995c`，对全部 11 项原失败和 6 项相关夹具/FIFO/坏档校验执行 exact 短测，共 17 项真实通过。四个进程并行、每个进程 `RAYON_NUM_THREADS=2`，每项都有独立 10000ms 外部 deadline。小样本完整日用例 5.30 秒；单独 4 worker 复验为 5.35 秒。日志保存在 `.tmp/main-regression-2026-10-05/session-repair-green.log`。
- 独立复核：非作者 `session_regression_review` 已审查完整 Session diff 与有关生产 API，确认生命周期与生产初始化一致、不改变大 A 语义、原断言完整保留，未发现阻断问题。本轮只有定向验证，全量回归结果由 root 统一登记。
