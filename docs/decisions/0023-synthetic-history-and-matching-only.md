# ADR-0023：虚拟开局前史与撮合生成的游戏行情

- **状态：** accepted
- **日期：** 2026-09-30
- **决策者：** 用户确认；AI 记录与实现边界核对

## 上下文

旧量价差距清单 C06 与验收 manifest 将真实市场统计校准列为“缺授权数据、尚未完成”。
用户明确要求不使用真实市场数据：开局之前直接生成虚拟历史，进入游戏后由实时撮合生成行情。
因此授权数据不再是当前版本的依赖，也不能把生成的虚拟数据称为真实市场校准样本。

## 决策

1. 开局前的价格历史使用带 seed 的虚拟日 K。现有生成器为每只股票生成 360 根负时间日 K，
   隔离于会话决策 RNG；其 `trade_stats` 为 `None`，不伪造本局逐笔成交。
2. 从第一个游戏交易日开始，价格与成交量、成交额、笔数来自游戏引擎实际受理与撮合。
   不能在游戏运行期间通过随机 K 线、预画走势或导入真实行情替代实际成交。
3. **日序边界：** 现有内部 `day = 0` 就是 UI 的“第 1 个交易日”，其日 K 时间为 0；
   合成前史的时间均小于 0。用户所说的第 0 天之前按“开局前”解释，不将第一天也合成为历史，
   不把实际撮合的起点推迟到内部 `day = 1`。
4. 无成交时允许以前收建立零成交占位 K，不生成成交量、成交额或笔数；首笔真实成交取代
   占位 OHLC。零成交与缺样本仍须如实表达，不能为填图而制造成交。
5. C06 真实市场校准为**不适用/不在产品范围**，而不是“已校准”或“等待授权”。
   保留基于虚拟输入与游戏实际成交的多 seed、分布、极端样本、规则边界及量额对账验证。
6. 当前开局不导入真实市场前史；未来若改变这一范围，必须另有用户决策与 ADR。
   ADR-0018 曾预留的外部前史导入不是当前待办。

## 实现与验收边界

- `packages/engine/src/session/candles.rs` 的 `generate_preset_daily_candles` 已生成虚拟前史，
  `update_active_daily_candle` 与收盘归档维护游戏运行记录；本轮不重写已有撮合或价格生成逻辑。
- `session/pipeline/continuous_tick_finalizer.rs` 与 `auction_day_end.rs` 将实际成交接入日 K；
  公司经营与公开信息前史也已有独立虚拟生成，不是外部真实公司数据导入。
- K7 当前 `after`/`sensitivity` manifest 保留字段 `c06_external_market_calibration`，
  值改为 `not_applicable_synthetic_history_only`。runner 版本同步升至
  `2026-09-30-synthetic-history-policy-v8`，当前验证器明确拒绝旧“等待授权”或“已校准”标记。
- 旧密封 manifest、日志与证据不改写，继续作为历史政策下的记录保留；新版验证器拒绝旧政策标记
  不表示当时的记录被篡改，也不冒充新版验收通过。
- 本轮只核对既有 Rust 接线并运行 Node 小型 fixture 短测，不执行真实 K7 长矩阵或完整回归。

定向验证：`verify-simulation-artifacts.test.mjs` 15/15 通过，`baseline-run.test.mjs`
仅选择 source identity、canonical sensitivity reuse 与 v7 runner resume 拒绝三项，3/3 通过。
版本负例重算修改后的 identity digest，且核对拒绝后 checkpoint 字节未变，不冒充真实旧矩阵重跑。
两条命令以独立 Node 进程并行执行，均使用 `scripts/run-with-deadline.mjs 10000`、
`--test-timeout=10000`、`--test-isolation=none`、`--test-concurrency=4`；没有重跑 Rust 测试。
本地红绿与定向日志在 `.tmp/synthetic-history-policy/`，不作为真实长矩阵或发布证据。

## 备选方案与后果

- 真实行情导入与授权市场数据拟合：用户不采纳；不作为阻塞项或隐含后台请求。
- 游戏运行期间随机画图：不采纳；会使订单、成交、资产与行情失去可解释关联。
- 采用虚拟前史加实际撮合：无需真实数据授权，适合独立模拟游戏；但不能宣称已验证真实市场
  的统计分布、预测能力或投资收益。

## 关联

- [ADR-0005](0005-unified-engine-three-deployments.md)：撮合驱动价格
- [ADR-0018](0018-long-running-immutable-timeline.md)：已知历史与不可变时间线
- [量价差距清单](../price-volume-simulation-gap-checklist.md)：C06
- [工作状态](../work-status.md)
