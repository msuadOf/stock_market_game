# CompanySystem / Simple core 独立复核

日期：2026-10-06。

## 结论

本次冻结快照不予验收。最新需求澄清了 Simple 的产品语义：基本面按月从上期结算百分比增减生成；账面展示不追踪真实公司资金，不设现金余额、生成额度或 `SyntheticFunding` 概念；营收和费用变化仍须推导净利润，并满足共同财务／披露／股本行为能力。真实投资者现金和股份仍由共同结算约束。当前 core 仍用 `net_margin_bp` 直接生成净利润，缺少费用输入及共同财务／股本能力，能力字段也返回不支持，因此仍属范围失配，不能验收。财务图表和账面展示由 finance owner 接续，本记录不代表该跨层功能已完成。

另有一项尚未关闭的恢复严格性 finding：新增测试覆盖了发行人及 RNG 对象中的未知字段，但当前 `CompanySpec`、`OperatingRng` serde 仍可忽略未知字段。Root 已许可对这两个共享类型增加 `deny_unknown_fields`，当前契约不保留兼容；修复后需验证精确拒绝路径。

## 检查范围

- 已通读 `AGENTS.md`、`docs/principles.md`、`agents/company-system/simple-core-contract.md`、相关 ADR-0016／0024／0025／0029、`docs/open-questions.md` 及更新后的 Q14 财务模型蓝图。
- 静态检查 `company/{api,config,identity,capabilities,persistence,system}.rs`、`company/simple/**`、`company/mod.rs` 与日历 `days_in_month` 可见性改动。
- 未运行 Cargo、测试、编译或提交；测试结果以实施者/root 提供的日志为准。

## 月度生成器复核（2026-10-06）

新冻结的 `company/simple/monthly.rs` 为纯月度营收／开支生成模块；本次只复核该生成器，不涵盖 `CompanySystem` 新 core、共同财务接线、披露或股本行为的完整验收。金额以 `AccountingAmount`（分）表示，变化率为整数 bp；独立配置收入、固定开支，变动开支在收入比例与自身金额增长之间互斥；亏损由收入与开支差额自然产生。生成过程先复制 RNG 再计算，成功才返回候选状态，计算错误不会改变调用方 RNG。零收入不会自动恢复，显式复业只重置收入输入；比例变动开支随复业后的收入计算。

**舍入 finding 已静态关闭：** `grow_amount` 改为 checked 计算 `10000 + growth`，并对目标金额一次应用基点；不再先舍入增量。新增 tie 测试覆盖 `1 分 × 1.5 → 2 分`、`3 分 × 1.5 → 4 分`，符合 half-even 规则。`growth=-10000 bp` 仍得到零金额；基点因子合成溢出显式返回错误。

**定向测试证据：** 实施者/root 提供 `.tmp/checklist-wave4/host58-monthly-green.log`，本复核者亲读确认 6 个 `monthly::tests` 全部通过、0 failed、0 ignored，包含 half-even parity 回归用例。未自行运行测试。

参数校验目前允许 `revenue_growth_bp`／`fixed_expense_growth_bp`（及 Growth 型变动费用）取 `i32::MAX`，但运行时复合 `10000 bp` 后必溢出。建议配置校验拒绝这类必然不可执行的基础值，或把比例组合提升至可表示该值的内部类型；本次按配置范围约定不清列为防御性建议，不阻断纯 monthly 的舍入 finding 关闭。动态冲击导致增长低于 −100% 属输入组合运行时错误，保留 checked 错误和整月原子性。

**新 core 恢复待查：** `prehistory_months=0` 时 `history` 可为空；当前 `SimpleFundamentals::validate` 校验 generation amounts 非负，但未见将无历史情况下当前三项金额与 `config.generation.initial_state(...)` 对照。应测试并拒绝被编辑为与显式初始配置不一致的无前史基准，或明确允许编辑该基准的契约。该点属于 core 严格恢复，不阻断纯 monthly generator 限定范围。

月度生成器限定复核通过：舍入 finding 关闭，短测证据为实施者/root 提供且已亲读。本章节不覆盖后续新增的 annualized / trend 增长规则，也不代表整个 CompanySystem / Simple 功能完成。恢复基准漂移、发行人／RNG 未知字段拒绝仍属 core 级待审项。

## 领域与实现观察

- 金额使用 `AccountingAmount`（分）和现有 checked / half-even 基点运算；Simple 未构造 `Books`，没有把权益伪装成现金，复业命令也不直接改动投资者资产。最新版允许不追踪 Simple 的公司现金余额；费用、利润及共同财务报告字段仍须由 finance owner 按契约接通。
- 公司指标按 `CompanyId` 和月度期间建模，身份复用 `CompanySpec`；Simple 未携带客户、合同或占位账簿。`Simulation` 明确返回 `Unsupported`，未偷偷回退到其他运行路径。
- 月末并行计算先收集完整结果，再提交公司状态、历史和 RNG；失败路径维持原状态。连续日期、自然月末、零收入复业、负净利润及恢复后 RNG 续行均有针对性测试。
- 信息层拥有披露排期；Simple 历史与基本面查询已收窄到 crate 内，候选不再重复承载发布日期，避免未公开月指标从公共 API 直接泄露。
- 以上观察只说明 core 已覆盖部分边界，不抵消最新版下费用推导和共同财务／股本能力缺失的验收阻断。`SyntheticFunding` 与公司现金余额不再是 Simple 的强制要求。

## 待关闭项

1. 按最新范围重定 Simple 输入：按上期金额及明确百分比变化生成营收和费用，再推导净利润；不要沿用 `net_margin_bp` 作为独立利润驱动。
2. 落实两模式共同的财务查询、披露及股本行为能力；Simple 不要求持有或生成公司现金余额，但投资者现金与股份变化须走共同规则。财务图表／账面接线由其 owner 完成，需跨层复核后才可报告整体完成。
3. 完成恢复未知嵌套字段拒绝测试，并按 Root 许可对发行人身份及持久化 RNG 增加严格字段检查；不保留旧契约兼容。
4. 对更新后的 core 与相关 finance 接线重新执行独立复核；本记录不作为最终通过签核。
