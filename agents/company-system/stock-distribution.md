# 共同送股与转增股分配基础

日期：2026-10-06。

## 范围

新增 `packages/engine/src/company/stock_distribution.rs`，提供纯函数 `allocate_stock_distribution` 和 `packages/engine/src/company/stock_distribution_tests.rs`。输入只读 `RegistrationSnapshot` 及显式 `StockDistributionPlan`；计划分别标识 `BonusShares`（未分配利润转股本）或 `CapitalReserveConversion`（资本公积转股本），并要求 `approval_reference`、比例和获批新增股总数。比例使用百万分之一股为整数单位：`123456` 表示每股 `0.123456` 股，不经浮点换算。股数、ratioMicros 和 tie-break seed 使用规范十进制字符串，支持完整 `u64`，不依赖 JavaScript 安全整数范围。

算法按 `HolderId` 汇总登记日账户股数，排除 `IssuerTreasury` 的权益与碎股计算但在回执保留其被排除数量。其余账户逐户计算 `持股数 × ratioMicros / 1_000_000` 的 floor 和余数；获批总股数同时必须落在逐户 floor 总和至逐户 ceiling 总和，以及 eligible 总持股数乘比例所得 floor 至 ceiling 的交集内。这样既不做每户四舍五入，也不能借碎股补整把总获批股数扩张到比例总量之外。差额按余数降序逐户补 1 股，同余数由显式 `tie_break_seed` 驱动的可复现洗牌排序。结果中保存方案、登记快照身份、每户基数/余数/整股数以及原 lot 完整只读快照。

回执显式标记 `SourceLotAttributionPending`。分配基础不创建、修改或分摊 `ShareLot`，不生成新 lot 可交易状态，也不伪造送转后税务持有期限。之后的结算必须先取得 lot 来源、税期限及限售传承规则证据，再将账户级奖股转换成 registry 变更。深市指南明文确认限售送转股继承原限售性质和截止日；沪市本轮未定位对应条文，不能跨市场推断。

## 官方依据

按研究记录中的原件全文核对，依据《中国结算上海分公司证券发行人业务指南》（中国结算沪业字〔2024〕6号，2024-01，2.5节）、《中国结算深圳分公司证券发行人业务指南》（中国结算深业〔2025〕68号，2025-12，2.5节）及中国证监会公告〔2023〕63号《上市公司股份回购规则》（2023-12-15施行）第13条。指南 PDF、正文摘录和哈希在 `.tmp/company-system/corporate-rules-followup/`；回购规则官方 PDF 及全文在 `.tmp/corporate-actions-research/repurchase.pdf` 和 `.tmp/corporate-actions-research/repurchase-pdf.txt`，官方来源与适用日期记于 `agents/remaining-questions-and-features/corporate-actions-research.md`。

沪市指南要求每股送转比例最多六位小数，碎股以投资者账户碎股数降序、同数由电子结算系统随机排列，然后依序各登记一股直至完成。深市指南的权益分派申请比例最多六位小数，并规定相同的账户碎股排序；另明确限售股份孳生的送转股仍为限售股且截止日与原股份一致。两地回购专户排除具有共同上位依据：证监会公告〔2023〕63号第13条规定，上市公司回购股份过户至回购专户之日起即失去权利，不享有利润分配、公积金转增股本等权利。模块将随机排序 seed 作为显式游戏输入，使用 SplitMix64 与拒绝采样的均匀区间抽取作可重现模拟；官方材料只规定系统随机，不规定离线 PRNG 或种子。

`HolderId::IssuerTreasury` 是 Registry 中本发行人自持股份的权威持有人身份，与 `Account`、`External` 一样是游戏当前登记事实；它不证明持仓的历史取得来源，也不代表真实账户资金流水。`BonusShares` 的自持股排除依据为《公司法》第210条，可由该明确类型身份表达；`CapitalReserveConversion` 的排除依据为证监会公告〔2023〕63号第13条，要求额外证明股份已转入回购专用账户。`ShareRegistry` 现保存必填 nullable 的 `issuer_repurchase_account` 事实 DTO，要求账户引用、来源凭证和建立日期；恢复时拒绝缺键，事实日期不得晚于 registry 已结算日，已有登记快照时不能回溯事实建立日期。登记时仅在事实已于登记日建立的情况下，将其复制到不可变 `RegistrationSnapshot`；恢复时校验顶层与历史快照资格事实按日期一致。分配逻辑对 `CapitalReserveConversion` 只在快照有此事实时排除 `IssuerTreasury`；`BonusShares` 按公司自持股规则排除，不以回购专户事实扩大或缩小该规则。

