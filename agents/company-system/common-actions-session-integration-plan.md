# 共同股本投资者结算接线计划

研究基线：2026-10-06。只定义从新局事实到 `GameSession` 成交、自然日日结、公司行为、恢复和宿主契约的下一段实现顺序；不是实现/验收结论。目标按 checklist 推进到可由实际账户执行的公司行为，而非再增加孤立纯模块。正式实现仍须 TDD、每批完整 diff 独立复核；本计划不要求先完成 `CompanySimulation` 经营 dispatch。

## 当前连接断点

- `share_registry` 能登记完整已发行股本、逐 holder lots、日净变更及当前日登记快照，但 Session 没有持有它；开局 `seed_float` 只将 `StockSpec.float_shares` 分给 NPC，玩家不分配，`total_shares - float_shares` 没有受益人事实。不能把差额暗中填作公司自持、外部匿名股东或玩家股份。当前 `validate_request` 还拒绝所有 `NonTradingTransfer`，所以认购、送转、发行人库存股变化不能误走现有 public-fill 接口。
- `cash_dividend_tax` 可记单账户/单证券的税务 lot、证券账户日净变化、派息与税款事实；`CashDividendTaxBook::validate` 当前只支持 `IndividualPublicMarket`，尽管 profile enum 还列出居民企业、基金、非居民。其金额为精确有理分，但 `collect_due` 遇到非整分税额返回 `NeedRoundingEvidence`。
- `cash_dividend` 可冻结登记 snapshot、计算整数分/股 gross entitlement 并记录逐 holder 成功/失败；它自身不转移现金。独立复核早期指出不同付款批次重复失败的恢复缺陷；提交 `582720b1` 已将失败投影改为按 holder 更新，14 项定向测试通过并经独立复核，因此不是当前 blocker。
- 撮合账户已由 `ReceiptSettlementPlan` 原子结算，`EnvelopeReceipt` 仅在候选事务内，提交后 `PersonalTradeConfirmation` 按账户持久保留成交日、证券、方向、价量和全局 receipt id。自然日日结已有 `clone_for_tick_shadow` 全局回滚，但没有登记簿、税账或公司行为候选。
- `SaveSlot` / `SavedRuntimeState`、Rust restore、Web 严格 parser、业务 hash、clone 和 TS typegen 均是权威状态契约；新增 Session 字段不能只落 Rust struct。

## 必须先确定的权威输入

在新局 setup 中加入与 seed 结果一致的股东分配事实；行为偏好提供配置入口但字段/数值另待产品契约确定。税务身份需明确纳税人，不能从策略类型猜出；字段名由实现者据现有 public DTO 风格定：

1. **完整股东初始分配**：每只证券真实 `ShareRegistry` 必须有与 `issued_shares` 守恒的完整持有人事实，才可声称股东名册/全额 entitlement 完整。当前 `total_shares` 高于 NPC 开局分配时缺少剩余股份持有人事实；剩余由具名外部股东、发行人自持或其他产品定义的 owner 承接，以及该事实来自逐户 setup 还是可编辑 seeded allocation，是需产品决定的输入来源，不能在本计划中规定用户必须逐人手录，也不能默设为匿名外部/公司自持/玩家。该事实未就绪时可继续做真实市场成交和已知账户净持仓适配，但不得伪称全体股本/全体派息闭环。外部受益人的现金结果必须进入其具名外部结算事实/端点，不能漏掉、倒给玩家或伪造成证券账户。
2. **初始取得批次**：税务 FIFO 需要可追溯 lot id、数量、取得日、来源及限制/解禁属性。开局税务取得日期与来源如何由可编辑 seeded preset 或其它 owner 提供仍是待确认的游戏事实；不能偷设为已持有一年，也不能因这项待确认事实阻断无税计算依赖的 share registry、gross entitlement 和账户现金接线。`StockSpec.float_shares` 不是全部股东名册。
3. **税务身份事实及契约候选**：要对个人执行适用税务规则，必须有明确个人身份事实。`AccountKind::{Retail,Inst,Hot,Player}` 只代表策略/玩家类型，绝不推导纳税人身份；缺少 profile 时不得假称按个人税率结清。开局逐 holder profile 配置、NPC/新玩家默认身份和开户时是否必填尚非已批准产品要求，属于需确认的契约候选；profile 对应 holder/lot 的关联与税务适用范围仍须可验证。`CashDividendTaxBook` 目前以 `AccountId` 为主体且仅支持 `IndividualPublicMarket`，外部主体及其他 profile 需后续税务主体/规则接线。适用身份未确认时，gross entitlement 和账户 gross 到账仍可独立完成，不能声称对应税务已结算。
4. **公司行为偏好/计划来源**：Q14 已确定每家公司具有自己的行为偏好，但具体配置字段与数值未获批准。允许提供按公司可见、可编辑、seed 可复现的虚拟 preset；preset 的值及来源需显式显示，不得伪称已批准的经济事实。偏好只用于生成方案，不能替代股东会批准、可分配利润、发行/回购条件、市场披露或账户现金检查。可以复用同一个明确标为待确认的 seeded preset 值初始化多家公司，但须保留每家公司可配置性，不能把预设伪称为统一批准参数，也不因行为授权自动批准方案。玩家/脚本明确提交的决议事实须保留来源/批准主体、公司、证券、计划 id、金额/股数和日期。

