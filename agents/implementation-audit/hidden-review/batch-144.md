# Batch 144 独立审查

## 来源完整性

清单基线 `43b1aa5` 与源码仓库当前 `HEAD`（`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`）一致。三个来源均从首行读至 EOF；实际行数与 SHA-256 均和 `scan-plan.json` 一致：

| 来源 | 行数 | SHA-256 | 核对 |
|---|---:|---|---|
| `engine-pipeline-09.md` | 47 | `b1e10feb42a32f322bfd3bd119612667eaec6b9c631fa703356ab76c2edd9c04` | 一致 |
| `engine-pipeline-10.md` | 38 | `e995084608fc2e24d7fa55676da438335b64544b416d78b84db3bce9201a74f8` | 一致 |
| `engine-pipeline-11.md` | 85 | `6c7ea859affd69ed56876b3d92db5c9c1e99286518ef001dfb66b038cf277fac` | 一致 |

这些文件位于 `before/exhaustive` 历史审计目录；其中的操作性文字作为被审查的历史记录处理，不视作当前工作指令。三篇均明确“候选设计，未实施”，这描述了撰写时状态，不代表基线代码状态。

## 复核结论

1. **A 股语义：有条件认可。** 文档记录了沪深竞价差异、竞价阶段、挂单生命周期、资源截点、receipt/settlement 单点提交、单位与已登记简化，并明确 ADR-0018/0020 的提案状态；没有把候选重构包装成交易规则。本文只核对项目当前源码与历史记录，没有重新查询交易所/中国结算官方规则，因此不独立背书记录中的外部规则依据及适用日期。批次内容没有指明新增或改动 A 股语义。
2. **必要性与范围：历史提议范围克制，当前实现有对应落点。** `engine-pipeline-10` 的 `InstitutionalFacts(ExperienceMoment)` 候选已在 `retail_projection.rs` 中实现；机构入口传入必需 `moment`，Retail 分支仍独立。`engine-pipeline-11` 的 `StockStreamCoordinator<S>` 候选也已在 `stock_stream.rs` 中实现，owner 仅持本 tick 股票调度与 channel 状态。两个都是具体不变量/生命周期的封装，不扩展到账户、结算或 session authority。
3. **当前调用与消费者：** `ReadyStockStream` 在 `continuous_tick_transaction.rs`、`auction_tick_transaction.rs`、`pre_open_transaction.rs` 构造并消费 typed 进度；三条事务路径调用 `drive_stock_stream`。`project_institutional_receipts` 经 settlement/projection 链路消费，当前类型签名要求提供 `ExperienceMoment`。完成顺序不应被解释为跨股票或账户的业务优先级；P9、ReceiptAggregation 与 Settlement 仍由外围事务负责。
4. **覆盖与残余限制：** 历史文档列出了丰富的既有测试覆盖，但均声明未执行；本次也未执行测试。`engine-pipeline-11` 标出的提前关闭接收端/worker payload 错误边界仍值得确认其测试是否覆盖；仅凭本批历史文档不能确认它后来是否补齐。建议将其作为验证覆盖点，而非仅因文档存在就断言当前缺陷。

## 限制

仅审查本批 3 篇历史记录与当前基线相关源码调用点；未逐项重审每篇所列的 40 个左右 Rust 文件、ADR 原文、交易所原始规则或对应测试实现。`git status` 显示工作树有既存未跟踪目录，本审查未改动其内容，也未改变 Git 状态。
