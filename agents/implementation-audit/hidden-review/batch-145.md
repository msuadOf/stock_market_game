# 批次 145 独立审查

## 来源完整性

scan-plan batch 145（owner 5）所列三篇来源均从首行连续读取至 EOF；实测行数与 SHA-256 和清单一致。当前 caller worktree `HEAD` 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。历史来源中的候选、迁移步骤与 reader 操作要求均按历史材料审查，不视为本轮实现授权。

| 来源 | 行数 | SHA-256 | 别名数 | 校验 |
|---|---:|---|---:|---|
| `modules/engine-pipeline-12.md` | 45 | `3e48df345266ac33d03ee2b70fb3af89be170f91841fdd35e002c93634747cc7` | 4 | 与 scan-plan 一致 |
| `reviews/engine-pipeline-01.md` | 52 | `ebda0069b094de4c498d555cf25217472116fa5aff8ef0eabc43963bf745fb7b` | 2 | 与 scan-plan 一致 |
| `reviews/engine-pipeline-02.md` | 45 | `bb7652fc20cbc4aaf0d6c83f705cbf54db015b0a0058a8ed0d5c793597386441` | 3 | 与 scan-plan 一致 |

已阅读当前 caller 根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`，以及实现审计总账、candidate-resolution-03 和适用决策。ADR-0017 为现行 escrow/receipt 基线；ADR-0018 整体仍为 proposed，其明确接受的局部交易顺序约束按文本适用，未接受的 immutable timeline、COW、WAL 等提案不升级为承诺。ADR-0024 确认费用可使投资者资金池减少，不补钱。开放问题中相关 escrow 与资金池问题已有决策；没有因本批对象归属材料而重开问题。

## 当前实现与职责

`transition.rs` 在基线中已有 `BuyFillInput`、`SellFillInput` 和纯输出 DTO `FillTransition`，并实现 `buy`、`sell`。买入分支检查正成交量/金额、checked 累计金额与费用、现金预留和释放；卖出分支核对历史 nominal/charged 费用事实、按现有规则计算累计费用、实收分配和净交付。错误映射为 `StepFatal::InvariantViolation`。它不持有跨腿或订单生命周期状态，也不决定撮合入口次序。

实际 caller 是 `stock_auction.rs::fill_receipt` 和 `continuous_matching.rs` 的成交处理：二者按 `Side` 构造对应 input。它们将 transition 的 `ReceiptDelta`、nominal/charged 增量及累计值、交付股数/现金纳入 `EnvelopeReceipt` 和 envelope audit；外围 owner 继续控制订单簿、阶段、撮合先后、shadow 和后续收据/结算。未发现历史模块提出的额外聚合对象有必要，也未发现候选 retain 判断与当前调用链冲突。

## Review 交叉核对与 G/Q

- `engine-pipeline-01` 的旧 review 曾指出 A01 lane patch adoption 覆盖不全、`apply_update` 不应凭空 fallible；其 delta review 明确两项候选文本缺陷已修订，并准确区分候选设计通过与源码尚未迁移。基线 `account_validation.rs` 仍由 `AccountValidationState::prepare_account_round` 合并 lane 字段，`apply_budget_update` 使用 `expect` 的内部不变量边界仍在。它们属于该 review 明示的源码状态/独立线索；不能误说 A01 已实施，也没有足够证据把 `expect` 描述成合法输入必然可触发的交易失败。该 A01 与本批 transition owner 无直接关系。
- `engine-pipeline-02` 最终 review 对 `AdaptivePlanChainCoordinator` 的 P3 预检错误早退描述精确区分了 `failed` 状态与 `finish` 的 exhausted/pending 前置条件。当前实现缺少预检失败锁存及相应边界测试；candidate-resolution-03 已将它裁为一项独立确定缺口，建议另登记 G，且明确不并入既有 G/Q。它不是 transition/OOP 候选，也不应从本批重复扩张。现行总账 G01–G68 未列为已核销项；本批不更改任何 G/Q 状态。
- 未发现本批可直接映射到现有未决 Q 的 transition 对象抽取项。G16 是历史状态复制的性能缺口，不等于成交腿计算边界，也不因 `FillTransition` 存在而核销。

## 大 A 语义与结论

本批属于现有成交腿计算职责的归属复核，没有提出或实施交易规则变更。文档描述与 ADR-0017 的费用收据、资源 envelope、卖出费用逐腿实收封顶及佣金/印花税/过户费分配顺序相符；ADR-0024 的资金池减少决定也没有被误表述为应补款。成交腿 transition 不负责 T+1、交易阶段、证券类别、订单优先级或结算。未重新检索交易所/中国结算官方材料，因此不把本记录视为官方规则复核或完整大 A 合规认证。

历史模块记录了既有买卖费用与预留测试及建议补充的零/负值、溢出、费用分项跨界测试；review 明确未运行测试。本轮仅做静态核对，未运行测试、构建或回归，不能宣称行为验收通过。未修改产品代码、正式规则或 G/Q 状态。

结论：transition 的历史 retain 结论与基线 owner/caller/consumer 一致，无需对象迁移；独立 P3 coordinator 缺口按 candidate-resolution-03 跟踪，不归并至本批。完整性结论只覆盖 scan-plan 指定三篇来源及上述局部代码/决策交叉核对。