开户（`memberships` 及 `shared_ingress.register_player`）新增账户时不会自动持股。开户 command 是否必须携带 profile 是待确认的契约候选，不作为当前已批准的开户失败条件；首次处理适用税务事实时仍不得按 Player 猜个人身份。

## 接线次序与事务边界

1. **共同状态归属与配置红测**：先给真实 `GameSession::new` setup 加股东名册/lot 配置 owner；装配发行人后，对已声明 holder 校验公司 identity、证券、account 对应关系、唯一 external endpoint 和 lot 结构。只有选择了完整登记方案时，才要求 holder 股数总和 `== issued_shares` 并验完整覆盖；选择/生成完整 ownership facts 的产品入口仍待确认。NPC random split 继续由现有 seed 复现。并行确定税务 profile owner/type 和绑定方式，但不能把尚未确认的 profile 默认/开户必填写入失败门禁。
2. **权威 equity aggregate**：在 Session committable state（并进入 SaveSlot/SavedRuntimeState、Web root parser、business hash、clone 与 restore）持有每证券 `ShareRegistry`、每 account-security 已知 `CashDividendTaxBook`/profile、公司行为偏好/RNG、计划/决议/付款状态和本轮消费游标。初始化时用 seed_float 的实际分配建立 Registry lots，外部与 treasury 使用显式 allocation；验证股份总数只计一次，`IssuerTreasury` 不得产生分红应收。
3. **成交事实适配**：只采纳已由 `ReceiptAggregation` 校验且 `ReceiptSettlementPlan` 成功安装的正数量 Fill；使用实际 qty/gross/charged 和 `receipt.index` 建可重放的已提交成交证据。成交不能直接改 Registry 一次一 fill，否则 A 股税务按证券账户日终净增减，不按日内成交 FIFO。日终账户证券净变化以候选中已提交 `Account.position` 前后差为权威，再与所有账户当日已提交 Fill 的买卖方向/数量合计交叉校验；`PersonalTradeConfirmation` 可作为已提交历史证据来源，但本身不是现成的自然日净变化接口。任何消费游标只随 DayEnd 最终候选整体提交，不能随中间 candidate/UI confirmation 推进。
4. **日终股东登记 + 税账同日净变化**：在自然日 `CivilClock` 候选内，确定全部市场已完成当日结算后，按证券/holder 对账户真实日终净变化聚合一行：sum(change)=0，正数恰有一个 acquisition lot，负数无 acquisition、零变更不造 lot。为每只证券连续推进 Registry（包括交易所休市及周末逐自然日推进；只为已有 Registry 的 holder 生成零变更，不将无股账户放入 `changes`），并为已有适用税账的持有人以相同事件 id/日期/net change 推进 FIFO tax book。新开户无股份不强行注册成 holder；tax book profile/开账策略待契约确认，不能臆造一个默认身份。消费游标只随 DayEnd 最终候选提交；逐项校验真实 account shares 与已登记账户持仓一致；异常使整份 DayEnd candidate 回滚。
5. **登记日与股利税务冻结**：方案 `registered_on` 当天 DayEnd 先完成该日已提交净变化和已登记 holder 对账，再 `ShareRegistry::register(plan_id, date)` 取当前不可变快照，并把同一 snapshot 交给 `CashDividendBook::register(snapshot, calendar)`。只对快照中有正 entitlement 且具有受支持、明确税务主体的 holder 调 `CashDividendTaxBook::register_dividend`；零持仓 holder 不调用（API 要求 lots 非空）。登记日买入（T+1 锁定与权益资格分离）计入；登记后卖出不改变已冻结 entitlement。税务 profile 未决/不支持不应阻止可独立执行的 gross 权利登记，但必须阻止声称该 holder 的税务资格/税款已结清。
6. **到期支付原子候选**：每个付款批次为所有当前未付 holders 构造完整 `HolderPaymentOutcome`，允许不同 holder 一部分 `Paid`、一部分带原因 `Failed`；不能要求跨 holder 全部成功才提交，也不能只安装成功者而漏记失败。以整个批次候选原子准备真实 Account gross 入账/具名 External gross 到账 receipt、`CashDividendBook::settle(payment_id, paid_on, outcomes)` 和关联 tax facts；候选技术/不变量错误整体丢弃。业务上单一 holder 收款失败是 `Failed` outcome，不使同批其他真实成功者回滚；它与成功 outcomes 一起原子进入 book，失败 holder 不收款并保留未付权利。每个 `Paid` 金额须精确等于该 holder 全额 gross。仅对成功收到 gross、已支持其 profile 且此前已在该 holder tax book 注册同一 `plan_id` 权利的账户，才调用 `CashDividendTaxBook::record_payment(plan_id, payment_id, paid_on, received_gross, evidence)`；此 API 记录股东收到的税前红利，不是收取税款，且要求 `paid_on == tax_book.settled_on`。失败 holder 不得写 tax dividend-receipt。外部主体须有单独明确税务/收款契约，不能将 AccountId tax book 强套其身。稳定 id 从 plan id + holder/account + installment 构造；同命令幂等、异载荷冲突。
7. **卖出触发税务收缴及余额不足**：正向日净卖出由 tax book FIFO 生成 disposal/deferred liability；按101号已确认的个人上市股票口径，派息时不预扣，后续转让时核算最终税。`record_payment` 只记录税前股息实际到账；收税必须另调 `CashDividendTaxBook::collect_due(event_id, day, available_cash)`，不能混为同一事件。当前 `collect_due` 仅当 outstanding 精确分母为 1 时才写收税 receipt，并最多收可用整数分；若欠税本身小于一分，返回 `NeedRoundingEvidence`，不能部分收税、不能写 tax collection receipt，应保留精确债务并显式拒绝该次收缴。整分税款而可用现金不足时，API 可按可用现金部分收取并保存余欠；具体未来追缴行为仍需明示。真实 Account 扣款、tax book collection receipt 和通知状态同一候选原子提交；不能回滚已合法完成的卖出、生成现金或允许账户负现金。中登指南待获取不阻断 gross entitlement、Account gross 到账及整数分纯现金除息基础。
8. **行为计划与其它真实结算**：把偏好生成的计划接入自然日生命周期和可见披露；需批准的事项没有 approval fact 就不执行。分红使用上述端到端闭环。送股/转增、配股/增发认购、回购需分别接共同股份 lot/限制、本人真实资金/真实成交与交易确认；计划不能当完成，认购不能补投资者现金，回购不能伪造卖方/成交。证券持有人证券、账户现金和公司行为 book 的更新同一个不可重复候选，来源 event id 固定；需拆阶段时保存已完成阶段 receipt、未完成债权/义务，失败不重新执行成功部分。
9. **除息接市场锚**：仅纯现金整数分/股、标准公式事件，将 `ex_reference_price` 经沪深各自 `TradingCalendar` 计算出的值接入当前尚未接线的实际 Market `last_close`、reference-price/涨跌幅基准字段，在登记日次一交易日开市/首个适用行情前生效；先盘清当前 Market 中 reference price 与 last-close 字段的职责，证明涨跌幅校验实际消费该值。不得停在计算结果 DTO/“投影”；不改历史成交价、不走模拟成交。显式携带 `CashDividendFormula`，获批特殊调整当前明确 unsupported。混合送转/配股行为待各自市场公式官方依据，不混用纯现金公式。
10. **宿主完整闭环**：对 Rust/wire 定义严格 required fields 与 `deny_unknown_fields`，同步 Web parser/typegen、Native/Server host（若支持该存档契约）和新局表单/setup 的账户税务/完整持有人/偏好编辑入口。`SaveSlot` 日终必须保存 Registry receipts、tax operation seq/FIFO lots/deferred liabilities/dividend payment receipts、行为 phase/cursor/RNG/来源；restore 校验重放一致、issued-share conservation、account quantities、现金凭证与 tax profile，不新开兼容默认、不由 restore 重派息。失败恢复后同事件重试恰好一次。

