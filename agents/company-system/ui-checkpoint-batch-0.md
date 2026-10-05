# UI checkpoint batch 0

## 结论

- Review 完整度：30/30 个清单项已读到当前 EOF；每个 HEAD diff 均已核对；新增文件读完整个源码；删除项读了完整 HEAD 源码与删除 diff。`apps/web/src/App.css`（1129 行）分段连续读取至第 1129 行，之前一次超长输出有截断，已用连续区间补读。
- 范围：按 `.tmp/company-system/ui-review/batch-0.txt` 原序逐项检查 styles、tests、helpers 和 UI source。未检查 Cargo/index，也未修改实现。
- 领域：没有发现新增/改写 A 股交易制度。新增本人交割、分钟历史/分钟 K 均消费明确标识的真实成交事实，界面保留“股/手/元”区分；行情涨跌仍采用 A 股红涨绿跌。周期 UI 仅为自然日历周期与明确分钟周期，没有把自然日、交易日口径混用。
- 需求与范围：QuickTradingPanel 增加无本人账户时的提交禁用；远程会话、存档、seed、指标数据源与真实历史展示变更均由配套测试或类型接口覆盖。没有发现相对已有 UI 优化的无必要扩张；没有提出 HTML title tooltip。
- 有效 P1/P2：未发现。
- 未运行测试；本 checkpoint 为静态完整源码及 diff 审查。交接上下文记录的 typecheck/Node 状态不作为本次执行结果重述。

## 文件级 EOF 与 SHA-256

以下均已独立读完当前文件，行数为实际 EOF；`diff` 列概述 `git diff HEAD` 检查结果。HEAD 无变化的项仍完整读了当前文件并确认 diff 为空。已删除文件列出其 HEAD 源码 SHA。

