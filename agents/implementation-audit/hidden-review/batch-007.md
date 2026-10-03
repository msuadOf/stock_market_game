# 批次 007 全文复核

## 范围与 EOF 证据

来源根目录为 `/data1/baiyifan/workplace/stock_market_game`；产品代码按任务要求从 `.worktree/implementation-reaudit` 的 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 读取。依据主仓 `AGENTS.md` 与 `docs/principles.md`，这里只做静态审计，不运行 Git、产品测试或构建。源材料里的实施限制与结论视为审查材料，不覆盖本轮要求。

| 来源 | 计划行数 | 实读行数 | SHA-256 | EOF 证明 |
|---|---:|---:|---|---|
| `agents/oop-refactor-audit/challenge-2026-10-03/pipeline/report.md` | 1033 | 1033 | `a66e57938699821be8964fcd9de5876a85e1e1d4be0cef281cacbe23a613cf81` | 连续读取 1–256、257–527、528–805、806–1033；最终一行为 1033 行正文末行；现场 `wc -l` 与 SHA-256 均匹配计划。 |
| `agents/oop-refactor-audit/challenge-2026-10-03/pipeline/review.md` | 96 | 96 | `10797fd1fd7f71b98b5e95b8c18008d38bacd9b30f9e2c8427e51879ac45451d` | `cat` 输出全文件至末尾“后续任一受审文件改变仍须再核”；现场行数与 SHA-256 匹配计划。 |
| `agents/oop-refactor-audit/challenge-2026-10-03/README.md` | 17 | 17 | `9de913353cc15d2c2b2c7e8cc2fb2dd2470b564c03e48a6578b7eea369f29b12` | `cat` 输出全文件至末行“行为修复另列，不混入等价 OOP 提取”；现场行数与 SHA-256 匹配计划。 |

计划和现场统计：3 份来源、1146 行，行数与三项哈希逐一匹配。长报告分段连续读取，前后区间相接，无输出截断遗漏。另读 `agents/oop-refactor-audit/challenge-2026-10-03/pipeline/review.md` 与 README 的全文；没有把搜索片段当全文证据。

## 全章矩阵

