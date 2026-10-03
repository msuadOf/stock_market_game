# Batch 196 独立核读

## 范围与读取校验

- 目标基线为 `.worktree/implementation-reaudit` 的 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。已读该工作树 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`，并核对相关 ADR-0017、0018、0023、0025、0026。没有运行测试/构建，没有改产品代码或执行 Git 写操作。
- 三篇指定材料均连续阅读全文至 EOF，行数与 SHA-256 校验如下。三条来源路径均不在 43b1aa5 的 tree 中；所以它们只作为现存历史材料核读，不当成该基线提交时已经存在的承诺证据。

| 来源 | 行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-session-07.md` | 13 | `18e8e7110273781ab4fe09e4684df82def3f0958ca26949d2887110ad14e6caf` | 是 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-session-area-final.md` | 24 | `f6d8328b293670bc5f53dffb95eb41aae6d76d66c20cea7c0de0b4c03cecef81` | 是 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-strategy-01-caller-final.md` | 10 | `fe2ae7fbeeba916177282273046d9e28d565e51eadb6ee29028b6b5f2805b514` | 是 |

## 当前实现与旧结论复核

1. **Session 状态 owner：候选已实施。** 旧材料把 `CommittableSessionState` 作为唯一提交状态集合候选，并将 poison/test failure hooks 留在 facade。当前 `GameSession` 在 `session.rs:1125-1132` 持有 `state: CommittableSessionState`，状态字段集中于 `session.rs:1134-1160`；`clone_for_tick_shadow` 和 `commit_tick_shadow` 经由 `session.rs:1328-1341` 委托 clone/commit。与旧结论一致，不能把候选的历史“未实施”状态延用到本基线，也不能据此再提一个同义状态 owner。
2. **K 线 owner：候选已实施。** `SessionCandleBook` 在 `candles.rs:90-95` 统一持有 histories 与 active；`record_trade_or_mark` 在 `candles.rs:132-179` 保留零量昨收占位并在首笔真实成交时重置 OHLC/成交统计。`GameSession` 的委托入口见 `candles.rs:191-205`。旧区域报告对活动与历史 K 线归属的结论得到当前源码支持。此对象抽取不证明保存投影或历史复制成本已消失。
3. **决策 root 与 caller：已落地，旧 caller 更正与当前代码吻合。** `DecisionChainObservation` 在 `decision_chain.rs:63-87` 保存批次共享观察；`RootReadContext` 在 `decision_chain/roots.rs:5-29` 收集只读输入；`InstitutionDecisionRoot` 在 `roots.rs:187-221` 持有单账户临时个人状态并返回 typed 操作结果。真实调用端 `PlanRootCoordinator::start_ready_accounts` 在 `plan_chain_candidates.rs:290-340` 捕获共享 context、take 个人状态并 spawn root。实际 `NpcObservationContext` 构造在 `roots.rs:400-405`，传入 `&context.library` 和 `market_view`；所以最终 caller 材料关于 `run_chain_for_account` 的更正没有被本次源码核读推翻。
4. **生命周期边界：当前仍在 coordinator。** `adaptive.rs:149-184` 对就绪 assessment 重新调用 `collect_plan_lifecycle_actions`，再应用动作；root 只暂存账户个人工作并返回 typed batch，没有冻结生命周期动作或取代 session/coordinator 的权威状态。该边界与旧材料一致。

## G/Q、ADR 与候选反证

- 对照现行实现审计账本中的 G01–G68 和 Q 项，只核对与本批候选直接相关的关联，不声称重审所有 G/Q。三类 OOP 候选没有证据可核销新的或既有 G/Q。尤其 G16 仍明确涉及 `RootReadContext::capture` 复制 `PlanBook` 及普通 shadow 中的历史/报告状态复制；当前 `roots.rs:18-29` 仍克隆 `accounts`、`library`、`plans`，而 `DecisionChainObservation::build_technical` 仍从完整 candle histories 构造输入（`decision_chain.rs:88-109`）。对象提取不能反证或核销 G16。
- Q02（主动读取公开历史是否记入个人经历）和实现账本中的 Q11（更正/真实违约 cause 的生产分发）仍是消费边界问题，不会由 `DecisionChainObservation` 或 `InstitutionDecisionRoot` 的存在自动解决。产品 `docs/open-questions.md` 中 Q11 的机构个人方向已由 ADR-0026 收敛；它与实现账本 Q11 的 cause 分发同号异义，不能混淆。相关 G06–G09、G28、G35–G38 也属于生产功能链，不是上述 owner 抽取的等价项。
- 较新的 ADR-0018 明确取代 ADR-0017 中旧来源类全局排序与密封身份作为跨实体先后顺序的历史契约；ADR-0023 收敛合成前史/撮合行情范围，ADR-0025 保持日终存档边界，ADR-0026 明确机构个人经历/暂停假设。它们没有要求再引入本批同名对象，也没有推翻这里核对的 owner/caller 边界。来源没有提出交易制度或金额、股数单位变更，本次不新增 A 股规则主张。

## 结论

旧材料关于三项结构候选和生命周期动作留在 coordinator 的结论，在 43b1aa5 当前代码中得到支持；caller 更正也与真实调用位置一致。候选状态已过时，不能作为当前待实施承诺。未发现可确认的已批准遗漏、错误历史核销或新增 OOP 动作。G16 及 Q02/实现账本 Q11 等既有关联边界继续独立保留；本次没有验证其行为或性能。
