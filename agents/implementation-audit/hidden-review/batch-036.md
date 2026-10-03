# 隐藏扫描批次 036

## 来源与读取完整性

按 owner=1 完成指定三篇来源的连续全文阅读至 EOF。每篇各自前后核验 SHA-256，观察行数、摘要均匹配分配值；没有来源文件内容漂移。

| 来源 | 行数 / SHA-256 | 全文章节状态 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/contracts-01.md` | 296 / `69a9c4895c92213151f45f875cafee92d41a587c110947d4a8dfae61aa7ee44a` | 结论、分层与协议、双生成目录差异、迁移方向、256项逐文件覆盖、调用方/测试/限制均读至 EOF。表内全部声明均 retain；没有候选类迁移。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-company-01.md` | 261 / `11c7501d3f8032a794969fac834d4f16934c1a2c1fe417757a7e31f14ff82321` | 会计语义与金额边界、24个文件对象归属、缺陷/失败窗口、迁移顺序和限制均读至 EOF。现有值对象、账簿、报告流程是主要 owner；没有新对象迁移。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-company-02.md` | 557 / `4577c6145924e8b85e093da572220df3d4ba1d6bd2fe81996f8a93b97989c773` | 报表、银行、经营合同及对手方25个源文件、D01/D02、跨文件关系、读取依据和限制均读至 EOF。没有新状态对象/方法迁移；存在局部后置失败与累积器部分写边界。 |

本次按当前调用审计而非复述历史候选：对照工作区基线 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad` 的实现记录 `agents/implementation-audit/implementation-audit-2026-10-02.md`、`reaudit-engine.md`、已有实现复查以及相关现行 ADR。根工作区 HEAD 为该产品基线；只读了产品代码和审计材料，未写产品文件、未运行测试/构建、未执行 Git 操作。

## 当前实现和旧结论核对

- **DTO 保留结论仍成立。** `contracts-01` 所述 Web `WasmApi` 是 handle/API 契约，generated 声明是 JSON/serde 投影；源文件并不拥有行为或资源生命周期。当前生成由 `package.json` 的 `types:generate` / ts-rs 负责，Web 目录由一致性检查覆盖；历史根 `bindings/` 被忽略且同名类型有 number/bigint、字段及可空性差异，不能当成已经验证等价的生成物。未发现可通过把 DTO 类化解决的对象遗漏。Money=分、委托/盘口量=股、StockExchange 与 SecurityCategory 独立、公开财务十进制字符串与交易 Money 的单位区分没有被这些候选混淆；不把类型存在视为运行时校验。
- **公司账务 owner 与调用边界仍相符。** `Books` 持有权威 Journal 与派生 Ledger；`InventoryLedger`、`TradeOpenLedger`、`ClosingEngine` 和 `BankBooks` 各有既有领域状态/生命周期。现行 `GameSession::close_accounting_periods` 对行业 Books 调 `ClosingEngine::close_year/close_month`（`packages/engine/src/session.rs:2181-2235`）。这证明会话确有封账调用，但不证明跨 Books/ClosingEngine 的整段失败原子性。Q17 对 `close_year/correct` 后置步骤失败的待定范围仍有效。
- **G35 仍开放，不能由现有 handler/OOP owner 核销。** `OperatingDayRun` 的现行阶段调用、工商日处理及计息分派见 `agents/implementation-audit/reaudit-engine.md:57-60`；会话日终确实调用日经营，但工商折旧、所得税与商业债务支付仍未进入经营闭环。`accrue_income_tax`/`pay_income_tax` 的存在只证明库能力，不是调度生产 caller；Q23 也要求明确年度税重复调用与时点，不可默认为幂等。
- **G36 仍开放，四行业算法不等于四行业会话闭环。** 当前 listed company 装配仍固定 `CompanyKind::Industrial`，生产证据见 `session/company_assembly.rs:331-345`；新公司冲击抽样不接收行业类型且适用范围是局部行业，运营日对注册公司投递并可形成公告，见 `company/events.rs:12-20,209-235`、`company/operations/day.rs:135-145`、`session/disclosures.rs:107-124`。银行、保险、地产 Books 与 report DTO 本身不能核销四行业自定义会话/存档/披露缺口。ADR-0016 的批准计划契约固定四行业交付；ADR-0024 只解决投资者现金池不循环，不扩大或替代经营模块。
- **已知财务缺陷是先前边界，不因抽取而关闭。** `InventoryLedger::receipt` 在 cost checked-add 之前写 quantity（`accounting/inventory.rs:85-99`），工业采购 caller 在 receipt 前已 `post_with_commit`（`company/industrial/purchasing.rs:103-115`）。`TradeOpenLedger::write_off` 先清零开项再 checked-add 累计额（`accounting/receivables.rs:130-147`），`IndustrialBooks::write_off_receivable` 在调用它前已过账（`company/industrial/sales.rs:214-245`）。这两处边界和其极值可达性已由历史复审单列；resolution/调用者没有把它们提升成 OOP 漏项或普遍端到端事务承诺。本批维持“不静默吞错、单批 Books 原子性不外推”的既有限定，不新造重复 G。
- **G/Q 对照。** 对应的当前总账条目是 G35、G36、G58、G59，及待决边界 Q17、Q19、Q23；具体文本见 `implementation-audit-2026-10-02.md:114-120,155-161`。G58 为 Industrial 授信按全部 loans 汇总后与单 lender limit 比较，G59 为保险保障期结束后仍排新赔案；二者独立于本批对象保留结论。Q19 所记 `BankBooks` 独立 serde ECL policy 校验及当前公共会话恢复边界仍成立：`EclPolicy::validate` 目前只核两张情景表（`company/bank/ecl.rs:87-93`），`BankBooks::new` 会验证（`company/bank/mod.rs:107-130`），但直接 serde restore 没有加同一验证；该条件库级面不能泛化成公共日终档已损坏，也不能借 G36 抹掉验证器要求。所有其它 G01–G68/Q 均按总账维持各自状态，本批不改号、不核销。

