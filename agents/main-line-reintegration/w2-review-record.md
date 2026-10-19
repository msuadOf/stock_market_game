# 重集成 W2 批（web K线周期+均线/指标源接线）实现与复核归档

> 2026-10-19。分支 `feat/web-reintegration-w2`（基于 main 0d9f15a1，含批次1取回件）。
> 实现：W2 批实现 agent；复核：独立 fork（未实施本批改动），只读。

## 取回结果（3 提交 / 33 文件，+644/−140）

| 组 | 提交 | 内容 | 自身测试 |
|---|---|---|---|
| 组1 聚合核心 | 292de65d | checkout：turnover.ts、kline-periods.ts、KlinePeriodSelector(+test)、calendar-candles(+test)；main 版：market-model.test/mobile-kline-projection.test 聚合 hunk；删 candle-aggregation(.test) | 47/47 绿 |
| 组2 周期接线 | d5a5446a | main 版：mobile-ui-state（MobileChartPeriod=ChartPeriod）、ChartPeriodTabs；手改：MarketKlinePanel 周期路由、MobileStockDetail/LocalRefreshViews 分派+显式占位、trading-rules.md 登记 | 35/35 绿 |
| 组3 均线/指标源 | 138f04a1 | checkout：useMovingAverageSettings、indicator-source-policy(+test)、store/indicator-source.test + main 版 8 文件（chart-settings-slice/ChartDisplayMenu/kline-moving-averages/price-chart-runtime(+test)/DesktopIntradayChart/PriceChart）；手改：store.ts（仅 IndicatorDataSource hunk）、engine-host.ts（仅 4 纯类型）、LocalRefreshViews/MobileStockDetail 接线+能力面派生 | 51/51 绿 |

MarketKlinePanel.tsx 与 18725d07 **逐字节一致**（`git diff 18725d07 HEAD -- <path>` 为空）。

## 关键裁决与 W3 剔除面（复核已逐项核对）

- **basis 裁决**：删 trading-days 口径，依据 trading-rules.md L134「按用户最新决定，
  不保留每5／20个交易日聚合选项」；candle-aggregation 为 HEAD 零调用方死设置。
- **刻意未取（留 W3/B3）**：store.ts 的 Coded\*→裸类型回滚与 remoteMembershipReducer；
  engine-host 的 EngineHost 接口/HostCapabilities 扩展；MobileStockDetail 的
  TradeConfirmationTable/canControl/confirmations 族 props；market-model.ts 的
  PublicTrade/intradayAverageYuan 重构；App.css `.archive-manager`；测试 hunk 中
  MarketSnap 新字段与 maker/taker 字符串化。
- **占位策略**：五日/分钟K 选择器选项原样取回，未接线周期渲染显式「尚未接入」占位，
  不静默空白也不回退画日K；「分时」不可能到达 MarketKlinePanel（两处渲染点均
  calendarCandlePeriod 守卫）。
- **指标源能力面**：localIndicatorCapabilities 按计算器就绪派生（main 的 PriceChart
  默认值同款）；calculateIntradayAverageCurve 保持可选，选 Rust 源时显式
  「计算入口未接线」报错。默认前端源下 KDJ/MACD 显示不支持 = main 既有语义
  （store 测试断言默认 frontend，前端源只算分时均价）。

## 验证口径

- `tsc -b`：错误集与基线 32 条逐行 diff 完全一致（.tmp/w2/tsc-baseline.txt）。
- 逐 shard 串行全量：**1129 通过**；失败=既有 3 项（2×wasm worker flaky、
  file-target-permissions 环境失败，批次1台账已登记）+ color-contrast（chromium
  负载抖动：输入仅本批未触碰的 3 个 CSS，组2/组3 状态均复现过通过）。
- 并行全量 runner 出现整 shard 静默死亡：查实为同机另一 agent 基准进程占 89 核
  （load 76）导致批死线击杀吞输出，与代码无关（stash A/B 同条件复现）。
- oxlint：IntradayPanel 2 条 exhaustive-deps 告警为 18725d07 原版代码自带（deps
  刻意字段级），其余告警均为基线既有。
- 过程事故（已复原）：一次 CWD 漂移把 checkout 误落入本机另一检出
  /home/baiyifan/data1/stock_market_game，已完整恢复并核实无残留。

## 复核结论（阻断 0 / 建议 3，已登记）

1. FundsPanel 保留 HEAD「旧存档缺统计」文案，未取 main「不从截取成交缓存推算」
   新措辞——W3 随 G6 数据面统一。
2. App.css `.archive-manager` 样式未取（批次1 G7 遗留，组件未挂载暂不可见）——
   挂载批次（B3 App.tsx 接线）补。
3. 五日 tab 已启用但渲染占位（与 KlinePeriodSelector.test 的 no-disabled 断言
   一致）——W3 接线后占位自动消失。
