# 批次 070 独立复核

## 范围与证据

仅核对 scan-plan.json 分配给 owner 5 的三份审查入口，均全文读取至 EOF。路径、行数、SHA-256 与 scan-plan 一致：

| 文件 | 行数 | SHA-256 | 核对 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/engine-strategy-02.md` | 13 | `672fb5c1b4623f9738c5da806a305a97b5cd6ad00b25c8948d3b0f8f4cf4793c` | 一致 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-strategy-03-manager-delta.md` | 21 | `4b57e91234c38047a938343be3adcc42d8288b3160984a76b12791ba05fa62d6` | 一致 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-strategy-03.md` | 13 | `80836d9c446fd7835680341b2a32aab4d689522c36576b3eb307290ef3396958` | 一致 |

基线为 `43b1aa5`，当前工作树 HEAD 精确指向 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。只读检查了 G06–G09、G16、G28、G35–G38、Q02/Q11 的现状索引与 engine 复核、Q11 开放问题、相关策略 ADR，以及策略源码、factory、state、session 消费入口和 items/relationships/modules。没有运行测试、构建或 Git 写操作。

## 复核结论

1. **策略所有权与 A 股边界：通过。** 当前实现仍由每个 `ZiNoiseStrategy` 实例拥有其零售参数；工厂为账户构造实例，`StrategyState::ZiNoise` 负责状态承载。生产 session 的 `npc_decisions` 通过 `decide_with_experience` 使用本人观察/经历；内核与意图转换保留纯数据输入，买入可负担性和本人可卖股数边界清晰。ADR-0006 的实例化策略方向、ADR-0012/0013 的观察与经历边界、ADR-0021 的仓位步幅/报价语义及 ADR-0026 的个体机构经历均未被这些候选描述混淆。没有新交易制度主张，不需要外部规则复核。
2. **必要性与最小范围：基本通过，状态标注须更新。** engine-strategy-02 的保留判断与 manager final 中“无 OOP actions”一致。engine-strategy-03 的 A01 是窄范围重复映射收敛，但其入口文件仍称调查“未实施”，且 manager delta 称 unit 导入未实施动作；基线源码 `zi_noise.rs` 已有私有 `strategy_data()`，三个入口均调用该方法，且已有 `strategy_data_projects_all_individual_retail_parameters` 测试。因此 A01 已实施的事实与复审文本当前状态矛盾，应更新审计入口/增量记录，不能把本轮报告成尚未实施的候选。
3. **跨层与遗漏：未见策略层语义漂移。** `retail_position_decision_to_intents` 仍以费用配置和涨停价算买入能力、以本人 `sellable_qty` 限制卖出；交易受理及 T+1 由下游负责。机构普通 `decide` 空意图与 session decision-chain 的 plan path 隔离一致。coverage 中 G07/G08/G09 及 Q02/Q11 等开放或缺口状态应与策略对象重构分开；这三份候选审查不能核销这些生产接线缺口。未发现需要增加本批候选动作或测试的证据。

本复核只覆盖指定审查文档及相关调用边界，不重做其历史源码全文审查，不表示运行任何测试或 A 股官方规则核验。