## 历史候选的新增性反证

- `engine-company-01` 的合并余额零/负值、库存/应收子账局部写入、年结/更正多步骤失败窗不是新发现；其限制、正数契约及验证需求已经在模块与后续审计登记。结账窗口仍由 Q17 精确表达“未确认整体失败零变更契约及默认调用可达性”，不会从局部 Books 原子性推出相反结论。
- `engine-company-02` 的 ECL `version=0` 候选仍没有本批可依赖的 accepted 规则声明版本最低为 1；`version` 字段存在与典型从1编号不足以单独证明规范缺陷。保留为旧候选/需定义语义，不升格新 G。`build_notes` 的缺失分类回退属于局部内部一致性边界；报告描述标准 `generate_report_set` 校验路径不可达，不能声称当前常规发布会错误归类，也没有新 caller 反证其可达。
- Window `Accumulator` 溢出后保留部分桶、所得税重复直调不具幂等性、银行利息 handler 的 post 后步骤以及 Income tax line scalar/列报行差异，均在来源中明确限定为局部观察或调用方待核范围。未见现行总账/已接受 ADR 将这些扩展成“必须所有底层 operation 全事务”或改变列报/税务语义的要求。
- `contracts-01` 中两个 TS 生成树差异是实际元数据/生成边界警示，但这三篇没有给出所有历史根 bindings 的可复现生成来源，且也没有证据表明它是 approved implementation omission；不手工统一类型，也不增加候选类。

## 结论

三份材料都支持现有对象分层/保留，而不是 OOP 抽取遗漏。现存公司经营、四行业会话、独立银行存档、后置失败与生成物差异，分别继续由总账的 G/Q 或明确的历史局部边界承担。未发现本批可确认的新 G、错误旧核销，或足以推翻既有对象归属的生产 caller 证据。涉及现金、股数、T+1、证券分类或交易规则的行为未改；本轮不重新认证交易所/会计法源，也不将静态核对称为测试验收。
