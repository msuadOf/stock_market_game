# CompanySystem 实施清单当前代码审计

审计日期：2026-10-06

审计基准：`git rev-parse HEAD` = `edd435a81cad6aeb561ce630a0f9eb00ee254322`

审计范围：`agents/company-system/implementation-checklist.md` 中所有未勾选项，依据当前工作区代码只读核对；本记录不代表代码验收或测试通过。

## 审计边界

- 完整阅读实施清单、[Q14 公司基本面与经营系统设计蓝图](../remaining-questions-and-features/q14-financial-model-design.md)、[当前交接](current-handoff.md)、工程原则及相关 engine/session 代码。
- 本次没有运行测试、编译或回归。`current-handoff.md` 所列短测和构建结果是既有交接记录，不是本次重跑结果。
- 审计时工作区有未提交改动：`packages/engine/src/company/mod.rs` 已修改；`agents/company-system/corporate-rules-refresh.md`、`packages/engine/src/company/cash_dividend/`、`packages/engine/src/company/ex_reference_price.rs`、`packages/engine/src/company/ex_reference_price_tests.rs` 为未跟踪内容。它们当前尚未验收，不能作为下文“已实现”结论的证据，也不能推断其测试通过。以下结论以已逐项核对的既有实现和短测证据为准；工作区变动若改变契约，应另行复核。
- 按任务范围，`CompanySimulation`、simulation dispatch、客户金融链及行业经营模块留给用户后续分支，不作为本轮 Simple 工作的阻塞项。其未实现不等于本轮 Simple 已完成共同公司行为契约。
- “部分实现”表示有可定位代码或短测支持该项的某些行为，但仍有列明的实质缺口；不能据此把原清单整项勾选。

## 逐项核对

### 1. 盘清身份与复用边界

- **1.1（清单第 28 行）—部分实现。** `CompanyId`、发行人身份映射和股本匹配已有 `packages/engine/src/company/identity.rs`、`packages/engine/src/company/spec.rs` 及 `packages/engine/src/company/system.rs`。但旧实体 `Company` / `CompanyConfig` 仍在 `packages/engine/src/company/mod.rs` 强制持有 `Books`、`CounterpartyLedger`、`ContractBook`、`OperatingBudget`；共同跨模式公司行为也没有实现。短测证据：`packages/engine/src/session/company_simple_session_tests.rs` 的身份、显式 `CompanyKind`、缺失/重复配置与股份匹配相关用例。结论只覆盖身份基础，不覆盖解耦及共同行为。
- **1.4（第 31 行）—财务部分已实现，公司行为复用仍缺。** 唯一账簿和报表算法由 `packages/engine/src/accounting/` 提供；Simple 财务使用 `Books`、`income_tax` 和 report generator，见 `packages/engine/src/company/simple/finance.rs`、`finance_posting.rs`。`ShareRegistry` 提供股份登记，不提供分红/认购/回购完整执行，见 `packages/engine/src/company/share_registry.rs`。因此会计算法未见 Simple 复制，但“识别并复用共同股本行为实现”仍缺。
- **1.5（第 32 行）—真实缺口。** Q14 说明的持久设计入口仍为 `packages/engine/src/company/economy/system_sim/README.md` 与 `DESIGN.md`；新父目录接口虽已有代码，设计入口迁移、引用修正和旧入口删除尚未完成。该项不是 Simulation 分支阻塞，而是文档路径迁移项。

### 2. 定义共同契约

