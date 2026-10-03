# 批次 085：web-07 三份 delta 复核材料

## 输入完整性

基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。三份指定材料均按 EOF 全文读取；行数与 scan-plan 登记一致，SHA-256 也逐一匹配：

| 材料 | 行数 | SHA-256 | scan-plan |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/web-07-binding-final.md` | 30 | `4ebc0423dd3e6b82eeb3fe92d45b048acba897f90cb6a61dcefd551bc4e87e86` | 相符 |
| `agents/oop-refactor-audit/exhaustive/reviews/web-07-depth-final.md` | 26 | `994d30d16a552f71c232cb9f27297cfbd6533e784b2ae92b2cf3bd48bdaafbd2` | 相符 |
| `agents/oop-refactor-audit/exhaustive/reviews/web-07-runtime-final.md` | 21 | `eb95e99d7228dc002863d17d7946d641db0c8c9b80230af573ec596a280e4bad` | 相符 |

## 基线关系核对

- `market-depth-sync.ts` 定义 `applyPriceTickMarket`，以 `PriceTickEvent` 更新单个 `MarketSnap` 的 last price、盘口和首档 best bid/ask；缺盘口数组会抛错，空档会令 best 值为 `null`。基线中唯一引用是 `market-depth-sync.test.ts`，测试直接导入 helper 并覆盖非空盘口成功同步；没有发现 production caller。材料明确限制生产 wiring 结论，描述准确。缺盘口异常及空档 `null` 分支尚未由该测试覆盖。
- `runtime-v2.ts` 由 `save/schema/root.ts::parseStrictSaveEnvelope` 调用 `parseSaveRuntimeV2`，并由 `SaveRuntimeV2` 提供生成存档类型；position 边界和整体 schema 测试分别覆盖该 parser。字段归属、封闭词表、有限 IEEE-754 位串、u64 十进制字符串与 `position_step_bp` 基点单位的材料摘要与基线实现相符。未发现 UI/Redux 运行时消费方；其消费是存档解析/恢复边界。
- `store.ts`、`useMarketChartRuntime.ts` 和 `protocol-coordinator.ts` 的基线关系支持 binding-final 对 snapshot writer 的摘要：`setSnapshot` 只更新 snapshot/lastSeq，保留 generation 与工作委托投影；baseline 安装清理投影后图表回调仍可二次派发 snapshot。该关系与没有 runtime snapshot 的增量帧使用 `applyProtocolFrame` 的路径一致。
- ADR-0004 确定 Redux Toolkit 作为前端状态层；ADR-0010 确定统一 host update、generation/序号边界及前端局部刷新。ADR-0007 规定三端共享前端框架。`Q5` 已由 ADR-0004 解决；没有发现三项 delta 对未决 Q 的依赖。
- 本轮 G/Q 对照未发现这三条材料 delta 可直接核销或新增的 G 编号。既有 `G46` 是桌面五档标签与真实报价 rank 不符，属于展示档位映射问题；`applyPriceTickMarket` 的首档投影与该缺口不同，不应据此视为已核销。Q11/Q12 及 ADR-0026/0024 涉及策略与资金规则，与本次 parser/状态投影盘点无直接关系。

## 独立复核结论

三份报告的限定结论与基线核对一致：binding-final 只确认三条 delta 的材料绑定，不宣称完整 web-07 审计通过；depth-final 只通过 helper 直接测试证据并披露未覆盖分支及无 production caller；runtime-final 只通过 DTO/parser delta 并限定 schema 外单位文档与运行时调用方未核实。必要性和最小范围成立，未见大 A 语义变化：此次是审计材料复核，没有改变交易制度、撮合、价格单位或费用计算。现有覆盖遗漏已如实披露；没有把直接 helper 测试扩大解释为生产链路测试。

binding-final 已指出 runtime-final（以及范围外 snapshot-final）抄写的 module SHA 片段错误；本次确认 runtime-final 的输入文件 SHA 本身符合 scan-plan，且该元数据笔误不影响其限定的 DTO 字段审查。没有修改产品、来源材料或旧复核记录；未运行测试、构建或回归。