## 实施批次与完成门

- **批次 A：事实 owner + Session 存档**：显式 setup/完整开局 holders，初始化 Registry 和 strict serde/parser/hash/restore；红测涵盖缺外部 holder、总股本差额、初始取得日期非法及恢复错 holder。并行确定 tax profile owner/type 和绑定方式；产品确认前不把“所有 NPC 配 profile”或“开户必填 profile”写为失败测试。
- **批次 B：成交与日终 owner**：以真实买卖 fills 接入日净变更/连续自然日与 registry-tax 双账，覆盖当天净零（买卖相抵）、净增批次、跨日 FIFO 部分卖出、休市/非交易所差异日、新玩家加入但无 shares、恢复前后 source cursor、账户与名册不平；后置故障证明全 Session 不变并可同日重试。
- **批次 C：Cash Dividend from plan to account**：使用已修复的付款失败投影；将偏好提案、显式批准/可分配利润、公告/登记快照、除息 anchor、到期付款和适用 tax deferred right 接入一条真实 `GameSession` fixture。最小验收包含登记日买入、登记日卖出、登记后卖出、散户+外部+发行人自持、税务身份适用/不支持分流、整数分到账、重复日期/重试、缺适用税务事实时不伪称税已结清、Simple 不受展示现金阻塞及任一步失败原子回滚；保存恢复续跑相同结果。
- **批次 D：共同行为余项和宿主 UI**：配股/增发须做真实认购订单/资金和股份登记；回购须真实市场订单、成交并且仅按 fills 改真实持有人/库存股；送转等要按各自适用类别建立限制 lot。给 Web/Native 的能力、偏好/计划来源、未完成状态与显式错误可见入口；两模式相同调用契约。Simulation 现阶段明确无法创建，不能报告两模式共同验收，待其后续分支实现共用能力。