## 验证状态与限制

模块声明已由 root 加入 `company/mod.rs` 并公开；此模块仍未接入 `Session` 或 `Finance`，不代表真实送转登记结算完成。

测试源码覆盖账户级 floor/碎股补整、`IssuerTreasury` 排除及其不参与比例目标、seed 平局可复现、逐户与汇总审批总量可行区间、超过 `2^53` 的股数、u64 最大 ratio/seed 字符串往返、资格事实恢复缺键拒绝、日期校验和一次性登记，以及审批引用/零比例错误。root 观察 `source-and-announcement-red-0.log` 与 `source-and-announcement-red-1.log` 时，两条缺少 nullable 事实字段的 Registry/Snapshot 恢复负例均按预期失败；`source-and-announcement-red-4.log` 中送转基础 8/8 通过。千户汇总总量负例此前暴露原实现只约束逐户 ceiling 的问题，随后实现同时约束 eligible 总股数比例 floor/ceiling 和逐户 floor/ceiling 区间。当前基础仍未接入 `Session` 或 `Finance`，不代表真实送转登记结算完成。

最新统一编译后的 Registry 21/21、送转分配 9/9 短测通过，证据分别为 `.tmp/company-system/session-actions/final-registry.log` 与 `final-allocation.log`；Web 对应严格存档解析 11/11 通过，证据为 `latest-web-registry-green.log`。各组外部 deadline 均为 10000ms，Rust 组内 8 线程、组间并发。修复非法零股本测试 fixture 后，仍验证普通账户持有全部发行股份、没有 `IssuerTreasury` 持仓时可以登记专户事实；非作者增量复核已确认该修复没有弱化断言。

余项：已补查送转 lot 税务期限继承以及沪市限售继承证据，研究明确尚不能确定混合来源的逐 lot 分配。取得规则并形成独立的明确来源分配契约之前，账户级回执继续保持 `SourceLotAttributionPending`，不把上述算法与解析短测表述为实际送转结算完成。

## 实际登记接通（2026-10-06 B1 批次）

本批把账户级碎股分配算法接通为真实股份登记全链路，代码与语义边界如下：

- `stock_distribution.rs` 新增 `StockDistributionEventPlan`（发行人/市场/法定日期；`ex_rights_on` 同时是交易所除权日与中国结算 R+1 入账日，两者按各自法源校验为同一交易日）与 `StockDistributionBook` 状态机（Approved → Announced → Registered → Credited）。R 日 `register` 按登记快照跑既有分配算法并冻结回执；tie-break seed 由事件身份经 FNV-1a + SplitMix64 派生并记录在回执中（官方只要求"系统随机"，可复现是显式游戏输入）。恢复时用同一 seed 重放分配校验回执未被篡改。
- `holder_credit_lots` 由冻结回执推导 R+1 每户新 lot：原 lots 全部为同一限售类时继承限售属性与 `release_on`（深市指南明文；沪市条文未定位，不跨市场推断）；混合限售来源显式拒绝（`SourceLotAttributionConflict`，整日候选回滚）；原限售截止日早于入账日的按 Unrestricted 登记。送转个税（送股按面值计税）本批明确不支持。
- `share_registry.rs`：`close_day` 支持 `NonTradingTransfer`（CorporateAction 来源、只增不减、合计必须为正）在公开市场日结之后的同一自然日追加落账，`issued_shares` 守恒累加；恢复按回执连续性（公开市场次自然日、非交易过户同日）校验。登记快照新增必填 `settled_receipts`（冻结时已落账回执数量）：同一天"先登记后入账"与"先入账后登记"都会发生，仅凭日期无法回放区分，历史快照发行股数按回执序号回放核对。
- `ex_reference_price.rs` 新增送转除权公式，沪市（4.3.1"流通股份变动比例"）与深市（4.4.1"股份变动比例"）分列参数化，公式变体与交易所不匹配显式拒绝；本批只实现送转分量（配股价格分量为零）。除法取整数分使用银行家舍入，登记为游戏简化（官方分位舍入口径未核验）。非正分子/参考价、零比例与溢出显式拒绝。
- Session 接线：`SessionCorporateActions.stock_distributions` 持久状态 + `process_stock_distributions_on_day_end`（公告→R 日登记并勾稽 Simple 声明事实→R+1 NonTradingTransfer 落账、`Account::credit_position_shares` 真实加股（不动 invested/recovered、不进 T+1）、`IssuerRegistry::record_share_issuance` 更新发行股数、Simple 账面回填 `credited_on`）；事件幂等，失败由日终候选整体回滚（混合限售负例验证整日回滚）。`GameSession::approve_stock_distribution` 以注册资本法定事实整除推导每股面值受理方案（送股受可分配利润上限约束）。
- 行情锚：`applied_ex_dividend_groups` 推广为 `applied_ex_reference_groups`（`AppliedExReferenceGroup`，现金计划与送转事件同组合计、一证券一除权日一参考价）；同日多起送转事件的合并口径未核实，显式拒绝。恢复链路（`persistence.rs` 与 session 重建）允许发行股数等于 setup 初始股数加名册非交易过户增发回放，其余发行人身份字段仍须与 setup 完全一致。
- Simple 账面：`SimpleFinanceState.stock_distributions` 只冻结面值口径展示事实（`StockDistributionFinanceFact`），不做借贷过账、不变更注册资本法定事实；转增的资本公积余额/来源类别/法定公积金 25% 留存校验未建模并显式登记。
- Web：严格 parser 同步（NonTradingTransfer 回执、`settled_receipts` 回放、送转账簿与入账勾稽、锚组新形状），三个存档 fixture 由当前 release Engine 正规重生成，ts-rs 绑定更新（`AppliedExReferenceGroup`、`StockDistributionExRightsFormula`）。

