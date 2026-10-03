# 批次 176：Engine Company 02/03 历史审计复核

## 阅读范围与方法

已从主工作区连续阅读全文至 EOF：`engine-company-02-projection-first-recheck.md`（25 行）、`engine-company-02.md`（25 行）、`engine-company-03.md`（70 行）。行数与 SHA-256 见同目录 JSON。三份材料均已读到末行，未把搜索片段代替全文阅读。

同步阅读 worktree `AGENTS.md`、`docs/principles.md`，并复核 43b1aa5 之后的 `agents/implementation-audit/implementation-audit-2026-10-02.md`、`reaudit-engine.md`及相关现行代码。43b1aa5 的产品调用者复核将 G28、G35、G36 的实际会话生产入口与历史 OOP 评估区分开；G01–G68/Q 中与本批相涉的公司经营/披露/复核问题按较新记录和 ADR-0019、0024、0026 校正，不执行旧材料中的迁移或修复指令。此项是静态源码审阅，不运行测试/构建，也不重新取证 A 股官方交易规则。

## 章节族与现行对照

- **engine-company-02：报表窗口、财报 DTO 与纯投影。** 旧 `engine-company-02.md` 将 `StatementWindows` 作为窗口状态所有者，保留按窗口构造和报表生成函数；Income/Balance/Equity 等公开产物是序列化 DTO。当前对应 `packages/engine/src/accounting/reports/window.rs:27-50,62-86`、`income.rs:125-175,239-263`，`StatementWindows` 字段确实 `pub` 且可变，但结构是 `pub(crate)`；`IncomeColumns` / `IncomeStatement` 仍是公开字段 DTO。closing 的生产调用从 `packages/engine/src/accounting/closing/mod.rs:380-399` 进入 `generate_report_set`。不应将“派生投影”误读成 Rust 类型提供不可变保证。
- **projection-first-recheck 的措辞纠错：再证。** 先前指出第 268 行把“类型保证的不可变投影”改成“类型保证的派生投影”会混淆派生与不可变性，随后 `engine-company-02.md` 已只保留首段“派生投影”的替词，并准确保留“不能称为类型保证的不可变投影”。这个唯一 delta 与当前可写字段代码一致；旧复核结论通过。该审计对象是文案，不是源行为。
- **engine-company-03：company aggregates、DTO、操作用例与候选缺陷。** 旧报告关于 `IndustrialBooks` / `InsuranceBooks` 持有各自账套与子账、处理器按用例拆分、报表/转换函数保持纯函数的对象归属总体仍成立。当前例如 `IndustrialBooks` 仍在 `packages/engine/src/company/industrial/mod.rs:55-78,121-160` 聚合账本和配置，`ContractGroupState` 与 `ContractMeasurementState` 在 `insurance/groups.rs:24-63` 聚合合同组状态；保险释放/重估依旧由 `preview_*` 与 `apply_*` 分开，详见 `groups.rs:243-385,388-421`。提取对象本身不等于解决行为缺陷。
- **旧 D01 缺陷：再证。** `engine-company-03.md` 记录 `seed_inventory` 只核对种子 map 已出现的科目，建议单独处理“非零 1403/1405 总账余额但缺对应种子”的构造接受问题。现行实现 `packages/engine/src/company/industrial/config.rs:97-123` 仍仅遍历 `seeded` 键；空缺种子的支持科目不会进入余额核对。因此这是仍在的独立构造校验候选，不是 OOP 工作，也不能描述为已修复。尚需单独确认该输入的产品契约、补最小失败边界并独立审查。
- **旧 unit-064 风险：再证。** `ContractMeasurementState::apply_release` 在多次可失败金额计算之后逐字段修改，且 `units_released += batch.units` 为普通整数运算（`groups.rs:388-410`）；`apply_remeasure` 先写余额字段后继续 checked 累计（`groups.rs:413-421`）。前置 `preview_*` 降低常规输入触发失败的概率，但不能据此宣称 apply 对所有输入原子。风险边界与旧报告一致，应作为行为修复候选，不应为了 OOP 迁移顺手改。
- **旧 unit-071 / unit-078 风险：再证。** Company 注册表验证及公开访问仍见 `packages/engine/src/company/mod.rs:85-110,222-245,267-269`；审查材料所述 issuer mapping “仅以首个同代码项校验股数”的局部风险需按该函数继续独立评估。`IndustrialBooks::available_credit` 仍以 `Option` 暴露结果，并在 `industrial/mod.rs:178-182` 对贷款余额/减法使用 `.ok()?` / `.ok()`，计算错误与无授信可能合并；该行为问题仍不属于对象提取。
- **更高层 caller 与决策交叉：** G35 当前公司日推进仍为 shock、due、行业 flow、排次日利息（`company/operations/day.rs:79-85`），不能把经营闭环缺口归因于 OOP。G36 的 `SessionSetup` 仍在 `session/company_assembly.rs:331-346` 将上市公司装配为 Industrial；单公司 shock 在 `company/operations/day.rs:135-145` 按公司逐一抽样，尽管事件目录标明生产中断只适用于工业/地产、减值只适用于工业（`company/events.rs:12-20,209-237`）。较新 re-audit 将此作为行业适用/生产披露边界，且说明四行业局部内核已存在；故应保留限定，不夸大为错误交易或默认必然触发。ADR-0024 明确公司经营现金与投资者资金池分离；ADR-0026 属个体策略而非交易规则；ADR-0019 移除机器容量配额但保留真实交易约束。上述决策均不授权改变本批会计或证券语义。

## 结论

1. **历史 OOP 结论：** 多数“已有聚合、DTO/纯投影不额外包装”的归属可由现行结构再证；模块 02 的公开 DTO 可写、内部投影非类型级不可变，文案需维持这一区别。
2. **旧结论校正：** `seed_inventory` 遗漏无种子非零余额的候选仍在；保险组 apply 部分写入/普通计数、issuer mapping 局部校验、`available_credit` 错误折叠等源码风险仍是独立行为边界。它们不是已批准的 OOP 修复，也不应把审计材料中的建议当成实施命令。
3. **需求与领域语义：** 本次只是复核历史审计文件；无交易语义或产品改动。未发现可据此宣称现行沪深交易规则已重新验证的证据，也不需要为了这些 OOP 判断引入新的交易规则。
4. **测试/调用边界：** 本次没有行为改动，不新增测试；现有审计发现若进入后续 bug-fix，应按其具体输入边界补测试并独立审查。没有运行测试或构建。

复核仅完成历史结论对照，未修改产品文件或执行旧 agent 指令。
