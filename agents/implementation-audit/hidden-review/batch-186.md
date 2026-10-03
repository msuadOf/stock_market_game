# 批次 186：engine pipeline 07/08 历史复核材料审计

## 来源读取与范围

基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`（worktree `HEAD`）。目标 worktree 的 `AGENTS.md` 与 `docs/principles.md` 已阅读；同时核对 `docs/open-questions.md`、决策目录及相关 ADR。三篇来源逐篇连续全文读取至 EOF，实测行数、SHA-256 与任务给定值一致，无搜索片段代读。

| 来源 | 行数 | 章节族及覆盖 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-pipeline-07-final.md` | 27 | 复核范围与依据、两处文档修正核验、三项独立复核门禁、结论与限制 | 已读至 EOF |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-pipeline-08-caller-final.md` | 39 | caller 生命周期门禁、继承的局部 mutation 风险、生产 dispatcher/transaction 边界、三项门禁、结论 | 已读至 EOF |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/engine-pipeline-08-final.md` | 36 | local admission、EnvelopeLedger、三项门禁、结论与尚待 caller 核验边界 | 已读至 EOF |

## 当前代码及决策核对

- `local_admission.rs:25-58,61-80,98-156,159-220` 仍由 `AccountReceipts` 维护计划命令的账户局部 ordinal，`ReadyAdmissionPlan` 只拥有一个 ready batch 的候选索引、偏序和股票 gate。资源 lane 按同账户现金或账户-股票股份建边；股票 gate 记录该股票入口的实际并发登记顺序。身份、输出排列不是跨账户或交易所优先级。
- `local_admission.rs:178-218` 的 quote dependency 校验候选身份唯一、前驱唯一且存在、前驱先于 replacement、同账户同股、Cancel 到 Place；没有校验 Side。撤单没有 Side，故不能把旧材料任何“方向”字眼扩写为买卖方向验证。此处只做游戏内局部资源受理，不替代交易所申报校验。
- `local_admission.rs:49-55,61-80,98-102` 中计划 receipt ordinal 先递增；后续 admission 处理可能以 `Err` 返回，helper 本身不回滚 ordinal。沿当前生产 caller，`ready_ingress.rs:72-90` 正常构造时初始化 transaction-local `AccountReceipts::default()`，`:97-119` 通过 `admit_ready_batch(...)?` 传播失败；`authoritative_tick.rs:20-49` 连续、竞价及盘前准备失败均返回错误，只有 prepare 成功并通过 precommit 才消费 commit。故既有 caller-final 对**当前这些生产 phase caller**的有限丢弃结论仍有证据支持；它不证明未来 caller 或独立重试路径安全。
- `ledger.rs:55-63,76-110` 的 `EnvelopeLedger` 继续集中 live/terminal envelopes、audits、conservation、receipt keys 与 cursor。公开 `apply/remove_terminal/insert_created/rebase_live_for_next_tick` 在 clone/candidate 上成功校验后安装（`:113-125,168-173,242-251,311-315`）；private transition 可先改 owned candidate 再失败（`:128-145,175-196,256-294,318-355`）。因此旧结论对 public 与 private 失败原子性差异仍准确；private 路径要求外围 candidate owner 在失败时整体丢弃，不能仅凭账本 owner 本身推断所有调用点已满足。
- 现行交易边界由 `docs/trading-rules.md:77-94` 登记；ADR-0017 的局部受理/双资源约束见 `docs/decisions/0017-escrow-parallel-tick.md:38-40`，其中旧 source-class 顺序已经被后续决定取代。ADR-0018 仍为 proposed（文件头第 5 行），不能把其未接受的长期时间线方案当成现行义务；其中已明确的局部冲突决定与 ADR-0017 后续补记、trading-rules 当前登记一致。未发现本批材料提出新的沪深交易制度主张，不需用历史 OOP 提取建议改变交易语义。

## 旧结论再证与候选反证

1. **07-final：再证。** 其范围是历史复核文档本身的统计和符号清单修正：15 个模块的生产/测试拆分为 7/8，`retail_account_input` 已补入 snapshot 测试 helper 清单。该记录明确只做增量文档复核，没有声称重新审查 15 个源码文件或运行测试。当前源文仍保留该范围限定；没有证据将该历史文档问题转成当前生产 G/Q 项。
2. **08-caller-final：再证但有限。** 当前 ingress 初始化、admission 错误传播及 phase dispatcher 成功后提交的代码路径与其结论一致。其结论只覆盖现存这些生产 phase caller，不外推未来 caller。无需把 admission helper 的 mutation-before-Err 独立升级成生产状态污染 bug。
3. **08-final：再证其局部职责判断，保留 caller 限定。** receipt 资源偏序、Cancel→Place 依赖和 EnvelopeLedger owner 的说明与当前代码相符；需按 caller-final 将 08-final 尚未闭合的现有生产 phase caller 生命周期项视为后续已有复核补齐。private ledger 方法的整体丢弃约束仍是每个 owner/caller 的责任；本批未完整枚举全部 private ledger 调用点，不能据此宣称任意 caller 均已闭合。
4. **未见候选反证或新 G/Q。** 三篇均为历史 OOP 文档/实现复核，不以提取类或迁移提案本身作为缺陷修复。与实现审计 G01–G68/Q 对照，没有发现同一已登记缺口被这三篇材料核销或重开；也没有证据需要因其建立新项。这里不将静态代码与文档复核表述为测试、构建或全量验收通过。

## 门禁结论

- **大 A 语义：** 范围内通过。材料涉及游戏内部资源受理和 envelope 审计模型，不赋予身份、账户号或布局交易优先权；Side lane 不等于交易所方向优先。沿用现行 `trading-rules.md` 和已记录 ADR，不作新的官方规则断言。
- **必要性与最小范围：** 通过。当前 owner 已承担相应职责；不因历史提取建议重复增加账本、admission wrapper 或其他抽象。
- **边界与跨层语义：** admission 当前生产 caller 的失败丢弃边界有 caller-final 证据；EnvelopeLedger private 方法的保证仍依赖实际调用 owner。无新已确认产品缺口；未穷举全部 private ledger caller，也未运行测试。

**结论：** 三份历史复核的核心职责和局部失败边界在当前基线未被反证；08 caller 的有限生命周期结论已由现行调用链支持。候选状态为无新增 G/Q；private ledger caller 的全覆盖不在本批结论内。
