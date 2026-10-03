# 按职责命名与契约版本

变量、函数、类型、字段、模块、文件和测试名称描述它们实际承担的职责、包含的数据或验证的行为。
名称不借用 AI 实施计划的阶段、任务、批次或工作编号，也不把内部实现轮次包装成业务概念。
需求追溯使用规范、ADR 或独立审计记录链接；计划编号留在历史记录的身份字段中。

## 职责名称

内部类型和模块不使用 `V2`、`V3`、`v2`、`internal_v2` 等实现版本后缀。需要区分多个对象时，
先明确对象的数据来源、生命周期或用途，例如 `SavedRuntimeState`、`DecisionResourceSnapshot`、
`FrozenPlanChainObservation`；不能以版本号代替职责分析。局部对照值也说明各自角色，
如 `saved_policy` 与 `future_default_policy`，不能只叫 `v1` 与 `v2`。

| 当前名称 | 职责 |
|---|---|
| `SavedRuntimeState` / `runtime_state` | 存档中继续运行所需的 runtime 事实及对应字段 |
| `saved_runtime` / `runtime-state.ts` | Rust 存档投影/恢复模块与 Web 严格校验模块 |
| `parseSaveRuntime` | 解析存档 runtime 事实 |
| `IntentCandidateKey` / `AccountValidatorDriver` | 委托候选身份与账户校验驱动 |
| `PreparedContinuousTick` / `prepare_auction_tick` | 已准备的连续交易 tick 与集合竞价准备入口 |
| `QuoteExpiry` | 事件或收据的报价到期来源标签 |
| `expiry_released` / `allocation_live` | 到期释放量与分配截点存量 |
| `project_id` | 测试中的地产项目身份 |
| `indicator_result_request_action` | 工作生成器中的指标请求审计动作 |

当前函数、类型、CLI、错误定位、cache、schema 与 format 的职责名称不使用 `P0`–`P9`、
`K1`–`K7`、`W1` 或 `Task-33` 等实施代号。不同 scope 的同名局部量须分别根据实际角色判断；
不能把所有 `p1` 都改成同一个 tick 阶段。

## 执行阶段与计时

`TickPhase` 的身份描述阶段职责；独立数值 `rank` 保留已声明的顺序和身份域。
阶段名称不授予无关委托交易优先级。同股价格时间、同账户实际资源争用和计划真实依赖
仍决定实际受理关系，见 [ADR-0017](decisions/0017-escrow-parallel-tick.md)。

| rank | `TickPhase` | `phase_wall_ns` key |
|---:|---|---|
| 0 | `ExpiryShadow` | `expiry_shadow` |
| 1 | `SealAllocationSnapshot` | `seal_allocation_snapshot` |
| 2 | `DecisionShadow` | `decision_and_coordinator_work` |
| 3 | `AccountValidation` | `account_validation` |
| 4 | `StockProcessing` | `stock_processing` |
| 5 | `ReceiptAggregation` | `receipt_aggregation` |
| 6 | `SettlementShadow` | `settlement_shadow` |
| 7 | `DerivationAudit` | `derivation_audit` |
| 8 | `PreCommitValidation` | `pre_commit_validation` |
| 9 | `CommitTick` | `commit_tick` |

rank 2 的计时覆盖决策、计划反馈与并发 coordinator 初始化，因此其 key 明确为
`decision_and_coordinator_work`。它不等于单纯的 `decision_shadow` 耗时。
`EventSourceIndex::QuoteExpiry` 与 `TickPhase::ExpiryShadow` 是不同契约；不能把阶段名当来源 tag。
Session 生命周期的 ordinal scope 为 `session_lifecycle`，数值 6 保持原身份域，
不能因与结算 rank 数值相同而改为 `SettlementShadow`。事件中的真实 `phase_rank`、
`classRank`、`local_event_index` 与执行事实序号保留各自含义，不以新名称重新推断编号。

审计建议的 `priority` 表达 `optional`、`recommended` 等建议类别，独立 `priority_rank`
保留其实际排序。它与 tick phase 的 rank 无关；历史材料中的优先级代号不能机械映射到执行阶段。

## 格式身份与真实版本

机器契约的 `schema`、`format`、source 与 scenario 身份表达用途，真实数值版本放在
独立的 `version`、`schema_version` 或对应明确版本字段中。生产者、严格消费者、CLI、
复用身份、fixture 与精确键集校验必须同步，不靠别名或静默转换维持旧拼写。
当前存档使用 `schema_version=3`，只接受 `runtime_state` 与当前来源 tag；schema 1/2
明确拒绝。`simulation_policy_id` 使用 `a-share-simulation`，本轮只改身份，不改模拟行为。
详见 [ADR-0029](decisions/0029-responsibility-names-and-contract-versions.md)。

当前机器契约明确分开身份与版本：

| 职责身份 | 独立数值字段 |
|---|---|
| `escrow-committed-phase-timing` | `schema_version=2` |
| `escrow-conservation-snapshot` / `escrow-determinism-observation` | `schema_version=1` |
| `simulation-acceptance-checkpoint` | `schema_version=4` |
| runner `simulation-acceptance` | `runner_policy_version=8`；`effective_date=2026-09-30` |
| `simulation-source-fingerprint` | `algorithm_version=1` |
| `simulation-resource-policy` | `schema_version=7` |
| `escrow-performance-config` / `escrow-performance-sample` | `schema_version=2` |
| `escrow-performance-report` | `schema_version=3` |
| `escrow-verification-matrix-summary` | `schema_version=1` |
| matrix `escrow-runtime-matrix`；scenario `escrow-runtime` | `matrix_version=1` |

模拟验收的当前来源身份为 `current_session_setup`。`volume_denominator_assumption`
表达实验假设回显，不能声称已校准真实成交量；`external_market_calibration_scope` 的值为
`not_applicable_synthetic_history_only`，继续遵守虚拟开局前史与撮合生成行情的边界。

真实数值不可因含有数字而清理：会计科目表的 `version` 及 `version()`、存档 schema 数值、
协议 generation、报告 revision、quarter、交易日与分钟、T+1、ECL `Stage1/Stage2/Stage3`、
价格或数量单位以及第三方 API 的固定版本均保留。例如 UUID `new_v4` 和 Tauri 2 是外部
真实契约，不属于内部实施编号。金额 cash 使用 Money 分，shares 与委托数量使用股，不能合并单位。

## 历史证据边界

密封 `.omo/evidence/`、压缩旧档、`chinese-localization/before`、已签署审计 JSON/Markdown、
原始输入引用、源码见证和原 SHA 保持原字节。历史候选的 `C01`、`C06`、reader/task/action ID
若用于检索原记录，也保留其真实来源身份；生成器中的当前说明和局部对象名称使用职责名称。
修改生成源不授权重跑它覆盖历史结果或重新签署旧审计。

历史 decoder 只在明确的证据读取用途解析历史标签、原始内容和 hash。它不构成当前游戏
存档兼容入口，也不能把旧输入转成新格式后声称新验收通过。旧改名与测试记录独立见
[历史命名验证记录](naming-refactor-validation.md)；其中旧提交和当时结果不是本轮验证结果。