验证证据（均为 10000ms 外部 deadline、组内多线程）：Registry 24/24、送转 18/18（含 Session 全链路与混合限售回滚）、除权公式 15/15、Session Simple 19/19、corporate_actions 7/7、simple finance 65/65；Web corporate-actions schema 16/16、save 消费组合与公司 schema 组全绿；日志在 `.tmp/company-system/stock-distribution/`。基线核对：`cash_dividend_tax` 两项、`retail_analysis` 四项、`persistence` 十三项与 `failure_tests` 三项失败在未含本批改动的 f1fc21f6 基线同样失败（属主工作区另一批股息税修复在飞），本批未触碰对应文件。

## 修复轮（2026-10-06，非作者门禁 findings）

对 fba94e8b 的非作者复核发现六条问题，本批以 TDD 修复（红→绿证据均在 `.tmp/company-system/stock-distribution/fix-round-*.log`，全部 10000ms 外部 deadline）：

- **major-1 面值推导缺陷（路线 a）**：现实语义为面值恒定、送转后注册资本按面值增加。`approve_stock_distribution` 的面值改为「首次送转声明时由法定事实 ÷ 当时发行股数整除推导并固定；已有送转声明后沿用同一面值」，不再用增大后的股数反推；`SimpleFinanceState::record_stock_distribution_credit` 入账时把注册资本法定事实演进为 当前注册资本 + 声明的 `capital_increase`（`source_evidence` 不变、bind-once 入口照旧拒绝改写），历史由各事实 `registered_capital_at_approval` 冻结。配套修正两处随之必须按「批准时点口径」重构的校验：`finance_validation.rs` 的分红声明注册资本等值检查、送转事实批准时点注册资本演进链核对（新增，含篡改负例），以及 Web 严格 parser（`apps/web/src/save/schema/company/simple-finance.ts`）的同构检查。连续两次 10送3 的会话级测试断言面值恒定、注册资本按 T→T+2→T+5 演进、超上限送股按演进后事实被拒；单元测试覆盖法定公积金 50% 免计提门槛按演进后注册资本重新核定（送股上限从 27_000 收紧到 24_300 的场景）。
- **minor-3 同日第二起拒绝时点**：`approve_stock_distribution` 受理时直接拒绝同发行人同证券同除权日的第二起事件（错误信息指明合并除权口径未核实），不再推迟到 R+1 首 tick `prepare_ex_references` 才 StepFatal。
- **minor-4 恢复反向勾稽**：`SessionCorporateActions::validate` 补「名册每条 NonTradingTransfer 回执 → Credited 状态送转账簿（事件前缀、入账日、数量合计）」的反向断言（ghost 回执负例）；forward 方向的 `u64::try_from(change.change).unwrap_or(0)` 改为显式校验错误（`sum_nontrading_changes`）。
- **minor-2 R+1 当日不可用**：不改日终入账管线，在 `docs/trading-rules.md` 送转章节显式登记「公司行为资金/股份均在相关日期日终入账，当日盘中不可用」的游戏简化（覆盖送转与现金分红），并注明与 R+1 上市当日即可流通官方语义的差异留待后续批次。
- **note-5 吞错误**：`validate_stock_distribution_books` 末段 `.filter_map(...ok())` 改为显式传播 `?`，仅对「该公司确无 finance 状态」的合法缺省走过滤。该路径当前不可由公共行为触发（`stock_distribution_facts` 实际不可失败、发行人必有 finance），属防御式编程修复，无可行红测试，如实登记。
- **note-6 错过入账日防护对称**：`process_stock_distributions_on_day_end` 与恢复 `validate` 各补「已过 ex_rights_on 仍 Registered 显式失败」检查（两条直接负例：日结处理与恢复校验）。
- **附带发现（本批修复）**：Web 严格 parser 的 `capital_increase = 面值 × 新增股数` 校验原先多乘 100（从未被真实存档覆盖的潜伏缺陷，引擎 wire 以分为单位）；按引擎权威口径改为两侧均以分核对。

