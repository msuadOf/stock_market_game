# 隐藏扫描批次 136（owner 1）

## 范围与完整性

- 来源基线 `43b1aa5`；读取根仓库为 `/data1/baiyifan/workplace/stock_market_game/`，现行 caller/总账核验树为 `.worktree/implementation-reaudit`。
- 已先读取 worktree 的 `AGENTS.md` 与 `docs/principles.md`。三篇来源逐篇从首行连续阅读至 EOF，实测行数、SHA-256 与 scan plan 一致；没有以搜索片段代替全文。
- 未运行测试/构建，未执行 Git 写操作，未改产品代码。本文只记录历史结论复核；OOP 提取本身不视为缺陷修复，历史 agent 指令不执行。

| 来源 | 行数 | SHA-256 | 章节族与 EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/domain-before/reviews-engine-company-01.md` | 62 | `1431199d485745e374ace00c598bcdb884c09c00e7c3390b5dc0726eecbf65f0` | 复核范围与结果；需要修正；D01–D03 缺陷线索；逐文件 OOP/验证矩阵；路径哈希与领域边界；已达 EOF。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/domain-before/reviews-engine-company-02.md` | 48 | `1ed73289ec1097b8497d38aea0166ad4108adbfd803962135afe707c3c2806a4` | revision4 D02 delta；覆盖/JSON/哈希；D01、D02 fallback 限定；25 个文件对象归属；领域语义与结论；已达 EOF。 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/managers/domain-before/reviews-engine-company-03.md` | 46 | `7dcaba552f440e6fc5fc9828c42c893187f784c48411bc2133c2057489953f05` | 26 文件对象判断；D01 存货种子漏检与候选测试；领域语义与结论；已达 EOF。 |

## 当前代码与调用者复核

- **复核 01 的 D01 合并往来非正值：保留为候选。** 来源确认 `IntercompanyBalance.amount` 公开可反序列化、消费端只比较镜像值。现行 consolidation owner 是 `consolidation/eliminate.rs` 的工作底稿预检/抵销；实际入口为 `accounting/consolidation/mod.rs::consolidate`，调用方按组快照构建合并结果，不是 OOP 迁移遗漏。来源所指零/负同额进入 worksheet 的路径未由独立反证材料证明已修复。当前候选需按消费入口验证声明金额正值和拒绝时成员 `Books` 不变；不把静态判断写成已有测试证明。
- **复核 01 的 D02 库存/应收子账部分提交：两条具体失败窗口仍应保留，范围限于可编辑 serde 状态或直接子账 API。** 当前 `company/industrial/purchasing.rs:103-115` 是 `post_with_commit` 后调用 `InventoryLedger::receipt`；赊购/采购调用者 `IndustrialBooks::purchase` 可在子账错误前提交总账。可编辑账套状态可构造数量溢出。应收核销入口在 `sales.rs::write_off_receivable`，先过账后调用 TradeOpenLedger 写销；其失败状态的可达性也依赖编辑存档。来源已正确区分直接子账失败原子性与 caller 总账已提交，不应扩写成所有合法运行态普遍失败。
- **复核 01 的 D03 结账/更正部分提交：属现有 Q17，不另建重复候选。** `accounting/closing/mod.rs:197-207` 的 `close_year` 先完成 `close_month`（封 12 月并保存月报），再生成年度报告；失败可留下月报/已封期间。`correct` 在 `:269-293` 先过账，再记重述，之后才生成并保存更正版。总账 `implementation-audit-2026-10-02.md:155` 的 Q17 已记录更正报告溢出和部分状态，且明确不擅自要求整个 API 强事务；按此限定保留 Q17，不将复核 01 的同一事实另计。年度关闭分支若无 Q17 等价登记，可作为现有结账 API 行为边界报告，不能把它说成已经修复或已要求强事务。
- **复核 02 的 D01 `EclPolicy.version`：历史行为候选，不见后续确认修复或正式产品承诺。** 当前 `company/bank/ecl.rs:87-93` 的 `validate` 确实只校验情景表，`version` 未设正值约束；`BankBooks::new` 调用构造期校验是实际生产消费边界。来源报告建议补版本 0 拒绝/版本 1 接受，但版本格式语义尚无 ADR 明确要求，不能直接升格为确定的产品缺陷。总账 Q19 涉及独立 serde 的 ECL 政策恢复，不等同构造时版本下限；不以 Q19 代替版本语义决策。
- **复核 02 的 D02 `notes.rs` fallback：保留为内部一致性风险，标准生产路径不可达结论仍成立。** 现行 `reports/mod.rs::generate_report_set` 先建完整行业分类并验证，之后才调用 `notes::build_notes`；`notes.rs:208` 的 PaidInCapital fallback 只在内部分类缺项时使用。来源核实公开生成路径已校验分类，缺项目前不是标准 caller 可提交输入。不得称用户当前报表已静默错列；若将来内部输入契约变化，显式错误比兜底更符合原则。对象级结论仅是没有 OOP 必要动作。
- **复核 03 的 D01 工商开局存货漏检：候选成立，且与复核 01 中其他库存溢出缺陷独立。** `company/industrial/config.rs:97-123` 的 `seed_inventory` 仅检查 `seeded` 的键；`IndustrialBooks::new` 在 `company/industrial/mod.rs:80-118` 局部过账后调用对账，再返回账套。合法公开构造可在总账 1403/1405 有余额但 seed 为空而成功返回。会话/运行 caller 包含 `company/operations/day.rs:356` 的账套 fixture 构造。现有证据不是法定会计违规，也没有证明修复已落地；建议独立候选仍须按 TDD 验证，且不触及 WIP 5001。

## G/Q、ADR 与结论边界

- 对照 `agents/implementation-audit/implementation-audit-2026-10-02.md`、`reaudit-engine.md`、`hidden-review/candidate-resolution-01.md` 与 `batch-037.md`：G01–G68 中没有与工业 opening seed 对账、非正合并余额、版本号下限或库存/应收溢出 caller 边界明确等价的条目；G27 已单项核销，其余 G 项不因 OOP 归属判断而核销。G35/G36 是经营闭环/多行业装配，G58 是按贷款人授信计算，不能覆盖这些事实。候选裁定文档也明确将 opening seed 校验列为未映射现有 G/Q 的独立候选。
- Q17 与结账更正的部分提交事实重合，应去重；Q19 是 ECL 恢复校验边界，不能据此推导 `version > 0`。产品 `docs/open-questions.md` 与 ADR-0023–0028 未发现对上述其他候选作出明确取代或授权修复的决定。ADR-0016/0024 关于不执行股东分红、增发、回购、清算分配等范围决定不改变本批会计实现结论。
- 本批材料未提出证券交易行为变更；没有重新核验交易所规则。结论是静态代码/契约审查，未运行测试。来源内的 OOP 对象提取与文档同步问题不构成行为修复证明。