- **2.1（第 36 行）—部分实现。** `packages/engine/src/company/config.rs`、`api.rs`、`system.rs`、`capabilities.rs`、`persistence.rs` 已提供模式配置入口、Simple 日期推进、基础命令/结果、能力结构及严格恢复。`CompanyImplementation` 当前仅有 `Simple` 变体，解释契约、共同股本命令与双实现分派没有完成。
- **2.2（第 37 行）—缺少双实现行为契约。** `packages/engine/src/company/system.rs` 中 Simulation 创建显式返回 `Unsupported`，所以两实现同状态同命令的行为一致性当前无法成立或验证。该结论记录实现状态，不把未实现 Simulation 作为本轮 Simple 工作阻塞。
- **2.3（第 38 行）—Simple 报表基础已实现，跨模式字段契约未完成。** `packages/engine/src/company/simple/finance.rs` 复用 accounting 生成报表，`finance_posting.rs` 形成汇总事实；但现有查询/披露是否对两个实现提供一致财务字段、期间、范围、来源和单位尚无可比较的第二实现证据。`cash_flow` 字段有意表达汇总模型的零真实收付，不应解释为已经模拟真实现金流。
- **2.4（第 39 行）—部分实现。** `packages/engine/src/company/system.rs::report_availability` 区分已公开报告、开局前、未结算和未表示期间；持久化使用严格反序列化，错误也有显式类型。尚不能证明两模式共同数据条件及共同操作错误分类，因为没有第二模式和完整股本操作。
- **2.5（第 40 行）—能力结构存在，条件能力缺失。** `packages/engine/src/company/capabilities.rs` 有共同 DTO；`system.rs::capabilities` 当前返回固定财务字段，并固定 `cash_settlement: false` 与缺口说明。它未依据持股、方案或业务条件计算能力；Simple 展示现金不作为付款条件的共同执行路径也未建立。
- **2.6（第 41 行）—部分短测存在，列出的契约对照测试缺失。** Simple 查询、来源、披露与恢复有定向测试（见 `packages/engine/src/company/simple/tests.rs`、`packages/engine/src/session/company_simple_session_tests.rs`）；没有两模式对照、共同条件拒绝或公司行为结果测试。

### 3. 实现 SimpleFundamentals 收入费用与汇总财务

- **3.3（第 47 行）—部分实现。** `packages/engine/src/company/simple/finance_posting.rs` 将周期收入及费用映射到账簿并计提所得税，报表由 `accounting` 派生；`packages/engine/src/company/simple/finance.rs` 通过候选状态保证财务变更原子。当前生成输入只含营收、固定费用、变动费用（`packages/engine/src/company/simple/period.rs`）；没有利息、折旧和其他明确项目的 Simple 生成输入，也没有 ROE/平均权益计算。
- **3.4（第 48 行）—勾稽有实现；公司行为不覆盖尚无从验证。** `packages/engine/src/company/simple/finance_tests.rs` 验证平衡报表、净利润、权益和账面现金不随汇总摘要冒充真实收付款；但实际股本操作尚未进入此账簿/状态，因而不能声称已验证其不被下一结算覆盖。
- **3.6（第 50 行）—已有多项短测，整项未完成。** 周期和年化换算、周期独立噪声、趋势状态与恢复见 `packages/engine/src/company/simple/period.rs`、`growth.rs` 中的测试；同长度基准、亏损、零基数/显式复业、失败原子性见 `packages/engine/src/company/simple/tests.rs`；财务税务、重复期间拒绝、财报勾稽见 `finance_tests.rs`、`finance_period_tests.rs`。ROE 期间/平均权益及公司行为前后权益测试缺失。既有交接记载 Simple 等组 85 项短测通过，日志为 `.tmp/checklist-wave4/host70-simple-short.log`；本次未重跑。

### 4. 隔离账面公司财务与投资者实际结算

- **4.1（第 54 行）—部分实现。** `packages/engine/src/company/simple/period.rs` 按显式趋势/比例更新营收及固定、变动费用；`finance_posting.rs` 以应收/应付做汇总入账，明确不制造真实现金流。配置中没有公司账面现金或投资额按百分比更新的模型字段/规则，故原项整体仍未完成。
- **4.3（第 56 行）—真实缺口。** 没有分红/认购/回购对投资者现金与股份进行共同、稳定来源 key、可恢复状态及原子结算。`packages/engine/src/company/system.rs` 返回 `cash_settlement: false`；`share_registry.rs` 的登记和 `cash_dividend_tax.rs` 的个人税务不足以证明公司行为已能从 Session 执行。
- **4.4（第 57 行）—真实缺口。** 没有 Simple 公司账面结果与投资者真实结算分离后共同执行的公司行为路径。当前 `packages/engine/src/company/scheduler.rs` 对 `ShareholderDistribution` 明确拒绝；因此尚无分红不看 Simple 展示现金、本人现金约束认购、回购真实委托成交及失败原子性的实现/测试。

### 5. 接通共同公司行为与披露