验证（各组均 10000ms 外部 deadline、组内多线程）：finance 送转 4/4、`company::simple` 69/69、Session Simple 24/24、送转基础 13/13、corporate_actions 7/7、Registry 24/24、除权公式 15/15、Web simple-finance 7/7、corporate-actions+system schema 30/30。基线对照：`session::failure` 3 项、`session::persistence` 13 项失败在 stash 本批改动后的 fba94e8b 上同样失败（属主工作区在飞的股息税批次，本批未触碰）。存档 fixture 均不含送转事实或法定事实，无需重生成。

## B1+B2 集成合并（2026-10-06，集成 subagent）

本节登记 B1 送转分支（ceaeda2b）并入已含 B2 新局默认税务的 main（0b96981a）时的合并解法与集成验证。冲突逐文件按两侧门禁复核过的语义合并，未用 ours/theirs 整体覆盖（四份 fixture JSON 除外：真值统一由合并后引擎重生成，占位侧选择不影响最终内容）：

- `session/corporate_actions.rs`：B2 的 `TaxpayerIdentity` 等税类型块与 B1 的 `holder_key`/`sum_nontrading_changes` 辅助函数同位置插入，两块全保留；`SessionCorporateActions` 采用 B1 的超集形状（`stock_distributions` + `applied_ex_reference_groups`，base 的 `AppliedCashExDividendGroup` 被 B1 重命名推广，B2 未触碰这些行）。
- `docs/trading-rules.md`：个人股息税（含开局税务模式）与送转两章节都保留。
- Web `schema/corporate-actions.ts`：类型块双保留，`SessionCorporateActions` 取 B1 超集；B2 新增测试的对象字面量按合并后必填字段集适配（`applied_ex_reference_groups` + `stock_distributions`），断言未弱化。
- `closed-day-fixture-generator.rs` 校验字段列表为两侧字段并集。
- 四份存档 fixtures 由合并后 release Engine（rustc 直连 rlib，三 producer 并行编译）正规重生成：均携带 `dividend_tax_mode: IndividualPublicMarket` 与合并后 `corporate_actions` 完整七字段（无送转事件时 `stock_distributions`/`applied_ex_reference_groups` 为空数组，如实不伪造）；company slice 为重生成主档 `company_system` 精确投影（每公司 finance 新增空 `stock_distributions` 对象）。producer 内置 restore+resave 深等与场景守卫全过；三档经 Web `parseSaveSlot` 严格解析深度相等。注意：同引擎两次生成的市场轨迹可因并发受理调度不同而合法不同（ADR-0017 修订语义），fixture 以 generator 内置守卫与解析深等为验收，不跨次比较字节。
- ts-rs：`export_bindings` 147 项通过，生成物含 B1 `AppliedExReferenceGroup`/`StockDistributionExRightsFormula` 与 B2 六个新税类型。

