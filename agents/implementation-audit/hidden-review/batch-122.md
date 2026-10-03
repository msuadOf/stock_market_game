# 隐藏复核批次 122（owner 2）

## 范围与完整性

按 caller `agents/implementation-audit/hidden-review/scan-plan.json` 的 batch 122 清单，从主仓逐篇连续读取三份来源至 EOF。路径、行数和 SHA-256 均与冻结计划一致，读取记录见同目录 `batch-122.json`。产品代码基线为 `.worktree/implementation-reaudit` 的 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；本批只新增本审阅记录及 JSON，不修改产品、规则或历史来源，不运行测试、构建或联网核验。

## 来源审阅

- `pipeline-manager.md`：记录对 engine-pipeline-06/07/12 中文化版本的翻译复核，明示没有重新审读生产源码。它保留资金/股份分离、P0/P1/P9、资源守恒和排序不构成交易优先级等原语义；发现“零售”可能模糊 `retail` 账户含义后，实施者改为“散户”，最终字节再次核对。该记录只能证明当时文档翻译差异的范围与复核结论，不能证明其中引用的生产行为现状。
- `pipeline-redo03.md`：记录 engine-pipeline-09 两份文档的翻译复核和修复闭环。`PreOpen` 静默窗口、P0/P1/P9、资源释放、失败后丢弃局部 `TickShadowPlan` 等原文限定被保留；报告明确不把局部 shadow 消费称为权威状态自动回滚，也未重新实施源码审计或外部规则核验。其 `passed` 只绑定报告列出的文档最终哈希。
- `pipeline-sol-domain.md`：记录 engine-pipeline-01/02/04/05 中文化差异核对，说明字段/符号/结构保持和术语修复。它特别区分 `stop-price` 的涨跌停语境与止损、`maker` 与做市方、`escrow` 资源守恒与仅资金守恒；02 恢复原文且 Markdown 未变化。报告明确范围是翻译等价，不是源码行为审计或交易所规则复核。

三份材料均属历史文档复核。它们的“通过”结论不可继承为当前 engine 行为、测试、完整产品验收或官方 A 股规则核验结论。没有从文档翻译复核本身推出新的实现需求。

## Caller、owner 与 consumer 核对

在 `43b1aa5` 基线中，`packages/engine/src/session/pipeline/mod.rs:194-253` 将 `TickShadowPlan` 定义为可丢弃的候选 owner，持有 `TickShadow`、事件/回执缓冲、到期处理结果与 P1 `DecisionResourceSnapshot`；`plan_tick` 在候选上完成 P0 expiry 与 P1 allocation。`pre_open_transaction.rs:91-164` 的 `prepare_pre_open_tick_with_evidence` 是盘前路径 caller：它建立 plan、调用盘前 transaction，再将 plan 交给 `prepare_tick_shadow_plan_commit_with_evidence`。transaction 消费资源快照并取出候选 session；若中途失败，局部 plan 不能就地恢复，外层失败路径丢弃候选。成功后由准备好的 commit token 到 P9 单点提交。这里存在清晰的 owner、caller、consumer 链，历史翻译评审没有提出额外生产 owner 或替代调用者候选。

领域边界以基线当前 ADR 为准：ADR-0017 §阶段契约及 P0/P1/P3/P4/P8/P9 明确资源截点、真实局部冲突和单点提交；其已被 ADR-0018 取代的旧来源类优先级及自由调度字节等价要求不能恢复为现行交易规则。ADR-0018 仍为 proposed，但其明确已接受的部分实施目标及 ADR-0017 中已接受修订继续适用。P0/P1/P9、现金/股份、同股撮合和收据的翻译术语须保留各自边界；本批没有以历史译文代替当前源码与现行规则依据。

## 总账与裁定

当前总账 `implementation-audit-2026-10-02.md` 及 `reaudit-pipeline-contracts.md` 将 ADR-0017 交易管线列为现行生产调用链复核主题（R02），同时把长期验收债与已接入生产契约分开；G39 单独记录验证工具未固定真实受理轨迹的问题。上述三篇历史翻译审阅未覆盖 G39 验收工具实现，不能核销或反证 G39，也没有与总账其他 G 项或 `docs/open-questions.md` 的未决产品问题建立直接等价映射。批次不新增、不核销 G/Q，不将未运行测试视为产品缺陷。

结论：三份历史文档审阅对自身范围、修复记录和验证限制的描述一致，未发现其翻译结论引入交易语义变化或要求扩大实现范围。状态为 `passed_document_review`；不代表重新验证 engine 行为、A 股官方规则或产品验收。
