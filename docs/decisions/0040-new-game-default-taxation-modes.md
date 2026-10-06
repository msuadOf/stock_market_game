# ADR-0040：新局默认扣税与税务模式选项

- **状态：** accepted（用户 2026-10-06 批准产品决策；实现归后续批次，不以本 ADR 宣称代码完成）
- **日期：** 2026-10-06。
- **决策者：** 用户拍板；AI 记录。
- **关联：** [ADR-0035](0035-company-system-simple-fundamentals.md)；[trading-rules.md](../trading-rules.md) 个人股息税登记口径；未定项见[当前交接](../../agents/company-system/current-handoff.md)「默认开局名册与税务身份未定」。

## 背景

个人流通股股息红利差别化个人所得税已按官方口径接通：`DividendTaxProfile::IndividualPublicMarket` 可在 Session 装配期显式配置，税账按 20%／10%／0% 三档、FIFO 持股期限与转让时扣收运行，资金不足部分收缴并追缴（[trading-rules.md](../trading-rules.md)）。但默认开局名册与税务身份未定：宿主／UI 配置入口仍缺，企业、基金、非居民身份显式不支持，未配置身份仍是 `TreatmentNotConfigured`，即新局默认不产生个人税事实。

用户 2026-10-06 拍板（原文）：「新局默认用户交易都扣税，还可以选择用大A的方式扣税（个人扣税、机构/企业另外算，这种就是company/simulation那种复杂模拟的机制了，企业部分先留下抽象层接口，可以先接通simple模型，按简单的方式来，但是要留好后续复杂开发的空间），还可以配置为不扣税。此外，分红要扣税」

## 决策

1. **新局默认扣税**：新局默认对所有个人身份账户（玩家 + 个人 NPC）按个人公开市场差别化股息税计税，即默认配置 `IndividualPublicMarket`，不再默认落在 `TreatmentNotConfigured`。分红默认扣税；计税语义本身（三档税率、FIFO、转让时扣收、部分收缴与追缴）不变，仍按 [trading-rules.md](../trading-rules.md) 登记口径执行，本决策只定默认值与模式选项。
2. **新局税务模式选项**，创建游戏时可选择：
   - **默认（扣税，`Simple` 简单方式）**：个人身份账户按 `IndividualPublicMarket` 计税；交易环节现行税费不变；
   - **大 A 方式（个人扣税、机构／企业另外算）**：属 company/simulation 复杂模拟机制，本轮不实现。机构／企业税务身份（`ResidentEnterprise`／`SecuritiesFund`／`NonResident`）保持显式不支持，先留抽象层接口，可以先接通 `Simple` 模型按简单方式来，留好后续复杂开发的空间；
   - **不扣税**：可配置为不计税的新局模式。
3. 税务模式属新局配置，同局不切换；**严格持久化、恢复不改语义**（沿用 ADR-0035 严格存档、无 schema 代际／迁移策略）。既有装配期限制（名册已有历史日结回执或已登记分红后不得再配置税账）继续有效，默认配置在新局装配期一次完成。
4. 「不扣税」模式是否同时豁免交易环节税费（印花税等），实施批次按用户原意核对后另行明确并登记，不擅自扩大或缩小。

## 备选方案

- 维持现状（默认 `TreatmentNotConfigured`，显式配置后才计税）：拒绝。用户要求新局默认扣税。
- 本轮一并实现机构／企业差别计税：拒绝。用户明确该部分先留抽象层接口，属后续 company/simulation 复杂机制。

## 后果

- **正面：** 新局从装配起就有确定税籍，个人分红税直接生效；未清税额追缴视图有明确默认语义；模式选项为后续复杂税务机制预留了接入点。
- **负面：** 新局配置、严格存档契约、fixtures 与 UI 均须承载税务模式；「大 A 方式」在实现前须在选项面显式标注不可用，不能静默降级为默认模式。
- **后续批次：** B2（默认扣税／开局选项／UI 入口）批次 scope 的直接依据——engine 新局税务模式选项与严格持久化、web-wasm 税务身份查询导出、Web 新局选项与税务状态／未清税额面板、存档 fixtures 重生成、[trading-rules.md](../trading-rules.md) 登记默认税籍与开局选项语义。[ADR-0039](0039-rights-placement-split-scope-and-toggle.md) 的配股／增发启用开关共用同一新局选项面。

## 关联

- [ADR-0035](0035-company-system-simple-fundamentals.md)：严格存档与装配边界。
- [ADR-0039](0039-rights-placement-split-scope-and-toggle.md)：共用新局选项面。
- [trading-rules.md](../trading-rules.md)：个人股息税完整登记口径。
- [当前交接](../../agents/company-system/current-handoff.md)。
