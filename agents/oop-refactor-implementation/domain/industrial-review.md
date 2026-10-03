# industrial 独立审查

- 审查日期：2026-10-03。
- 审查者 canonical 身份：`/root/implement_domain/review_industrial`。
- 审查者未参与本簇实现；未派生其他 agent，未修改产品源码，未执行 Cargo、产品测试或 Git 写操作。
- 比较基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 完整 diff 范围：`packages/engine/src/company/industrial/{config,expenses,interest,loans,mod,purchasing,repayment,sales}.rs`，以及未跟踪新增文件 `ownership_tests.rs` 的完整内容（269 行）。
- 额外只读核对：上述变更文件完整现行源码、`accounting/tax.rs` 完整源码及相对基线 diff、`accounting/mod.rs` 公共重导出、既有 `industrial_accounting/{chain_gold,tax_gold,failures/guards}.rs` 完整内容、`company/operations/{bank,dispatch}.rs` 必要接线片段，以及本簇 `industrial-result.md`。
- 已阅读约束：`AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`、ADR-0016、`docs/company-accounting.md`，以及权威 `action-index.md` 的 domain-R2-N13/N14/N16 与 domain-N06 正文。

## 最终版本 SHA256 绑定

2026-10-03 补充冻结时，重新完整读取当前相对基线的 8 个变更文件 diff，并再次全文读取新增 `ownership_tests.rs` 的 269 行（7 个测试与全部 Fixture/helper）；与本审查此前读到的 diff/测试内容一致，没有新增源码变更需要另行审查。初审时未存储 SHA256，因此不虚构历史哈希对比；以下为此次重新核对后实际读取文件字节的 SHA256，静态审查结论绑定到这 9 个版本。本文仍不声明运行过这些测试。

文件均位于 `packages/engine/src/company/industrial/`。

| 文件 | SHA256 |
|------|--------|
| config.rs | `0d6a435940f2293d5965796ac653694d435d4a77fa73e378271fa375dff5fcb6` |
| expenses.rs | `a3e1aed8a4a59f2eddc9bdc6b3051afe1cf1269f973e93ba3db94e6d3d284c6f` |
| interest.rs | `1eff08bbbfb7658036ac4f4b9ff1793b23f68008b5d05dea72e42f0897b20db8` |
| loans.rs | `540513c622ead925b9104e45f7ba6930f7789a29877e5e3ea3fc726508679fda` |
| mod.rs | `d771b7f38fb2b8f13d24772c740f62430c33849fb467abfcbc967b6d40ac5ca5` |
| purchasing.rs | `5da6c4cccb784d9ba26c38272c11340106cc59780a66237f0133c822491701a9` |
| repayment.rs | `02c5ab803b5655cd01cda0f495229727e18d8ecab69769dff72f69b84c28373b` |
| sales.rs | `24ee743e113618d62e9f6ff62afa2d0b77e3d77aa09ddb55600d862a32c51b01` |
| ownership_tests.rs（新增，全文已读） | `5373b8d6ff0c10c52f074fc67f8713d77f8bc7188173e8b8365e9d26ad2e03ad` |

## 结论与门禁

未发现需要修复的产品源码行为回归；发现的 1 项实施记录 caller 归属错误已修正并独立复核关闭。本簇静态独立审查门禁已完成。静态审查不能替代协调者的编译和测试门禁，本审查未执行测试，不声明任何测试通过或 Red/Green 已完成。

### 1. 大 A 语义及依据

本次为职责迁移，不新增或修改沪深交易制度、税率、借款条款或会计计算规则。金额仍为 AccountingAmount（分），序列化金额仍沿用原金额类型；ACT/365F 分母、半偶舍入与 FractionUnits 的累积公式不变。普通借款的短长科目选择与 OPENING-DEBT 恒记 2001 的既有游戏语义保持；借款、付息、还本仍为公司筹资现金流，未与投资者资金混算或补钱。

N06 的 policy 方法实现与原 free 算法逐句相符：进项仍两次半偶舍入，所得税到期判定仍为 saturating_sub 后严格大于年限，亏损稳定排序/FIFO/全额 DTA 简化不变；原公开 free API 仍仅委托同一方法。未新增结构 validate 或默认税率。`docs/company-accounting.md` 的 VAT/CIT 税法与 CAS 18 原文取证受阻、合成 Fixture 与游戏简化边界继续有效；本审查沿用这些已登记依据，不声明重新访问官方原文或完成真实税法/CAS 合规核验。

### 2. 必要性与最小范围

改动符合已授权的三个可选内聚性动作：N13 的借用视图将 ledger 与开局种子绑定，维持分阶段装配；N14 私有 LoanPortfolio 实际收口 loans_mut，集中稳定遍历、合同配对预览和受控提交；N16 私有 IncomeTaxPosition 实际收口跨年 loss_pool 与年度计算/提交。ContractBook、OperatingBudget、CounterpartyLedger、Books 与 TaxPolicy 仍保持原 owner，没有重复 payable 账本、合同副本、默认政策或泛化 TaxService。N06 industrial caller 只改调用 receiver，无扩大领域行为。

