# Simple 功能提交边界盘点

日期：2026-10-06。范围：只读检查当前工作树、暂存区、未跟踪文件及本主题交接／复核记录；未修改产品代码，未暂存文件，未运行测试。

本盘点保留整合提交之前的分组风险与判断，不是当前未提交文件清单。后续已按周期算法、共同财务／披露、Session 契约、Native 宿主与 Web 消费端分别保存代码；当前实现和未完成边界见[交接](current-handoff.md)。不能再据本文的约 700 个路径或待验证接缝推断源码仍未提交。

## 结论

不能把当前暂存区或所有 `company/` 路径作为一个“Simple 提交”直接提交。状态中有约 700 个跨主题变更；同一 Rust/TS 文件还混有共享股本税务、report correction、公共协议、信息历史、money wire 等差异。最有意义的 Simple 闭合提交需要把 Engine core、finance、information、Session 持久化接线、Web schema/UI 与生成契约按依赖纳入；目前它们的公共接缝未都通过最终门禁，且几个关键生产文件的同一文件 diff 混有别的主题。

建议保留两个层次：

1. 可以单独形成一个**Simple 基础模型**提交：CompanySystem 身份／配置／状态、Simple 周期生成与定点复利算法、独立核心测试及其专属类型绑定。但它只能作为可复用 Engine 功能提交，不表示 GameSession 已采用，不能对外描述为可玩的完整 Simple 功能。若不能从共享 `company/mod.rs`、`company/spec.rs`、`company/rng.rs`、`company/query.rs` 准确拆出纯 hunk，则不要声称该组独立可提交。
2. **GameSession 实际采用的 Simple** 是更大的依赖闭包：上述 core + 汇总 finance/共同报表 owner + information 来源与披露 + Session setup/assembly/hash/save/restore/日结 + Rust/TS 生成绑定 + Web 严格 schema、preset、seed 与 UI。它依赖的 `session.rs`、`company/mod.rs`、`information/mod.rs`、多个公共类型与 generated 文件当前都混有外主题改动，应先按 hunk 归属拆分，再决定一个或几个有序提交；不能仅凭文件名 `simple` 选整文件。

## 交接与复核给出的状态

- `simple-core-contract.md` 与 `final-period-core-independent-review.md`：period core（四周期、自然时间趋势、增长定点、严格恢复）有 15+10+6 项限定验证及非作者 core 签核。签核明确不包括完整 finance、股本投资者结算、Session 全路径或宿主。
- `session-integration.md` 与 `session-review.md`：Session setup、公司状态 hash、恢复、日结事务、披露边界接线独立成组。旧记录指出进度检查项及最终整合边界；不得把原 CompanyOperations 低层 fixture 消费者覆盖误报成 Simple Session 经营闭环。
- `simple-finance.md` 与 `simple-finance-review.md`：Finance 11 项有 host63 PASS，之后前史开账基准出现 host65 红并加 guard，当前记录称待 root fresh green；Q17 更正接线由另一 owner 实现，不能用 Finance 基础 PASS 代替更正集成验收。
- `information-contract.md`、`information-independent-review.md`、`information-final-review.md`：公共完整 ReportSet、必填来源、已公开时点、本人获知和 availability 属信息层；限定短测已记录，但结论不覆盖 Simple 全功能或股本结算。
- `web-simple-source-handoff.md`、`web-host-integration.md`、`final-period-web-review.md`、`seed-preview-runtime-review.md`：严格 schema、初值预设、seed 预览与消费、恢复回填、披露来源展示是 Web 接线；Web 限定复核 PASS 同样不等于 generated bindings、真实完整 fixture 或全宿主接通。
- `fixture-migration.md` / `fixture-independent-review.md`：旧测试迁移保留断言，但完整存档正例仍等待真实生成 fixture。不得提交过时 JSON fixture 后宣称完整保存恢复验证已通过。

本主题适用决策为 ADR-0035、ADR-0036 及 Q14 蓝图：Simple 是账面汇总生成，不追踪真实公司资金流；报告和投资者股本操作应遵共同语义；当前不实现 Simulation。不是本轮交易制度改动，也不能用这些文档替代股本行为所需的大 A 规则审查。

## 依赖闭包路径

### 可考虑单独提交：Simple 基础模型（Engine-only）

需从当前 worktree 精确取出以下 Simple-owned 文件，不含同目录其他主题：