| 来源及章节 | 行号 | 全章核对要点与当前裁定 |
|---|---:|---|
| report：范围、来源、语义和组合边界 | 1–18 | 111 文件/46935 行、reader/hash 修订、法源边界与 P0/P1/P4/P9 语义均属历史审计陈述；实施阶段仍须遵守现行局部受理语义，不能将对象抽取解释成规则变更。 |
| report：全部候选处置表 | 19–52 | 30 条候选处置逐项核对：12 new、10 covered、5 rejected、3 extension。拒绝理由有具体反证；未发现因短、纯、已有 struct 或未测试而错误排除对象。 |
| report：N01 PlanChainFactConsumption | 58–125 | 四组身份集合及共同提交点、continuous/auction callers、payload 检查交错次序均有明确边界。当前实现 owner 存在，见 `adaptive_plan_chain.rs:47`；实现台账列 `prepare_continuous_round`、`commit_round` 等方法及 caller。 |
| report：N02 TradingDayEndTransition | 126–191 | 两个日终入口共享一次 candidate transition；日界顺序、T+1 解锁时点、候选失败丢弃、非 CivilUpdate 边界均明确。当前 owner 位于 `auction_day_end.rs:2127`，台账列连续和竞价 caller。 |
| report：E03 ContinuousFillReceiptProjection | 192–256 | 旧 A01 已覆盖 processor 方法迁移；E03 只细化单 Place builder，不重复计数。逐腿 receipt、ordinal 和费用顺序约束明确；当前 owner 在 `continuous_matching.rs:1039`，生产 caller 经 processor。 |
| report：N04 ContinuousDayEndLifecycleProjection | 257–320 | DayEnd release 对 parent/NPC/retail/facts 的投影边界与共同 transition 区分清楚；唯一连续 finalizer caller。当前 owner 在 `continuous_tick_finalizer.rs:241`。 |
| report：N05 CapturedExperienceObservation | 321–385 | 具名 enum 绑定同源 observation，同时保留非 Retail 有 experience 的合法组合；不提前 risk 计算、不更改 capture 时点。当前 enum 在 `decision_snapshot_capture.rs:273`，调用 `capture_decision_snapshot_in_place`。 |
| report：N06 IncrementalContinuousStockShadow | 386–456 | 每股 market、ledger、event cursor、outbox 属 shadow；跨股调度/聚合仍归 coordinator。失败消费及 coordinator invalidation、清簿前 closing price 和 P0/P1/P9 边界明确。当前 owner 在 `incremental_continuous_stock_shadow.rs:71`。 |
| report：N07 SellerChargeAllocation | 457–526 | 本腿 seller cap、历史欠费与费用分项；独立 ledger validator 继续校验，不能把共享算法当真实清算。当前 owner 在 `transition.rs:190`；现行 ADR 将 fee/cap 作为游戏简化，材料明确未重新认证官方法源。 |
| report：N08 ReadyAdmissionPlan | 527–594 | 资源边、quote dependency、股票 gate 与拓扑输出由当前 ready batch 拥有；不构造账户/股票全序，不将布局顺序变交易优先级。当前 owner 在 `local_admission.rs:85`，`admit_ready_batch` 为受理入口。 |
| report：N09 NpcOrderLifecycleBook | 595–662 | 普通 NPC quote 生命周期与 ParentOrderPlan、订单簿及期限算法分离；重复 ID panic 的时点、三字段 remove、存档 Vec 形状明确。当前 owner 在 `quote_expiry.rs:22`；GameSession 注册/移除、P0 expiry 与日终为消费者。 |
| report：N10 ExpiryOutput | 663–725 | 明细和 account ResVec 汇总联合写；汇总无生产 consumer，因此不构成 P1 性能改善，也不可再叠加进 post-P0 budget。当前 `quote_expiry.rs:16`。 |
| report：N11 AccountFillProjection | 726–805 | 单账户跨 order 的数量、经验、机构成本、fees 和最终 position 对账；保留先收集所有 job error 再报告 final-position error。当前 owner 在 `retail_projection.rs:438`，`project_receipts` 调用。 |
| report：N12 ReceiptSettlementPlan | 806–870 | 重用 `SideTotals`，保留零量跳过、实收费、按账户并行准备、同股 Buy-before-Sell 和单次 patch 安装。当前 owner 在 `settlement.rs:40`，生产 caller 为 `prepare_settlement_transaction_with_beliefs`。 |
| report：N13 SelfViewCashReservations | 871–927 | reserved/replaceable 只属于 observation SelfView，不是 P1/P3 执行预算；保留 sub→add 及 Money 错误定位。当前 owner 在 `decision_snapshot_capture.rs:430`，caller `build_self_view_for`。 |
| report：E01、E02 局部受理语义修订 | 928–1028 | 仅纠正旧动作依据，不增加对象或改算法；ADR-0017 修订及 ADR-0018 §7 已接受语义不能当未决提案，也不能据输出排序建跨实体优先级。 |
| report：覆盖和复核结尾 | 1029–1033 | 原文声称的是旧调查自己的独立审查，不等于本轮对产品的独立实现审核；此批只确认历史复核记录已读。 |
| review：身份、首次签署、审查范围 | 1–31 | 清楚区分静态审计与运行证据；记录 111 文件覆盖及错误 hash 修订。reviewer 明示未全文重读全部源码，故不把其范围扩称为本轮源码全审。 |
| review：发现闭合 | 32–44 | N03 去重为 E03、N09 duplicate panic 时序、reader hash、测试阅读归属、N01 caller 符号更正及短 ID 修正均逐项闭合；这是材料完整性证据。 |
| review：候选全向去重及 rejected 理由 | 45–69 | N01–N13 与 E01–E03 方向逐项检查；五个 rejected 有边界反证，12 new + 3 extension 一致。没有新产品缺陷的可靠生产证据。 |
| review：三门结论及限制 | 70–82 | 结论限调查方案静态迁移门禁；A 股规则法源没有重新外查，测试/build 未运行。不能把这段扩张为本轮产品通过声明。 |
| review：domain 16-1 增量修订 | 83–91 | 删除被拒 `OrderCashRules` 依赖，N07/N13 改回 config 现有 helper/receiver，未扩大迁移范围。 |
| review：现行冻结哈希 | 92–96 | 当前冻结 SHA 与现场哈希匹配；已读取至 EOF。 |
| README：简介、入口、反查方式 | 1–13 | 文档界定 78 项新增/6 项增强，介绍五组角色、caller/owner/错误序核对及源码阅读与 hash 的区别；作为总清单导航，不是实施证据。 |
| README：范围与限制 | 14–17 | 范围/排除和“不实施、不跑测试构建、不改产品/正式文档”的历史任务边界，视为来源陈述；本次按照协调者新任务静态扫描产品代码。 |

## 最新 caller、owner 与总账交叉核对

现行实现证据取 `.worktree/implementation-reaudit` 的 `43b1aa5`，最新完整 OOP 状态取主仓 `agents/oop-refactor-implementation/pipeline/implementation-ledger.json`。台账将 N01–N13 列为“实现与生产caller完成；独立完整diff复核通过；所列精准短测试及成熟源码编译检查通过”，E01–E03 归在已完成增强，不增加目标数。这里仅核读既有台账，未重跑其记载验证。

