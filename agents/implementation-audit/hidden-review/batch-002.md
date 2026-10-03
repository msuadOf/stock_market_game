# 批次 2 隐藏来源复核

## 范围与完整性

- 基线：产品代码 worktree `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。已读 worktree `AGENTS.md` 及 `docs/principles.md`；按来源所涉领域抽查 ADR-0002、0005、0016、0026。未运行测试、构建或 Git 写操作，也未改产品代码。
- `domain/report.md`：实读 2706 行，SHA-256 `614936394a7ac13b13f9d64ae259c4f0263ff2f0f8885c7f3042ed0d7136b94d`，分段连续至 EOF。涵盖候选处置表、N01–N41 各动作完整提案、逐文件处置索引说明、独立复核限制。
- `domain/review.md`：实读 20 行，SHA-256 `0c61b54a4529ba8359509c684226a5e935f24bf8d1ab72584991ce30504d6ed7`，至 EOF。审查修订重点为过期 caller、更正 OOP 候选门槛、部分变更边界、公开字段无法强制不变量及跨层读取范围。
- `file-index.md`：文件实测 1232 行、1,366,301 字节、864,825 个 Unicode 字符、SHA-256 `97271e86b138981a86060e9669b0d9d258e46f25de9378d57b920538c2c7002f`。逐字符已读 L1–185；之后 Unicode 字符 offset 182870–864825 连续核读至 EOF。

## 来源章节矩阵

- 游标续读补记：连续核读 Unicode 字符 offset 530870–536870，覆盖个人 beliefs/common/experience/information schema 及机构策略 parser。当前 cursor 536870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 536870–554870，覆盖 plans/primitives/root/runtime/snapshot schema、store slices 与 generated Civil* 协议 DTO。当前 cursor 554870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 554870–566870，覆盖 generated Company*/Daily*、Due*/Engine/Event 与 Experience DTO 条目。当前 cursor 566870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 566870–572870，覆盖 generated GameConfig、策略 params/style/intent、LimitPrice 与 MarketMinuteClose DTO。当前 cursor 572870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 572870–578870，覆盖 generated MarketSnap/Money/NPC 生命周期与配置、策略意见、订单 DTO 条目。当前 cursor 578870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 578870–584870，覆盖 ParentOrderPlan、Pause/PendingNpcBatch、Personal* 与计划 DTO 条目。当前 cursor 584870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 584870–590870，覆盖玩家工作单 DTO 与 public report 财务/比较报表 DTO 条目。当前 cursor 590870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 590870–596870，覆盖 PublicReport 各类财务字段、查询、scope/version 和不可用原因 DTO。当前 cursor 596870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 596870–602870，覆盖 PublicationId、Rejection/ResumeReason、RetailExperience 与计划复核 DTO 条目。当前 cursor 602870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 602870–608870，覆盖 runtime delta、save DTO、session setup、security/stock 和 price-memory 类型。当前 cursor 608870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 608870–614870，覆盖策略配置、tick protocol DTO 与 TradingPlan/Urgency 相关声明。当前 cursor 614870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 614870–620870，覆盖生成策略 DTO 后的前端格式/交易输入 helpers 与 bindings DTO。当前 cursor 620870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 620870–626870，覆盖 bindings Auction 与 Civil 协议 DTO。当前 cursor 626870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 626870–632870，覆盖 bindings CivilUpdate 后续与 Company/Daily DTO 条目。当前 cursor 632870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 632870–638870，覆盖 bindings Due/Engine/Event/Experience DTO。当前 cursor 638870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 638870–644870，覆盖 bindings GameConfig、策略与价格/行情 snapshot DTO。当前 cursor 644870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 644870–650870，覆盖 bindings NPC、订单及策略 DTO。当前 cursor 650870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 650870–656870，覆盖 bindings Personal、Plan 与 Player DTO 条目。当前 cursor 656870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 656870–662870，覆盖 bindings PublicReport 财务、附注与分页 DTO。当前 cursor 662870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 662870–668870，覆盖 bindings PublicReport 查询、版本、发布标识及 RetailExperience DTO。当前 cursor 668870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 668870–674870，覆盖 bindings 复核与 runtime/save/snapshot/session DTO。当前 cursor 674870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 674870–680870，覆盖 bindings 股票、策略、Tick 与 TradingPlan/Urgency DTO。当前 cursor 680870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 680870–686870，覆盖 bindings Watchlist、mobile 原型与测试索引开端。当前 cursor 686870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 686870–692870，覆盖 accounting/analysis/attention-discovery 测试条目。当前 cursor 692870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 692870–698870，覆盖 attention-discovery 与 auction/bank-accounting 测试。当前 cursor 698870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 698870–704870，覆盖 bank_accounting、behavior、calendar 与 causal/civil-clock 测试条目。当前 cursor 704870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 704870–710870，覆盖 civil-clock 与 company-decision/opening 测试条目。当前 cursor 710870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 710870–716870，覆盖 company operations/public financials/query 测试。当前 cursor 716870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 716870–722870，覆盖 company scenarios 与 consolidation fixture/golden/failure tests。当前 cursor 722870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 722870–728870，覆盖 diagnostics、experience、experience_feedback 与 replay 测试条目。当前 cursor 728870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 728870–734870，覆盖 fundamental_beliefs 测试套件条目。当前 cursor 734870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 734870–740870，覆盖 fundamental beliefs 与 industrial accounting 测试条目。当前 cursor 740870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 740870–746870，覆盖 industry reports 与 information acquisition 测试条目。当前 cursor 746870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 746870–752870，覆盖信息获取与 insurance-accounting 测试条目。当前 cursor 752870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 752870–758870，覆盖 market、money、observations、orderbook 与 plan-allocation 测试。当前 cursor 758870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 758870–764870，覆盖 plans、policy manifest、protocol 与 publications 测试条目。当前 cursor 764870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 764870–770870，覆盖 publications 与 real-estate accounting 测试条目。当前 cursor 770870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 770870–776870，覆盖 real-estate accounting、save-contract、scale-restore、session 与 step-skeleton 测试。当前 cursor 776870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 776870–782870，覆盖 strategy、technical-memory、tick、urgency 测试条目。当前 cursor 782870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 782870–788870，覆盖 desktop build/example/actor 文件与 actor 测试条目。当前 cursor 788870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 788870–794870，覆盖 desktop actor tests、server actor/deployment/publisher 条目。当前 cursor 794870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 794870–800870，覆盖 server routes、auth/deployment tests。当前 cursor 800870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 800870–806870，覆盖 server tests、WASM registry 与清理脚本/示例条目。当前 cursor 806870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 806870–812870，覆盖 escrow 验证 harness、runtime/path artifact 与 baseline 示例。当前 cursor 812870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 812870–818870，覆盖 production 示例及 build-targets / doc-symbols scripts 条目。当前 cursor 818870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 818870–824870，覆盖脚本生成类型、WASM 检查、CI、desktop build-matrix 与分发流程。当前 cursor 824870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 824870–830870，覆盖 frontend build、package distribution/static-web、pages isolation 与性能报告脚本。当前 cursor 830870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 830870–836870，覆盖 release policy、run-full-regression、run-long-validation 与 web-test scripts。当前 cursor 836870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 836870–842870，覆盖 bounded command、server build、audit diagnostic 与 simulation harness 条目。当前 cursor 842870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 842870–848870，覆盖 simulation baseline、escrow performance harness/source manifest/conservation contract 条目。当前 cursor 848870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 848870–854870，覆盖 escrow performance config 与 verification matrix/artifact tests。当前 cursor 854870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 854870–860870，覆盖 simulation verification tests、smoke deployment、workspace paths 与 CI/distribution workflows。当前 cursor 860870；EOF 未到。
- 游标续读补记：连续核读 Unicode 字符 offset 860870–864825，覆盖 release workflow、build workflows 与 package scripts 条目；末段实读 3955 个 Unicode 字符并到达 EOF。
- Unicode 总长度核对：全文实测 864825 字符，最终游标 864825，剩余 0；来源行数、字节数及 SHA-256 与记录匹配。

- 游标续读补记：连续核读 Unicode 字符 offset 524870–530870，涵盖 policy schema、公开报告库/报表 wire DTO 与 shared scalar parser。当前 cursor 530870，未到 EOF。

- 游标续读补记：连续核读 Unicode 字符 offset 518870–524870，索引范围进入 company accounting/books schema 与 flow policies。当前 cursor 524870；仍未 EOF。

- 游标续读补记：现已连续核读 Unicode 字符 offset 488870–518870；`last_cursor_unicode_offset` 同步为 518870，EOF 仍未到达。范围涉及 save command/repository/schema 及 civil/accounting parser。

- 游标续读补记：已连续核读 Unicode 字符 offset 488870–506870；当前 last_cursor 为 506870，JSON 对应 `last_cursor_unicode_offset` 已同步；剩余索引正文未读，EOF 状态仍为 false。

| 来源 | 全文章节/审阅结论 | EOF 与限制 |
|---|---|---|
| `domain/report.md` | L1–8 范围、方法及声明；L9–66 全部候选处置表（41 new、4 extension、覆盖、拒绝）；L69–2547 N01–N41 具名动作边界、caller、owner、迁移及测试边界；L2548–2701 147 文件索引说明；L2702–2706 复核签名和未验证限制。候选报告明确是调查清单，不是实施承诺。 | 已连续分段至 L2706；哈希/行数匹配扫描计划。未重新验证官方 A 股规则。 |
| `domain/review.md` | L1–6 审查身份、文件覆盖声明与需要修订发现；L7–18 候选/领域边界修订，包括 stale callers、N09/N17/N24/N29 拒绝、N22 非原子边界、N26/N27 方法迁移、N34/N11 公共可变字段；L19–20 要求最终复核及来源限制。 | 20 行完整输出至 EOF；这是 preliminary，不是通过结论。 |
| `file-index.md` | 索引自称覆盖 1186 主文件与 12 编排入口；L5–156 公司与引擎基础域，L157–272 执行流水线，L273–390 会话/协议/策略，L391–916 Web/生成契约，L917–1218 宿主/工具/测试，L1219–1232 命令/workflow。它汇总对象、owner、动作关联、retain 理由，report 及 review 都提醒 reader 结论不等于 manager 结论。 | 哈希、行数、字节数匹配；本批连续核读至全文 EOF，仍不能将索引判断等同于逐文件实现复核或产品行为验证。 |

## 当前实现与候选状态

当前代码核对使用 `.worktree/implementation-reaudit/packages/engine/src`。下表按来源提出的动作逐项交叉分类；已看到候选相关对象/方法存在，不表示提案全部字段、调用时序、错误边界均实现或已测试。

| 来源候选 | 现行基线定位及结论 | 与当前总账 / G-Q 交叉 |
|---|---|---|
| domain-R2-N01 restatement register | 来源证据：`closing/mod.rs:44,50,223-225,317-329`、`closing/save.rs:17-69`。候选并不修复更正先过账/登记后生成失败可能留下部分状态。 | Q17 仍记录该失败面（`implementation-audit-2026-10-02.md:155`）；不可核销为 OOP 抽取解决。 |
| N02 FixedAssetEntry | `accounting/fixed_assets.rs:26-76,132-209` 有 entry 和计量/应用方法；`company/industrial/capex.rs` 仍是交易协调调用方。候选迁移边界仍属建议。 | 无新增 G；资产账务规则未重新取证。 |
| N03 InventoryItemState | `accounting/inventory.rs:29-45` 状态，`receipt/preview_issue/apply_issue` 为 ledger 行为。来源特别记录 receipt 数量先变更、金额 checked add 后失败会局部变更；迁移若先全量预览属于行为修复。 | 无新增 G；不可把提案或存在类型当原子性修复。 |
| N04 IntercompanySale | `consolidation/sale.rs` 为声明及生成入口，`eliminate.rs` 仍负责跨成员配对/抵销；边界适用于公司报表，不改变交易撮合。 | 无新增 G。 |
| N06 EquityPresentation、N07 ReportClassification、N08 WindowConsolidationBuilder | 来源分别举证 `balance_sheet.rs:167-293`、`notes.rs:66-126`/`income.rs:140-181`/`reports/mod.rs:198-229`、`consolidated_window.rs:55-290`。属提案中的私有列报计算聚合；不能据此将公司 equity 当投资者资产。 | 无相应已登记 G；不能推成已实施的证明。 |
| N10 DepositState / N11 ECL borrowed view / N12 BankLoanState | 来源证据 `deposits.rs:26-244`、`ecl.rs:69-220`、`loans.rs:102-183` 和 handler `lending.rs:21-133`、`writeoff.rs:17-125`。当前基线有对应对象/方法；总账、event id、现金流仍应由 BankBooks 管。ECL 列表公开可变，view 不能永久保证其不变量。 | Q19 仍是独立恢复/校验问题（主账 L157）；贷款/存款候选不能替代行业恢复校验。 |
| N13 industrial opening seeds / N14 LoanPortfolio / N16 IncomeTaxPosition | 来源调用证据 `industrial/mod.rs:86-95`、`loans.rs:24-70`、`expenses.rs:126-258`。工商税种和金融量纲仍为公司经营账，不是投资者资金。 | Q23 仍记录年度所得税重复直接调用的语义不确定（主账 L161）；不声称存在税务生产日调用。 |
| N18 insurance group state | 当前 `insurance/groups.rs:26` 有 `ContractGroupState`，`:389` `apply_release`，`:414` `apply_remeasure`；claim / group 与 Books 过账分层可见。提案不证明整个 handler 原子，claim/LRC/remainder 边界仍需验证。 | 无可核销 G；该域不涉及证券 shares/T+1。 |
| N19 IndustryPairView / N20 OperatingDayRun | 来源证据 `operations/config.rs:18-22,90-95`、`core.rs:186-199`、`day.rs:22-224`；仍是可选的借用view/短期协调重构。日推进按自然日，不等于证券交易日。 | 无对应 G；不得把自然日与交易日合并。 |
| N22 BorrowingCostAccrualPlan | `borrowing_costs.rs:120-249` 先 post，再 apply loan，再对项目金额 checked 汇总，存在 post 后 Err 面；项目/贷款仍为不同 owner。 | 不得宣称候选提供原子提交；无新增 G。 |
| N25 ProjectState / N26 ProjectLoanState / N27 PresaleContract | 当前 `projects.rs:38`、`loans.rs:22`、`presales.rs:19` 分别为真实状态 owner；修订意见要求 N26/N27 明确新增 validate/preview 方法而非仅重列既有 API。 | 住房 units 为套，不是证券股数；预售收款为合同负债、交付确认收入。 |
| N28 RetailPositionDecisionContext | `behavior/decision.rs:11-618` 与 `heuristics.rs:55-211` 提案收束具名仓位输入/目标和 T+1 可执行量；不是宽决策 policy 拆分。意图/委托/成交不可混淆。 | 无 G 由对象存在核销；A 股整手买入、零股卖出余量需维持。 |
| N30/N31 diagnostics | 来源 `diagnostics.rs:253-257,393-823,895-1060`，候选是诊断投影，不是权威现金/持仓/撮合状态。 | 无新增产品 G；测试源码仅说明覆盖目标，不证明跑过。 |
| N32 CausalReportBuilder / N41 CausalCollector | `diagnostics/causal/aggregate.rs:10,235` 与 `diagnostics/causal.rs:110-127` 分属报告归并和采集期事实索引，不可合并成一个阶段。主来源自认 N41 的跨范围 caller 文件不属其 147 文件完整读取范围。 | 无新 G；causal 诊断的 filled/股数与金额分不改变真实成交。 |
| E03 RetailExperienceState | 来源将 N07 扩展限定在同 owner。现行总账 G08 表明 dated writer/衰减尚未进入目标消费链（主账 L51）；结构方法存在不能核销真实 caller 缺口。 | G08 仍开；不能声称散户 lifecycle 已完成。 |
| N34 price memory / N35 retention | 当前 `experience/price_memory.rs` 有条目与观测/读取；历史 review 已更正 `session/decision_chain.rs:802` 的真实 `observe_price` caller。公开 fields 意味着条目方法不能声称强制不变量。 | G42 仍开：仅 watchlist prune，price memory prune 缺生产 caller，恢复边界问题（主账 L54）。N35 抽纯淘汰排序不能核销它。 |
| N37 BookState / N38 persistent treap | 来源证据 `orderbook.rs:240-252,410-545,666-698`，撮合价格优先/同价 seq 与身份索引职责分离；`filled_orders.rs:34-117`、`resting_index.rs:42-184` 有持久索引现状。 | 无新增 G；treap priority 绝非委托成交优先级。 |
| N39 PhaseTimingLedger | `verification_evidence/phase_timing.rs:35-99,230-362` 已有 phase/Collector 组织。读取只代表采集能力，不是 OS 实际 runnable worker 或 CPU 利用率。 | 无新 G；保留 registry 配置线程数的真实语义。 |
| N40 EMA recurrence | `indicators.rs:115-154` 的 fast/slow/DEA 独立系数与浮点运算次序为行为边界。 | 无新 G；不能将指标描述为权威成交价。 |

## 候选处置矩阵补记

来源处置表还明确拒绝下列“只搬函数/薄包装”提议：C05-1 地产列报、04-income-column-behavior、08-trade-cycle、08-manufacturing-run、10-operations-hash-projection、11-shock-cohort、12-real-estate-transaction-coordination、12-query-projection、13-rng、14-retail-decision-policy、16-1 OrderCashRules、17-stock-summary、17-ensemble-summary、experience_input_snapshot、19-history、20-market-price-boundary、20-observation-domain-modules。已覆盖：C05-2 → domain-N06；14-position-ledger → engine-foundation-01-A03；19-indicators → domain-N01；19-lifecycle → domain-N07 extension。此处只保留处置关联，不把旧 reader 提案恢复为现行缺口。

## 复核判断与限制

- 这是历史 OOP 候选调查资料，与当前实现基线不是同一时间快照。结构存在的候选需标“对应实现已存在/待逐项承诺核对”，拒绝项保持拒绝。尤其 E03/G08、N35/G42、N01/Q17、N16/Q23、银行恢复/Q19 仍是不同事实，不能相互核销。
- 交易语义判断仅确认这些候选是公司会计、行为决策或诊断内部边界；没有新增或复查交易制度，未重新访问交易所/中国结算官方规则。不能报告 A 股现行规则已独立认证。
- 文件索引正文已连续读至 EOF，SHA/行数/字节数匹配；但索引审读不等于逐文件实现复核。未执行产品行为验证、测试或构建。
