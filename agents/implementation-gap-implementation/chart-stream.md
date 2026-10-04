# 分时与逐笔流补齐记录

## 范围与依据

- 本批负责 G10、G11、G31、G14，并提供 G12 的真实价格方向；UI/CSS 消费者由 `implement_ui_contracts` 协作接线。
- 已读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、ADR-0009、ADR-0010、`UX-CONTRACT.md` 的分时/分钟/逐笔/局部刷新条款及 `agents/implementation-audit/reaudit-ui.md`。
- 本批只修复展示投影，不改变沪深市场受理、撮合、计费与交易单位；时间沿用现行 ADR-0009 默认 900 tick 开盘窗口、240 交易分钟、压缩午休的游戏映射。Q04/Q07/Q08 不作裁决。

## 实现

- G10：同一连续分钟把各帧累计量差累加，开盘竞价 `Completion.matched_volume` 初始化累计量基线，连续分钟不会再次计入竞价量。
- G11：按权威 frame.tick 的交易日清空旧日分时/竞价/累计量；同一批次越过日界后继续保留新日采样，权威日 K 不清空。
- G31：accessor 返回稳定冻结的只读数组；无关证券、同分钟无变化采样与未变化日 K 均保持引用。`replaceSnapshot` 与 `replaceActiveCandles` 按实际 OHLCV/成交统计判定变化；hook、Provider、PriceChart 的数组类型同步为 readonly。
- G12：连续分钟末价对前分钟末价判定涨跌，首个连续分钟优先取最后有效竞价价；没有前价时首点为涨色。竞价按相邻有效指示价判定；null 不冒充价格。平价沿既有 collector 的 `>=` 涨色语义。颜色不表示主动买卖。
- G14：每个 protocol trade effect 增加所属 NormalizedTickFrame.tick，CivilUpdate 使用自身权威 tick；前端 TradeEvent 增加可选 tick，不改生成的 Rust/wire Trade。`formatTradeTime` 使用逐笔 tick 显示秒级时间，午休/收盘端点明确，缺字段展示“成交时间缺失”。MobileStockDetail 逐行消费者由协作 owner 修改。
- 旧 MobileIntradayProjection.tradeTime 字段保留以避免无关公共 API 删除；正式逐笔消费者应不再使用当前统一时间。

## TDD 与验证

- 首次隔离默认 Node 启动退出 1，未得到有效 case 结果；改用现有项目的 `--test-isolation=none` 后复现 4 个明确失败：分钟量覆盖、数组可写、竞价量基线/跨日及引用不稳定。
- `trade-time.test.ts` 在实现 helper 前明确失败：模块未导出 formatTradeTime。
- 定向命令统一使用 `timeout 10s node --experimental-strip-types --test --test-isolation=none --test-timeout=10000 ...`。
- `market-chart-projection.test.ts` 最终 7/7 通过；覆盖同分钟累加、竞价基线、同批次跨日并继续采样、分钟内部反弹方向、竞价 null 价与降价方向、不可写及分时/竞价/日 K 引用稳定。
- `protocol-effects.test.ts` 3/3 通过，约 0.26s；新增跨帧各自 tick 与 exact-retry 不重复追加。
- `trade-time.test.ts` 1/1 通过，包含 09:25、09:30、11:30、13:00、15:00、次日、缺失/非法 tick。
- 上述三套与 `protocol-reducer.test.ts`、`market-model.test.ts` 最终合并定向短测 52/52 通过，约 0.24s。
- 编译与定向短测通过 Promise.all 独立进程并行；`timeout 10s node node_modules/typescript/bin/tsc -p tsconfig.app.json --noEmit --incremental false` 两次在约 6 秒内退出 2。本模块 readonly merge 参数错误已修，剩余是并行施工中的 UI Props/setter 与 host async 接口不匹配，已通知协作 owner，不将编译失败宣称通过。
- 未运行完整回归、浏览器矩阵或发布验收；完整 diff 的独立复核与 UI 消费验证由主任务统一安排，未经复核不宣称已完成门禁。

## 独立复核修正

- `review_chart_stream` 指出两项有效发现：同六秒竞价槽把自身当比较前价，平价更新翻色且更换引用；`AfterClose+BeforeOpen` 的 CivilUpdate 携旧日历史，tick 不前进时旧图未清空。
- 两项均先补失败测试，分别明确得到同槽数组引用/方向断言失败、production SSR BeforeOpen 分时非空断言失败。
- 竞价方向现改为与前一个有效六秒槽的最后指示价比较，与连续分钟的前槽语义一致；当前槽同价同量不刷新引用，槽内反弹仍低于前有效槽时保持下跌色。
- production hook 对含 BeforeOpen 的 CivilUpdate 显式重建空分时/竞价/累计量缓存，再安装权威日 K；AfterClose-only 非交易日仍保留真实收盘图。
- 修后原定向短测 53/53 通过，约 0.27s；`apps/web` 内执行相同 timeout/Node 参数的 `src/app/market-chart-runtime.test.ts` production SSR 5/5 通过，约 0.77s。两套独立进程并行执行。
- 再次编译约 5.88s 退出 2，仅余并行施工 `App.tsx`/`UserPanelProps.pausePreferencesPending` 契约一项，已交主任务协调。
