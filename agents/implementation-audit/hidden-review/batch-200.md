# 批次 200 独立复核

## 范围与读取核验

按唯一计划读取三份指定 review 至 EOF，核对 SHA-256 与行数；基线为 `43b1aa5`。另检查该基线的 `reaudit-engine.md` 调用链记录、策略生产调用点、相关 `analysis_profiles` 测试，以及 ADR-0006、ADR-0016、ADR-0021 和 `open-questions.md` 的现行语义。未运行测试或构建，未改产品文件。

| 来源 | SHA-256 | 行数 | EOF |
|---|---|---:|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-strategy-02-specificity-delta.md` | `bfed3ef1251a782e4e63d1d6b6b404479cee834680eb3f87149c85ae0c333b30` | 26 | 是 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-strategy-02.md` | `b7a4ee1f4e3da6abf8996d791856559126deee25ce441822db83e2c138e865fa` | 13 | 是 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-strategy-03-manager-delta.md` | `7ca99251088aee5b0f9e053c4e41dc019ae3e09aa0bab32a0f5466d5889e6152` | 21 | 是 |

## 发现

1. **engine-strategy-02 的验证核销结论存在未解释冲突。** specificity delta 明确判定逐文件 validation 证据仍不足、原阻断未消除，并以 `factory_profiles.rs` 为例指出最终 items/modules 未给出 `analysis_profiles` 测试文件和具体用例。随后 `engine-strategy-02.md` 的简短入口却声明最终独立复核“通过”，只链接 `engine-strategy-02-manager-final.md`，没有提及或说明如何处置 specificity delta 的未通过结论。历史材料因此不能共同支持一个明确的最终验证状态；应补一条裁定，说明后续审查是否已逐项补齐证据，或将未解决的文档验证限制保留在最终入口。

   基线源码侧可见该疑问具有可操作的核销路径：`packages/engine/tests/analysis_profiles/gold.rs` 有 `retail_long_term_full_worked_example`、`institution_active_trader_zero_fundamental_worked_example` 等具体派生/随机消耗用例；`packages/engine/tests/analysis_profiles/invariants.rs` 有零权重、10,000bp、最大余数并列和稳定性用例。它们能支持相应边界，但不能仅凭测试存在推断所有 23 个历史条目的验证记录均已核销。`engine-strategy-02.md` 未显示其链接审查完成了这一逐文件映射。

## 基线调用链与领域边界

`reaudit-engine.md` 将策略相关 G/Q 分别追至真实生产入口：机构分析由 `InstitutionDecisionRoot` 使用个人 `BeliefBook`，散户仍由 `npc_decisions.rs` 水合 `StrategyState` 并调用 `ZiNoiseStrategy`；预算分类缺口则明确限定为单账户计划续行与新机会。当前 `factory_profiles.rs` 的档案派生由 session 创建路径与 `StrategyFactory` 周边消费，测试独立覆盖派生契约。没有证据把历史审查报告中的文档核销问题转化成新增产品代码承诺。

ADR-0006 的可插拔 `Strategy` 与逐实例参数、ADR-0016 的个人信息/预期边界及 ADR-0021 的策略目标与下单约束，和本批所述 profile/RNG 工作相容。抽查的增量不改变沪深 A 股交易制度、账户资金/股份、T+1 或申报规则；本批没有新的官方规则主张。ADR-0006 中早期隐藏 V 轨道等表述受后续决定覆盖，不应从旧策略材料恢复为现行实现要求。

## 结论

确认 1 项历史审查材料的状态一致性问题；这是验证记录的可追溯性缺口，不是已确认的产品实现缺陷或新增大 A 语义变化。engine-strategy-03 的 delta 仅增补完整符号清单并明确继承既有复核范围，当前材料没有发现与其“通过（仅限清单增补 delta）”相矛盾的证据。未运行测试；结论不代表行为验收通过。
