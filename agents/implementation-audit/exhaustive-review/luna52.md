# Luna52：Bank 三份实施记录全文复核

## 范围与证据边界

- 目标：产品 `08e4fc7`（merge 同）；当前只读 worktree HEAD 为 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`。按任务限定，未写产品源码或 Git。
- 连续读至 EOF：`bank-result.md` 66 行、`bank-review.md` 39 行、`coordination.md` 34 行，共 139 行；下表逐章覆盖，无跳读。
- 已读 `AGENTS.md`、`docs/principles.md`。权威 `agents/oop-refactor-audit/challenge-2026-10-03/action-index.md` 在本 worktree 不存在，故不以缺失正文补造需求。银行准则语义以正式 `docs/company-accounting.md` §2.3/§2.6 与登记官方依据为准；本批只做公司经营子账归属重构，不改证券账户或沪深交易制度。
- 对照正式银行范围：`docs/superpowers/specs/2026-09-13-company-information-learnings.md` 任务9段（ECL 权重和 10000bp、建设/重估校验、计量及分母）；正式公司恢复校验当前要求见 `docs/open-questions.md`、`docs/principles.md` 与 session persistence。存档中非法 serde 极值自身不作为“正常拒绝必须原子”的依据；只有正式恢复边界明确要求验证的非法业务事实才构成独立候选。
- 本轮未运行测试、Cargo、浏览器或长验收；历史“已通过”仅作为原记录陈述，不作为本轮验证。

## EOF 逐章—caller 矩阵

| 原文行/章节 | 承诺原文与当前实现/caller | 复核结论 |
|---|---|---|
| `bank-result.md:1-9` 范围、依据、验证交接 | 原文：“范围：`packages/engine/src/company/bank/**`”；worker 未运行 Cargo，root 曾报告短测通过。当前文件落在 bank 子模块；此处仅能确认实施/验证记录存在。 | 未发现范围外代码承诺；既不把历史验证当此次证据，也不因本轮未跑测试推翻历史结果。引用 action-index 的出处在本 worktree 缺失，属于来源证据边界。 |
| `bank-result.md:11-18` domain-N04 | 原文：“`BankBooks::new` 在 `EclPolicy::validate` 之后、`Books::post_batch` 之前调用。” 当前 `config.rs::check_opening_lines` 被 `BankBooks::new` 调用；`mod.rs:110-119` 顺序为 policy validate、禁止科目 guard、post。 | 真实 caller 与首错顺序吻合；未见把禁止科目清单改允许清单。对 ECL **无效恢复政策**的校验问题另见候选 C02，不混入开局种子守卫判定。 |
| `bank-result.md:20-28` domain-R2-N10 | 原文：“账套保留总账、事件号、现金可支付检查及客户资金流水”；“稳定合同顺序均保留”。当前 `deposits.rs::validate_withdraw` 被 `withdraw_deposit` 调用；`interest.rs:42-75,130-168` 对 loan/deposit 逐合同预览、统一 post 后逐项 apply；提款和付息仍由 BankBooks 生成凭证、校验支付能力并登记 flow。 | owner receiver 已进真实生产 handler。完整拒绝保证仅对 pre-post 校验失败成立；post 成功后的局部 apply 不可概括成绝对原子，见 C01 的文档措辞问题。合同稳定顺序可由 BTreeMap 遍历静态确认。 |
| `bank-result.md:30-39` domain-R2-N11 | 原文：“`BankBooks::new` 经 `EclPolicy::validate` 校验两表”；“原 `issue_loan` 没有再次校验通过 serde 恢复的政策”。当前 `new` 确实校验；`lending.rs:36` 将 initial allowance 交给 `EclPolicy::initial_allowance_target`；`ecl.rs:95-105` 明示不重验；`assess_credit` 在查 loan 前运行受检 `EclScenariosRef::validate`。 | 算术 owner/caller 与首错顺序明确。保留 issue 旧接受集的重构解释成立，但它不能覆盖正式持久化恢复的完整状态校验要求；C02 是跨恢复边界的独立正式契约候选，不要求每次 issue 重复完整政策校验。 |
| `bank-result.md:41-51` domain-R2-N12 | 原文：“BankBooks 保留 validate→post→apply”；“没有前置原在 apply 时才会发生的算术错误。” 当前 lender/interest/writeoff/ECL handlers 调用 `BankLoanState` guards/previews；`writeoff.rs:75-100` 为校验→post→apply→flow；`loans.rs:335-341` recovery 先减 recoverable 后 checked add allowance。 | 封装迁移与实际消费闭环。旧 partial apply 明确存在，不得宣传所有失败事务化；对只可由非法 serde 极值造出的 arithmetic overflow 不升格为本批产品回归。 |
| `bank-result.md:53-58` 短验证入口 | 原文列 9 case、`company::bank::behavior_tests` 与 bank_accounting 既有 suite，另称 worker 未跑完整回归。 | 源码定义和历史证据路径存在；本审计没有运行，也没有 sequence 输入、跨合同 apply 等动态覆盖证据。缺测试证据不是实现缺陷的自动证明。 |
| `bank-result.md:60-66` 语义/范围 | 原文：“存款为负债、贷款为资产、客户现金流为原经营分类”；金额分、利率/ECL bp；不引入股票跌幅推 PD 等。 | 与 `docs/company-accounting.md:72-84,130-133` 的游戏边界一致；不触及沪深 A 股规则。官方准则依据沿用项目登记，本审计未重新联网核验。 |
| `bank-review.md:1-9` 范围与结论 | 原文称完整审查 8 个源码文件及行为测试，且：“未发现本批引入的阻断性 finding。”并声明静态结论、未运行测试。 | 作为实现 diff 静态审查结论范围清楚；不等于所有旧接受状态正确，也不覆盖正式恢复校验以外的层级。其指向 action-index 的审查范围因路径缺失不能独立复原。 |
| `bank-review.md:11-15` 三门 | 原文核对 bank/liability、loan/asset、分/bp/余数、方法 owner 与错误顺序。 | 对照当前 owner/caller 未发现跨层语义漂移或多余新状态；准则依据只沿用已登记依据，非本轮新核验。 |
| `bank-review.md:17-21` 特别复核 | 原文：“serde 可接受的极值 state 可能产生旧有部分提交”；并记录空 stage1 policy 可恢复后 issue、private 字段不影响 serde map/sequence。 | 原记录已准确暴露两个不同范围：C01 的 overflow 来自特意构造的非法极值；C02 的空 ECL 是业务非法恢复状态，且正式恢复验证职责需独立查证。map/sequence 仅静态比较 derive/字段，未动态验证。 |
| `bank-review.md:23-39` 验证限制与 SHA | 原文明确未动态覆盖 accrue overflow/多合同部分 apply/sequence；列 9 个 SHA。 | 测试债与源码缺陷应分开；SHA 是历史冻结审查证据，本轮未重算九文件，不能声称本轮复现其指纹。 |
| `coordination.md:1-8` 批次总述 | 原文报告 39 动作、104 精确短 case 与 build/check 成功，worker 不单独跑 Cargo。 | 仅为历史协调状态，不能替代本轮测试证据，也不能证明 bank 状态在 session 恢复入口得到领域校验。 |
| `coordination.md:10-20` worker 矩阵 | bank 行列出 N04、N10、N11、N12。 | 与 bank-result 四章匹配；本审计不复核其它 worker 内容或据总表推断领域功能全完成。 |
| `coordination.md:22-28` 跨组接线 | N07、N41、A03、N06 汇报其它模块 receiver 接线。 | 超出本分片银行实现范围；不将跨域汇总当 bank 缺项或本轮已复核其他组的证据。 |
| `coordination.md:30-34` 验证说明 | 原文明确行为保护测试 baseline 预期通过，编译红不是行为红；root 统一复验。 | TDD 记录诚实区分了行为红与编译红。历史状态保留，不补造测试日志。 |

## 旧结论复核与候选

### C01：`BankBooks` 顶层“任何拒绝字节不变”过度承诺

`bank/mod.rs:8-12` 原文笼统承诺：“任何拒绝（含 `PaymentFailed`）账套与子账字节不变”。但 `interest.rs:70-75,162-168` 是 post 成功后逐合同 apply；`writeoff.rs:77-100` post 成功后才回收并登记 flow；`loans.rs:339-340` recovery 可先减少 recoverable、后因 allowance checked-add 失败。行为测试 `recovery_overflow_retains_existing_post_then_partial_apply_order` 明确以 serde 写入 `i128::MAX` allowance 固定该部分状态。

`bank-review.md:19` 和既往全文审计 `sweep53.md` 的 C01 旧结论——“这是既有非法 serde 极值边界，不是本批新增回归；模块注释与异常边界需区分”——本次复核**部分确认**：它准确反驳产品行为新增缺陷，但不能消除源代码公开文档的绝对字面矛盾。该测试输入不是经正常 API 可构造的有效银行状态，不能据它要求正常业务失败也必须覆盖所有 overflow 极值。建议把顶层承诺限缩到“所有校验失败及 `post_batch` 失败”，明确 post 成功后的内部 apply 算术错误语义；这是契约文字准确性问题，不据此扩大改产品行为。

### C02：公司存档恢复未见银行 ECL policy 全量校验，需按正式恢复契约闭环

正式 `docs/superpowers/specs/2026-09-13-company-information-learnings.md` 任务9明确“概率加权=情景列表 Σ权重必须恰为 10000bp（构造期与逐次重估双守卫）”，并在任务27相关正式恢复边界要求业务状态校验。`BankBooks` 以 serde derive 直恢复，`BankBooks::new` 不参与恢复；session `persistence.rs:977-1049` 的 `validate_company_domain` 检查公司映射、日期、scheduler mirror 与公共信息库，但没有调用银行/ECL 领域验证。当前 `issue_loan` 依赖启动时已校验的政策，却对恢复实例不重验；`behavior_tests.rs` 的 `restored_initial_policy_is_not_revalidated_during_issue` 将空 `stage1_default` 反序列化后成功放贷，且 `ecl_policy().validate()` 明确失败。这里不是要求 issue 对有效实例每次重复做全表校验，而是恢复边界没有建立其后置条件。

旧 `bank-review.md:20` / `sweep53.md` C02 把它称为“候选，需查公共恢复入口”是当时谨慎的；本次检查了实际 `validate_company_domain` 后，候选**仍成立且可精确化**：正式保存恢复路径应拒绝空/权重和不合 10000bp 的 Bank ECL policy，或在可证明等效的统一 domain validation 中校验；不能由 `issue_loan` 的旧行为保护测试取代恢复校验。限定为正式存档恢复可达的无效业务政策，不外推为所有任意 serde 极值都必须被业务 API 再次拒绝。源码修复不在本任务授权范围，已仅登记供主控处置。

### 已核销 / 未升级的候选

- “receiver 只定义未消费”：核销。N04/10/11/12 的当前真实生产 caller 分别是 `BankBooks::new`、withdraw/accrue handlers、`issue_loan`/`assess_credit`、各 loan handler；上层 `operations/bank.rs`、`operations/dispatch.rs` 继续调用 BankBooks API。
- “private 字段改变 serde 接受集合”：未发现证据。derive、字段名/类型/顺序保持；只核对静态代码，没有声称 sequence 动态测试通过。
- “所有非法 serde 极值都必须恢复失败”：不成立为本次默认要求。只就正式恢复契约明确覆盖的非法 ECL policy 记录 C02；recoverable/allowance 算术极值只支撑 C01 的文档限定，不扩为领域业务失败要求。
- 大 A 语义候选：无。本批不修改证券交易路径，既有存贷款资产负债与现金流分类未漂移。

本轮只新增此复核记录；未运行测试、未改产品文件。