任何批次必须短 TDD、原子失败/重复恢复负例与独立非作者完整 diff 复核。`cash_settlement` 只有在实际 Account/Registry/Plan 状态闭环及恢复证据齐全后才可变为 true；纯模块绿灯、编译和 DTO 不够。

## 依据可实现范围与未核实项

可直接依现已读取的正式原文实现的是：公司法（2023修订，2024-07-01施行）第210、212条已明确的亏损/法定公积金/股份分配比例/公司自持股不分红及决议后六个月期限；上交所 2026 修订规则第4.3.1—4.3.3（2026-07-06施行）和深交所对应第4.4.1—4.4.3、5.2.3（同日施行）的登记日次一交易日除息及纯现金前收减税前每股红利公式；财税〔2015〕101号（2015-09-08施行）适用公开市场个人的持股期限分档、20%税率及持有不超过一年的派息暂不扣缴；85号仅在101号不冲突的日终净变化/FIFO/自然月年等操作规则范围；现金额为整数分/股且分配总额仍在 `Money` 范围内时，每户 gross=`Money cents/share × integer shares` 无尾分，可完整做实际账户资金入账及纯现金除息，不需要等待中登指南。

现金整数分并不等于完整税收尾数规则：税额×适用计入比例可能形成小于一分的有理欠税，税款最终取整/收缴与个人税收规则的适用版本需有依据。保留精确 numerator/denominator，不暗取整；只在具体收缴需要的精度确实不足时显式拒绝该收缴，不阻断 net share facts 或 gross dividend entitlement。

中国结算沪深发行人业务指南正文在当前研究中均被维护页替代而未取得（见 `corporate-rules-refresh.md` 与 `dividend-research.md`）。故现阶段不能将实现说成真实中登操作已完全复现，不能擅自定逐户不足分尾差/碎股排序、红利到账/发行人预划、配股认购资金冻结及发行失败退款路径/期限/利息、送转股份限售继承、回购专户或注销登记程序。对应动作必须按类别清楚标为简化/不支持/待结算，取得沪深各自正文后再细化。该指南缺口不推翻上段已有法律/交易所/税法所能确定的整数分纯现金基础。

资本公积来源个人税判定、居民企业/证券投资基金/非居民的全套税率及扣缴情形也不能拿个人上市股票规则推断；`DividendTaxProfile` 枚举存在不代表已有税务算法。对这些身份不得按 `AccountKind` 代填税务待遇。

## 本轮刻意保留的当前状态

本文件为接线计划，没有修改产品源码或测试，也没有编译/运行回归。实现仍以 `implementation-checklist.md`、`current-handoff.md` 和 `q14-financial-model-design.md` 为状态真源；当前 `Simulation` 仍显式拒绝创建，当前共同投资者现金结算仍未从 Session 端到端完成。
