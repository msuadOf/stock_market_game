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

## 终验（rebase main `1554b89a` 后）

- 分支提交链（rebase 后哈希）：`f6553831`（红证）→`b1a373c8`（修复主体）
  →`6f89263c`（复核建议补强）→`caa4eb12`（台账）。原 `ab45c082`
  （main 预存编译红收口）在 rebase 时被 main 侧同值收口取代（B4b
  预存债批/合并带入，注释更佳——取 HEAD 侧自然为空跳过）。
- 红证归因复核（新 main 上重跑红证 commit）：双形态仍红（占位 v=0/
  冻结对账硬红）——B4a 的 settle_fill 重锚定为不同层，无误判。
- 日界测试 4/4 绿（占位/部分丢失/多日独立/清算价≠锚定）。
- `save_contract` 38/0（预存债收口未回退）；wasm check
  `cargo check -p engine --target wasm32-unknown-unknown` 0 错。
- tsc：分支 diff 零 .ts 文件（engine .rs+台账 .md），tsc 面≡main；
  本 worktree 未装 web 工具链，未重复跑（B4b 台账 tsc 0 旁证）。
- **main 预存红（非本批）**：`tests/session.rs::
  full_day_accounting_rejects_a_missing_day_boundary` 在 main `1554b89a`
  上同样红（休市股分钟桶×元旦开局 fixture 与日历面——已实测对照；
  cargo 默认 fail-fast 使历代全量在中途目标失败后不再跑该目标，疑似
  因此长期被掩盖）。本批不修（非日界编排域），上报协调者。
- 全量画像：见 full6（--no-fail-fast）——以 main 同口径对照区分
  本批红与预存红。

## 实施记录

- 提交链（rebase 至 main `74006973` 后）：
  - `7f45bfa7` 红证（双形态；占位=install v0 vs 期望 200、部分丢失=冻结对账
    bars=500/candle=300 硬红——**在新 main 上复跑仍双红**，归因闭合：B4a 的
    `settle_fill` 占位 K 重锚定修复（main `5a940264` 带入）是测试锚定泄漏层，
    与本批编排本体不同层，无重复功劳）。
  - `8a9da711` 修复主体（desk unroll/reroll+协议扩两字段+ends_day 段补真+豁免
    收紧）。
  - `ab45c082` main 预存编译红收口（74006973 回收批夹具漏 B4a 新增
    `PublishedReport.source`——文本干净合并语义断裂；取值对齐生产前史播种
    `publish_closed`=SimulationAccounting。**非本批引入**，解锁全量测试）。
- 实施修正 1：应答终 K 初版取 `candles.last()`（closing=0/tpd=1 极端几何下取到
  **预置历史**条目，`public_step_day_boundary_candle_state_machine` 显红）——
  重构为 `roll_day` 显式暂存归档 K（`rolled_day_candle`），unroll 从暂存还原
  （不 pop 归档位）、reroll 原位覆写归档末条；应答终 K 仅在**本次 Drain 实际
  执行收盘清算**时携带（`drain_cleared_closing`），closing=0/未跨日手动日结
  =None（driver 跳过补真，install 面维持既有形态）。
- 实施修正 2（B4a 合并后重定位）：rebase 零冲突；B4a 的 settle_fill 首笔成交
  重锚定与本批正交（收盘-only 日的开盘价=清算价由 B4a 侧保证，归当日量/额/
  笔由本批保证）。
- desk 面 8 元组的额/笔投影恒 0（`DeskCandle` 结构无 stats）——desk/install
  逐位对账覆盖 OHLCV；额/笔经 install 面断言+bars 冻结对账双重覆盖（测试注释
  锚定）。

## 独立复核（fork，未实施者）

结论：**阻断 0**；建议 7（S1-S7）。处置：
- S1 run_day_end 失败路径补 `pending_day_candles` 对称清理——**已修**（b24b82c9）。
- S3 多日连续日界 session 级测试——**已补**（双时钟契约 civil 驱动；日 0/日 1
  独立冻结+install 面两条不互覆）。红证期间实证：纯 step 路径不推进 civil，
  日 1 冻结会被同日幂等检查跳过——测试按 authoritative_tick_tests 同式驱动。
