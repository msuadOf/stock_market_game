# 隐藏扫描批次 060

- 基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`；source root：`/data1/baiyifan/workplace/stock_market_game`；caller/output root：`/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 依据：读取 caller 的 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、当前 implementation-audit 总账和 `reaudit-engine.md`；核对 ADR-0026 与最新 ADR-0028。ADR-0026 对应机构个人经历策略；ADR-0028 为最新但与本候选无关。Q11 的机构个人阈值补充已有 ADR-0026；实现审计内部 Q11 是更正/违约 cause 路由待定，两者不可混为一项。未查阅交易所规则；该类型变更不修改交易规则。
- 方法：仅审阅 scan-plan batch 060 列出的三篇材料，逐篇连续读取到 EOF 并核对实测行数、SHA-256。核对 43b1aa5 当前 caller、状态 owner、consumer；未运行测试/构建、未执行 Git 写操作、未修改产品代码。

## 来源完整性与逐章判断

| 来源 | 实测 | 全文范围与判断 |
|---|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-10-chain-full.md` | EOF；32 行；SHA-256 `8b8f01d3d7c45f5f216c38c4df83cb099d9596a0b23ba07d673ab08ccf0581a5`；aliases=1 | 全文包括调用链、候选边界及三项门禁。记录将 `InstitutionalFacts(ExperienceMoment)` 作为候选，并说候选尚未实施；当前基线源码已有该模式及透传，未实施判断已过时。调用链和行为边界与当前实现相符。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-10-final.md` | EOF；26 行；SHA-256 `dff3746d1264dc13064c4badbf8968ee7f0ed2a6830685f96ca55208fb3e6bb0`；aliases=2 | 全文含绑定的 candidate 哈希、限定阅读范围、候选复核和三门禁。其“源代码仍保留 Option 与 expect、候选尚未实施”与 43b1aa5 不符；这是历史状态声明，不能据此认定当前源码仍有该问题。 |
| `agents/oop-refactor-audit/exhaustive/reviews/engine-pipeline-10-retail-full.md` | EOF；19 行；SHA-256 `be50b1c557dd7dd29b5921bcb826532fe831690135aa5f4a549070fea7776f7e`；aliases=1 | 全文含零售源核查范围、三项门禁和未实施声明。当前源码已经使用带 moment 的机构模式；其关于零售/机构行为区分、实际 `charged` 费用及分支不变的判断仍与源码一致。 |

## 当前调用方、owner 与消费者

- `apply_session_settlement_transaction` 在 `account_settlement.rs` 从 session civil date、market minute、trading day 构造一个 `ExperienceMoment`，传给 settlement prepare；prepare 基于 receipts 准备 settlement shadow 及结算前后持仓，之后分别调用零售与机构投影。
- `project_institutional_experience` 是机构投影中间 owner：筛选 Fill、`AccountKind::Inst` 且提供 belief chain 的账户，从其 `BeliefBook` 读取 experience，调用 `project_institutional_receipts` 并传递同一 moment，最后将投影出的 experience patch 写回克隆的 belief book。缺少 belief book 时返回 typed error。
- `retail_projection.rs` 当前公共（模块内）入口将机构时刻包进 `ExperienceUpdateMode::InstitutionalFacts(moment)`；`project_receipts`、`AccountFillProjection` 只接收该 mode，机构分支按订单费用/持仓更新机构事实，零售分支继续使用零售 `record_fill_with_order`。`ExperienceUpdateMode` 是模式 owner；机构体验仍由每账户 `BeliefBook` 持有，结算成功后由 session 提交相应 patch。
- 基线 `git show HEAD:packages/engine/src/session/pipeline/retail_projection.rs` 内容 SHA-256 为 `10fc159d7c91932216f27c5e561b034a02032b78810f5b9457c03b62d9e50e48`；当前 worktree `retail_projection.rs` 哈希与之相同。代码位置：`retail_projection.rs:322-343,440-459,519-535`；session 入口/prepare：`account_settlement.rs:41-63,153-210`；机构中间层：`institutional_experience_projection.rs:9-78`。

## G/Q 关联、门禁与裁定

- G/Q 总账中未发现此候选直接核销的现行缺口。G08 是散户日期与失败衰减消费链未接入；总账和 `reaudit-engine.md` 明确区分该缺口与已存在的机构经历链，A01 类型形状不能核销 G08。G07 的散户基本面分析/五路消费也不是此投影 mode 的契约。Q11 需区分 `docs/open-questions.md` 中已由 ADR-0026 明确的机构个人策略补充，与 implementation-audit 内部“更正/违约 cause 分发”待决项；本候选都不改变。其余 G/Q 未见直接关系。
- **大 A 语义：通过。** 该模式只表达既有机构投影必须携带的时刻。现有 Fill/账户筛选、订单费用、持仓变化及结算 patch 边界保留；没有成交顺序、收费、交易单位或 A 股制度变化。无需新增官方规则主张。
- **必要性与最小范围：对候选设计通过；实现状态已满足。** 当前 enum 已让机构时刻成为 `InstitutionalFacts` 变体的必需 payload，消除了分离 `Option` 与 `expect` 的不一致表示；改动限定在投影模式参数传递，不新增服务对象或权威状态。
- **遗漏边界/跨层漂移/复杂度：通过。** 同一 session moment 经 prepare 和机构中间层传到末端；belief experience 来源和提交 owner 明确。零售分支仍使用 retail experience，机构事实写入 BeliefBook。旧审查所列两个机构 settlement 行为测试和零售 replay/persistence 测试是审查记录提及的测试，不代表本批运行或通过；没有私有 enum 非法态的外部可构造边界需求。
- **历史记录修正：** 三份材料中“候选尚未实施/当前仍有 Option 与 expect”仅对其所述审查时点有效；43b1aa5 上已实现目标类型形状。此批不是实施后完整 diff 复核，也不宣称测试通过、交易所规则已重新核验或该批代码由本审查实施。

结论：三篇指定材料均完整读取并校验指纹。当前基线的 A01 类型候选已满足，未发现新增 G/Q 缺口或大 A 语义偏差。
