# 批次 187：engine pipeline 09/10 历史复核材料审计

## 来源读取与范围

基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，与目标 worktree `HEAD` 一致。逐篇从头连续读取三份指定来源至 EOF；实测 SHA-256 和行数均与计划一致。遵循当前 `AGENTS.md` 与 `docs/principles.md`，并对照 `docs/open-questions.md`、相关 ADR、当前 engine consumer、`agents/implementation-audit/exhaustive-review/resolution.md` 及实现审计总账。未改产品文件、未运行测试或构建。

| 来源 | 行数 | 阅读覆盖 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-pipeline-09-final.md` | 42 | unit-098 Delta 范围、候选 plan 局部消费、测试计数与复核门禁 | 已读 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-pipeline-10-chain-full.md` | 32 | A01 两段结算/机构投影调用链、候选状态与三项门禁 | 已读 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-pipeline-10-final.md` | 26 | A01 最终候选、继承范围、调用链事实与独立复核结论 | 已读 |

## 当前 caller、owner 与 consumer 对照

- 当前 `account_settlement.rs` 仍有 `apply_session_settlement_transaction`、`apply_settlement_transaction_with_beliefs`、`prepare_settlement_transaction_with_beliefs` 和 `apply_prepared_settlement_transaction`；历史材料所述同一 `ExperienceMoment` 经准备路径传入机构投影，与现有入口组织相符。
- 当前 `institutional_experience_projection.rs` 保持独立投影 consumer；`retail_projection.rs` 当前 `ExperienceUpdateMode` 仍有 `Retail` 与携带 moment 的 `InstitutionalFacts(ExperienceMoment)` 两种形态。历史 A01 是尚未实施的类型设计建议，不能记作现有生产缺陷或已完成实现。
- 历史材料的事务语义须限定在 caller 边界：准备阶段产生组合 patch，consumer/owner 在成功后安装；不据此推导任意私有 helper 或未来 caller 都具有强事务保证。09-final 所述 take 后失败会消费本地候选，也不能写成权威状态已提交或允许原地重试。
- 总账 resolution 保留机构经历主干已存在的判断，同时明确既有 G/Q 按其原承诺粒度核定。未发现这三篇历史材料提供足以核销既有条目的新证据，也未发现需要新增编号的已确认缺口。ADR-0013、ADR-0026 与现行交易规则登记提供领域边界；本轮未重新查验交易所或中国结算官方材料，不新增交易制度断言。

## 候选与反证

1. **09-final：历史 Delta 复核再证。** 记录限于 unit-098 调查归属和 NPC lifecycle projection 测试计数修正，并明确继承其余源码审查而未重审。其结论不承诺生产行为迁移；plan 局部消费边界也没有被扩大成权威提交故障。无新增 G/Q 候选。
2. **10-chain-full：调用链补核再证。** 记录将 A01 限定为候选 enum 类型调整，结论基于特定 settlement 与 institutional projection 调用链。当前 consumer 模式仍可观察到同样职责分离，没有出现其前提反证；历史记录不是 A01 已实施的证据。
3. **10-final：候选审查记录再证。** 该文绑定特定 candidate 文件哈希，明确源代码仍保留原 `Option<ExperienceMoment>` 与 `expect`。现有源码的 `ExperienceUpdateMode::InstitutionalFacts(ExperienceMoment)` 已显示必需 moment 被模式携带；本次仅据当前源码确认历史记录未误称 A01 实现完成，不评判某个历史快照的设计建议是否已成为需求。
4. **大 A 语义与范围：** 所审内容为经历投影模式和 session candidate 提交边界，不更改撮合、成交顺序、收费或 A 股交易规则。建议保持局限于调用链已有职责，不从账户类别或 receipt 顺序推导交易优先级。
5. **边界与限制：** 本次是历史材料审计及有限 consumer 对照，不重新审查 pipeline 全部源码、不运行测试，也不核销实现审计总账项目。未发现候选反证或需上报的新增 G/Q；保留历史材料明确的继承范围和失败边界。

**结论：** 三份来源与当前 caller/owner/consumer 及总账边界没有发现实质矛盾。记录为无新增、无核销 G/Q；不把候选提案当作产品缺陷修复或实现完成证据。