### 3. 边界、跨层语义与复杂度

- N13 保留 tax validate → opening post → inventory → assets → counterparties.register → debt 顺序。Inventory、1601、1602、2001 各自算法与拒绝面不变；未配置 debt 时不额外校验短债归零。所有失败仍丢弃局部装配结果，未返回半构造对象。新增首错测试固定了同时多错时的先后顺序。
- N14 的 preview_accruals 是 BTreeMap 稳定序上的 lazy iterator，逐项计息仍先于本项 event id 生成、再进入下一合同；没有预先批算造成错误优先级漂移。days < 0 拒绝、days == 0 跳过、正天数零金额仍更新 carried/date 的旧行为不变。accrue_interest 按 item 数消耗 event id；repay_principal 的零分计提不增加分录 event。新增测试明确固定这两种不同规则。
- N14/N16 的写入时点与基线一致：Books/post_with_commit 成功后才提交贷款状态或亏损池；PaymentFailed 还本未提交 preview；封期税务 post 失败未提交 pool/id；零分录所得税仍提交 ending_pool。原 post 后 apply/counterparty 检查等旧失败面没有被本次重构强化或重排；本结论不将这些继承路径扩大为所有可编辑非法子账都具备完整回滚的证明。
- 年度所得税仍从全年收入/费用索引汇总，同年再次调用会包含已记所得税费用并再次增添亏损，新增测试诚实固定了旧非幂等行为；本次未引入年度防重或更改税前定义。
- serde：IndustrialBooks 仍有原 12 个字段，loss_pool 位置及序列化名称不变；未新增 flatten/default/alias/deny_unknown_fields。IncomeTaxPosition 与 LoanPortfolio 均 transparent，直接委托原 Vec<LossEntry> 与 BTreeMap<ContractId, LoanState> 的 Deserialize。另只读核对 Cargo.lock 锁定的 serde_derive 1.0.228 中 deserialize_transparent、expr_is_missing、expr_is_missing_seq：透明包装直接调用内层 Deserialize；derive(Default) 不等于 serde(default)。据源码可确认外层 map/positional sequence 接受路径、缺字段拒绝、null 拒绝及原 inner 接受集未被包装改写。未使用运行时测试验证非标准 JSON 输入。
- 新增 7 个测试与既有金样相互补充，未修改或弱化旧断言；尚未发现跨层字段/单位漂移或需要追加的产品抽象。

## 有效发现

### IR-01：实施记录将 BankBooks caller 误归属至 IndustrialBooks（低，记录准确性）

- 位置：`agents/oop-refactor-implementation/domain/industrial-result.md:23`。
- 行为：记录声称“银行经营迭代 caller 和 dispatch 利息 caller 继续通过原公开 API 使用唯一组合”，把银行经营迭代列为本簇 LoanPortfolio 的运行时使用证据。
- 证据：`packages/engine/src/company/operations/bank.rs:43` 的 advance_day 参数为 `&mut BankBooks`，`:115` 调用的是 BankBooks::loans；IndustrialBooks 的实际日流利息调用在 `company/operations/dispatch.rs:42`。
- 影响：误报本次 owner 迁移的真实 caller 覆盖，模糊银行贷款资产与工商借款负债的归属。权威历史 action-index 本身含同一错误引用，不宜将其直接复制为现行实现验收证据。
- 修复建议：仅修正本次实施记录，删除银行经营迭代作为 IndustrialBooks caller 的表述；保留 dispatch 工商分支的真实接线。无需修改 BankBooks 或扩大本批产品源码范围。
- 状态：已修正并复核关闭。2026-10-03 复读 industrial-result.md:23 与新增的 :52 修正记录，并核对 operations/dispatch.rs:33–57：现记录仅将 IndustryBooks::Industrial 分支的 accrue_interest 列为本簇真实运行时接线，BankBooks 误归属已删除。修复仅涉及工作记录，未要求改动历史 action-index 或产品源码；原静态源码审查结论不变。

## 非阻塞的覆盖建议与局限

- ownership_tests.rs:149 当前锁定正常 JSON map 往返，没有独立测试 outer positional sequence、loss_pool/loans 缺字段/null/重复字段以及错误 wrapper shape。静态 derive 核对未发现接受集变化；若后续改写这些 serde derive，宜用短输入矩阵显式固定上述接受/拒绝面，不能用正常往返代替接受集证明。
- ownership_tests.rs:114 当前为单贷款 fixture。既有 chain_gold 覆盖两贷款金额与 carry，但未断言多个合同中后项拒绝时整批不写入以及首个失败合同的 id 顺序。现实现 lazy preview 与基线相同，未发现回归；后续变更遍历或批量预计算前应增加这类保护。
- 本次没有编译、测试执行、官方规则重新取证或整个 domain 区域验收；未审查不相关修改的语义。所有“保持”均指本簇与给定基线的静态对照结论。
