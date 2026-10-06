# Rebase 完整证据构造独立复核（2026-10-06）

## 结论：通过

本次只读复核限定 diff：`packages/engine/src/session/pipeline/ledger.rs` 与 `packages/engine/src/session/pipeline/ledger_state_tests.rs`。没有实施代码，也没有运行测试、构建或基准；性能与短测结果沿用 `rebase-evidence-proof-findings.md` 中的记录，未作独立实测。

1. **大 A 语义与依据：通过。** 变更只复用账本完整校验证明，不改变委托、成交、资源预算、费用、T+1 或其他交易制度。本次不涉及新交易规则，因此没有重查交易所或中国结算材料，也不把已有来源核对说成本次官方复核。卖方费用累计应计/实收审计字段被保留；ADR-0017 §2 与分歧 #9 明确将其作为游戏内账本契约，并声明该封顶规则是游戏简化，不等同真实清算。参考 `docs/trading-rules.md` 的已记录来源边界与适用日期。

2. **必要性与范围：通过。** 原完整输入门禁在 `rebase_private_for_tick_commit` 起始处仍执行。门禁确认来源行、audit/守恒行对应关系、非负资源和 checked 全局聚合合法。新缓存只在 rebase 其余操作均成功后安装，避免把未完成的结果记作证明。每个变化 Envelope 仍调用 `rebase_for_next_tick`；该方法设置 `basis=live`、清零 tick 局部 `spent/released` 并执行 `validate()`。terminal envelope 的 audit 与 conservation 行一起删除；未终结行的 audit 保持原样，conservation state 归零，live 行不被删除，终结行已由输入门禁证明 live 为零。全 terminal 情形得到无行 ledger，仍保留 `next_receipt_index`。新增逻辑只把已成功执行的完整证明绑定到现有缓存 identity，没有增加另一份权威状态或交易路径。

3. **边界、跨层漂移与复杂度：通过。** 数学边界成立：对每个通过验证的 Envelope，`basis = spent + released + live` 且各分量非负，因此新的 `live` 不大于旧 basis。旧全局 aggregate 已通过逐资源 checked 求和；新 aggregate 只包含存续行的 live，terminal 行被删除，故新总额不可能高于旧已验证基准，也不会引入更大的 checked 溢出风险。剩余行的新守恒状态为零，新的逐行方程是 `basis=live`；旧 audit 字段完整保留，包括 nominal 与 charged 费用可能存在的累计差额。新 proof 因而覆盖完整门禁所需的四张 map 及全局约束，而不只是缓存计数。

   `EvidenceCache` 仍以四个 map 的 `Weak` identity 为完整匹配条件；`SharedRows` 的写访问通过 `Arc::make_mut`，写入时会分离被弱 identity 指向的 allocation。四 map 分别变异、分支 candidate 与 authority 隔离的现有测试覆盖此边界；新增测试还验证 mutation 后原只读门禁实际重新执行并拒绝破坏数据。公开 `rebase_live_for_next_tick` 仍先 clone candidate，只有私有 rebase 成功后才写回；已有失败原子性测试保持适用。没有发现数据或交易语义漂移，也没有不必要复杂度。

复核锚点：`ledger.rs:85-93`（四表缓存捕获）、`372-438`（输入校验、逐行 rebase、terminal/审计/守恒处理及成功后安装证明）、`552-648`（原缓存身份门禁与完整校验）；`envelope.rs:178-188,224-245`（变化行 rebase 与 Envelope.validate）；`ledger_conservation.rs:65-168` 和 `conservation.rs:107-145`（逐行及 checked 全局求和）；`ledger_state_tests.rs:922-1090`（证据复用、费用债保留、四表 mutation、极值资源及清空边界）。

未发现阻塞问题；无重要可选建议。
