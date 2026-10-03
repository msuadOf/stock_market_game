# 批次 25 隐藏来源复核

## 范围与来源

- 来源基线：主工作树与 caller worktree 均为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。caller root 为 `.worktree/implementation-reaudit`。
- plan.sources 为 parts/12.md、13.md、14.md：分别 12/15/25 行，SHA-256 分别为 `e914f8c6005f1730050950c45625910876d0cb85d4b213dd2e747cb89f1c5ab1`、`9b293e94c5b1b2d4c9719613ffd2f66c11ed6a7dc9f81d396431fb763f5bd4e7`、`37ae1e44e2f660086d18e8fecc39e6afa932d5bfee042c012fcbe88f33a1b2d8`；均连续读取至 EOF。三篇分别是基本面 OOP、plan chain、plan execution 的历史审查材料。
- collateral 旁证 `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/25.md`：实测 9 行，SHA-256 `ad3fdb4f9da2067604420ec51e9d22dc589db27f6c80b4edc10828e15bf0cafa`。L1 是范围/方法，L3–5 为总体 retain 结论及依据，L7 为证据定位，L9 为限制；连续读取至 EOF。它不是 plan.sources 内的 batch 源，仅作额外旁证。
- 阅读 caller worktree 的 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`，及 ADR-0006、0012、0013、0021、0026。未以旧 agent 记录替代现行决策。ADR-0006 中隐藏 V/首批策略的历史方案已由后续 ADR-0016 等实现状态更新限定；ADR-0026 对机构个人成本/风险触发方向已有决定。Q11 总体方向已解决，具体参数依据 ADR-0026，不应再把它误标为开放问题。
- 复核当前 owner/caller/consumer 与 `reaudit-engine.md` 的相关 G/Q：策略接口和持久状态、个人机构经历政策、参数工厂、纯决策 kernel 均有真实实现。part25 的 OOP retain 记录只作为 collateral 佐证。静态结论不等于测试通过；未运行测试、构建或交易验收。

## 逐项复核

- `InstitutionExperiencePolicy` 持有逐机构冻结阈值，含显式版本化持久 DTO、构造校验和受控读取器（`strategy/institution_experience_policy.rs:40-75,77-119,217-249`）。consumer `session/institutional_behavior.rs:31-64,75-110` 从该机构自己的 `BeliefBook` 读取策略，使用个人成本、账户风险和个人经历生成行为信号；它不直接创建订单，也不强制止损。该结构支持原 retain 判断。旧材料所说“策略 owner 独立”并不代表其已覆盖散户的不同经历接线。
- `Strategy` 是统一决策契约，暴露可恢复身份、策略族、可选的行为/经历决策入口和 Intent 产生（`strategy/mod.rs:192-283`）。它仍是被调用的行为接口，不应另包无状态 manager。ADR-0006 的 NPC 独立参与者、策略隔离和 RNG/Intent 边界仍可用于核对；当前类型已不再是旧文档示例中的同一个小 trait 形状。
- `MomentumStrategy` 持有个体风格、lookback、threshold、order size 及观察参数，序列化为明确的策略状态；`decide` 映射到 `StrategyData` 后委托 `decide_hot` / `decide_hot_reversal`（`strategy/momentum.rs:16-33,93-132`）。状态与纯 kernel 分层合理，无需新增 kernel 对象。
- `StrategyParams` 聚合群体配置并执行验证（`strategy/params.rs:7-73`）；`StrategyFactory` 按类别采样个体参数，反证“没有生产 consumer/实例个体化”的假设（`strategy/factory.rs:36-161`）。`RetailStyle`、`InstitutionStyle`、`HotStyle`、`StrategyFamily`、`StrategyProfile` 是身份/分类值（`strategy/profile.rs:1-81`），不承担需要独立 owner 的生命周期。
- `decide_retail` 是输入驱动的决策 kernel，消费注入 RNG 与快照并返回 Intent（`strategy/retail.rs:11-65`）；`sampling.rs` 的抽样 helper 同样由工厂传入 RNG，既无跨调用可变状态，也未发现需要迁移的跨文件聚合。其生产接线可见 `zi_noise.rs:1-6,230-235` 与 `factory.rs`。

## 分类与反证

- **已实现：** 本批提出的策略契约/实例状态、参数汇总与采样职责、个人机构 policy owner、纯 kernel 边界在当前代码存在。part25 retain 分类成立；无新 OOP 候选、无可核销的已批准 OOP action。
- **现行缺口：** 此处不产生新 G。相关总账 G07/G08 是策略/观察/经历行为接线问题，不是策略 struct 缺 owner 的证据；尤其机构经历实现不能反证散户日期观察/衰减已接入。`reaudit-engine.md:14-16,31-45` 持续记录了该区分。不能由“对象存在”或本次 retain 结论核销行为缺口。
- **待定：** Q11 策略模型总体及 ADR-0026 机构经历方向已经解决，不应再列为开放方向。open questions 中另有 Q02（主动读取公开历史是否需记录）的边界；它不属于本批 OOP 责任，也不能用价格观察 writer 冒充读取历史记录。
- **文漂/历史证据：** part25 是完整性反查记录，不是正式验收证据；其未运行测试/构建的限定必须保留。ADR-0006 §5、§7 早期隐藏 V/策略映射措辞已有后续状态注记，不能照抄成当前实现契约。没有证据支持把 OOP retain 结果升级为全局 NPC 行为完成。
- **大 A 语义：** 当前 retail kernel 仍将卖出数量限制为 `sellable_qty`，并明确保留 T+1（`strategy/retail.rs:47-62`）；本批没有改动价格、股数、费用或撮合路径，不重做交易所法源核验。

## 结论与限制

当前实现反证了“这里缺少策略 owner/参数工厂/实例状态对象”的猜测；保留现有聚合比额外包装更符合 ADR 与最小范围原则。现有 G07/G08 和 Q02 边界维持原分类；没有新 finding。只做静态源码与记录核对，未跑测试、构建、官方规则查询；因此不声称行为验收或独立法源复核完成。