集成验证（各组 10000ms 外部 deadline；编译/typegen 用 300000ms 长验收）：finance 送转 4/4、`company::simple` 69/69、Session Simple 30/30（两批用例合集）、送转基础 13/13、Registry 24/24、除权公式 17/17、`session::corporate_actions` 12/12（B1 时 7 项 + B2 新增 5 个税类型导出）、`dividend_tax_mode_tests` 8/8、`cash_dividend_tax` 16/16、Web corporate-actions schema 19/19、system schema 11/11、simple-finance 7/7、save-contract/save-commands/fixture 消费者 28/28、`cargo check --workspace --all-targets` 通过。基线对照：合并树与合并前 main 在独立 worktree 各跑全量 `cargo test -p engine --lib -- --test-threads=32`，失败集合逐项一致（280 条全同、各自 257 failed；合并树多 23 个通过即 B1 新增用例）；`session::persistence` 13、`session::failure` 3 与文档基线一致；Web 全批在两树均有的失败集一致，`public-financials-render` 的利润表用例经单树隔离复跑确认为 main 既有失败（非合并引入）。日志在 `.tmp/company-system/integration/`。B1 遗留的 15 个 tsc 类型错误（本测试文件 exchange 联合收窄等）由后续独立修复提交处理。

## 集成修复轮：送转×税账交互（2026-10-06，集成修复 subagent）

### 缺陷（集成复核 major，静态确认 + 红测试复现）

`session/corporate_actions.rs` 的 `sync_dividend_tax_days` 对所有名册回执不分 scope 一律派生税账事件 id `session-market:{stock}:{account}:{day}`。除权日 R+1 当日：`close_registries_through` 先落 PublicMarket 回执并记入税账 → `process_stock_distributions_on_day_end` 追加同日 NonTradingTransfer 回执 → 下次 sync 派生 id 与已记录公开市场日撞车，被 `existing_day` 幂等跳过——**送转新股永远不进入税账 FIFO lots**。三个后果（均有红测试或红测试中的失败形态对应）：(a) 默认 `IndividualPublicMarket` 自动开账 + 送转入账后，现金分红 per_share 按名册快照（含送转股）计算，税额只对不含送转股的 lots 求值，且税账 `register_dividend` 的「received ≤ per_share×lots」校验在付款日直接失败（红测试中表现为 `received cash exceeds registered gross entitlement` 卡日终）；(b) 含送转股全额净卖出时 `apply_net_change` 报 insufficient shares，日终永久卡死；(c) 税账 lots 清零而名册仍持送转股时 `register_dividend` 报 needs held shares。

### 官方口径研究结论（结论 A：法规明文组合）

核心问题：个人差别化股息税下送股/转增股份的持股期限起算日。结论：**自送转股份到账日（R+1）起算，不与原股份视为同一批次；FIFO 按 R+1 当日净增排序在所有更早取得日之后**。依据链（全部为法规/结算指南明文，原文已在本仓 `.tmp/` 取证）：

1. 财税〔2012〕85号**第六条第（八）项**：本通知所称「个人从公开发行和转让市场取得的上市公司股票」包括「取得发行的股票、配股、**股份股利及公积金转增股本**」——送股与转增并列列为「取得的股票」，两口径一致无区分（两税种差异只在送转本身是否计税：送股按面值计税、溢价转增不征税，该差异本项目已显式登记未实现）。
2. 85号**第一条第二款**：持股期限指个人取得上市公司股票之日至转让交割该股票之日前一日的持有时间——锚定「取得之日」。
3. 85号**第三条**：股份按每日日终净增（减）数、取得日先后先进先出——送转到账是 R+1 当日净增。
4. 沪业字〔2024〕6号 / 深业〔2025〕68号 2.5节：送转新增股份上市日为 R+1——「取得之日」即到账日。
5. 旁证：沪指南 2.5.3 注意事项 7/8/9 对「连续持股」作列举式例外规定（约定购回不连续、资管/两融划转连续、确权登记重新起算），送转不在连续持股例外清单内，反证默认按新取得日起算。

诚实例外说明：未检索到单句直接表述（如中国结算《上市公司股息红利差别化个人所得税政策常见问题解答》原文），但结论由上述条文组合直接推出、无解释空隙，且未发现任何相反权威表述；已按 A 级登记并在 `docs/trading-rules.md` 写明推理链与「FAQ 单句原文未取得」的事实。

