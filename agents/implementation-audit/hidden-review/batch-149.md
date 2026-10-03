# Batch 149 复核

## 来源完整性

清单为 `scan-plan.json` 中 `source_baseline=43b1aa5` 的 batch 149。三份来源均已从首行连续读至 EOF；当前工作树行数与 SHA-256 和清单完全一致：

| 来源 | 行数 | SHA-256 | 完整性 |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/execution-before/reviews/engine-pipeline-12.md` | 28 | `10511f8477e6ee05bfd8bf4ea74f61561bf0e40e13f4506d37992b822ccc7c6e` | 一致 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/execution.md` | 15 | `5b2dfb2caab216689f22612996aff9a24d564f28ba6a1abdc98c7a1affaf6170` | 一致 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/frontend-before/reviews-contracts-01.md` | 29 | `ff6417b99019cd80591d5d8e27de32fa628238b110a66e8d901eefe73704966c` | 一致 |

这三条路径不在 Git 基线 `43b1aa5` 中，故无法从该提交独立重取原始字节作基线对照；这里只能确认工作树文件符合计划内记录的哈希/行数。这些记录描述的是过往审计工作，不把其中要求 reviewer 执行或 manager 已完成的任务视为本轮指令。

## 内容与现状核验

- `engine-pipeline-12` 历史复核的领域边界判断与当前源码吻合：`FillTransition` 仍是单成交腿纯计算边界；当前调用方为 `stock_auction.rs` 与 `continuous_matching.rs`。查阅到的当前测试调用也包括 `transition_tests.rs`、`persistence/v2_tests.rs`、`ledger_state_tests.rs` 和 `continuous_matching_tests.rs`。资源与费用语义被限定为游戏内 envelope/收据及文档登记的简化，没有声称它等同交易所清算；本批没有交易制度变更主张。
- `execution.md` 是执行交接/历史状态记录，不是实现契约。它将源码未决事项、审计材料范围、曾经的并发及阶段动作描述为当时状态，不能据此认定当前事项仍待做或已在产品中实现。当前 `agents/oop-refactor-audit/exhaustive/modules/engine-pipeline-12.md` 仍把此模块明确记为候选设计、未实施。
- `contracts-01` 的 DTO 保留判断与当前层次一致：Web `Event` 为生成传输类型，`apps/web/src/types/engine.ts` 仅导入并别名引用它；实际解析、协议验证和 effects 分别由 host/protocol 消费。生成机制由根 `package.json` 的 `types:generate`/`types:check` 声明，生成目录为 `apps/web/src/types/generated`。
- 历史复核已将根 `bindings/` 来源和消费方标作未核实。当前 `.gitignore` 仍忽略 `/bindings/`，工作树中存在 `bindings/Event.ts`，但本次未找到足够证据确认其实际消费者或历史生成来源；继续保留“未核实”限制，不将它与 Web generated 声明视作已证明等价。

## 结论与范围

未发现三份历史记录与可直接核查的当前所有权/调用关系相矛盾的实质问题。边界：本批是对三份审计记录及其关键当前引用的复核，不是对完整 `contracts-01` 256 个 DTO 或 `engine-pipeline-12` 两个完整源码文件重新审计；未运行测试或构建，未修改产品文件或 Git 状态。基线提交不含这些档案来源，是明确的来源核验限制。
