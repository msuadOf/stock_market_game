# 隐藏扫描批次 037（owner 2）

## 范围与完整性

- 扫描计划：`agents/implementation-audit/hidden-review/scan-plan.json`，batch 37，owner 2；来源基线 `43b1aa5`，产品树 `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 来源原文根为主仓 `/data1/baiyifan/workplace/stock_market_game`。三份来源均从首行连续全文读到 EOF；首次并行读取 03 和 04 的文本完整显示，05 初次读取因文件描述符暂时耗尽失败，恢复后重新从头完整读取至 EOF。未以搜索片段代替全文。
- 计划行数与实测行数分别为 424/424、308/308、43/43；计划 SHA-256 与实测值全部相同；每份 `aliases=1`。
- 已阅读主仓 `AGENTS.md`、`docs/principles.md`；核对现行 ADR-0016、ADR-0024、ADR-0019 及总账 `agents/implementation-audit/implementation-audit-2026-10-02.md`、现有 G/Q 项。来源中的旧任务指令和“候选设计，未实施”只作为历史材料，不作为当前产品规范。

| 来源 | 计划/实测行数 | SHA-256（计划=实测） | EOF 与全章节矩阵 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/engine-company-03.md` | 424/424 | `e44ba4796c261a4d91cee63e9029450ddf24d7150e7b6f4577dff700e5e253b1` | 完整 EOF。总体判断；领域/调用契约；逐文件 26 项（defaults、error、events、industrial capex/chart/config/error/expenses/interest/loans/mod/production/purchasing/repayment/sales；insurance chart/claims/config/csm/error/groups/mod/premium/remeasure/service_release；company/mod）；可复用验证矩阵；批次关系与限制。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-company-04.md` | 308/308 | `e141477f5f4e133433fe2595db2520150d1533f7ea1b2318ee60a138491e1f74` | 完整 EOF。模块判断；§1–14 opening/operations 各子模块与公司经济状态；§15 company/query；§16–28 real_estate 的借款成本、chart、config、债务偿还、delivery、development、error、impairment、land、loans、mod、presales、projects；§29 rng。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-company-05.md` | 43/43 | `4d811ca7bf9dd5208d2ba6c8a251faaa6f5db27cdd5cc9ee69606f03467391a8` | 完整 EOF。公司规格与经营到期队列；`scheduler.rs` owner/调用/测试及两个恢复/序号独立候选；`spec.rs` owner/验证/测试；领域语义与范围。 |

## 当前实现与调用链

