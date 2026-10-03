# 隐藏复核 batch 157（owner=2）

## 来源与基线

按唯一计划连续读取三份主仓来源至 EOF；实测行数合计 219，SHA-256 与计划一致，逐项记录见配套 JSON。调用方 worktree `HEAD` 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。来源均标注为历史候选材料，不将其提议或测试陈述视为已实施、已批准或本轮运行结果。对照读取调用方根守则、工程原则、开放问题、架构、最新总账、coverage index、candidate checks、pipeline/core contracts，以及 ADR-0017、ADR-0018、ADR-0019 和交易规则相关条文；未运行测试、构建或修改产品、正式文档及 G/Q。

## 来源核对与当前链路

- `engine-foundation-04.md`：保留 verification/evidence 现有对象所有权的结论与基线一致。当前 `Collector` 持有单次 timing capture 状态，`GameSession::step_with_phase_timing` 只在成功提交后返回证据；生产性能 example 消费 phase 记录，测试模块覆盖相应契约。`runnable_threads` 所指 Rayon registry 容量与 OS runnable 数/CPU 利用率的区别应继续保留。Panic 后 thread-local capture 状态清理是来源提出但未核实调用方恢复策略的边界，不足以升级为缺口。
- `engine-pipeline-01.md`：唯一 OOP 候选 A01 是将预算字段与 cash/share lane patch 的更新收进 `AccountBudget`。在基线中 `AccountBudget` 已提供 `ensure_from_snapshot`、`ensure_sellable_from_snapshot`、`apply_update`、`adopt_cash_lane` 与 `adopt_shares_lane`；`AccountValidationState` 负责校验、worker round staging 和结果提交。生产入口由连续、竞价及盘前事务使用 `AccountValidatorDriver`/账户校验；`Cancel` lane 不采用预算 patch。故来源候选在基线已实现，不应重复登记。预算现金与股份仍分开，买入预留取配置费率及保护限价，卖单现金占用为零，P1 资源快照和同账户真实争用语义未见被该封装改变。
- `engine-pipeline-04.md`：其 retain/support 判定与候选提交边界相符。当前 `execute_authoritative_tick` 分派阶段事务；各阶段候选在 P9 前保持私有，`PreparedCandidateCommit` 承接可失败准备与单点提交；`TickCommitEvidence` 是显式捕获的验证证据，不替代 authority 或普通结算。连续生命周期投影保留单 tick 诊断职责，同订单局部事件整理不构成跨实体排序。来源列举测试只作为覆盖描述，本轮未运行这些测试。

## 领域语义及总账去重

涉及的现金单位为分、股份单位为股；P0 过期释放进入 P1 资源截点，密封批释放不回补同轮预算；账户资金与股份预算分离，T+1 与配置证券类别规则沿用现行路径。以上与 ADR-0017 和 `trading-rules.md` 的游戏契约一致，来源未主张新的交易制度或真实清算行为。ADR-0018 仍为 proposed，不能用其未决观察、存储和恢复内容扩写实现承诺；ADR-0019 禁止以任意请求/挂单配额替代性能处理，也未被候选建议冲突。

最新总账将 A01–A11 主要生产能力列为已有，并将 G01–G68、Q 项分开管理；其中 G16 是历史数据复制边界，和本批 `AccountBudget` 候选不同。没有发现来源足以证明遗漏已批准承诺、错误核销现有 G/Q 或新增需登记的候选。不得从候选文档中的旧状态或“已有测试覆盖”推断当前缺口已运行验证或关闭。

## 结论

本批历史 OOP 保留意见与基线调用链相容；A01 预算封装已在基线中实现。没有确认新的 A 股语义漂移、必要的对象迁移或漏接消费链，不升级或修改 G/Q。该结论限于三份指定来源与所核对调用链，不代表行为测试或长时验收通过。
