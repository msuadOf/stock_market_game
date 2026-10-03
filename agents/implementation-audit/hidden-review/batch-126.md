# 批次 126 历史对象化材料复核

## 范围与逐篇读取

任务清单取自 `scan-plan.json` 的批次 126，source root 为 `/data1/baiyifan/workplace/stock_market_game`，产品审计基线 `43b1aa5`。三篇材料均按完整文件连续读取至 EOF；哈希与清单一致。

| 来源 | 行数 | SHA-256 | 读取状态 |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/reviews/session-strategy.md` | 49 | `6bc757c3d05cca91bed9438b75ebbd97ddc9262db6cf7d9d1a644ec786cc4120` | EOF |
| `agents/oop-refactor-audit/chinese-localization/tool-review.md` | 202 | `cbb9a39490c033e793d585e6914b25e67bf24aeb8af7317ff91c58395fe4f6c4` | EOF |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/action-index.md` | 1187 | `5673b086887e83e298339484ef7a7936d5376383ba9ff3f54b9ca64354f905a6` | EOF |

根 `AGENTS.md` 与 `docs/principles.md` 已读。历史文档审查结论只证明其记载的翻译/工具差异与版本绑定；不得沿用旧 agent 身份或把历史“通过”解释成当前实现通过。OOP 提取只表示所有权/组织迁移，不自动修复原有缺陷或任何 G 项。

## 章节族与候选状态

`action-index.md` 是全量候选索引而非已实施清单；所列代码位置和迁移均为建议。对应中文化复核保留其翻译限定：不重发源码语义审计结论，不抹掉未决错误/原子性边界，也不把建议测试说成已运行。

在 43b1aa5 的产品 caller 复核及当前工作树代码中，以下候选的目标组合均可定位。该状态仅为“抽取已在当前代码出现”的静态观察，不是候选方案的完整验收或缺陷修复声明：

| 候选族 | 当前代码锚点 | 状态与限制 |
|---|---|---|
| `engine-foundation-01-A03` Account/Position 写边界 | `packages/engine/src/account.rs:95`、`:103`、`:609` | 受控状态/Position 组合已存在；复核仍须区分生产 restore/strategy 写口与测试 fixture API，不把私有字段视作解决账户会计缺陷。 |
| `engine-pipeline-01-A01` AccountBudget | `packages/engine/src/session/pipeline/account_validation.rs:1110`、`:1115` | 预算值及 receiver API 已存在；P1 快照、现金/股数单位和并行 lane 受理次序仍是领域契约。 |
| `engine-pipeline-03-A02/A03` 竞价 shadow/coordinator/projector | `packages/engine/src/session/pipeline/auction_day_end.rs:175`、`:200`、`:1779` | 三个所有权边界均有对应类型；不可据类型抽取宣称竞价 FIFO、receipt、T+1 或 P9 原子性已由本候选新增/修复。 |
| `engine-pipeline-05-A01` 连续撮合 round processor | `packages/engine/src/session/pipeline/continuous_matching.rs:254` | processor 存在；round 内校验与跨 round finish 边界依旧不同。 |
| `engine-pipeline-10-A01` ExperienceUpdateMode | `packages/engine/src/session/pipeline/retail_projection.rs:336` | `InstitutionalFacts(moment)` 变体及分支可见；只消除非法分离参数组合，不改机构投影/成交规则。 |
| `engine-pipeline-11-A01` StockStreamCoordinator | `packages/engine/src/session/pipeline/stock_stream.rs:292` | coordinator 存在；调度结构不产生证券或账户优先级，也不改变本地冲突顺序。 |
| `engine-session-01/02/03-A01/A03/A01` | `packages/engine/src/session.rs:1135`；`session/candles.rs:92`；`session/decision_chain.rs:64`；`session/decision_chain/roots.rs:6` | 状态容器、日K owner、观察与 RootReadContext 均有锚点；它们不替代 TickShadow/P9，不改变 save DTO，也不扩张根决策权。 |
| `engine-strategy-01-A01` / `engine-strategy-03-A01` | `packages/engine/src/plans/state.rs:193`；`packages/engine/src/strategy/zi_noise.rs:42` | TradingPlan receiver 与 ZiNoise 参数投影已存在；不修复文档另列的版本溢出/期限问题，也不改变零售整手、T+1、费用和序列化参数。 |
| `hosts-01-A01/A02` / `hosts-03-A01` | `apps/desktop/src-tauri/src/actor.rs:334`；`apps/server/src/actor.rs:607`、`:624`；`apps/web-wasm/src/lib.rs:53` | actor handles 与 WASM registry 均可定位。桌面/服务端命令仍只是入队或回执各自既有契约；不可把候选私有化写口与 G40 的宿主应用确认混为一谈。 |
| `tooling-01-A01/A02` / `tooling-04-A01` / `tooling-05-A01` | `packages/engine/examples/escrow_verification_harness/runtime.rs:587`、`:646`；`scripts/build-targets.mjs:154`、`:219`；`scripts/simulation/baseline-run.mjs:1291`；`scripts/simulation/run-escrow-verification-matrix.mjs:557` | 路径/写入策略、build owner、batch context、matrix run 均有实现锚点。它们不意味着进程树 deadline、sampler 清理或自由 rerun 确定性门禁已自动满足。 |
| `web-01-A06` / `web-01-A27` / `web-08-A01` | `apps/web/src/app/useSessionHostLifecycle.ts:218`；`apps/web/src/app/useSaveCommands.ts:243`；`apps/web/src/app/useTradingCommands.ts:23`；`apps/web/src/app/market-chart-projection.ts:56`；`design/ui/mobile/mobile-trading-concept.html:120` | hooks、图表投影和原型控制器已落点；命令输入仍是前置校验，EngineHost/engine 为受理边界；原型示例数据不构成 A 股制度。 |