- **工商开局存货对账 D01 仍可在当前实现复现为代码路径风险，尚非已批准承诺遗漏。** 当前 `packages/engine/src/company/industrial/config.rs:98-125` 先对开局 seeds 累计成本，再仅遍历 `seeded` 映射键比对总账。故如果支持的 1403/1405 科目开局非零、但该科目没有任何 seed，该 key 不会被检查。`packages/engine/src/company/industrial/mod.rs:83-119` 的 `IndustrialBooks::new` 在局部 `Books` 上过账，调用此构造校验后才返回账套；当前 owner 是 `IndustrialOpeningReconciliation`/`IndustrialBooks::new`，caller 包含 `operations/day.rs:356`、company operation fixtures 与工业会计测试。现有 `tests/industrial_accounting/failures/guards.rs:279-299` 覆盖有 seed 金额不符，未覆盖无 seed 的非零余额。候选最小验证仍是分别对支持科目缺失 seed 的非零余额断言 `OpeningSeedMismatch`，保持既有有 seed 错配用例。不是 OOP 迁移任务，也不是已核实的真实会计法缺口。
- **保险合同组状态 owner 已实现，但部分失败原子性未证明。** `insurance/groups.rs:389` 的 `ContractGroupState::apply_release` 逐组件变更；`service_release.rs:24-99` 先准备批次、过账，再调用其应用。来源指出应用中多个 checked 运算可失败，需沿字段顺序核算部分更新。`apply_remeasure`、单位进度常规整数运算亦见来源的边界提示；没有从“该方法存在”推导整个 handler 具备回滚保证。当前消费者为 `InsuranceBooks::release_service` 与 insurance operations dispatch。保持 GMM、责任单元、赔案数据 owner 与总账分离符合现有领域结构。
- **工业事务与地产事务 owner 已拆分为 aggregate 实现。** `IndustrialBooks` 定义于 `industrial/mod.rs:51-119`；insurance owner 为 `insurance/mod.rs`；地产 `RealEstateBooks` 在 `real_estate/mod.rs`。子模块 handler 仍在对应账套 impl 上执行 validate/post/apply；`operations/dispatch.rs` 依行业变体路由日流。`real_estate/mod.rs` 的 `post_with_commit` 不能单独证明后续子账、counterparty flow 等多阶段整体 rollback；需逐 handler 核实。当前没有需求要求再引入 service/class。
- **经营调度器存在两个历史候选边界。** 当前 `packages/engine/src/company/scheduler.rs:123-158` 的 `submit` 在 `next_seq` 以普通 `+= 1` 增长，u64 耗尽时可能 panic（启用溢出检查）或回绕；`from_parts` / serde 恢复位于 `:179-238`，检查排序、key 唯一、ID 小于 next_seq 和 settled floor，但没有检查不同待办项的 `ScheduledDueId` 重复。`CompanyOperations::submit_due`/`dispatch_due_on` 为 caller，clock wiring 对 pending ID 建镜像。候选分别需要恢复重复 ID 拒绝（包括时钟镜像接缝先验证再写入）及 submit 序号耗尽显式零变更错误；当前没有这两条专门错误变体/边界断言证据。适用对象是游戏内公司经营自然日队列，不是沪深交易委托队列。
- **公司规格/股本概念当前仍明确。** `company/spec.rs` 的 `CompanySpec::validate_set` 校验集合 ID、股票映射及母公司图；`company/mod.rs:203-246` 的 `CompanyRegistry::new` 调用验证并建表，`validate_issuer_mapping` 对证券映射股本做精确匹配。源材料指出重复股票输入 tuple 的检验细节应结合 session 外部去重/调用者校验继续追踪；不得把 `issued_shares` 解释为流通股数。交易规则无改动。

## G/Q 与候选裁定

- 当前总账声明 G01–G68 分域缺口，G27 有单项核销，其余仍有未完成部分；Q 编号与 `docs/open-questions.md` 的 Q 编号命名空间不同，Q10 已转 G39。此批来源没有指出这些 G/Q 的直接映射。不得将会计 owner/测试存在或 OOP 提取描述映射为 G28/G35 等已解决证据；G28、G35 和 Q23 仍按各自日终合并、工商期末经营 caller 与税务调用边界裁定。G39/K7 亦与本批对象结构无直接关系。
- 既有总账中与候选相邻的项不能强行合并：D01 的工业构造校验、保险组本地部分应用、调度器 ID/序号恢复错误均不对应上述产品 G 条目的明确内容；未发现现行开放 Q 的直接对照。候选保留为历史风险线索，本审计未创建或更新 G/Q。若要单独立项，仍须按产品 TDD 先写边界失败测试，并由独立审查复核。
- 来源 03 对利率、成本单位、精算和凭证的描述是当时调查总结，不是重新认证的法规依据。现行文档将工业现金/证券账户隔离及不执行分红、增发、回购、清算分配登记在 ADR-0024/ADR-0016。产品行为未改变，未进行新的官方规则取证。
- 来源报告多处称 validate→post→apply、合同组、子账、总账为既有 owner；这些状态结构不能替代失败原子性证明。也不能因为来源曾未运行测试，就推论实现缺失。

## 核验边界

只进行了指定源全文/hash/行数核验、当前 caller/owner/consumer 静态阅读及 G/Q/ADR 对照；没有运行测试、构建或回归，没有修改产品文件或 Git，没有重新核验会计法规。具体尚未核事项：`InsuranceBooks` group 应用在所有 `checked_*` 失败输入上的可达性/合法值域、时钟镜像失败分支对全部调用顺序的零变更保证、CompanyRegistry 重复 tuple 的完整上层输入约束。不得把本报告当作完整 OOP 审查、完整 G01–G68 验收或业务账务法规认证。