| # | 文件 | EOF / SHA-256 | diff 审查范围 |
|---:|---|---|---|
| 1 | `apps/web/src/App.css` | 1129 / `23fa8dc2298cf5a9f4de9aa703e69dc80605d52df08619491e59bf3b511564cf` | 新增 archive-manager 样式；全文布局、移动端、主题和响应式规则 |
| 2 | `apps/web/src/app/QuickTradingPanel.tsx` | 56 / `63f29735c1442867977048294bfca92422ef00914c2292f82ae90341cfcd5828` | 本人账户选择器及提交/自动下单禁用条件 |
| 3 | `apps/web/src/app/command-host-test-fixture.ts` | 27 / `138c435e02b8133655230ecf3cb96775fea9f7bd6c8dc9942ef4472dfb1c0c68` | capability、host stub 与 archive fixture 扩展 |
| 4 | `apps/web/src/app/market-chart-projection.ts` | 171 / `fa1b916edabfcbe0813a7a792cd4ef289fede5b4a1f1ff734f75379ba06e5e1b` | trade count 投影及同点比较完整性 |
| 5 | `apps/web/src/app/private-history-query.test.ts` | 32 / `e63c00d309a49e348a8b73ac6cd11c2996793be44dc65d1c223ff5c18602e25d` | 新增文件全文；无 HEAD 源码 |
| 6 | `apps/web/src/app/remote-login-behavior.test.ts` | 187 / `21d447cfa6c5561725d00d6aedec18c842019559103d399375613693b8e1c1bd` | 新增文件全文；无 HEAD 源码 |
| 7 | `apps/web/src/app/save-commands.test.ts` | 399 / `5371ba2fb0f682b52670e07a3c686c945bf84c833df3c50c0f116e7c84298645` | 全部测试与 diff；host/archive 边界、远程新局、恢复/失败/替换流程 |
| 8 | `apps/web/src/app/startup-recovery.test.ts` | 196 / `00b881e8455b6049d088f65a08e58814493b4c5a0cc1420d6a21fd2f56058e43` | 全文与 diff；App harness、预览 seed 与周期变更边界 |
| 9 | `apps/web/src/app/useSessionHostLifecycle.ts` | 275 / `8b2b66923bf2921b92b9789de1c704208d84d09348f950955d778f1d16d1b0a9` | 全文与 diff；启动/seed/host ownership/remote capability/释放流程 |
| 10 | `apps/web/src/components/ArchiveManager.tsx` | 59 / `6a651afc06730a86924fc5c64884dc8dc1af83a4adf30ed3dffd2c78cbdcafdb` | 新增文件全文；列表与槽位管理错误/忙状态 |
| 11 | `apps/web/src/components/KlinePeriodSelector.test.ts` | 32 / `0a86021df18809411e78e0a899068c01c9a67108c046594368f3f1a13f2d9102` | 新增文件全文；无 HEAD 源码 |
| 12 | `apps/web/src/components/MinuteKlinePanel.css` | 18 / `b5dcf9b134a8fd295a5c3bf3dd97d2370acc944f9c1e9e2d2b80721a8584d457` | 新增样式全文；无 HEAD 源码 |
| 13 | `apps/web/src/components/PriceChart.tsx` | 209 / `2f690cf716e116721998198650dee0906922452fb75bbb13da4aeebf2592c0fb` | 指标来源路由、真实均价样本校验、MA 与 async 结果状态 |
| 14 | `apps/web/src/components/TradeConfirmationTable.tsx` | 50 / `34f92138bf2077a1c751dbb843d714f8b78379b115e9ad0d12d3c026b9d01b18` | 新增文件全文；无 HEAD 源码 |
| 15 | `apps/web/src/components/company/ReportNotes.tsx` | 43 / `e87dc2e80ffc138673c8ce08dff69fa1c46c6c3cdfdfca25f4e4301794d45b84` | report.source 展示、口径文案与附注边界全文 |
| 16 | `apps/web/src/components/company/public-financials-render.test.ts` | 51 / `728ffc9d22e022443d56d8de7b9b22aceb299455a474df98ec8f341e055e8261` | 全文与新增 Simple/仿真完整附注断言 |
| 17 | `apps/web/src/components/company/report-correction-panel.test.ts` | 22 / `226b4b830a7ccde266ba1aebb3d2055375e71b6e88dd5c40a68c44267eb78d32` | 新增文件全文；无 HEAD 源码 |
| 18 | `apps/web/src/components/kline-moving-averages.ts` | 19 / `06a1086c8a6509d0fddd56ef1b068dc2456fc6b6ac9976e77cd73384be793e30` | 复用 exactMovingAverage；历史先算后裁窗及输入边界 |
| 19 | `apps/web/src/components/minute-kline.ts` | 122 / `809604e4927010309cc2e6ce3d21e2e89adadf17f85a45e6c8f50fc6843cf686` | 新增文件全文；分钟周期、时段、真实量额/笔数及 u64 聚合校验 |
| 20 | `apps/web/src/components/personal-trade-history-panel.test.ts` | 154 / `762273a61a41952d65a3e7eb10291adc4e24988d24e9537baa36a262c63965cf` | 新增文件全文；filters、receipt ceiling、分页、过期请求与错误 |
| 21 | `apps/web/src/components/price-chart-runtime.ts` | 280 / `24681b601db2d0af946838e5bdf7b5f4980cf096b5c61a7a1124ff2fc7425292` | 新均价/可编辑 MA series 生命周期及旧 MA 实现移除 |
| 22 | `apps/web/src/components/retained-history-panel.test.ts` | 100 / `a7c5cf3d8705c72f53289ecb36f74623570fe04eacb2031c6fa602f3be4694ea` | 新增文件全文；日期分页、五自然日口径、异步过期处理 |
| 23 | `apps/web/src/config/candle-aggregation.ts` | 已删除；HEAD 全文 SHA `f19a935047a2f4632af92b495a03eef42027e53e165c35dbc3ff2f04528059fa` | 完整核对删除的 storage key/parser/load/save API 与 diff |
| 24 | `apps/web/src/config/moving-average-settings.test.ts` | 19 / `1280167adcf03df72a704480995d63ce20e0270d163314b6bc149c0ac4bca72c` | 新增文件全文；偏好持久化与拒绝坏格式/失败 |
| 25 | `apps/web/src/dev/NpcDecisionInspector.tsx` | 90 / `14e823ee915cfda40162a5d9691aa72615b278fbeb68a9fead96b6e8c45ef46c` | u64 文本解析取代 JS safe integer；诊断数据展示全文 |
| 26 | `apps/web/src/mobile/MobileStockDetail.tsx` | 291 / `7af78d8be6ac3d22d2c3ef7f3052fd8177ea644c3575368f7268a9cfb7e6a2c1` | 指标源、真实分时均价、K线/历史面板与本人交割信息接线 |
| 27 | `apps/web/src/mobile/market-model.ts` | 801 / `fee69b4e90734d40f28552e82b169362a52441e821c664346a3d139bb1bb88ee` | turnover parser 与 PublicTrade 买卖方向采集；全文模型上下文 |
| 28 | `apps/web/src/mobile/mobile-kline-projection.test.ts` | 99 / `d6243a75a183ff9f8fc0a6a39f1ae53cfc80b02a34213d4a4b9b54e9373e6a63` | 自然周月聚合替代交易日模式断言及完整既有投影测试 |
| 29 | `apps/web/src/store/indicator-source.test.ts` | 11 / `c3e37005ac9d336185b758545aff9283c550c1a2604c42b7fc7d520b1fa24aab` | 新增文件全文；无 HEAD 源码 |
| 30 | `apps/web/src/store/store.ts` | 257 / `c8812b268563baa6049faf8a42d9ddd2fcd89c9d4de0c168d41f69e083170c91` | Store slice、新 indicator source state 与 remote membership 装配全文及 diff |

## 独立复核答复

1. 大 A 语义：本批主要是 UI/展示与成交历史传输消费，不改 engine 撮合制度。分钟历史明确使用实际成交量、成交额和笔数，UI 明确以股或手显示；没有发现板块规则被误用成统一默认值。
2. 必要性与范围：UI 的能力声明、宿主生命周期和新组件具有对应跨层用途；已审未发现不必要的实现扩张。
3. 边界与跨层：覆盖空历史、不可用指标、查询错误、过期请求、本人账户缺席、u64、自然周期以及保存/恢复失败路径。未发现具体 P1/P2 漏洞或语义漂移。
