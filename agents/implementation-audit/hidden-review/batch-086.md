# 批次 086 文档复核

## 输入完整性

- `agents/oop-refactor-audit/exhaustive/reviews/web-07-snapshot-final.md`：原文 21 行，SHA-256 `73fa3bc5b6d6ff321658e5fb21c31cd19d3691bca0439b1f9c5f1eae322d1f47`，连续读至 EOF。
- `agents/oop-refactor-audit/exhaustive/short-scan-guide.md`：原文 12 行，SHA-256 `c13c337280573ba64efd31a061a86197e459839096def8a3dacc34188b78e69e`，连续读至 EOF。
- `agents/oop-refactor-audit/exhaustive/unit-guide.md`：原文 11 行，SHA-256 `bef6f6f3b41b0fba9dfd0245248d9d7b633d5f8c62e327578e996a49abcd94a3`，连续读至 EOF。

## 复核结论

snapshot-final 对候选里 `setSnapshot` 的状态契约与实际生产调用链的描述准确。当前 `store.ts:43-46` 只改 `snapshot` 和 `lastSeq`；`installProtocolSnapshotBaseline` 在 `:47-53` 还更新 `generation` 并清空委托投影/ready 标记。`ProtocolCoordinator.installBaseline` 在 `protocol-coordinator.ts:74-85` 先安装协议 baseline，再回调图表；`publishSnapshot` 的自然日及带 runtimeSnapshot 路径在 `:117-127` 先派发协议 baseline 或 runtime delta。图表 `useMarketChartRuntime.ts:25-29` 的 `replaceSnapshot` 再派发 `setSnapshot`；`acceptReduction` 在 `:51-70` 将自然日/完整 runtime snapshot 送入该路径，无 runtime snapshot 的 tick batch 则派发 `applyProtocolFrame`。`installBaseline` 在 `:78-84` 也调用 `setSnapshot`。因此候选没有把双写误说成 legacy-only，也保留了与 delta 路径的区别。

章节族/状态：现行产品审计总账 `implementation-audit-2026-10-02.md:25-28` 将 G01–G68 定义为产品缺口族（G27 已核销，其余保留未完成部分）；`web-07` 是 OOP 调查候选族，不是新的 G 编号映射。当前文档没有证据把 snapshot writer delta 对应到某一 G 条目，也没有证据核销既有 G 项。总账 `:133-150` 对 Q01–Q11 区分了未决契约与缺口；本候选不改变这些领域事实。后续 ADR-0023–0028 主要覆盖合成历史/撮合边界、资金池、机构经历、日终存档、部署与发布；未见其替代或冲突该 Redux writer 调查。Q11、Q12 的专门决定（ADR-0026、ADR-0024）也不涉及此候选。

领域语义与必要性：这是 Web Redux/chart 状态写入口调查，不触碰撮合、申报、T+1、费用或 A 股交易规则；无需增加交易所依据。完整列明 writer、generation/委托状态差异和 caller 顺序，是未来对象提取保持行为所需的最小材料。既有复核认为仅覆盖 snapshot writer delta；本轮产品 caller 核验再证其结论。没有发现需要新增的反证或遗漏边界；是否减少重复派发仍须另定行为契约，不应当作本 OOP 提取任务的修复。

本结论只确认所列 Redux/chart 调用路径和文档 delta，不等于对 G01–G68 进行源码全量复审、对其它 web-07 条目作完整 diff 审查或宣称行为测试通过。