### 修复设计（TDD 红→绿）

- `company/cash_dividend_tax.rs`：`record_net_day` 允许**同日正向续记**（`day == settled_on && net_change > 0`，仅限已有日结记录之后；首条日结仍须紧邻开账日下一自然日），`validate` 同构放宽。处置仍每日只按公开市场净额一次（同日负/零续记显式拒绝）。`TaxDayReceipt`/`TaxDisposition` 字段提升 `pub(crate)` 并新增 `tax_day_receipts()` 只读视图。
- `session/corporate_actions.rs`：`sync_dividend_tax_days` 以穷尽 `match MovementScope` 派生税账事件 id（PublicMarket 沿用 `session-market:{stock}:{account}:{day}`；NonTradingTransfer 用回执自身事件身份 `{receipt.event_id}:{account}`）——未来新增 scope 变体编译期强制显式决策，铁律 2 的最强形态。`process_stock_distributions_on_day_end` 在非交易过户落账与投资者加股之后**当日立即**为每个持税账的 Account 持有人落同日续记（取得日 = R+1、来源 `CorporateAction`、限售继承经 `convert_tax_class` 映射 `StatutoryRestricted`），保证日终 validate 时覆盖完整、不依赖次日 sync 补账。`SessionCorporateActions::validate` 新增**税账↔名册回执覆盖勾稽**：每个已配置税账的名册回执（含非交易过户）都必须有对应事件 id 的日结事实，缺失显式报「送转×税账交互未入账」。
- Web `schema/corporate-actions.ts`：`validateTaxBookReplay` 同步允许同日正向续记；**附带修复**（本批暴露的潜伏缺陷）：原实现对正向净增日恒要求 `disposed === -net_change`（负数），任何含买入日结或送转续记的真实存档都会被误拒——改为与引擎 `expected = net < 0 ? -net : 0` 同构。存档 fixtures 的税账均为空数组、不含送转事件，无需重生成。
- 顺序保证：同日「先公开市场后送转」两条回执按名册回执插入顺序分列两条税账日结（市场在前）；同日两条的取得日相同，FIFO 之间顺序不影响档期，仅保证确定性。

### 已登记的实现口径偏差

85号第三条为逐日净额口径；本引擎对同日公开市场与送转分列两条事实、不做事前净额合并。当同日既有卖出又有送转到账时（例如持 100 股旧股、R+1 卖 100 股、当日到账 30 股）：净额口径为 −70（FIFO 处置 70、留存 30 股旧取得日），本实现处置 100、留存 30 股 R+1 新取得日。差异方向偏向多计处置、新股按新取得日记档；已在 `docs/trading-rules.md` 登记为待后续批次校正的偏差。事前净额合并需要重排日终管线（送转先于分红登记）或改单日多批次事实模型，超出本修复轮最小范围。

### 验证证据（红→绿，各组 10000ms 外部 deadline、组内多线程；日志 `.tmp/company-system/integration/interaction-fix-*.log`）

- 红：Session Simple 新增 6 用例全红（缺税批次/缺回执/同日单条/恢复缺事实/validate 静默通过/付款日 received-exceeds 卡死）；`cash_dividend_tax` 同日续记单测红；Web 同日续记解析红（原报「税账日结必须紧邻」）。
- 绿：Session Simple 36/36（30 存量 + 6 新增）、`company::cash_dividend_tax` 17/17、`session::corporate_actions` 12/12、`company::simple` 69/69、`dividend_tax_mode_tests` 8/8、送转基础 13/13、Registry 24/24、Web corporate-actions schema 20/20、Web simple-finance 7/7、`cargo check --workspace --all-targets` 通过。基线对照：`session::persistence` 13 失败、`session::failure` 3 失败与文档化 main 基线逐项一致（均为 runtime envelope/费用/NPC 域，与税账无关）。
- 新增组合用例覆盖：默认 `IndividualPublicMarket` 自动开账 + 送转入账（取得日 = R+1、月末钳制边界 01-31→02-28、限售继承 10% 立即计税、同日先市场后送转顺序）+ 其后现金分红税额手算精确断言（16 分 / 13 分）+ 含送转股全额卖出不卡日终 + 严格恢复深等与继续日终不重复入账 + 税账↔名册覆盖勾稽负例。