- `packages/engine/src/company/api.rs`
- `packages/engine/src/company/capabilities.rs`
- `packages/engine/src/company/config.rs`
- `packages/engine/src/company/identity.rs`
- `packages/engine/src/company/persistence.rs`
- `packages/engine/src/company/system.rs`
- `packages/engine/src/company/simple/**`
- `packages/engine/src/company/mod.rs` 中只与上述模块、`CompanySystem`/公开 Simple 类型导出有关的 hunk
- `packages/engine/src/company/rng.rs`、`packages/engine/src/company/spec.rs`、`packages/engine/src/company/query.rs` 中只被 Simple 核心所需的 hunk（先审 hunk，当前文件另有变更）
- `packages/engine/src/company/simple/growth.rs` 所需 `num-bigint` direct dependency 的 `packages/engine/Cargo.toml` hunk；根 `Cargo.lock` 仅在确有新解析差异时收录
- `packages/engine/src/company/simple/tests.rs` 等 Simple 自有单测随实现一起
- `apps/web/src/types/generated/{AnnualTrendConfig,CompanySystemConfig,PeriodGenerationParameters,PeriodNoiseConfig,PeriodVariableExpenseRule,SimpleCompanyConfig,SimpleConfig,SimpleEnvironmentConfig}.ts` 及财务专属 generated types（仅由该核心契约拥有者生成的类型）

这组明确**排除** `share_registry`、`cash_dividend_tax`、通用 `income_tax`/correction 其他 owners 的新增 hunk；但 core 与 finance 实际存在强引用，若基础 Engine 为编译而需要这些共享接口，说明无法按上面范围分离，应将其归共同前置而非硬造 standalone 边界。此提交也不包含 `SessionSetup`、日结、save fixture、Web 启动 UI，不能叫做 GameSession Simple 完成。

### GameSession Simple 采用闭包

在基础 core 之上还必须包含下列 Simple-specific 实际依赖，逐文件按 hunk 抽取：

- Finance：`packages/engine/src/company/{income_tax.rs,report_correction.rs,correction.rs}` 及 `packages/engine/src/company/simple/{finance.rs,finance_config.rs,finance_period_tests.rs,finance_posting.rs,finance_report_validation.rs,finance_state.rs,finance_tests.rs,finance_validation.rs}`；另含其依赖的 `accounting/{journal.rs,error.rs,closing/mod.rs}` 和必要报告／税务 owner 接口 hunk。这里 `company/mod.rs` 已同时导出 `share_registry`、`cash_dividend_tax`，不要整文件带入这些无关实现。
- Information：`packages/engine/src/information/{mod.rs,publication.rs,public_view.rs,prehistory.rs,schedule.rs,monthly_schedule.rs,simple_disclosures.rs,source_tests.rs}` 中来源、`ReportFrequency`、Simple 定稿披露及 availability 的实际引用 hunk；`packages/engine/src/company/query.rs` 中可用性 DTO/facade hunk。该集合与 Q23 月报排期及其它信息修订重叠，须逐 hunk 拆。
- Session owner integration：`packages/engine/src/session.rs` 的 `SessionSetup.company_system`、`SaveSlot.company_system`、状态持有／clone／commit、公司 API／查询与错误映射 hunk；`packages/engine/src/session/{company_assembly.rs,company_simple_session_tests.rs,hash.rs,persistence.rs,snapshot.rs,decision_chain/roots.rs,disclosures.rs,intraday_disclosures.rs,protocol/civil/session.rs}` 中构造、自然日日结候选、hash、save/restore、来源检查和 public report path 的 Simple 相关部分。
- Session 测试 fixture：`packages/engine/test-support/simple_company.rs`、`packages/engine/src/lib.rs` 的 cfg(test) include hunk、Session/宿主原有 setup 迁移 hunk，以及重生成后的 `apps/web/src/save/fixtures/current-schema-save.json`、`current-company-slice.json` 和新增 closed-day fixture。只收已有断言对应真实生成产物；不要把未生成 fixture 的测试迁移说成绿。
- Rust typegen 与 Web generated contract：`SessionSetup.ts` 的 `company_system` 属性、`SaveSlot.ts` 的 `company_system` 属性、Simple/Finance/信息相关 DTO。现有 `SessionSetup.ts` 与 `SaveSlot.ts` 为 `MM`，另有 `report_frequency`、历史/协议字段改动；必须按字段/hunk拆分，不能将整文件视为 Simple-owned。`apps/web/src/types/engine.ts` 也含不属于 Simple 的 wire 变动，不整文件带入。
- Web schema：`apps/web/src/save/schema/company/{system.ts,system-config.ts,simple-values.ts,annual-growth.ts,period-generation.ts,simple-finance-config.ts,simple-finance.ts,report-corrections.ts}` 及对应测试；`books/{income-tax.ts,tax-owner-test-fixture.ts}`、`accounting/amount.ts`、`journal.ts`、`reports.ts`、`root.ts`、`market.ts` 只取 Simple strict parse / owner 引用 hunk。`reports.ts` 与若干 books/parser 当前同时有其他 owner 契约，需分 hunk。
- Web 预览与应用接线：`apps/web/src/config/{company-initial-preset.ts,seed-draft.ts}` 及其测试；`apps/web/src/components/company/{CompanySystemInput.tsx,ReportNotes.tsx,CompanyPanel.tsx,company.css}` 与 source rendering/tests；`apps/web/src/App.tsx`、`StartupScreen.tsx`、`useSessionHostLifecycle.ts`、`useSaveCommands.ts`、相关 app tests 中 setup/seed 草稿字段与新局消费 hunk；`apps/web/src/app/company-config-commands.test.ts`。`App.tsx`、启动/lifecycle/CompanyPanel 目前混有其他 UI/refresh/report query 改动，不能整文件归并。

