# ADR-0040：新局默认扣税与三层税务模式

- **状态：** accepted（用户 2026-10-06 批准、2026-10-08 按原话升级为三层结构并批准；实现归后续批次，不以本 ADR 宣称代码完成）
- **日期：** 2026-10-06 初记；2026-10-08 三层修订。
- **决策者：** 用户拍板；AI 记录。
- **关联：** [ADR-0035](0035-company-system-simple-fundamentals.md)；[trading-rules.md](../trading-rules.md)「开局税务模式与默认税籍」节（三层语义的完整登记口径）；未定项见[当前交接](../../agents/company-system/current-handoff.md)。

## 背景

个人流通股股息红利差别化个人所得税已按官方口径接通：`DividendTaxProfile::IndividualPublicMarket` 可在 Session 装配期显式配置，税账按 20%／10%／0% 三档、FIFO 持股期限与转让时扣收运行，资金不足部分收缴并追缴（[trading-rules.md](../trading-rules.md)）。用户先后两次拍板税务模式的产品形态：

- 2026-10-06 原文：「新局默认用户交易都扣税，还可以选择用大A的方式扣税（个人扣税、机构/企业另外算，这种就是company/simulation那种复杂模拟的机制了，企业部分先留下抽象层接口，可以先接通simple模型，按简单的方式来，但是要留好后续复杂开发的空间），还可以配置为不扣税。此外，分红要扣税」
- 2026-10-08 三层修订要点（原文摘录）：「简税默认+大A可选（按原话三层）」「简税比例 10%（推荐）——分红到账时直接扣，无持股期档位；开局参数可编辑，机构不另算」「不扣税：连印花税也免（佣金/过户费照付）」。

## 决策

新局税务模式为**三层结构**（`SessionSetup.dividend_tax_mode`，开局选定、同局不切换）：

1. **简税（`FlatWithholding`，默认）**：分红付款日对名册每位**账户**持有人按开局比例
   （`SessionSetup.flat_withholding_bp`，默认 1000bp = 10%，可编辑，合法域 0..=10000bp）
   对税前应得直接代扣；无持股期档位、无税账 FIFO、机构持有人同样代扣（机构不另算），
   卖出时不产生任何补税。**不创建、不持久化任何 `CashDividendTaxBook`**；代扣事实以
   `flat_withholding_receipts` 回执严格持久化。交易环节税费（印花税等）不变。
2. **大 A 方式（`AShareIndividual`）**：现行个人公开市场差别化口径的全部行为——装配期
   配置股东名册时自动为每个「个人」身份账户（玩家与自然人散户 NPC）开
   `IndividualPublicMarket` 税账，按三档税率、FIFO 持股期限、转让时扣收与部分收缴
   追缴运行；机构/游资等非个人身份保持 `TreatmentNotConfigured`（企业/机构计税未实现，
   `TaxpayerIdentity::NonIndividualPending` 为显式扩展位）。交易环节税费不变。
3. **不扣税（`Exempt`）**：不产生个人股息税事实；**交易环节卖出印花税同时免征**
   （佣金与过户费照付）。装配期不为任何身份自动配置税账；宿主显式装配期命令
   `configure_dividend_tax_book` 仍是既有入口（与简税模式不同：简税模式显式拒绝该
   命令，避免对同一笔分红双重计税）。

配套契约：

- **严格持久化、无兼容**（用户 2026-10-07 无兼容原则）：模式与比例均为新档必填字段、
  无 serde 默认；旧两变体枚举值（`IndividualPublicMarket`）已整体删除，携带旧值的档在
  engine 与 Web 两端显式拒绝，不静默映射。简税比例是三态契约：仅 `FlatWithholding`
  必填、其他模式显式拒绝携带（engine `SessionSetup::validate` 与 Web `parseSetup` 同构）。
- **印花税按模式门禁**：`FlatWithholding`／`AShareIndividual` 要求
  `stamp_tax_rate == 0.0005`（现行 A 股基线），`Exempt` 要求 `stamp_tax_rate == 0`。
  配对在 setup 校验的单一入口强制；费用管线（结算、名义费用、NPC 估算与恢复重放）
  全部经 `GameConfig` 传播，不存在绕过配对的第二条计费路径。
- 税务模式属新局配置，同局不切换；恢复不改语义（沿用 ADR-0035 严格存档、无 schema
  代际／迁移策略）。既有装配期限制（名册已有历史日结回执或已登记分红后不得再配置
  税账）继续有效。
- 2026-10-06 决策中「大 A 方式下机构／企业另外算」的复杂模拟机制维持不实现：机构／
  企业税务身份（`ResidentEnterprise`／`SecuritiesFund`／`NonResident`）保持显式不支持，
  留抽象层接口供后续 company/simulation 分支。

## 备选方案

- 维持两变体（默认大 A 差别化 + 不扣税）：拒绝。用户 2026-10-08 明确简税默认的三层结构。
- 简税在 AShare 税账上以固定档实现（复用 FIFO 管线）：拒绝。简税的产品语义就是
  「无持股期、无税账」的游戏化简化；复用管线会保留无意义的账面事实与存档膨胀。
- 不扣税模式仅免个人股息税、保留印花税（2026-10-06 登记口径）：拒绝。用户 2026-10-08
  原话「不扣税：连印花税也免（佣金/过户费照付）」。
- 简税模式下保留显式配置个人差别化税账的装配期入口：拒绝。简税代扣已覆盖全部持有人，
  叠加会对同一笔分红双重计税；引擎与 Web 均显式拒绝。

## 后果

- **正面：** 新局默认即确定性税籍且规则直白（到账即扣、无追缴视图负担）；三层语义各自
  内洽，印花税门禁有单一权威入口；严格契约杜绝旧档静默兼容路径。
- **负面：** 存档契约、fixtures、UI 与文档均须承载模式与比例；简税模式的外部具名持有人
  无游戏账户、不代扣（与既有边界一致，登记为引擎边界而非税法口径）。
- **后续批次：** N2（自动名册）等批次会继续触碰 setup／税区；机构／企业差别计税仍为
  显式不支持的抽象层留白。[ADR-0039](0039-rights-placement-split-scope-and-toggle.md)
  的配股／增发启用开关共用同一新局选项面。

## 关联

- [ADR-0035](0035-company-system-simple-fundamentals.md)：严格存档与装配边界。
- [ADR-0039](0039-rights-placement-split-scope-and-toggle.md)：共用新局选项面。
- [trading-rules.md](../trading-rules.md)：三层语义、个人股息税完整登记口径与印花税门禁。
- [三层税制台账](../../agents/company-system/three-layer-tax.md)。
- [当前交接](../../agents/company-system/current-handoff.md)。