- **5.1（第 61 行）—真实缺口。** 分红、配股、增发、回购的共同条件、登记、税务和投资者结算未接通。`packages/engine/src/company/system.rs` 只有 Simple 实现且 `cash_settlement` 为 false；`scheduler.rs` 拒绝股东分配动作。
- **5.2（第 62 行）—真实缺口。** 尚无公司行为计划/执行区分、真实认购扣款或回购订单成交流程。现有交易撮合和股份登记模块不能替代公司行为与投资者账户间的完整接线。
- **5.3（第 63 行）—本批未完成规则核验。** Q14 要求按实施时有效的 A 股官方规则核验并记录依据日期；当前本批公司行为实现不存在，实施依据记录也尚未落入对应正式规范。不要将个人股息税短测当作公司行为制度已核验。
- **5.4（第 65 行）—报告窗口已部分实现，比率/共同契约仍缺。** `packages/engine/src/company/simple/finance_posting.rs` 只在事实覆盖完整窗口时生成相应报告；`finance_period_tests.rs` 用实际季度摘要证明不伪造月报；`system.rs::report_availability` 区分未结算和未表示期间。未找到 ROE 期间重算实现；长周期行为目前是 Simple 本身的报表事实，不是双模式共同 DTO 的完整证明。
- **5.5（第 66 行）—披露权限测试部分存在，公司行为测试缺失。** `packages/engine/src/session/company_simple_session_tests.rs` 覆盖 Simple 存档、披露游标/公开材料边界及恢复。所列公司行为方案条件、投资者现金不足、只按成交回购、结算幂等及行为后状态保留没有实现可测基础。

### 6. 新局、自然日及严格存档接线

- **6.1（第 73 行）—Simple 日结主体已实现，原清单状态偏旧；公司行为日结仍缺。** `packages/engine/src/company/simple/state.rs::advance_day` 连续推进自然日，非结算日更新日期，结算日基于候选状态生成并提交周期事实；Session 接线见 `packages/engine/src/session/company_assembly.rs`、`company_simple_session_tests.rs`。CivilClock 自身日结失败不提交的事务边界见 `packages/engine/src/session/civil_clock.rs` 与 GameSession 日结实现。Simple 尚无待处理/到期公司行为可在其余自然日执行。既有 handoff 记录 Simple/Session 短测结果；本次未运行。
- **6.2（第 74 行）—Simple 状态保存与恢复已实现，公司行为状态缺失。** `simple/state.rs` 序列化配置、随机状态、期间事实、历史及复业动作；`company/persistence.rs` 严格校验恢复；`simple_session_tests.rs` 覆盖真实 Session 保存/恢复。交接文档记载 release Engine 生成档案并验证恢复续行。由于公司行为没有接线，不能勾选其未完成行为恢复及幂等部分。
- **6.3（第 75 行）—共享 Engine 契约基础和部分宿主验证存在，完整多宿主行为未证实。** 当前 Session 持有同一 `CompanySystem` 状态；handoff 记录 ts-rs 128 类型导出、workspace 调用方检查及 WASM/Web 构建。没有公司行为 DTO/结算字段可验证跨 Server/Desktop 的完整语义，因此本项只可判部分实现。
- **6.4（第 76 行）—Simple 选择、restore 和日结相关测试存在，公司行为资金幂等缺失。** `packages/engine/src/session/company_simple_session_tests.rs` 覆盖显式模式、Simulation 请求拒绝、严格恢复和日结恢复；`packages/engine/src/company/simple/tests.rs` 覆盖失败不提交。公司行为/资金幂等无实现/测试，不能整项勾选。既有 handoff 记载短测和构建，不含全量回归；本次未运行。

### 7. Simulation 延后范围

- **7.1–7.4（第 80–83 行）—留待用户后续分支，不列为本轮 Simple 阻塞。** 不实现 Simulation dispatch、客户资金链或各行业 Simulation 业务；这些未来任务与 Simple 代码缺口分别记录。`packages/engine/src/company/system.rs` 当前明确拒绝 Simulation 是已知边界，不应描述为本轮 Simple 实现失败。

## 简短测证据索引

- 清单/交接记载 Rust Simple、周期、财务、披露、Session 等 85 项通过，九组短测日志：`.tmp/checklist-wave4/host70-simple-short.log`。
- 清单/交接记载 Web 新合同、初始化和真实日终档 45 项通过：`.tmp/company-system/web-final-kind-fresh-green.log`。
- 本记录引用短测仅用于判断已覆盖行为；未重新执行，不能据此扩大为全量验收或公司行为完成声明。

## 审计结论

当前 Simple 的周期生成、财务汇总、披露和严格存档有代码与定向短测证据。身份/模式/日结条目中有部分已实现却仍未勾选，适合拆分清单并更新状态。共同公司行为的实际投资者结算、ROE/平均权益、账面完整规则、旧持久设计入口迁移及公司行为官方规则依据仍是明确缺口。Simulation 后续工作按用户分支约定排除于本轮阻塞。工作区当前新增但尚未验收的模块不计入以上实现结论。
