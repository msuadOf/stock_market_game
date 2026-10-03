# Domain 实施协调记录

- Branch：`refactor/oop-complete`。
- Diff 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 权威动作正文：`../../oop-refactor-audit/challenge-2026-10-03/action-index.md`；assigned-actions 的旧增强字段仅为历史。
- 本组有 39 个唯一动作；N04 按三行业分实施，N06 分 accounting policy 与 industrial caller 实施。
- 当前阶段：39 动作及 N07/N32 正文子目标源码已实施且独立静态复核全部闭环。集中域内 104 个精确短 case 全部通过；最终冻结后 build07/check08 成功。各动作证据见 completion-ledger.json，整批总复核由 root 收口。
- 产品编译和测试由 root 集中执行，本组 worker 不运行 Cargo/全仓 formatter/Git 写命令。

| Worker | 动作 | 状态 |
|---|---|---|
| accounting | domain-N05, domain-N06, domain-R2-N01, domain-R2-N02, domain-R2-N03, domain-R2-N04, domain-R2-N06, domain-R2-N07, domain-R2-N08 | 源码已实施；指定短运行与静态复核已闭环，证据见对应记录 |
| bank | domain-N04, domain-R2-N10, domain-R2-N11, domain-R2-N12 | 源码已实施；指定短运行与静态复核已闭环，证据见对应记录 |
| industrial | domain-N06, domain-R2-N13, domain-R2-N14, domain-R2-N16 | 源码已实施；指定短运行与静态复核已闭环，证据见对应记录 |
| insurance | domain-N04, domain-R2-N18 | 源码已实施；指定短运行与静态复核已闭环，证据见对应记录 |
| real_estate | domain-N04, domain-R2-N22, domain-R2-N25, domain-R2-N26, domain-R2-N27 | 源码已实施；指定短运行与静态复核已闭环，证据见对应记录 |
| operations | domain-R2-N19, domain-R2-N20 | 源码已实施；指定短运行与静态复核已闭环，证据见对应记录 |
| orderbook | engine-foundation-01-A03, domain-N02, domain-N03, domain-R2-N37, domain-R2-N38 | 源码已实施；指定短运行与静态复核已闭环，证据见对应记录 |
| diagnostics | domain-R2-N30, domain-R2-N31, domain-R2-N32, domain-R2-N39, domain-R2-N41 | 源码已实施；指定短运行与静态复核已闭环，证据见对应记录 |
| experience | domain-N01, domain-N07, domain-R2-N28, domain-R2-N34, domain-R2-N35, domain-R2-N40 | 源码已实施；指定短运行与静态复核已闭环，证据见对应记录 |

## 跨组接线

- N07：session manager 接 institutional observation/reconciliation 两个 RetailExperienceState receiver。
- N41：session manager 接 CausalCollector receiver；连续和竞价保留不同事实追加/索引写入失败面。
- A03：session manager 接非 pipeline/plans/strategy；pipeline manager 接 pipeline；hosts 接 integration tests/examples/apps。所有 getter/restore/fixture API 已发三方。
- Account::restore_balances 返回 `()`，仅应用 save 已验证事实，避免重复检查改变首错。Position::from_restored_parts 返回 `Self`，保持旧 serde 事实接受范围。
- N06：industrial worker 与 accounting worker 对接税 policy receiver API。

## 验证说明

- 本轮行为保持重构，先添加原 API 的行为保护测试；此类测试预期 baseline 通过，不假称失败断言。
- 新 receiver API 的缺方法编译失败仅是编译红，不视为有效行为红。
- Root 明确允许保护测试已固定后继续成熟实现，统一构建/测试后按冻结源码复验；基线尚未取得的事实保留。
