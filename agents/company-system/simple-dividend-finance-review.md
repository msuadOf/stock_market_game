# Simple 分红会计集成独立复核

## 依据与范围

未参与本批实现。已阅读仓库 `AGENTS.md`、`docs/principles.md`、Q14 公司财务模型设计、ADR-0035、`docs/company-actions-design.md`、`docs/trading-rules.md`、`agents/remaining-questions-and-features/dividend-research.md`、既有 `cash-dividend-book-review.md`，以及本批 Simple Finance、账簿报表、CompanySystem、Session 公司行为与 Web 存档 schema 的当前实现。

公司法依据采用已核官方原文：《中华人民共和国公司法》（2023年修订，2024-07-01施行）第210条要求先弥补亏损，再按税后利润10%提取法定公积金，累计达到注册资本50%以上可以不再提取；股份有限公司按持股比例分配（章程另有规定除外），公司持有本公司股份不参与分配。第212条规定股东会作出分配利润决议后六个月内完成分配。来源、范围与取证日期见 `agents/remaining-questions-and-features/dividend-research.md`。这些条款不代替中国结算未取得的实施指南，也不能据此声称个人／机构红利税已经完成。

Q14 与 ADR-0035 确认 Simple 公司现金仅为账面展示、不作为分红预算；实际投资者到账仍要求明确方案、幂等及原子性，账面分红须与权益、应付款及报表一致。Simple 与 Simulation 共用上层功能语义，内部账面简化不豁免适用的公司行为条件。

## Findings

- **静态修复已出现，动态闭环待验：跨年弥亏后的法定公积计提基数。** 首轮核对发现 `distributable_profit` 以单独的完整年度利润直接乘10%，没有把以前年度累计亏损先从当年利润中弥补。例如前年度亏损500、当年税后利润1000时，应先补亏后以500为提取基数计50；直接以1000计则为100。Finance 实施者随后加入以前年度留存损益亏损抵减，并增加“期初亏损抵减当年税后利润”用例。须由 root 的 fresh 日志确认新增测试红绿闭环，并核对本次字段重构没有留下编译／恢复问题，之后才能关闭。
- **静态修复已出现，fresh 测试待核：同年度后续分红。** 当注册资本尚未达到50%但同年度累计已提足目标公积时，后续计划的新增公积为零；实现一度仍携带 `reserve_basis_year=Some(year)`，与恢复校验冲突。当前代码在 `remaining` 为正时才保存 basis year，否则保存零与 `None`；同年第二计划用例也已添加。等待 root fresh 测试日志确认闭环。
- **静态修复已出现，测试待 root 验证：所有者权益变动表分配列。** 首轮核对发现单体 `EquityStatement` 将 `distributions` 固定为零，并以全部权益科目运动推导 `capital_contributions`。Finance 实施者随后在 `StatementWindows` 按 `CompanyDividendDeclaration` 归集权益净减少额，单体报表将它列入 `distributions` 并抵销旧的负 `capital_contributions`。该净额同时把留存收益借方与法定公积贷方抵销，符合本批分录形状。新增 Finance 用例在付款后用 `CivilDate::from_ymd(2031, 2, 1).prev()` 结算至2031-01-31，再验证一月报表分配行、投入行与交叉勾稽；表达式最初被误读为二月末，现已确认 fixture 正确。等待 root fresh 测试日志。
- **静态修复已出现，Notes 勾稽测试待验：资产负债表权益明细。** 首轮发现 `simple_statutory_reserve` 被分配到 `BsLine::RetainedEarnings`（“未分配利润”）；实施者随后新增 `BsLine::StatutoryReserve`、法定公积金标签和独立 assignment，并已将此行从 `is_derived()` 移除，使 `ReportSet::validate` 应正常执行 notes cross-foot。最新 Finance 用例断言报表分别列示 statutory reserve 与 retained earnings并调用 `validate()`；仍待 root fresh 短测日志。
- **静态接线已修复，source evidence 仍属显式输入事实：** 当前 API 在声明分红前要求独立调用 `define_dividend_legal_facts`，绑定正注册资本及非空 `source_evidence`；方案不能自行首次确立资本额，后续冲突也会拒绝。GameSession 已新增对应显式入口，不从股票股数或股价推断。代码能校验存在和稳定性，不能证明自由文本证据真实；来源真实性属于调用者提供的事实，不可将该字段校验夸大为外部官方证明验证。
- **Web 字段恢复接线静态闭合，最终短测日志待收：** Finance 持久态当前字段为 required-nullable `legal_facts`（含注册资本及来源证据）；Web `parseSimpleFinanceState` 已跟随其最终字段，严格解析分红状态和每笔声明／支付凭证的 Journal 来源绑定，并交叉核对应付股利及公积科目余额。Web schema owner 报告定向5/5通过；root 尚需核对真实日志及整体 Session schema 路径。
- **待修：Session 公司账与 Finance 付款事实恢复时未跨账勾稽。** `SessionCorporateActions::validate` 将 CashDividendBook 成功 outcomes 与 account/external gross receipts 互核；Finance state 单独将付款批次金额与 Journal 互核，但当前二者之间没有按 `(plan_id, payment_id, paid_on)` 校验成功账户／外部 gross 合计等于 Simple Finance 付款金额。编辑后的存档可让两个子域各自内部一致而彼此金额漂移。已通知 root 增加只读付款事实查询或跨域恢复校验，并按成功收款总额核对。
- **税务标记已有静态实现，动态恢复待验：** 当前账户和外部收款 gross receipt 均带 `TreatmentNotConfigured`，存档校验要求该标记与对应成功付款一致。这正确地区分总额到账与税务结清；不把未配置身份解读成禁止总额派发。已读财税〔2015〕101号：上市公司公开市场个人持股期限影响计税，持有不超过一年者派息暂不扣缴、转让时扣收；机构口径也不能套用个人税率。待 root fresh 验证其存档往返与 Session 重试路径。
- **阻断：账户现金并未实际贷记。** 独立重读 `Account::credit_cash`，其当前只验证金额为正随后直接返回 `Ok(())`，没有增加账户现金；新增 shadow 测试期望 1000→1250，和实现矛盾。Session 将此 `Ok` 记录为 Paid、后续不再重试，公司账簿又会结清应付。该问题已独立见于 `agents/company-system/session-corporate-actions-review.md`，并已报告 root；实现修复前不能将实际到账/公司分红联动验收为通过。
- **年度公积结账非本批自动强制项。** 初审曾将“完整年度盈利但未分配时是否自动提取法定公积”列为实现缺口。经复核，公司法第210条原文是“公司分配当年税后利润时，应当提取……”，不可把它外推为每个有利润年度无分配也必须自动转提；项目设计也明确利润留在未分配利润且不自动分配。故撤回将“无股东分配仍须自动年末提取”作为法定缺陷。本批应在发生分配时先校验并提取对应公积；若将来需要年度结账额外列报，须另核 CAS 及正式利润分配决议语义。