## 需求与语义交叉核对

`43b1aa5` 对应产品 caller 的现行审计把 G01–G39 重新追踪，G27 核销、其余 38 项仍有缺口；新增 G40–G68 进入总账。Q10 已转为 G39，Q01–Q09、Q11 及新增 Q12–Q23 仍按裁定记录作为范围/契约问题。这里没有把 OOP 候选映射成新修复，也没有据“当前类型存在”核销任何上述 G/Q 项。

直接相关的既定边界包括 ADR-0010 宿主协议与命令确认、ADR-0017 escrow/P0–P9、ADR-0018（仍是 proposed 的部分时序契约）、ADR-0025 日终持久化、ADR-0027 部署目标；ADR-0019 只约束当前开发范围/容量，不能为任何重构推出任意容量保证。撮合候选继续以沪深 A 股游戏已登记规则为基线：钱用分、数量用股，证券类别限制须显式区分，整手买入/T+1/集合竞价时段与 FIFO/撤单限制不能被组合抽取模糊化。此次只做静态文档/代码核对，未查询交易所新规、未运行产品测试。

工具复核文档的最终签署范围是中文化工具及版本绑定、机械 diff/证据定位；其明确不替代产品源码语义审计或产品运行验收。其设计增量对历史审查身份、真实报告版本、全量预检/发布阶段的初查问题给出修复记录，但“哈希/标准 verifier 通过”也不能证明产品行为或 A 股制度正确。

## 反证与剩余边界

- 可见实现锚点构成“候选已被后续实现”的反证，不能反过来证明当年候选已经完成，也不能复用历史代理签名代替本轮审查。
- 旧 action-index 明示若干风险与行为分离：TradingPlan 版本溢出、restore 校验、`tooling-01-A01` 顺序写入的非原子性、BuildRun 不承诺进程树终止、Escrow symlink 独立缺陷等仍需按当前总账/后续决策判断；对象化不是修复。
- 43b1aa5 的候选裁定提及的是 08e4fc7 产品提交基线；本复核读取该提交下的 caller/决议与当前 43b1aa5 worktree 的实现锚点。没有宣称 43b1aa5 引入的历史资料改动本身改变产品行为。
- 不新增交易规则结论或测试要求；没有发现可据本批材料独立升级为 G 的证据。完整当前状态仍以实现缺口总账和后续正式 ADR 为准。

## EOF 记录

三篇指定来源全文连续读取至 EOF；三个文件行数与 SHA-256 均与 `scan-plan.json` 一致。行动索引全量章节族均已核对；当前实现状态使用具体代码锚点佐证，未将 OOP 抽取当成修复。