这是一组完整的跨层依赖族，不保证单个提交最合适；若拆成多个有序提交，必须保证每个提交在 HEAD 上可编译且契约两侧同步。若不能从 session/public contract hunk 剥离其他未提交前置（特别是 Session Money/Ingress、信息月历及股本登记），则此闭包现在只能作为延后整体提交的候选，不能缩成 pure Simple 源文件集。

## 阻止“纯 Simple 提交”说法的共享接缝

- `packages/engine/src/company/mod.rs` 同时改了 `simple`、共享税务／correction、`share_registry`、`cash_dividend_tax` 导出；当前 staged 与 unstaged 两份 diff 叠在一起。
- `packages/engine/src/session.rs` 含 Simple `company_system` 接线，但也含多个 ingress、history、membership、money 及 protocol 主题的类型／方法；Session 是产品接线的必要文件，却不能整文件纳入一个窄 Simple 提交。
- `packages/engine/src/session/company_assembly.rs` 从旧经营系统整个改为 CompanySystem 装配，同时保留 report frequency / company groups 变动。公司组移除及月度披露并非自动属于 Simple core。
- `packages/engine/src/session/persistence.rs` 是 schema/restore 的必要接缝，又包含 intraday disclosure 校验、history 与其它 validation 迁移；其测试 hunk含旧会计 fixture 迁移。
- `packages/engine/src/lib.rs` 聚合多个 API/export 与 Simple test fixture include；公开 DTO bindings 也由同一 `ts-rs` 导出集批量生成。
- `apps/web/src/types/generated/SessionSetup.ts`、`SaveSlot.ts`、`types/engine.ts` 与 schema root/store/app 初始化均同时承载多个兼容性不可接受的契约变化；需先有字段/hunk清单再 staging。
- `company/mod.rs` 的新增 `share_registry` 与股息税模块是未完成共同公司行为功能，不可因它们和 Simple 在同一目录/公司系统调用而暗中宣称本 Simple 批次已实现或复核了 A 股行为。当前 `CompanyCapabilities.cash_settlement == false` 也被 core 复核明确限定为共同实际投资者结算尚未完成。

## 给 root 的操作建议

1. 暂存前按上面的 owner 组建立逐文件、逐 hunk manifest；当前 `M`/`MM` 说明指数区与工作树区不是同一版本，先分清已暂存内容的主题归属。
2. 如果目标是尽快提交一批完整且真实可描述的成果，优先考虑 `Simple core` 的有限 Engine-only 提交，前提是它在脱离 Session 和共享新接口后的 HEAD 上能通过编译边界；其余明确标为后续依赖提交。若不能通过该边界，避免拆成破损过渡提交，待 finance/information/Session/Web 依赖准备好后按闭包拆分。
3. 不要将当前全部暂存区、整个 `company/`、全部 generated files、所有 Agent handoff、或 `current-schema-save.json` 手工修复值一次性提交成 Simple。删改文件数量大不等于同一功能闭包。
4. 任何 Simple 生产闭包若包含股本行为实际结算，需要另核对该行为实现及现行 A 股依据；Simple 周期核心与 seed 预设的既有复核不能代替该门禁。
