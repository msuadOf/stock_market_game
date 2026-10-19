# 日界拍占位日K漂移修复批台账（feat/day-boundary-kline-fix）

任务来源：`agents/main-line-reintegration/b2-side-effects-ledger.md` §"main 既有
漂移登记"1 + 复核建议 1。分支基线：main `a6911c52`。

## 静态归因（实施前，drive.rs/desk.rs/world.rs/failure.rs 通读结论）

日界生产时序（tpd=30 形态，日 0 = tick 1..30）：

| 拍 | desk 侧 | session/driver 侧 |
|---|---|---|
| tick 1..29 | 申报窗/开盘清算(t4)/连续/收盘窗(27..29 只挂单) | 每 install_candles 装真实活动 K |
| **tick30 拍开头** | `begin_tick(30)` 判 `day_after>day_before` ⇒ **`roll_day`**：日0 K 归档进 `market.candles`（**不含收盘段**）、量额清零、分钟桶暂存 `rolled_day_bars`；tick30 本身=日1 申报窗 | — |
| tick30 拍步尾（ends_day，`tick()%tpd==0`） | — | ①`install_runtime_day_boundary`：`commit_active` 提交 session 活动 K（=desk 拍内投影，**同样不含收盘段**）②`world_day_end_for_step`⇒Drain：`drain_all` 收盘竞价清算成交——**此时 desk 已 roll**，成交经 `settle_fill` 落进**日1 活动 K** + 900 桶落日1数组；`DeskDrained.drained_minute_bars`=暂存+900 ③freeze 保留快照 |
| tick31.. | 开盘窗 | 收盘成交事实**迟到本拍 DeskReport**（`trade_facts` 缓冲跨 Drain 残留）⇒ receipt `civil_date` 取组装时点=日1（次日）；`buy_vwap` 同样在 t31 才看到 |

三处漂移（同根：**roll 先于 Drain 清算**）：

1. **install 面日 K 缺收盘段**：`commit_active` 在 Drain 前——当日若只有收盘竞价成交，
   session 活动 K 仍是零占位 ⇒ 提交 `volume=0` 占位（B2 探针实证形态）；日间+收盘
   混合则部分丢失（candle=100 bars=200，B2 测试构造规避的形态）。
2. **desk 面**：日0 归档 K 缺收盘段；收盘段污染日1 活动 K（`get_or_insert` 建在已 roll
   数组）与日1 量额累计；日1 `last_close` 未取收盘竞价价（应=官方收盘价）。
3. **receipt 迟滞**（台账建议 1）：civil_date=次日 + 日结静点 save/restore 丢
   （`DeskMarketSeed` 无 trade_facts；driver `pending_receipts` 未消费）。

## 修法（最小改法——不重排相位状态机）

A. **desk `drain_all` 收盘段"unroll→清算→reroll"**：收盘分支先把 roll 掉的日0 状态
   （归档 K 弹回 `candle`、量额累计还原、`rolled_day_bars` 回填 `minute_bars`、日1
   零占位 K 暂存）恢复 → `execute_clearing_at` 自然落日0 → 清余单后重跑 roll 的行情
   段（归档/暂存/清零/`last_close`=清算价）。日1 状态还原（零占位 K 复位，close=现价）。
   `traded_days_total` 增量条件：终 K volume>0 且日内段为 0（首 roll 未计）。
B. **`DeskDrained` 扩两字段**（沿 `drained_minute_bars` 先例）：
   - `drained_trades: Vec<DeskTradeFact>`（收盘清算成交——`mem::take(trade_facts)`，
     杜绝 t31 DeskReport 双报）；
   - `drained_day_candle: Option<DeskDayCandle>`（日0 终 K 五元组+额/笔）。
   world 侧暂存（`pending_day_candles`/drained trades 缓冲），`abort_day_end` 对称清。
C. **driver/session 编排**（failure.rs ends_day 段，Drain 之后、freeze/事件 push 之前）：
   - 以 `drained_day_candle` 权威覆盖 session `candle_book` 历史末条（install 面）
     + `DayBoundary.closed_daily_candles`（facts 尚未编号 push，原地补真值）；
   - drained trades 走既有 drained 事件段（DayBoundary 之前编号）：Trade 事件投影
     + receipt 派生（civil_date=当日，组装即入账 `runtime_append_trade_receipts`
     ——日结静点 save 不丢）。
D. **收紧 B2 冻结对账豁免**：`assemble_retained_day_snapshot` 的
   `candle.volume==0 && volume>0 放行`分支退役（修复后恒可逐位）；手动 day_end
   无迁移的 candle 缺席跳过分支保留（既有登记，manual 面不做迁移）。

### 性能预期

Drain 是低频路径（每日一次）；A/B/C 均在日结段，热路径（每拍 install/撮合）零改动。
B 探针（1000 tick）预期持平；如实测量。

## 决策记录

- 不重排 ends_day 段内迁移/Drain 顺序（避免事件 seq/回滚面重构=架构级）；改"Drain
  应答扩字段+就地补真值"——台账提示方向、`drained_minute_bars` 同型先例。
- 收盘段污染日1 K 属同根修复范围（不修则 D 的逐位对账在日1 拍必红）；`last_close`
  取清算价（官方收盘价）一并归正——大 A 语义正确，若既有测试锚定旧行为则逐个核对
  语义归属再定。
- `buy_vwap`（新持仓入场参考）t31 不再看到收盘成交：收盘开仓的入场参考退化为现价
  近似（=D3 之前既有形态，D3 台账已登记"加仓迁移仍近似"）；不在本批扩 stash——
  登记待后续（见"遗留"）。

## 遗留登记（本批不动）

- receipt/civil_date 修复后，收盘成交的 VWAP 入场参考近似（上条）。
- tick30 拍挂的日1 开盘窗申报单会被日0 Drain 强撤（roll 先于 Drain 的另一面）——
  既有形态，独立登记待裁决（本批未改簿清语义）。

## 实施记录

（随批补）
