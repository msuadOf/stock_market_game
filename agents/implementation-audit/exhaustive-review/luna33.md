# Tasks 10–12 证据全文独立复核

## 范围与结论

- 复核目标工作树：`/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`；当前 HEAD 为 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`，与委托标识产品 `08e4fc7` 按委托所述为同一产品版本。只新增本记录；未改产品、未写 Git、未运行测试/构建/K7 长验收。
- 按 EOF 连续读取三份原文：Task 10 `divergence-audit.md` 28 行，Task 11 `execution-log.md` 299 行，Task 12 `validation.md` 23 行，共 350 行。另读根 `AGENTS.md`、`docs/principles.md`、计划收尾状态和现行受理规则，并静态追当前生产接线、约束验证器和终止规则。
- **结论：**Task 10 的 Task 9/10 commit-boundary 阻断依旧是被明确保留的历史缺证；seed-7 费用预留阻断已由现行预算分配路径修复，但本次不声称复跑通过。Task 11 的 94/94 是绑定旧源码的历史结果。新确认的 G39 候选成立：baseline 的 after/sensitivity finalizer、receipt 与独立 root verifier 构成第三条严格比较链，重复 seed 未冻结实际受理轨迹仍比较 raw stdout 完整哈希。Task 12 文档历史核验不转化为当前测试结果。
- **执行停止边界优先：**`.omo/plans/escrow-parallel-engine.md` 的 2026-09-23 收尾决定明确要求不再迟补 Task 9 历史 witness、不再运行新的验证批次。故本复核只做静态核对；不运行 K7、修复或重启历史验收。

## 章节矩阵

| 原文范围 | 当前代码/规则对照 | 复核结果 |
|---|---|---|
| Task 10 L1–8：诊断期望未改，delta 为空 | 这是当时执行记录，不是当前 diff 可重建的事实；当前树未据此虚构 sha9/sha10 或期望值。 | 历史声明保持原样，不能当前化为 PASS。 |
| Task 10 L10–13：示例 `step()?` 传播错误 | `packages/engine/examples/causal_diagnostics.rs` 当前仍使用 `session.step()?`。 | 兼容修正仍接线；未发现静默吞错反证。 |
| Task 10 L15–22：Task 9/10 无真实提交边界 | 计划状态仍把 Task 9、10 标成 BLOCKED；收尾决定说不迟补缺失 witness。 | 旧阻断未被修复，也不应伪造边界；它是用户停止的历史验证范围。 |
| Task 10 L24–28：无 game buy identity 的初始持仓 | `packages/engine/src/experience/position_transition.rs` 对卖出经历更新检查买入身份；旧审计已定位 seeded holding 的回归测试。 | 可复核为有实现/测试源码的旧问题修复；本次没有运行该测试。 |
| Task 11 L5–93：v4/v5 资源资格、作废 roots | 当前 `baseline-run.mjs` 使用预算化 execution pool；日志原文自身已标记旧 roots invalid。 | 旧 CPU 记录仅作历史，不冒充当前结果。 |
| Task 11 L95–189：v5 指纹、2h/6h watchdog、用户时限变更 | 当前 runner 明确拒绝超过 300000ms 的 child/batch 上限；旧 2h/6h 受理已在原日志永久 invalid。 | 原阻断处理符合新限时；不续跑旧 root。正式入口还需进程外 supervisor。 |
| Task 11 L191–213：v6 有界代表性矩阵和并行 | 当前 fixture 明示 bounded representative profile，runner 保留多进程预算及共享 deadline。 | 属工具实现证据，不证明当前机器运行时资源利用，也不代表完整规模压力。 |
| Task 11 L215–256：seed 7 fee reserve > cash | 当前 `packages/engine/src/session/pipeline/adaptive_plan_chain.rs` 调用计划预算分配；旧审计所定位 `plans/allocation.rs` 将 buy quote/fee 从剩余现金上限分配，而不再把 fee reserve 必须小于总现金作为可接受报价前提。 | 旧特定 panic 根因已修；当前 seed 7 是否通过未验证，不能从源码断言复跑成功。 |
| Task 11 L258–299：v7 post-commit 94/94 | 结果绑定历史 revision `f222417…` 与 fingerprint `5d3c…668e`；当前工具已演进，历史结果不证明现行比较契约正确。 | 保留为历史证据，不据此核销 G39。 |
| Task 12 L5–9：符号检查、旧 seal 符号 | 旧 `seal_allocation_snapshot` 已替换为 `DecisionResourceSnapshot::seal`；现行 docs-symbol checker 对 `engine::symbol` 做显式词法检查。 | 旧符号数不重写为当前计数；重命名不等同生产功能消失。 |
| Task 12 L10–17：panic、A 股简化、时间/并行规范 | README 与架构文档保持 panic 是进程级失败；交易文档继续标注 escrow/费用收取是游戏简化；AGENTS/testing 约束普通 10 秒、必要长验收 300 秒。 | 文档边界保持诚实；历史 PASS 不代表当前命令已跑。 |
| Task 12 L19–23：Task 9/10 阻塞、Task 11/12 PASS | 计划仍保留 Task 9/10 未勾选及 2026-09-23 停止迟补决定。 | 状态没有把缺失证据改写为 PASS；本次尊重停止决定。 |

## 生产受理与守恒交叉核对

- 现行 `docs/testing.md` 的“并发受理与重放”约定明确：seed 不携带并发任务实际到达轨迹；同股按实际受理先后；自由调度可合法改变同价排队及后续成交账户。生产 `packages/engine/src/session/pipeline/local_admission.rs` 将 stock gate 实际登记的相邻候选加为先后边，且注释明确无关股票输出布局不定义交易优先级；`stock_auction.rs` 的 `arrival_seq` 是股票局部受理位置。故不应为令全局产物相等而恢复来源/账户排序。
- 守恒路径仍有显式验证：`ledger.rs::validate_conservation` 委托到账本守恒验证；`validate_complete_evidence` 另外校验 live/terminal envelope 状态、证据行数量和 audit 一致。`ledger_conservation_tests.rs` 保留聚合守恒和跨 key 泄漏反例，`envelope_tests.rs` 保留逐 key 不匹配拒绝。此为源码覆盖，不是本次测试结果。
- 上述交易域判断沿用仓库已登记的 ADR-0017/0018 和 `docs/testing.md`，此工具/验收审计没有改变 A 股业务规则，也没有新增交易制度主张。

## G39 第三条比较链

- Task 11 原文 L193–195、L284–286 将每套矩阵的一次 deterministic rerun 计入 94 次。现行正式入口仍由 `scripts/simulation/baseline-run.mjs::finalizeSimulationMatrix` 对最后一个 seed 重新调用 fixture，未提供冻结的受理事实；随后直接比较 `rerun.sha256` 与 canonical raw stdout SHA-256，不同即失败。receipt 校验要求两份 digest 与 canonical 完全相同；`verify-simulation-artifacts.mjs` 独立核验相同约束并将 receipt 纳入正式 root manifest。
- 这构成 G39 的第三条生产验收链，超出旧总账只列 `run-escrow-verification-matrix.mjs` 和 `escrow-verification-contracts.mjs` 两个比较入口的范围。两份当前工具代码亦仍分别比较完整 artifacts/coverage：`escrow-verification-contracts.mjs::compareArtifacts` 用于 determinism 及 perturbation；不能因 verifier 的链式哈希和独立 root walk 正确，就认为比较语义符合实际受理契约。
- **候选反证检查：**seed、fixture 参数、源码指纹、二进制 hash、checkpoint 链和 manifest 完整性都能证明输入/证据绑定及防篡改；它们不冻结 Rayon 调度、stock gate 到达顺序或逐笔成交事实。`local_admission.rs` 的并发 gate 以实际到达建局部顺序，故仅要求同 seed/raw bytes 完全相等没有充分依据。相反，当前代码确实仍强制相等，未发现接收轨迹重放输入或按资金/股份/价格时间不变量对账替代该强比较的证据。
- 建议将第三条链加入既有 G39 追踪：保留 canonical/rerun 数量的要求可由未来契约裁定，但不得把删 rerun 或删错误负控当作修复；应冻结同一受理事实用于确定性重放，并分别核对自由调度运行的资金、股份、实际成交/收据、价时和因果不变量。当前不改工具、不启动受理测试；须遵循前述停止决定。

## 终止规则核对

- `baseline-run.mjs` child timeout 或共享 abort 会调用 `terminateProcessTree`；POSIX 按 detached process group 发送 SIGKILL，Windows 使用 `taskkill /T /F`，之后等待 child `close` 才 reject；批次 deadline 还对清理收敛设第二截止。当前 runner 的组边界与原政策一致。
- 正式命令的进程外 deadline supervisor 是调用入口要求，不能只凭 runner 内部 timer 宣称满足；原 Task 11 命令记录确含 `timeout 300s`，但那些执行历史不授权本次重跑。当前静态阅读不能证明在所有宿主/内核情形孙进程都已收敛，报告中不扩大为运行时保证。

## 旧结论复核与新候选

- **维持：**Task 9 历史 witness 缺失和 Task 10 commit-boundary 缺失仍未闭环；2026-09-23 用户决定停止迟补优先，不要求为审计补造历史事实或启动新验收。
- **维持但限缩：**seed-7 费用预留失败的旧特定实现阻断已有代码修复；没有当前重跑证据，不能声称新版本矩阵已通过。
- **新增候选 C33-01（扩展 G39）：**统计 after/sensitivity finalizer + determinism receipt + 独立 root verifier 仍按相同 seed 的无受理轨迹自由 rerun 比较完整 raw stdout；以现行 ADR 语义看属于过强的确定性门禁。它是工具契约问题，不是生产交易错误，不应通过改生产排序解决。
- **本次复核限制：**静态阅读不能代替正式命令、运行时进程采样或完整 A 股交易规则复核。没有新增性能/测试 PASS 声明。