| 对象 | 43b1aa5 产品 owner 位置 | 台账所列生产 caller / 消费链 | 核验结果 |
|---|---|---|---|
| PlanChainFactConsumption | `adaptive_plan_chain.rs:47` | coordinator continuous/auction 投影及 lifecycle 投影 | 实对象存在，未止于调查建议。 |
| TradingDayEndTransition | `auction_day_end.rs:2127` | `finalize_continuous_tick`、`apply_finished_candidate` | 共享日终 owner 与双入口已接。 |
| ContinuousFillReceiptProjection | `continuous_matching.rs:1039` | `ContinuousStockRoundProcessor::fill_receipts` → `process_continuous_stock_step_inner` | 单 Place builder 是旧 A01 的细化，不重复 new。 |
| ContinuousDayEndLifecycleProjection | `continuous_tick_finalizer.rs:241` | `finalize_continuous_tick` | 连续日终生命周期投影已接；与竞价 projector 区分。 |
| CapturedExperienceObservation | `decision_snapshot_capture.rs:273` | `capture_decision_snapshot_in_place` | 具名 observation owner 已存在。 |
| IncrementalContinuousStockShadow | `incremental_continuous_stock_shadow.rs:71` | `IncrementalContinuousStockCoordinator::apply_round/finish_for_tick` | 每股 shadow 行为由 coordinator 驱动。 |
| SellerChargeAllocation | `transition.rs:190` | `FillTransition::sell` | 当前 seller cap 值对象存在；费用简化与 validator 独立性保持。 |
| ReadyAdmissionPlan | `local_admission.rs:85` | `admit_ready_batch` | 批内偏序对象已由 admission 构造/消费。 |
| NpcOrderLifecycleBook | `quote_expiry.rs:22` | GameSession register/remove、P0 expiry、日终 transition | 生命周期集合 owner 在多入口共享。 |
| ExpiryOutput | `quote_expiry.rs:16` | `GameSession::apply_quote_expiry` | 生产只消费明细作 P0 身份边界；汇总不能当 P1 预算。 |
| AccountFillProjection | `retail_projection.rs:438` | `project_receipts` | 账户工作态进入真实 receipt projection。 |
| ReceiptSettlementPlan | `settlement.rs:40` | `prepare_settlement_transaction_with_beliefs` | 批次聚合交到生产 settlement 准备。 |
| SelfViewCashReservations | `decision_snapshot_capture.rs:430` | `build_self_view_for` | 仅观测现金累积，和执行预算区分。 |

最新流水线契约总账 `agents/implementation-audit/reaudit-pipeline-contracts.md` 记录 P0/P1、P3/P4、失败隔离/P9、settlement、交易日日界、存档消费者链完整；没有确认既有生产功能丢失，并明确未运行相关测试。总缺口账 `agents/implementation-audit/implementation-audit-2026-10-02.md` 的 G16（全历史状态复制）与 G39（K7 自由调度下的错误等价比较）仍是独立项；以上 OOP owner 不自动消除它们。G39 仅保持固定受理事实/业务守恒验证，不把无关交易结果强求同序。此批没有可合理关联的新增 G 编号，也没有把任何 Q 项误作实现缺失；Q05 是工具入口政策，与本批无关。

## 候选、反证及结论

- 新缺失候选：无。报告中的 12 个新对象、E03 细化及 E01/E02 语义修订均已在 43b1aa5 产品代码和现行实施台账中找到 owner/caller；没有发现新的生产断接证据。
- 保留的反证：测试或全量验收未运行本身不是功能缺失；OOP 对象存在也不表示 G16/G39 等别项已经解决；旧调查对官方法源未重新认证，不能据此提高真实 A 股规则保证。
- rejected 复核：r05 是既有 coordinator 保留，r09 无额外共同不变量且会扩大耦合，r10 fast check 与 evidence replay 生命周期不同，r17 snapshot 已有 seal/query owner，r26 仅 Vec DTO 包装。r13 构造 wrapper 被 adapter/worker 双防线反证。未见某项因“对象已经存在”一条理由就被拒。
- 未核实：未执行任何测试/构建/性能矩阵；未联网核验交易所/中国结算材料；未重读被历史审计列出的 111 个底层源码文件，本轮对其通过现行 owner/caller、总账和实施证据复核，不冒充 111 文件全文复读；不审查本批以外 77 项对象的实施完整性。
- A 股边界：本批是结构/状态 owner 检查，不提出撮合、费用、T+1 或日界规则变化。金额单位 Money 分、数量股、market minute 使用绝对标准交易分钟；NPC quote expiry、envelope、seller cap 均按已记录游戏简化处理。

## 引用的现行材料

- `docs/principles.md`；`docs/decisions/0017-escrow-parallel-tick.md` 顶部修订；`docs/decisions/0018-long-running-immutable-timeline.md` §7；其余交易语义边界见历史报告引用的 ADR-0009/0014/0019 等，本轮未重新外查法源。
- `agents/oop-refactor-implementation/pipeline/implementation-ledger.json`、`agents/implementation-audit/reaudit-pipeline-contracts.md`、`agents/implementation-audit/implementation-audit-2026-10-02.md`。