## 验证与结论边界

本 reviewer 未修改实现、未运行测试或复杂回归。等待 root 提供受单条及整命令进程树10秒 deadline 约束的定向测试真实日志后，再核对相关 finding 是否闭合。独立 Simple 账簿短测变绿不等于 Session 完整验收；尤其不代表税务结清、全部公司行为完成、中国结算尾差／操作规则已核实，或整个 Session 回归已通过。

## 最新跨域复核补记（2026-10-06）

按最新 Finance、CompanySystem、Session 恢复校验完整 diff 复核。root 报告 `fresh-4.log` finance 4/4、`fresh-6.log` session 2 项、`fresh-7.log` repeated 除息 1 项通过；本 reviewer 未独立打开这些日志或重跑测试，因此动态验证结论以 root 提供为准。

- **跨域付款金额勾稽已静态闭合。** `CompanySystem::validate_cash_dividend_books` 按 `(company, plan_id, payment_id)` 核对 Simple 付款事实与 `CashDividendBook` 付款回执，要求 `paid_on` 一致、成功 `Paid` outcome 合计金额等于 Finance 付款额，并拒绝无回执的 Simple 付款批次或无 Simple 声明的现金分红账簿。Session 恢复 validator 已调用该校验；公告／未完成付款阶段则不伪造 Finance 实付款。方案总额同时受现金计划 `distributable_amount` 上限约束，已登记方案再与名册资格总额相核。
- **required-nullable 法律事实与付款分录绑定已静态闭合。** Finance 的 `legal_facts` 持久字段必需但允许 `null`；有分红方案时必须提供正注册资本与非空来源说明，且与公司绑定事实一致。付款状态按 Journal source、kind、日期、现金流分类及两条借贷行逐项恢复校验。正数 reserve 必须有 `reserve_basis_year`，零 reserve 的生产路径保存 `None`，同年追加分红用例已由 root 报告通过。
- **剩余边界发现：恢复态允许付款日期晚于会话当前日。** `CashDividendBook::validate` 只保证付款日期不早于应付日；`SimpleFinanceState::validate` 只限制付款日期不早于批准日；跨域 CompanySystem validator 会比较账簿和 Finance 的日期相等，但不接收当前日。故伪造存档可在两侧一致地写入未来付款日期。建议在 `SessionCorporateActions::validate(current_date)` 对每个已记录付款日期要求不晚于 `current_date`，并让 Web parser 与 Rust 契约一致；若这是有意的未来付款预约，应另行区分“已付款事实”和“预约”。
- **已修复：零 reserve 与依据年度必须同时存在或同时为空。** 最新 Finance Rust validator 与 Web parser 均要求 `reserve.is_positive() == reserve_basis_year.is_some()`；Rust 恢复负例和 Web parser 的 `reserve: "0.00"` + 非空依据年度负例均覆盖了伪造状态。静态闭环成立；动态结果须以对应 fresh 日志为准。

未来付款日期上界仍是存档防御性边界，不改变 A 股分红金额、比例或交易所制度解释；本独立复核仅对付款逐批勾稽及 reserve 年度字段互斥约束确认静态闭合，不宣称恢复态所有时间字段组合已严格封闭。
