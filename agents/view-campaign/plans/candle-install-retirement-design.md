# candle_install 47μs 退役专项设计（报批稿）

> 第 20 任起草 2026-10-19。依据：台账分段画像 candle_install 47μs
> （1000 tick 稳态）+本任代码实勘（成本构成与既有 D1/D6 退役模式同构）。
> **协调者批准后实施；本稿只设计不动代码。**

## 0. 现状成本解剖（实勘）

生产路径 `install_candles`（drive.rs:2213）每拍：

1. **EchoCandles 往返**（world.rs:795 `take_candle_projection`）：
   广播 `DeskMsg::EchoCandles`×5 desk → desk 唤醒+组 `DeskCandles`
   {history: Vec<8 元组>, active: Option<8 元组>} → 收齐+sort。
   - **history 全量克隆纯浪费**：install 消费侧 `for (stock_idx, _desk_hist,
     active) in finals` 的 `_desk_hist` **整个被丢弃**——每拍 5 股×全部
     历史日 K×8 字段的堆克隆+channel 搬运，零读者。
   - 往返固有税：5 次广播 send+desk 唤醒+回传+协调者收齐（与退役前的
     EchoBelief/EchoResting/EchoAccountsSparse 同构——D1/D6 已退役，本件漏网）。
2. **install 循环**（drive.rs:2232）：每股 `session.export_setup_for_runtime()`
   （**循环内重复调用**——Setup 多字段导出×5）+相位判定+`set/clear_active_
   candle_for_runtime`（session 侧 BTreeMap 写）。

## 1. 方案（两刀，对齐 D1/D6 退役模式）

### 刀 1（主体）：active 观察 8 元组随 DeskReport 上行→EchoCandles 生产往返退役

- `StockPublic.daily_active` 现为 5 元组（desk.rs:2054，OHLCV）——
  **install 需要 8 元组**（+turnover_cents/trade_count，trade_stats 源）。
  发布协议加 2 字段（`daily_active_turnover_cents: i64`/
  `daily_active_trade_count: u64`；无版本概念，破坏性改直接上）。
- `install_candles_with_mode` 改读发布版本（`world.publication().load()`
  的 desks[].view.daily_active）——不再走 take_candle_projection 往返。
- `take_candle_projection`/`EchoCandles` 通道**保留为测试对账面**
  （drive.rs:2315 已注明"测试面通道"语义；生产调用退役）。

### 刀 2（机械）：install 循环瘦身

- `export_setup_for_runtime` 外提循环外（一次）；`tpd_now/auction_entry/
  day_tick` 预计算。
- 占位 K 合成的 `world_public_views()` 读改发布版本（同源 last_price）。

## 2. 等价论证（负担逐项）

| 负担 | 论证 |
|---|---|
| 发布时点 vs Echo 时点 | 发布版本 active=终轮 DeskReport 附带（Seal 收敛点）；Echo 时点=install（Seal 后）。desk active 拍内只在成交时演进、Seal 后 desk 只处理下一拍消息 ⇒ 两时点同值（与 accounts_sparse 随 SealAck 同一 S4 论证族） |
| post_day_end 形态 | 日结 Drain 后 desk roll 清 active（None）而发布版本仍持 tick N 值；post_day_end=true ⇒ `active.filter(\|_\| !active_must_be_empty)` 不装载（clear 分支）⇒ 两侧同结果（清空）——不依赖发布版本的 None 语义 |
| 占位 K（无成交日） | active=None 时以收束点 last_price 合成——发布版本 desks[].view.last_price 同源同值（StockPublic 单点构建） |
| DeskReport 载荷 | 每轮+5 股×16B 固定字段（多轮 Continue 重复携带）——对照 belief/resting 分片既有模式，微载荷可接受 |
| stats 字段缺口 | 刀 1 的协议加字段承载（turnover/count 不在 5 元组内——发布面缺口即本设计唯一协议变更点） |
| 测试对账面 | EchoCandles/DeskCandles 通道+world_desk_finals_for_candles 保留（`#[cfg(test)]` 收紧可选——首版不收，登记） |

## 3. 合同红测试（批准后第一批写）

1. `candle_install_matches_desk_authority`：N tick 后 session active K
   ==desk 权威（对账面 take_candle_projection 直读对照——等价锚）。
2. `candle_placeholder_on_no_trade_day`：无成交日占位 K（last_price 合成）
   不变（fault2064 家族回归承载）。
3. `candle_auction_window_active_empty`：竞价申报窗 active 恒空
   （fault2076 家族回归承载）。
4. `candle_post_day_end_clear`：日结后补装清空语义不变。
5. 突变红旋钮：①发布版本 active 篡改 1 分→合同 1 红（判别面=发布
   版本读路径）；②漏 turnover 字段→trade_stats 对照红。

## 4. 预期数字与风险

- **47μs → ~5μs**：残余=5 股 session BTreeMap set/clear+发布版本 load
  （RwLock 读）+循环判定。history 浪费面（5 股×250 条上限×8 字段×
  每拍）整体消失。
- B 1000 口径推算：+42μs/tick ≈ +2.9%（750→772 量级）；A 口径同理
  线性受益。
- 风险：①DeskReport 构建侧加字段的位置/顺序（desk.rs StockPublic
  单点构建——消费面全仓 grep 核对）；②发布协议 PublishedTick 组装处
  （publication.rs desks[].view 透传——字段穿透的编译器面）；③B 探针
  events/守恒锚不动（纯观察面改动，无交易语义）。

## 5. 分批提交计划

1. `test(engine)`：合同 1-4 红先行（现状绿基线+注入面桩），突变旋钮
   编写即红。
2. `perf(engine)`：刀 1+刀 2（合同全绿+突变红①②实证）——A/B 净机
   3 轮配对。
3. `docs(agents)`：证据+台账。

## 6. 开放点（实施中定）

- EchoCandles 测试面是否 cfg(test) 收紧（首版保留全通道，登记复核）。
- 占位 K 合成读 last_price 的 world_public_views 与发布版本切换时点
  （同批改或分步——倾向同批，同源论证一体）。
