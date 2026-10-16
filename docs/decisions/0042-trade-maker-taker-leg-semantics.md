# ADR-0042: Trade 事件 maker/taker 字段语义统一为腿方向

- **状态 (Status):** accepted
- **日期 (Date):** 2026-10-16
- **决策者 (Deciders):** 协调者拍板（方向 A）+ 第 10 任实现 agent 执行

## 上下文 (Context)

std::thread 迁移引入的 runtime 管线（`DeskTradeFact`，runtime/protocol.rs）自 Sol 六审起按
**腿方向**归一化 `maker≡卖腿/taker≡买腿`（费用归户需要：佣金双边、印花税恒卖腿、过户费双边）。
旧管线（`orderbook::Trade` → `Event::Trade`）字段含义为**到簿序**（maker=被动方先挂、
taker=主动方后到），两管线语义分叉——翻转终态（默认翻转）前必须统一。

关键事实：A 股真实交易制度**不存在 maker/taker 费差**（美股式流动性返佣不适用）；
字段名承诺的"主动/被动"语义在 A 股语境没有对应的费用/结算差异。前端消费面全量扫描
（`.tmp/maker-taker-consumer-scan.md`）证实：apps/web 全部消费点为对称判定
（`maker==0 || taker==0`）或纯透传，无一依赖主动/被动假设；逐笔成交染色按价格对比推导、
不读 maker/taker。

## 决策 (Decision)

统一为**腿方向语义**：全引擎 `Trade.maker≡卖腿、Trade.taker≡买腿`（字段名不变，赋值含义
翻转；`maker/taker_order_id`、`maker/taker_filled_value_before` 三对字段成组互换）。
实现：旧管线 `orderbook::Trade` 连续撮合构造（incoming=Sell 时互换）与集合竞价构造
（无条件 maker=sell/taker=buy，删除 seq 比较）；`continuous_matching` 的
buyer/seller EnvelopeKey 推导与 draft.side 解耦（buyer 恒 taker、seller 恒 maker）；
`causal::aggregate` 的主动腿校验按 side 选腿。序列化契约与 TS 类型结构不变。

依据日期：2026-10-16（A 股现行费用制度：佣金双边各付、印花税卖出方单边、过户费双边）。

## 备选方案 (Alternatives Considered)

- 方案 A（采纳）：统一到腿方向 — 无歧义、与 runtime 费用归户一致、前端零适配。
- 方案 B：runtime 侧 desk fill 事实带到簿序、事件层映射旧语义 — 保留"主动/被动"信息但
  引入映射层；A 股无对应费用差异，字段语义继续误导（Sol 在 D3a 复审已指出 DeskTradeFact
  字段名承诺了它没有的语义）；否决。

## 后果 (Consequences)

- **正面：** 两管线事件契约统一（翻转无分叉）；费用归户逻辑唯一化；后续前端可从
  taker=买腿直接获得真实主动方向（现为价格对比近似——登记为后续增益项）。
- **负面：** 旧管线约 20 处测试断言按新语义互换（本轮已适配）；`verification_evidence`
  的证据 JSON 基准值在卖主动场景互换——基准重建时须重算（登记）。
- **中性：** `Event.ts` 结构不变（注释变更后重跑 ts_rs 导出即可）。

## 前端消费面清单（全量扫描结论）

必改 0 处；复核通过 7 处（apps/web/src/App.tsx:336、player-orders.ts:60、
host/protocol/parse.ts:91、stock-identity.ts:149、types/generated/Event.ts:27（重生成）、
priceHistorySlice.ts:16 + market-chart-projection.ts:49（价格对比染色，不读字段）、
MobileStockDetail.tsx:140）；逐笔成交 UI（MarketTradeTape/TradesPanel/tradesSlice）纯透传。
