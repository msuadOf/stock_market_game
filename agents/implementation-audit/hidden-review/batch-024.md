# 批次 24 隐藏来源复核

## 范围与完整性

- 来源基线：`43b1aa5`。按计划从主仓读取 `session/parts/09.md`、`10.md`、`11.md`，均连续读取至 EOF；实测行数、SHA-256 均与计划相符。三篇原文无截断，本轮没有重新读取它们提及的所有冻结 JSON 或源代码历史全文。
- caller 基线：`.worktree/implementation-reaudit`，对照其 `AGENTS.md`、`docs/principles.md`、当前 `engine-session-05` 模块记录、实现审计 G/Q 总账、相关 ADR 与生产调用方。历史 reader 内容作为审计材料，不当成当前指令。
- 只做静态复核，没有运行测试、构建或 Git 命令；没有改产品文件。

## 来源全文章节矩阵

| 来源（行数 / SHA-256） | 全文结构与历史主张 | EOF |
|---|---|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/09.md`（15 / `740d4d7d1d69bb4266d38f8a080ac655d0c8cd3920b299d62742e532ba815b64`） | L1–3：一次性 OOP 反查范围，`persistence.rs` 与冻结条目/模块/ADR；L5–7：retain 结论、局部累加器、跨文件恢复/保存职责及量纲；L9–11：领域边界与测试覆盖；L13–15：限制和无新增 OOP 动作。 | 已读到 EOF；实测 15 行。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/10.md`（11 / `fd0c246592275a9493a7d3bceb596fa8a6880cf6959e483c220eb3014a0269ce`） | L1–3：runtime v2 范围、逐字段/方法复核；L5–7：restore 局部事务、caller 与费用游戏简化；L9–11：现有测试锚点及 retain 结论。 | 已读到 EOF；实测 11 行。 |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/11.md`（15 / `57bbbf97e5c1e814657d99833a7afebf4c24c04146b3e839705fce1fc152fa88`） | L1–3：计划候选协调器与 retain 结论；L5–7：队列、账户根生命周期、worker 回收/identity 语义；L9–11：已有覆盖与待补 schedule/key/typed-outcome 边界；L13–15：A 股/ADR 语义、符号归属、无运行测试。 | 已读到 EOF；实测 15 行。 |

## 当前 owner、调用方与消费者

- `GameSession::restore` 在 `.worktree/implementation-reaudit/packages/engine/src/session.rs:2716-2720` 先作 urgency/schema/SaveSlot 校验；在同一局部实例重建账户、市场、连续/竞价订单和其余状态，`validate_saved_order_state` 位于 `:2806`，最后在 `:2984` 调 `restore_runtime_v2`。失败不会返回半恢复 session。基础校验与订单校验仍是只读函数，局部 `SavedReservations` 在一次订单校验中累计资源；没有证据需要长期 validator 对象。
- `GameSession::save` 位于 `session/failure.rs:85-89`，先检查健康状态并调用 `capture_runtime_v2`，再生成 `SaveSlot` 投影，不执行宿主 I/O。ADR-0017 的引擎 quiet point 与 ADR-0025 的完整日终用户存档候选/写入边界不同；不能因内部 `save()` 可捕获状态就称宿主可日内持久化。
- `restore_runtime_v2` 位于 `session/persistence/v2.rs:372-431`：先校验并构造 ledger、receipt identities、strategies 和 shadow accounts，所有可失败工作完成后才整体替换账户、ledger、seen identities 与 cursor。保存费用字段是游戏内 audit projection；不能误称真实券商清算规则。
- `PlanChainOperationBatch` 在 `session/plan_chain_candidates.rs:89` 起持有单 tick 队列/route 协调；`AccountSource::InFlight` 持 channel receiver、剩余 root 数和冻结 snapshot（同文件 `:97-103`）。worker 回传 `PreparedRoot`，协调线程在 `:342-384` 安装个人状态、追加 operations。`decision_chain.rs:718-727` 建立/推进 roots；`plan_chain_candidates/adaptive.rs:75-115` 消费操作并生成 candidate，`:339-383` 按 account/stock/generation identity 消费 typed outcomes；NPC preparation、continuous 与 auction tick transaction 继续是这些批次的消费者。
- 股票 route 阻塞条件见 `plan_chain_candidates/adaptive.rs:273-309`；`StockRouteCoordination::install_pending` 用 account/stock 作为单 pending key，重复安装会断言（`plan_chain_candidates.rs:127-135`），typed outcome 用 generation 核验后才取出 route（`:138-165`）。candidate 序号由 checked increment 生成（`:27-72`），是 continuation identity，不是交易优先级。

## G01–G68 / Q 对照

| 本批主题 | 关联总账项 | 对照结论 |
|---|---|---|
| `persistence.rs` / `persistence/v2.rs` | G21、G29、G36；Q17、Q19 | G21 是 baseline CLI 只需读取 setup 的输入投影契约，不授权放宽公共 SaveSlot 深度校验。G29 是零 NPC 初始分配边界。G36/Q19 涉及四行业会话装配与银行完整恢复；存在通用 SaveSlot validator 或 v2 restore 不等于开放银行恢复，不能核销。Q17 是 ClosingEngine 更正失败部分状态边界，虽有“失败/原子”词项，但并非 `restore_runtime_v2` 的相同状态 owner 或调用链。均不构成本批新 OOP 职责。 |
| `plan_chain_candidates.rs` / adaptive consumer | G16、G37、G38；Q22 | G16 的全历史 `PlanBook` 仍由 `RootReadContext::capture` 复制；本批根 coordinator 对其使用 `Arc` 不消除该 clone，性能未测。G37 是 diagnostics 调用缺少真实订单事件，不由候选 batch 对象存在而解决。G38 是同账户 ExistingPlan/NewOpportunity 预算分类未进入生产请求，不是 route identity 或跨账户优先级。Q22 的来源拼接与真实接收轨迹需独立核验；generation identity 和 candidate 输出次序不能裁定撮合优先。 |
| 其余 G/Q | G01–G68、Q01–Q23 | 除上表明确列出的邻接对照外，未发现同 owner、caller 或领域状态的直接重合；不据 OOP retain 推导关闭任何 G/Q。G/Q 状态本轮不更改。 |

## 复核结论与边界

- **无新增 OOP 候选或生产缺口。** 基础 SaveSlot 校验、runtime v2 DTO/adapter/局部恢复事务和单 tick plan coordinator 的职责与当前代码一致；把无状态校验包成 service、为 restore 增加无必要 staging 层，或把计划队列并入全局 PlanBook，均未显示 owner 收益。
- **第 11 组的测试缺口需按当前代码修正表述。** 历史材料称 typed outcome 乱序身份关联仍待测；当前 `pipeline/adaptive_plan_chain_tests.rs:848-895` 已覆盖两个独立股票的反序 P4 facts 与分批按 1 再 0 返回 typed feedback，协调器按 candidate identity 关联 continuation。`adaptive_plan_chain.rs:544-565` 还显式检查重复/异来源 candidate identity。故不能照搬“完全没有乱序关联覆盖”的旧结论。
- **仍有聚焦的补测机会，但不是新功能 finding。** 当前上述测试使用确定性 fixture，没有扰动真实 root worker 完成/poll 调度并断言不同合法 route 到达顺序以及同股受理仍服从真实入口的局部顺序。`install_pending` 的重复 account/stock 安装由 assert 拒绝，静态阻塞检查应防止正常路径重入；本轮未证明该冲突可由生产操作队列触发，也未见专门负控覆盖该内部守卫。按“仅改善已有动作”登记测试候选，不将其升级成交易语义回归或要求不同 schedule 下 route 绝对 ID/成交全序一致。
- **大 A 语义保持边界。** 存档现金单位为分、股份单位为股；恢复校验继续保留单笔申报量、整手、T+1、可卖量、现金/股份预留及零股约束。512 MiB 输入字节门禁、策略状态 held+8 与 ADR-0019 禁止的任意订单数量配额不可混淆。plan identity 不代表受理优先级；候选 worker 的完成/poll 次序可能改变真实 candidate 到达先后，ADR-0017 与 ADR-0018 §7/§11.2.3 允许无关请求随并行调度出现合规的局部次序变化。这里依赖已接受 ADR 与项目交易规则，本轮没有重新核验交易所官方来源。
- 没有运行测试/构建，没有修改产品代码或 G/Q 状态。静态代码证据不等于测试验收完成。