- S4 清算价≠锚定变体——**已补**（双边同价 1005：开盘价=首笔成交价 1005，
  B4a settle_fill 重锚定经本批 unroll 通道生效；次日昨收=官方收盘价）。
- S2 手动路径 receipt 入账失败回滚不完整（civil 不回退）——登记（既有类，
  触发=结构性单调违约防御）。
- S5 restore 后继续跑次日——登记遗留。
- S6 手动日中日结后（bars_shipped_today 分支跳过 unroll）边界 Drain 收盘
  成交落次日 live K——manual 面既有漂移类窄形态，登记。
- S7 Trade/Auction 类段近似——已登记（web 按 seq 排序展示，无正确性影响）。
- 复核确认项：C1 大 A 语义（收盘段归当日 K/官方收盘价/涨跌停基准/traded_
  days_total 恰一次）C2 None 边界（fault1009 ⇒ tpd=1 必 closing=0，防御
  Err 合法 setup 不可达）C3 queries.rs 修复正确 C4 字段还原完整+abort 对称
  C5 ends_day 顺序与 seq 单调 C6 豁免退役安全 C7 回滚覆盖。

## 性能门（B 探针 1000 tick 配对，load 45-50 外部污染窗）

| 口径 | main 74006973 | 分支 | 差 |
|---|---:|---:|---:|
| 1000tick×3 轮中位（speed_x） | 952.2 | 905.8 | −4.9% |
| events（逐位） | 5931×3 | 5931×3 | 一致 |

固定侧逐轮劣化（924→905→832）与外部负载下行并存、跑窗仅 ~1.06s——
1 秒级窗噪声主导疑点大；本批热路径（每拍 desk/driver/install）零改动、
fixture 单日不跨日结（tpd=15300），机制上无回归面。

**3000 tick 加长窗仲裁（load 43-60 外部污染窗）**：
- 正序 5 对：baseline 中位 1109.2 / fixed 中位 1024.5（−7.7%）——但对内
  配对差 −25%/−20%/+19%/+3.4%/+3.5% 剧烈翻转；
- **换序 4 对（fixed 先跑、对间 5s）**：fixed 中位 1159.3 / baseline 中位
  1172.0（**−1.1%，±2% 门内**）；对内差 +4.7%/+6.6%/−1.3%（r1 fixed
  −36%=批次静置后**首跑冷启**——正序批的首轮慢者同样随位置翻转）。
- 判定：短窗批（1000 tick）的逐对负差与加长窗首两对的负差均为**冷启/
  外部负载伪影**（慢者位置随换序翻转）；暖轮中位 −1.1% 门内。本批改动
  在该 fixture 的执行面（单日无日结）**逐指令零差异**（events 21917
  逐位一致佐证）。按"测量受污染如实登记"收口：**不判定回归**，净机窗
  复测留待后续（B4b 批在更净窗对照 −0.02% 可作旁证）。

## 遗留登记（更新）

- Abort-重跑双计：日结 Drain 的市场面变更（清算成交入 K/桶）无 Market 快照
  进 undo（既有形态——今日 main 同样在重跑时经 live 桶 merge 双计）；本批
  unroll/reroll 未扩大该类（成功路径单遍精确）。独立批次可评估 Drain 前推
  Market 快照。
- manual `day_end` 路径（driver.day_end / day_end_with_runtime_market_day）：
  receipt 已即时入账（本批）；但该路径不做 boundary 迁移 ⇒ install 面无当日
  提交条目，终 K 无补真落点（冻结对账跳过分支保留）——既有登记不扩批。
- 收盘成交的 `buy_vwap` 入场参考：t31 不再复现（事实移入日结段）——收盘开仓
  的经历入场参考退化为现价近似（D3 之前既有形态）。
