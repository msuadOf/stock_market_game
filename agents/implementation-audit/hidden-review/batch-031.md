# 隐藏扫描批次 031（owner 1）

## 范围与完整性

- 源材料来自主工作区 `/data1/baiyifan/workplace/stock_market_game`；产品 caller 基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`，复核树为 `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 连续全文读至 EOF：`completeness-2026-10-03/session/report.md` 184/184 行，SHA-256 `814399a7fa5297751f465419232539718f45358b0532cc28a3a5f096f2611f20`；`exhaustive/action-index.md` 1187/1187 行，SHA-256 `e293789ffe326c06a759a7a9f3ac0d10cf9790404abbf8c2ba4eed55ab257e34`；`exhaustive/areas/engine-company.md` 59/59 行，SHA-256 `6bf58f7b5723cfaedfe95dca2a1249f0a8d877ee0b2375a3b5ddd17e5b02691d`。三份均从首行连续读到 EOF，SHA 与指定一致。
- 章节族状态：session 报告覆盖 session-N01、session-N02、原动作保留/归属、逐文件三栏与独立复核范围；action-index 覆盖 engine foundation/pipeline/session/strategy、hosts、tooling、web 动作族；engine-company 覆盖五批文件核销、跨批 owner 归属、OOP retain/support 汇总、独立行为缺陷候选与未完成边界。session 报告是 session/strategy 范围，不能用作 company 代码的证据；company 判断以下列区域文档和当前 caller 复核为准。
- 已阅读复核树 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`，及公司相关 ADR-0016、0019、0024、0026、0027 与 2026-10-02 实现总账、`reaudit-engine.md`。历史审计的实施建议仅作历史材料；更晚的产品承诺/总账优先。未重新联网核验会计或 A 股法规。

## 当前 owner 与 caller

- **公司 aggregate 保留结论成立。** 当前 `IndustrialBooks` 在 `packages/engine/src/company/industrial/mod.rs:55-72,74-119` 组合 `Books`、存货/固定资产/应收应付/合同/交易对手/预算/税务/贷款状态，构造中校验政策、过账开局凭证、对账 seed 后才返回；四类 Books 和 `CompanyOperations`/`OperatingCompany` 的权威边界没有证据需要另造通用 manager。文件拆分后的 `impl` 仍是同一领域 owner。
- **运行时经营 owner 已落到 `OperatingDayRun`。** `company/operations/day.rs:79-85` 依次过期 shock、抽样激活、派发 due、运行行业 flow、排下期利息；`CompanyOperationsClockWiring::run_day_end` 是会话日终 caller（现行调用链见 `agents/implementation-audit/reaudit-engine.md:57-59`）。因此结构 owner 存在，但不能据此核销经营步骤缺失。
- **工业期初 seed 候选仍在。** `industrial/config.rs:97-124` 只遍历 `seeded` 中已有账户键核对总账；非零 1403/1405 余额而完全没有对应 seed 时，该科目不参与比较。`IndustrialBooks::new` 在局部 `Books` 过账后调用 reconciliation，构造失败不会返回半成账套（`industrial/mod.rs:78-119`）；既有 guards 测试覆盖有 seed 金额不符，未覆盖无 seed 的非零总账。仍是单独的 opening validation 候选，不是 OOP 修复项或已证实的现实准则缺口。
- **经营 scheduler 两项历史候选仍在。** `company/scheduler.rs:123-158` 在插入前以普通 `next_seq += 1` 增长；`from_parts`/自定义 Deserialize 在 `:179-238` 验证排序、key、序号范围和 settled floor，但未拒绝不同待办项复用 `ScheduledDueId`。caller 是 `CompanyOperations::submit_due`/`dispatch_due_on`，并由 session clock wiring 镜像 ID。它是公司自然日队列，不是 A 股订单簿或撮合优先级。
- **公司 shock 的适用范围仍有缺口。** 行业 shock 在 `operations/day.rs:115-133` 按 `CompanySpec.industry` 过滤；个体 shock 在 `:135-145` 调用不接收行业的 `sample_company_shock` 并直接激活。事件定义限定适用工业/地产，但银行/保险仍可能持有并由披露层按 active 状态发布。此为当前 G36 的披露适用边界，不能从调度/聚合 owner 提取核销。
- **股本与账户语义：** `CompanyRegistry`/`CompanySpec` 保持发行人映射和已发行普通股数校验；`issued_shares` 不应改述为流通股。公司经营现金与投资者现金分离，现行决策不执行分红、增发、回购、股东清算分配，不据 OOP 分组创造投资者现金流。

## 总账与候选状态

- 既有公司审计材料没有 refactor action 的结论可保留：88 项 `retain`、18 项 `support`、106 项总计。action-index 中公司以外的候选（特别 session-N01/N02）不能映射为公司动作。
- 相关产品缺口按现行实现总账仍未闭合：G28 固定集团报告/合并公开链；G35 工商折旧、所得税及跨行业商业债务支付闭环；G36 四行业会话装配、封账/披露以及不适用 shock。公司方面还应区分 G41 经营失败记录被日终 caller 丢弃、G58 多贷款人授信核算、G59 保险保障期后新赔案等较新的具体缺口。OOP owner 抽取或模块测试不能核销这些生产 caller 缺口。
- Q23 是实现审计内部 Q 编号（与 `docs/open-questions.md` 的产品 Q 编号不同）：年度所得税重复直接调用的幂等/准入及 G35 计提时机边界仍待明确。不得把它与产品 open questions 中的 Q23 混为同一命名空间。
- 保留历史独立候选：工业漏 seed 检查；`ContractGroupState` 应用阶段的局部失败原子性未经证明；scheduler 重复 ID 恢复和序号耗尽显式错误。它们没有与现有 G/Q 项的一一直接映射，本轮不创建、关闭或改号 G/Q。重复 tuple 的上层输入去重及保险 `checked_*` 失败可达性仍需另外追踪。
- 大 A 语义判断仅限边界检查：上述经营和会计事实不等同沪深撮合/清算规则；未看到本次 OOP 保留结论改变价格、申报单位、现金/股份单位、撮合顺序或 T+1。未重新查官方来源，不宣称法规依据已重认证。

## 结论与边界

旧的 company OOP disposition 与当前对象/调用链相符，没有从类/模块名称或测试存在推断跨步骤事务原子性。新增候选仍是行为/校验边界，不能混进等价重构。只完成静态源文件、当前 owner/caller、G/Q 和 ADR 对照；未运行测试、构建或回归，未改产品文件、正式文档或 Git。
