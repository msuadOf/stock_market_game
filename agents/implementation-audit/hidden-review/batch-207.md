# 批次 207 隐藏来源复核

## 范围与完整性

- 唯一计划为 `.worktree/implementation-reaudit/agents/implementation-audit/hidden-review/scan-plan.json` 的 batch 207，owner 2；来源根为主工作区，产品 caller 为 `.worktree/implementation-reaudit`，HEAD `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。
- 三篇历史复核逐篇从头连续读取至 EOF，合计 229 行；实测路径、行数、SHA-256 与计划一致，详情见 `batch-207.json`。历史文字作为审查证据，不构成本轮实现授权。
- 已对照当前 `AGENTS.md`、`docs/principles.md`、实现审计总账、caller/owner/consumer 代码及 ADR-0017、ADR-0018、ADR-0025、ADR-0026。未运行测试、构建或 Git 写操作，未修改产品代码。

## 候选与反证

| 历史候选/结论 | 现行核对与处理 |
|---|---|
| engine-session-03 曾指出 root 产出 `PlanLifecycleAction` 会跨越在途子单结果，使用旧 filled quantity/child 状态决策。 | 历史记录称此 R1 已由修订后的 item/module 移除。当前总账未将其列为独立 G；ADR-0017 的 typed outcome 与依赖续行语义要求保持事实反馈边界。此项作为已修订的候选记录，不据此重开缺口。 |
| 生命周期 action 应由 coordinator 在路线 ready、消费准确 outcome 后重新收集并立即应用；账户私有信息不得变成共享状态。 | 历史复核及现行 ownership 记录支持此边界：`PlanPersonalState` 是按账户取出、随 root 结果返回并安装的暂态状态；root 操作经 coordinator 汇总，跨轮 continuation 以账户/股票/generation 关联 typed outcome。候选 identity 不成为交易优先级。ADR-0017/0018 与现行总账均强调实际冲突和到达语义；不升级为新 G。 |
| engine-session-04 曾发现封账顺序描述冲突：经营日终之后是否执行月/年封账，以及披露先后。 | 当前 `GameSession::end_civil_day_after_session_check` 明确调用 `close_accounting_periods(report.settled_date)`，顺序为经营日终 → 到期月/年封账 → 披露派发 → 事件登记；ADR-0025 同样要求完整日结后持久化。历史复核后续版本已修正该描述。总账 G35/G36仍分别记录经营闭环/行业装配缺口，不被此既有封账调用核销。 |
| engine-session-04 曾报告 `hash_contract_tests.rs` 计数 12/13 矛盾，以及 `disclosures.rs` 旧模块注释与调用事实不符。 | 历史记录称计数已统一为 13；旧注释被准确降为文档漂移，未误作运行行为。当前代码调用证实封账路径存在。由于本批仅扫描历史评审文件，未将历史文档状态冒充本次对该源码注释的全文复核，不升级为 G/Q。 |
| engine-session-05 的存档/计划候选调查判断无需另建 validator、staging owner 或全局账本。 | 与相邻复核及当前 ADR-0017、ADR-0025 的局部事务/日终存档边界一致。旧建议中任何候选实现状态均须以当前 caller/consumer 和主账为准；没有证据要求新权威 owner，也不因此核销 G16、G39 或存档/交易行为缺口。 |

## 结论

三份来源主要记录历史修订和边界核验。R1、封账顺序、测试计数属于来源中已注明修复的调查文档问题；个人状态所有权、计划 continuation、实际交易受理顺序、日终存档与封账边界有当前调用方/消费者及 accepted ADR 支持。G35/G36、G16、G39 等总账项继续按各自定义保留，本批不升级、不核销任何 G/Q。没有新增 A 股制度主张；未查询交易所/中国结算规则，本批未更改交易行为。
