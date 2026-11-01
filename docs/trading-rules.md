# A 股交易规则基线

游戏的证券交易规则以中国大陆沪深 A 股竞价交易为基线。参数属于权威会话配置，
前端只能预检查，最终由 Rust engine 拒单或结算。

金额与每股价格的 `Money` 内部单位为分，跨端统一使用规范有符号 `i64` 十进制分
字符串；例如 `"1"` 为0.01元，`"1000000000000"` 为100亿元。Web 金融计算保持整数
精度，不接受旧数字金额；`AccountingAmount` 仍为独立元字符串。编码不改变下列交易
规则、费用或舍入，图表近似不能用于反算委托。见 [ADR-0031](decisions/0031-money-decimal-cents-wire.md)。

Escrow 并行候选的资源边界见 [ADR-0017](decisions/0017-escrow-parallel-tick.md)：
SealAllocationSnapshot 的 `DecisionResourceSnapshot::seal` 读取报价过期处理后的资源；其他密封批释放不回补本批预算。
买单使用现金 envelope，卖单现金预留恒为零。分歧 #9 的卖费实收封顶/后腿追收是明确批准的
**游戏简化**，不是交易所或券商真实清算承诺；nominal 费率与累计最低佣金不因并行化改变。
`engine::TickCommitEvidence` 与 `engine::UpdateStreamProjector` 是验证投影入口，不是另一套交易规则。
本批工程接线复核日期：2026-09-23；下列交易所条款最近一次内容核对日期：2026-09-22；费用表本次访问失败的边界另列，
不将部分来源复核冒充全部规则验收。
2026-09-25 又核对了沪市现行规则 3.5.1 与深市现行规则 3.4.2：同价同向申报的先后
依据交易主机接受申报的时间。此次只复核竞价优先原则，不代表重查了下列其他条款。

## 混合交易所日历的分批实施边界

本项目已批准共享自然日与逐交易所开闭市；休市证券不接受交易、不生成行情，经营和公告
按自然日继续。当前此批仅恢复共享 `CivilClock` 的实际交易所集合及冻结日历聚合；逐股
市场执行、本股交易时钟、NPC/计划、严格存档校验与并发收件仍由后续片闭合，混合开闭市
不能据本片宣称完整可玩。当前默认政策沪深节奏相同不能替代该能力，也不构成将混合情况
降为不支持的产品决定。各所按本所公告休市的现行依据为上交所《交易规则（2026年修订）》
2.4.1 与深交所同规则 2.3.1（2026-07-06 起施行，2026-10-25 核验）；来源见
[模拟日历政策](simulation-calendar.md) §1 与 [公司会计依据](company-accounting.md) §5。

## RuntimeSyncPolicy 的 WASM 支持边界（2026-10-29）

**不支持：** WASM 新局和读档当前仅接受 `runtime_sync_policy: "every_tick"`；显式
`day_end` 或 `on_demand` 在构造 Session、登记句柄或初始化 facade arena 前报告
`WASM_RUNTIME_SYNC_POLICY_UNSUPPORTED`，不能静默改成 EveryTick。新局省略字段时
沿用 EveryTick；当前日终存档必须显式包含该字段。该限制只描述同步方式的支持范围，
不改变撮合、现金分、股数、T+1 或交易日定义。

## 已实现

- **全场休市市场门禁（2026-10-25 登记）**：`step`、runtime 与 frame 家族在
  市场事务开始前，按本局冻结 `CalendarPolicy` 逐股查询其所属交易所；当全部证券
  休市时显式拒绝，不消费玩家意图、RNG 或市场 tick；误调市场入口沿用 `StepFatal`
  的 poison 停止会话契约。正确驱动休市日时，自然日日结继续处理经营及披露，
  不把休市起点静默挪到开市日。依据为上交所《交易规则（2026年修订）》
  [2.4.1](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/c_20260424_10816482.shtml)
  与深交所同规则
  [2.3.1](https://docs.static.szse.cn/www/lawrules/rule/trade/current/W020260424690713155663.pdf)：
  每周一至周五为交易日，法定假日及本所公告休市日休市；规则自 2026-07-06 施行，
  本次原文重新取证日期为 2026-10-25。2030 年休市仍按
  [模拟日历政策](simulation-calendar.md) 明确标注为游戏假设。
  **剩余缺口**：沪深同日一开一闭时的逐股交易隔离、会话时钟及恢复一致性尚待完成；
  当前全场休市门禁不代表已实现该混合情形，不能据此宣称休市证券已受完整保护。
- **自成交限制（2026-10-15 登记；Sol 终审修订）**：同一投资者账户
  （本项目内=同一 NPC/玩家）的买卖委托不互相成交。**依据状态**：真实
  沪深市场由监控/警示机制处理异常对倒，竞价撮合引擎本身不做硬拦截
  （上交所交易规则防范市场风险条款精神；具体条款版本与生效日期
  **未经核验**——此前"深交所 2025 年修订"援引版本矛盾已撤回，核验
  前不宣称依据可靠）。**游戏额外限制（如实标注，非交易所引擎行为）**：
  本项目实现为撮合引擎硬排除——(a) 集合竞价：同主配对撤卖侧自单、
  买侧继续找非自对手（资源全闭合：退款退股+在途标记+终结事实），
  可配对量可低于选择量（选择量=上界）、公开成交量=实际成交量；
  (b) 连续竞价：taker 交叉档存在本人自单时**终止撮合**——这是
  **游戏额外限制**（会阻断本可先成交的第三方，与旧管线涌现行为
  不等价，此前"与旧管线等价"的结论撤回）。适用日期：2026-10-15 起。
- **日终未成交单失效（2026-10-15 登记，翻转批裁决 2）**：收盘集合竞价
  撮合后全部未成交委托失效（沪深交易规则：收盘后订单簿清空）。旧管线
  静默 clear（资源经 envelope 退回、无事件）；runtime **显式发
  OrderCanceled 事件**（有意分歧——依据：失效是真实交易制度事件，
  用户可见性符合显式呈现铁律；资源语义两侧等价=退款退股一致）。
  适用日期：2026-10-15 起（std-thread 翻转批）。
- **成交事件 maker/taker 字段=腿方向（2026-10-16 登记）**：`Event::Trade` 的
  `maker` 恒为卖腿、`taker` 恒为买腿（`maker/taker_order_id`、
  `maker/taker_filled_value_before` 三对字段成组同义）。字段名不承诺"主动/被动"：
  A 股无 maker/taker 费差（佣金双边、印花税恒卖腿、过户费双边），腿方向无歧义且
  费用归户唯一。完整裁定与前端消费面全量扫描见
  [ADR-0042](decisions/0042-trade-maker-taker-leg-semantics.md)。适用日期：2026-10-16 起。
- 普通主板与主板风险警示股票涨跌幅 10%，创业板 20%；每只股票显式配置，避免仅凭名称猜板块。
- **已知近似（std-thread 迁移期）：** runtime 镜像形态下 `StockView` 的
  `daily_upper_limit`/`max_buy_price`/`min_sell_price` 暂无 desk 精确源（desk 广播的
  `StockPublic` 携带精确值），`session/views.rs` 的 `bound_fallback` 以最近价 ±10%
  边界回退近似（`last×11/10` 上取、`last×9/10` 下取，缺最近价时按 0.01 元兜底），
  待投影收口后替换为精确值（2026-10-17 登记）。该近似只影响展示与下单界面预检查的
  边界参考值，不影响撮合、拒单或存档事实——订单簿照常按真实涨跌幅与笼子校验。
- 最小价格变动 0.01 元。
- 买入申报为 100 股整数倍；主板/ST 当前模型单笔上限 1,000,000 股，创业板限价单
  上限 300,000 股、市价单上限 150,000 股；权威配置拒绝把一手改成其他数量。
- 卖出不得超过“持仓 − T+1 锁定 − 已挂卖单”；不足 100 股的零股必须随一笔卖单一次性卖出。
- 当日买入股份不可卖，下一交易日日界解锁。
- 开盘集合竞价在 09:15–09:25 接受限价申报，09:15–09:20 可撤单；09:25 一次性撮合后，
  未成交的当日有效限价单继续进入连续竞价簿。09:25–09:30 不接受新申报。
- **集合竞价成交价=单一清算价（2026-10-17 登记）**：开盘/收盘集合竞价撮合产生的全部
  成交，**买卖双方按统一清算价成交**（清算价=按成交量/未匹配量/交易所差异规则选出的
  唯一价格），成交价与 `AuctionCompleted.clearing_price` 恒一致；连续竞价仍按对手档
  （maker）申报价结算。**依据**：沪深交易规则集合竞价撮合制度——集合竞价以单一价格
  成交。本次核对日期 2026-10-17。runtime 修复登记：
  std-thread desk 此前按 maker 申报价结算竞价成交（与旧管线 `orderbook.rs
  execute_auction_clearing` 按清算价不一致），2026-10-17 修复对齐（红测试
  `auction_fills_settle_at_single_clearing_price`：买 1000×卖 900 ⇒ 清算 950，双方
  按 950 结算）。
- 在允许撤单的时段，刚受理但仍有挂单余量的委托也可撤单；部分成交只撤余量，
  已全部成交则明确反馈“已成交、无法撤单”。撤单是否成功由对应股票订单簿的当前状态决定。
- **游戏简化：** 本 tick 撤单释放的资金或可卖股份，要到下一 tick 才能供新委托占用。
  本 tick 的新委托仍按轮初可用资源校验；计划即使收到撤单成功反馈，也不能在本轮复用这笔释放。
- 收盘集合竞价在 14:57–15:00 接受限价申报且不可撤单；窗口结束时按证券交易所各自的集合竞价
  清算规则一次撮合，未成交余单随紧接的日界失效。收盘竞价事件显式携带 `ClosingAuction` 阶段和
  `clearing_price`，不得误作开盘价或投影到开盘分时区域。
- **集合竞价时段不揭示买卖盘口（2026-10-17 登记）**：开盘/收盘集合竞价期间，公开
  行情面不揭示订单簿买卖盘（盘口）。**依据**：沪市交易规则（2026 年修订）行情揭示
  条款——集合竞价期间即时行情揭示前收盘价与集合竞价虚拟参考价格、虚拟匹配量、
  虚拟未匹配量，不揭示连续竞价式买卖盘口；深市为同构定义。本次核对日期
  2026-10-17，仅核对本条款，沿用已登记的其他条款核对边界
  （[上交所交易规则行情揭示条款](https://www.sse.com.cn/lawandrules/sselawsrules2025/trade/universal/c/c_20260424_10816492.shtml)）。
  **引擎实现（runtime desk 广播门禁）**：开盘集合竞价（CallAuction）公开簿=空；
  收盘集合竞价（ClosingAuction）只揭示 `public_cutoff_serial` 截点前受理的老单
  （收盘竞价时段内新申报不揭示，同条款真实制度面）；盘前（PreOpen）/连续竞价
  （Continuous）全公开。**性质澄清（Sol 终审 §118 勘误）**：收盘竞价保留展示的
  老单属"截点前已进入连续竞价公开面的存量可见性"，与开盘竞价"申报期完全不
  揭示"是两种不同制度面——前句"公开簿=空"仅指开盘竞价申报面，不与收盘老单
  展示矛盾。**尚未模拟（如实标注）**：真实制度的集合竞价虚拟参考价格/
  虚拟匹配量揭示本项目未实现——竞价申报期间的**增量揭示面**为空，不以其他量
  冒充虚拟参考价。
  适用日期：2026-10-17 起（std-thread 迁移批）。
- 交易所归属在每只股票上显式配置。沪市集合竞价最终同价时取候选中间价。深交所规则
  3.4.3 的最终距离参照为开盘前收价、盘中/收盘最近成交价；当前游戏所有深市竞价仍统一
  使用昨收，属于尚未修正的简化，不是完整真实规则。完全同距时取低价也仅为模拟确定性。
- 连续竞价限价申报默认执行价格笼子：买入上限取参考价的 102% 与参考价加十个最小价位中的
  较高者，卖出下限取参考价的 98% 与参考价减十个最小价位中的较低者；参考价按买卖方向
  依次使用对手一档、本方一档、最新成交价/前收。
- **可选游戏简化（2026-09-27）：** 新游戏配置 `price_cage_enabled` 可关闭连续竞价
  价格笼子，默认开启；关闭后仍执行涨跌幅和其他申报规则。该选择随当前存档保存。
- 买卖双方收取交易过户费 0.01‰；卖出方另收 0.5‰ 印花税；券商佣金率和最低佣金可配置。
- **可选游戏简化（[ADR-0040](decisions/0040-new-game-default-taxation-modes.md)，2026-10-08 批准）：**
  公司局新局税务模式 `Exempt` 同时免除卖出印花税，要求 `stamp_tax_rate` 精确为 `0`，
  佣金与过户费照付；这是游戏模式，不是真实 A 股普遍免税规则。
  `FlatWithholding`／`AShareIndividual` 以及无公司系统的局均要求现行基线
  `stamp_tax_rate == 0.0005`。模式与税率不匹配时显式拒绝创建或恢复，不补默认值。
- **游戏简化：** 统一市价意图按涨跌停价作保护并逐档立即成交，未成交余量不进入订单簿
  （余量显式撤销回执——玩家逐请求可见结果；零成交/部分成交同）。**市价保护价=日涨跌停
  带（不含连续竞价价格笼子）**——笼子只作用于限价申报；真实制度的市价申报自带客户
  保护价字段，本项目无该字段，以涨跌停整体带代替（登记简化）。尚未细分沪深市价申报
  类型及其最优五档、余量转限价等差异。
- 全部未成交委托共享账户资源占用预算：买单占用成交额及费用；卖单仅占用可卖股份，不占用
  现金。撤单、日终清簿后释放相应占用，快照公开 `reserved_cash` 和按股票汇总的
  `reserved_sell_qty`，界面显示的“可用资金/可卖”必须扣除对应占用。卖单名义费用公式、税率和
  最低佣金累计口径不变；实际费用按每一成交腿从成交所得封顶收取，并按佣金 → 印花税 → 过户费
  的固定顺序分配。这是游戏简化，不等同于交易所真实清算或一般券商规则。完整口径见
  [ADR-0017 分歧 #9](decisions/0017-escrow-parallel-tick.md#5-九条分歧台账)。
- **挂单资源占用的实体在途口径与 `frozen_cash=0`（std-thread 迁移，2026-10-17
  登记）**：runtime 所有权转移模型（[ADR-0043](decisions/0043-std-thread-ownership-runtime.md)）
  下，买单下单即从组账户实际扣出 purse（最坏情况对价+佣金上限+过户费上限）、卖单
  实扣 qty 股；挂单在途期间该笔资金/股票**不在**组账户——占用以实体在途呈现，
  快照 `frozen_cash` 恒为 0 是口径事实，不表示资源未被占用。三层现金语义：组账户
  可分配预算（已扣在途）/总现金观察值/desk 实际占用（价优差额退回后）。旧管线为
  escrow/envelope 预留口径（快照公开 `reserved_cash` 与按股 `reserved_sell_qty`）。
  **口径声明（如实登记）**：两种口径的资源总量等价——对在途挂单再计冻结即重复扣
  （迁移复审裁定）；「本 tick 撤单释放的资源下一 tick 才可复用」的既有游戏简化在
  两种口径下一致。适用日期：2026-10-17 起（std-thread 迁移批）。
- **跨拍透支重验的消除（2026-10-17 登记，供旧管线退役审计）**：旧管线 NPC 准备域
  需「队列批在下一拍重验现金」防御挂单后现金变动导致的拍中透支；runtime purse
  所有权模型下单即扣，该缺陷形态从根上消除（等价防御=组端 purse 校验）。旧管线
  防御面随其退役消亡，不再回补。

## 公司模型选源与完整存档（2026-10-27 候选分支登记）

本段描述 SOURCE-B 候选分支的契约；完整跨 Web 验证与独立终审通过前，不据此宣称
main 已交付。`setup.company_system` 缺席时沿 `LegacyOperations` 的经营、前史和公开
会计材料；选定 `Simple` 时只构造和推进 `CompanySystem`，公开材料标为
`SimpleGenerated`，不创建或后台推进旧经营网络。`Simple` 携带 `company_operations`
或非空 `groups` 时，在装配前按模式字段冲突明确拒绝；`Simulation` 仍明确不支持。
公司会计类别 `CompanyKind` 不表示沪深证券板块，股本单位仍为股且保留 `u64`。

完整 `SaveSlot` 只保存一个 required `company_state` 外部标记：
`LegacyOperations { operations, closing, groups }` 或
`CompanySystem { system, corporate_actions }`。旧并列运行字段不再接受，不提供迁移、
别名或缺字段补值；创建参数中的 `setup.company_operations` 和 `setup.groups` 保留各自
原语义。公开库和披露游标仍各只有一份，与选中模型、冻结时钟、报告来源和恢复资本事实
同时勾稽。名册 `SettlementDayMarket.binding` 保留实际物理日的前驱、股数单位与最终
确认或休市日历来源，不能简写成普通 `PublicMarket` 或丢弃 binding。

`Simple` 是明确的游戏财务简化，不声称逐合同经营、真实公司现金来源或偿付能力已经仿真。
此选源和存档改动不开放 R4 公司行为资产提交或 SRC-C live hook，不改变沪深撮合、
T+1、税费、账户资金或股份守恒规则。

## Simple 偏好的局内配置入口（2026-10-25 登记）

`GameSession` 与 `ProtocolSession` 提供 `company_simple_preferences` 和
`set_simple_preferences` Rust API，仅配置既有现金分红与送转偏好；这是
[ADR-0037](decisions/0037-company-action-initiator.md) 的游戏化策略配置，不是投资者
发起公司行为的权限。比例沿用 bp、股份比例沿用百万分比、利润门槛沿用 `Money` 分、
间隔沿用结算周期数。无公司线、未知公司、非法参数或两侧配置不一致均显式拒绝。

编辑原子地同步公司系统及 setup 的偏好配置，不改变资产、已有方案、提案台账或个人信息。
`GameSession.save` 包含已编辑配置；恢复时要求 `company_state.CompanySystem.system.config` 与
`setup.company_system` 完整一致，单侧编辑存档会被拒绝。`ProtocolSession.save`
仍返回上次已发布的日终存档，后续自然日日终发布才包含新配置。本入口不代表偏好自动提案
与公司行为日终执行链已经接通，也未开放配股、增发、回购或拆股策略参数；宿主 wire/API
与 UI 接线由后续批次实现。

## 计划来源与持久化边界（2026-10-28）

同一 `PlanBook` 按 `PlanEntry::Investor` 与 `PlanEntry::IssuerRepurchase` 区分个人交易判断和
发行人回购授权，共用账户内 `OwnerSequence`、`PlanId` 与 `(account, stock)` 活动索引。
公司授权没有个人 `opinion`、`confidence_bp`、`urgency` 或持有期，不能以个人判断公式续发；
同账户同股的两类非终止条目互斥。`Closed` 公司历史保留，后继计划只分配更高 sequence。

当前 wire 的 `plans` 是带唯一外部 tag 的数组；旧裸个人计划、未知或多 tag、缺少游标显式拒绝，
不提供兼容默认。公共日终档对两种来源都禁止任何 `execution`，WASM 入站保持数组。
本片只提供共同身份和值恢复：公司 child 安装、真实成交来源与资金/公司授权 binding 尚不支持，
完整 Session 恢复明确拒绝未能证明已提交 binding 的公司条目。低层 `Funded` 值不代表批准成功或
资金已经生成；回购制度与资金简化仍以 ADR-0038 和下节官方依据为准。

## 回购资金数值通道边界（2026-10-28）

`SyntheticBudgetCredit`／`SyntheticBudgetRecycle` 沿唯一 `PendingAdmin` 准备、检验、提交与撤销，
金额为正整数分。专户能力由实际 `AccountBook` 的 `IssuerRepurchase` kind、完整名册与公司目录
派生；组校验完整 `Owner`、`AccountId`、canonical company entry 的 issuer／stock，不用账户编号
或低层 `Funded` 值证明批准。一个共享专户可绑定不同发行人／证券，属于现有游戏模型，
不等同于已完整模拟现实中每发行人的独立证券账户。

信用和回收只改变 raw cash，严格检查原值及候选 `cash + held` 溢出。回收要求整个共享专户所有
证券都没有 held／foreign／pending 资源，也没有真实 child ticket；零余量残票仍须先完成真实回流。
数值原语不证明金额属于该计划，精确未用预算须由同一次 inspect 按唯一公司 book、已提交资金
binding 和真实成交／费用来源核验；不得按共享账户总现金回收或清掉其他计划与合法 surplus。

本通道不新增资金余额簿或批准幂等集合，不改变普通投资者交易制度。合成资金仍是 ADR-0038
已登记的 Simple 简化；批准 producer、公司执行资格、真实委托／成交／余款回收和完整公共恢复
继续由后续同事务接线完成，不能据数值通道可用宣称回购全链已支持。

### APPROVAL-C 实施候选的限制（2026-10-28，尚未完成验收）

内部共同批准候选将 `OpenIssuerRepurchase`、正额 Credit、完整 `PlanBook`、执行投影与
公司 book／finance／binding 放入同一次行政事务；当前仍等待与 R4 镜像通道组合验证，
不据源码存在宣称生产批准、玩家命令或真实回购已可用。

`repurchase_execution_bindings` 是公司行为容器的严格必填字段，禁用机制也输出空数组；
Rust／Web 契约必须同步，不能把旧档缺字段解释为没有历史。完整 Session 的公司计划恢复
仍显式拒绝，须待真实来源、已消费 cursor、日终无残票与投影重建全部验证后才能开放。

批准投影只是不可执行的初始基线。`issuance_in_progress=None` 明确尚无可信发行实施
source，不是 Clear；`material_restriction=NotModelled` 也不代表已完成重大事项识别。
`eligibility_as_of`／revision 不能代替执行现场的完整 CivilInstant、calendar、phase 与
价格限制 cut。不得从 `RightsOfferingStatus::Approved` 或其他状态猜发行已经开始实施。
沪市指引第20条／深市指引第19条规定的起点为发送认购邀请书，或取得注册批复并启动
向不特定对象发行，止点为新增股份完成登记；真实执行前仍须接入这些事实。

**尚不支持法定期限的完整自动核验**：当前候选校验日期顺序与窗口首尾交易日，但未自动
核验沪市第17条／深市第16条的一般12个月、维护价值3个月期限。窗口仍为显式参数，
不得把候选注资成功描述为完整制度合法或允许申报；REPURCHASE-X 启用前须收口此门。
本条沿用下节已登记官方原件与本轮独审读取的同一材料，不宣称新增联网核验。

## 玩家提案的 Session 批准入口（2026-10-28 登记）

`GameSession::propose_player_company_action` 接入 `CashDividend`、`StockDistribution`、
`RightsOffering`、`SecondaryOffering`、`ShareSplit` 的共同批准候选。
沿 [ADR-0037 玩家提案追记](decisions/0037-company-action-initiator.md)，
这是游戏化简化：当前玩家账户对上市证券的总持仓任意大于零即可提案，包含名册外持仓和
T+1 锁定股；账户身份、日期、证券与日历取当前 Session，通过同一批准校验。
返回的 receipt 只证明方案已批准，不证明未来登记、付款或新股到账；批准不改变玩家现金、
股份或个人获知，不制造观察点。native 使用同一个 INFO 行政事务安装当前库与 facts；
业务 prepare/inspect 拒绝保持可继续，World terminal、锁 poison 或 tick 漂移封存 Session 首错。
`StockDistribution`、`ShareSplit`、`RightsOffering` 与 `IssuerRepurchase` 的资本
restore 分别校验批准与已发生事实：`approved_on` 可到存档当前自然日；公告、登记、
REG receipt、实际入账、回购下单记录与成交仍只能到已完成的前一自然日，不因批准窗口放宽。

配股与增发受真实新局 `rights_offering_enabled` 开关控制；回购另受
`issuer_repurchase_enabled` 控制，禁用在启动 driver 或安装方案前显式拒绝。
玩家 `SecondaryOffering` 只定向本人 `NamedHolder`，缴款期为1交易日且
`lock_until=None`，沿既有游戏简化，不冒充现实定向发行的完整限售条件；配股日程
仍由真实冻结日历与显式缴款天数推导，NPC 默认 `FullByDefault` 不变。

资本批准从当前已安装的完整 REG receipts、发行股数及注册资本事实派生面值，
严格按整数分整除，不使用未来 split 声明的面值，也不把派生读取当作生产执行回执。
自动 foundation 沿 setup 面值根；手工 foundation 沿真实已绑定资本，不用 setup
面值替代。跨待生效 split 的单位与登记相位 producer 尚未接通：同 issuer、同 stock
的发行／送转入账若在 split 登记当日或之后，两个提案受理顺序均显式拒绝；严格早于
登记的独立资本动作可批准。已有 settled split 不受此 pending 门限制。同除权日
非现金组合仍显式不支持；两个 pending split 仍受原 finance 声明链约束，不能据此
宣称完整多事件单位组合能力。

回购真实资金授权 binding／allocator 与同次 prepare producer，以及公司行为生产日终
结算仍待接线，能力面不能据此宣称全部可执行。已对齐可信 E2 handover；Cash、
StockDistribution、RightsOffering 与 ShareSplit 在单证券、单买单非空旧簿 checkpoint
已实测通过首次移交、批准、raw World、save 与再次 restore。当日批准恢复与前一日
实际事实截止已分离验证；不外推其它订单形态或完整公司行为生产链。
Protocol、三宿主命令及数值 wire 拒绝面也须由后续真实接线验证。

## 回购专户的自主交易隔离（2026-10-25 登记）

`AccountKind::IssuerRepurchase` 禁止走普通 NPC 的自主决策下单通道。保留专户 kind、
强制 `next_due=u64::MAX`（包括已有 attention 的覆盖路径）是 engine 接线保护，
不是交易所规定的调度方式；“仅方案执行器可下单”是本项目的接线目标。

下列现行指引均自 **2025-03-28** 施行；最近核对日期为 **2026-10-25**，仅覆盖本条所列依据，
不代表重新核验本文件其他规则。沪市与深市分别适用各自指引，不互用条号：

| 市场与官方依据 | 本条核对的要求 |
|---|---|
| 沪市：[上交所《上市公司自律监管指引第7号——回购股份（2025年3月修订）》发布页](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/mainipo/c/c_20250516_10779134.shtml)、[官方 DOCX](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/mainipo/c/10779134/files/0f2fc421931a462698162bb874d830d9.docx) | 第21条：通过回购专用账户回购，专户仅限存放所回购股份；第36条：最终方案审议通过、开立专户后及时披露回购报告书；第53条：违反指引或未按回购报告书实施回购将受监管措施或纪律处分。 |
| 深市：[深交所《上市公司自律监管指引第9号——回购股份（2025年修订）》重新发布页](https://www.szse.cn/lawrules/rule/stock/trade/t20250327_612564.html)、[深证上〔2025〕223号修订通知](https://www.szse.cn/lawrules/rule/all/t20250328_612605.html)、[官方 PDF](https://docs.static.szse.cn/www/lawrules/rule/stock/trade/W020250327585488391328.pdf) | 第20条：通过回购专用账户回购，专户仅限存放所回购股份；第35条：最终方案审议通过、开立专户后及时披露回购报告书；第50条：违反指引或未按回购报告书实施回购将受自律监管措施或纪律处分。 |

**实现与验收范围**：本条只登记禁止专户像散户 NPC 自主买卖。现有测试覆盖非零现金与持仓、
无回购方案的真实交易拍逐拍零自主委托，以及普通 NPC 产生委托的正控；显式 `ExternalPlan`
测试只证明既有委托运输通路仍可受理显式买单，不证明完整回购方案执行器已经接通。
**尚不支持**：完整回购方案执行器，以及专户用途、回购专属申报时窗与价格限制的完整端到端校验。
沪市第19条与深市第18条另有申报要求，不能由本条隔离测试推定已实现。
[ADR-0038](decisions/0038-simple-issuer-synthetic-settlement-funds.md) 的 `Simple` 发行人侧合成资金
属于游戏简化；本条不改变该决策，也不宣称完成回购成交、注销及全部真实规则的验收。

## 图表周期的展示简化（2026-10-05）

默认 Web 局以每 tick 一秒显示交易时钟。自定义 `SessionSetup.ticks_per_day/auction_ticks/closing_auction_ticks` 的自定义长度局属于游戏时间缩放（简化）：时钟沿用 engine `observation_civil_instant` 的开盘900秒、连续14220秒（无收盘窗口时14400秒）和收盘180秒比例，跳过午间休市。整数 tick 可使阶段边界粗化；例如开盘10 tick 的申报结束 tick 7 显示09:25:30，不伪造另一套09:25时钟。开盘1 tick 时没有独立 PreOpen，开盘或收盘窗口为0时对应阶段缺失，均不是完整交易所时间制度。UI 常驻说明这些简化，不改变撮合阶段、原始 tick、成交或存档事实。

图表按 tick 覆盖区间的起点归入固定显示槽，默认仍每分钟60 tick、竞价每细线6 tick；自定义局没有覆盖的分钟槽保持为空，不插值生成交易。开盘 `AuctionCompleted` 落在竞价区末槽，表示本次竞价已结束；极短/非三整除局的准确事件时间以逐笔时钟为准。当前时钟在完整 tick 日界显示下一交易日开盘；逐笔末 tick 保留上一日15:00，午休端点成交保留11:30，不冒充13:00新成交。

Web 的周 K、月 K、季 K、年 K 使用 `aggregateCandles` 分别按自然周（周一至周日）、公历自然月、自然季度、自然年合并已有日 K；虚拟前史与实际运行均携带明确公历日期。分桶不因可视窗口首根变化而改变，不补造休市日或无成交区间，当前未结束周期只展示已有事实。按用户最新决定，不保留每5／20个交易日聚合选项。MA 周期用户可配（默认 5／10／20／30／60），在所选周期的合并收盘价上按精确分值计算 SMA，N 指 N 根所选周期 K 线；指标数据源为用户选项（前端／Rust），不支持时显式提示、绝不静默回退另一源（前端源只算分时均价，Rust 源算 KDJ／MACD，Rust 分时均价曲线入口随引擎查询 API 批次接线）。聚合只用于绘图，不改变 engine 日 K、成交、委托、结算或时间规则；合并量为股数之和，展示换算为手；组内全部日 K 均携带成交统计时，合并成交额按分精确求和、笔数求和并在详情展示，任一成员缺统计则不伪造合并成交额（显示不可用）。分钟 K（含五日）依赖跨日分钟数据，宿主接线后开放，当前选择时显式提示尚未接入，不回退画成日 K，不因列入产品需求而视为已经实现。

## NPC 策略选择

**NPC 仓位选择（2026-09-26）：** 公共执行路径不按固定单股比例或统一现金保留比例限制
策略目标，允许策略选择集中持股或持有现金。资金、真实手续费、股份、T+1 与申报规则继续
校验。个体参数表达一次目标调整步幅，不是最终仓位上限；见
[ADR-0021](decisions/0021-strategy-position-choice-and-noise-pricing.md)。同 ADR 经用户于
2026-09-27 澄清：每笔交易区分主动成交和指定价格等待，适用于各类 NPC 与玩家。
散户现有主动买卖与游资趋势买卖使用公共限价委托的“最高／最低”价格选项，轮到该股票
受理该单时确定一次价格：开启笼子的连续竞价取笼子与涨跌幅范围的交集，关闭笼子或
集合竞价使用涨跌停价。最高价买单数量按涨停价与本局费用估算并预留资金，实际报价
较低时释放差额；释放额不回补本 tick 预算。未成交部分按确定的价格留簿，机构自己的估值限制与游资反转
策略的等价报价保留。没有增加特殊做市账户或保证流动性的资金。

公共接口也允许最低价买入和最高价卖出，分别使用跌停价与涨停价。价格选项是游戏下单
入口功能，不是另一种交易所市价申报；它与已有的余量撤销市价意图不同。未受理请求保存
选价方式，已经受理的挂单只保存实际价格，行情变化、读档或竞价转连续时不自动追价。
指定价格单仍可能因盘口变化而不符合笼子；订单簿照常校验，不增加拒单后立即重试。
具体资金及类型契约见 [ADR-0022](decisions/0022-symbolic-limit-prices.md)。此次相关条款
核对日期为 2026-09-27，依据见 ADR-0021 第 5 节。

机构成本候选、风险暂停和真实买入后的不利选择为**游戏策略假设**，不是交易所制度
或真实市场统计规律。触发与恢复门槛逐机构冻结，恢复必须由下一本人观察确认；
只暂停继续买入，不自动强制卖出、返费或补钱。已有卖出减仓可按本人净值回撤提高
紧迫度，仍受 T+1、真实费用及合法撤单窗口约束。公式与数值见
[ADR-0026](decisions/0026-individual-institution-experience.md)。

**计划复审与报价的触发拍（std-thread 迁移差异，2026-10-17 登记）：** runtime 下
计划链的复审与报价只在链 **due 拍**的 `run_chain_decision` 内执行；旧管线每拍复审。
复审语义本身（`record_review`/quiet 门控、观察与恢复门槛、首子单
urgency=initial+pause=Unavailable）已对齐迁移；差异仅在**触发拍集合**为旧管线的
due 子集——决策频率结构不同，属组线程「一股一持久线程+内联决策」调度模型固有，
非交易所制度面（A 股无「计划复审频率」对应规则，不构成交易语义偏差）。行为差异
如实登记；回归面由 runtime 专项测试承载。适用日期：2026-10-17 起。

## 已知待修正的受理顺序偏差

**草稿阶段容量约定（2026-09-25）：** 请求队列、未成交挂单和待处理计划事件不按固定条数拒绝合法请求。资金、可卖股份、T+1、单笔申报数量与交易时段仍分别校验。机器资源故障应明确报告，不能伪装成交易拒绝。详见 [ADR-0019](decisions/0019-draft-market-scope-and-capacity.md)。

**计划策略（游戏规则）：** 积极交易型机构在每日开盘后的首个连续交易 tick 观察并可
建立当日到期计划；成长、均衡型机构每日观察但计划可跨日，深度价值和防御型机构
不因普通开盘自动复核；其随机观察频率也低于每天一次，允许跨日不观察。成交与撤单仍由
真实订单簿反馈，买入股份对外继续受 T+1
限制；“日内计划”指计划有效期至当日日终，不表示当日买入后可以卖出。

NPC 请求在上一 tick 的已提交状态上形成，下一 tick 受理；玩家请求验收入队后在下一 tick 参与交易。
盘前、连续交易和集合竞价已把本轮就绪的 NPC、玩家及部分计划根命令合入局部受理：
不同账户争同一股票的受理先后由该股入口实际发生的先后决定，来源标签与订单 ID 不充当优先级；
同一账户的现金和逐股可卖数量分别校验。计划链只在收到对应命令的结果后续行，
已全部成交的订单撤单会返回明确的业务失败，部分成交仍可撤剩余量。

当前 native runtime 的既有简化：desk 对本主不在簿中的未知或已结束订单统一返回
`OrderAlreadyFilled`；尚不支持区分这些状态，不能把该回执当作真实已成交证明。
他人在簿委托仍返回 `OrderNotFound`。这是 native 回执实现边界，不是交易所规则，
也不改变部分成交撤余量、撤单窗口或 closed 证券优先返回 `ExchangeClosed` 的约定。


计划根现逐个完成并交付，无须等同批其他账户的根都完成。三阶段的同账户资源争用已从候选数组位置改为接收事实：上一提交形成的 NPC 请求、两个 tick 间排队的玩家请求，以及本 tick 真正就绪的计划命令分别保留其账户内先后；这个比较只用于同账户现金或逐股可卖股份。跨账户同股委托的时间优先仍取该股入口的实际受理先后。现有 NPC 与玩家候选虽仍按来源拼成向量，但来源块不再充当账户资源裁决依据；后续仍需继续核对无关请求的等待范围，并提高真实活跃负载的整轮多线程加速比。
这些限制与交易所的价格、时间优先规则不同，迁移要求见
[ADR-0018 §7](decisions/0018-long-running-immutable-timeline.md#7-交易约束撮合顺序与并行)。

规则依据（交易所规则与财政部公告本次核对日期：2026-09-22；2026 版规则自 2026-07-06 起施行）：

- [上海证券交易所交易规则（2026 年修订）](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/c_20260424_10816482.shtml)
- [深圳证券交易所交易规则（2026 年修订）](https://docs.static.szse.cn/www/lawrules/rule/trade/current/W020260424690713155663.pdf)
- [上交所关于不足一个申报单位余额一次性卖出的说明](https://www.sse.com.cn/lawandrules/guide/stock/jyglywznylc/tz/c/c_20230209_5716007.shtml)
- [中国结算证券交易及相关业务费用表](https://www.chinaclear.cn/zdjs/editor_file/20220701154723234.pdf)
- [财政部、税务总局关于减半征收证券交易印花税的公告（2023 年第 39 号）](https://fgk.chinatax.gov.cn/zcfgk/c102416/c5211343/content.html)
- [财政部公告原文（2023-08-28）](https://www.mof.gov.cn/jrttts/202308/t20230828_3904235.htm)

本次读取了沪市规则 2.4.2、3.1.4、3.3.1/8/9/11/14、3.5.1/2 及暂缓实施附件，
深市规则 2.3.2、3.3.1/5/8/9/11/13/16、3.4.2/3；确认阶段、撤单窗口、申报数量、
价位、笼子与集合竞价参照差异。中国结算费用表上述地址本次返回 HTTP 404，
过户费来源仍沿用 2026-09-08 核对记录，尚不能标为当日重新核验通过。
此次文档核对未改变撮合、费率或其他业务实现。

规则会变化，调整实现时必须同时更新测试、默认配置和本文档。
存档只支持完整当前日结结构，不设代际版本，恢复事实使用 `runtime_state: SavedRuntimeState`；
额外版本字段和旧字段显式拒绝，不提供迁移或兼容。交易所、证券类别、T+1、策略状态、个人计划、
随机流及已承诺的日 K 与历史事实必须完整保存；缺失或矛盾时明确拒绝读档。
活动连续/竞价委托、日内母单、冻结 envelope、委托失效计时器和待受理请求只存在于内存，
不进入公共日终档。公开保存和回滚 checkpoint 的边界见
[ADR-0025](decisions/0025-day-end-only-persistence.md)。
std-thread runtime 的日结编排（2026-10-17 登记）：市场推进内 `run_ticks` 按
`(tick-1) % ticks_per_day` 自动触发日界（world 权威），`driver.day_end` 仅承担 civil
侧桥接；`last_day_end_tick` 守卫显式互斥——重复手动日结返回明确错误、已被自动日结
覆盖的请求显式跳过、Abort 回滚清空当日进度，`World` Drop 兜底关停线程。恢复契约
不变：ADR-0025 日界静点，`WorldSpec.initial_committed_tick` 与 `session.tick()`
锁步对齐贯通相位（ADR-0043）。

日末窗口约定（2026-10-26 核对）：完成 tick N 的委托属于 `phase_at(N-1)`；
最后一拍及同拍全部 Continue 使用同一个当日前收盘价。日末 Drain 先按现有沪深
清算规则完成收盘集合竞价，再归档终 K、更新下一日前收盘价并重置日内统计。
日中手动 Drain 不提前翻日；同一 Start 后重复 Drain 仅读取诊断结果，不重复清算或归档。
该时序不改变已登记的深市竞价参照简化和无收盘成交时沿用最后成交价的游戏口径。

`simulation-diagnostics` 的 rich Drain 候选沿用上述真实执行路径：当日有效余单记录为
`Termination::DayEnd`，只含剩余股数；不将其标成投资者主动撤单 `Voluntary`。
集合竞价 `Execution.side=None`，双腿仍按 `maker=卖腿/taker=买腿` 对账；
`before` 是每次真实执行前的内部完整簿，允许交叉报价，不冒充对外公开盘口。
Tick 与 Drain 使用显式不同的报告身份域，各自局部 ordinal 不可互相覆盖。
自动日终的 `FactTime` 取原提交 TickArc 的 after projection；手动日终取本次真实
DayArc、World 已验证 tick 与纯 Calendar projection，不补造 TickArc。没有真实 rich
Drain 事实的 Closed 日终不构造市场时刻；手动输入本日零完成拍仅表示当前首窗口，
其他输入表示最后完成窗口。shared market minute 的单位保持不变。
`FactTime.market_minute` 始终为 shared 累计已完成分钟；证券事实另携
`StockFactClock { code, completed_minutes }`，本股计数按该证券实际交易所日历推进。
`Submitted/Filled/Terminated/Execution/Quote` 必须有匹配证券的 clock；
`Decision/Acquisition/Budget/ObservationRestart` 必须不携证券 clock。两种分钟均以
十进制字符串输出；寿命与盘口恢复仅按同股局部计数做 checked subtraction，缺失、
错股或回退显式拒绝。shared 端点本身的回退仍独立拒绝，不以本股寿命替代该门。
Tick/Continue 使用实际 caller 原 TickArc 的 before projection、
phase_before 与 observation_instant_after；DayEnd 余单与清簿 Quote 使用原 after。
serial 的 Arc 仅暂存于私有 candidate，冻结提交前清除，不进入存档或 hash；INFO 与
restore 静点只读原 DayArc projection，不为 Closed 或 Completed 日期补造 TickArc。
诊断观测不改变当日有效委托、T+1 或现有日历简化范围。
连续竞价 Market 的 rich 生命周期沿用原即时撮合：Submitted 与 Execution 使用原单及
同一次 incoming 操作起点的真实盘口，非零未成交余量记为 `Termination::MarketRemainder`；全成不再生成
终止事实。该观测不改变上述市价保护带、余量撤销和沪深类型尚未细分的游戏简化。
已解析来源的 place 拒单已接通 rich 生命周期：保留真实取号申报尝试的来源与拒单前盘口，
并以同一订单身份记 `Termination::Aborted`；只覆盖 `PhaseRejected`、
`AuctionLimitOrderRequired`、`ExceedsOrderCap`、`NotRoundLot`、`PriceOutOfBand` 和
`PriceCageExceeded`，且逐项与同一 `DeskReport` 的拒收回执核对。`NonPlayerUnresolved` 来源仍显式拒绝。
TTL 到期、CancelAll 未解析来源及竞价自成交排除的 rich 生命周期仍不支持，遇到这些真实路径
显式拒绝报告，不能用空报告冒充完整诊断。

内存验证投影保留每股挂单 `seq` 与 `book_next_sequences`，不重新编号或改变同价委托先后。
下一序号按 u64 十进制字符串无损传输，股票键集必须与配置和市场一致，并严格大于该股
现有挂单的 `seq`；空簿也保留已消耗的序号。它们是明确的序号事实，不是实施版本后缀。
公共日结档不包含活动委托，不能把该验证投影当作日内持久化入口。

## 行政数值事务基座的支持边界（2026-10-25 登记）

runtime 的独立 AdminTransaction 承载上层已确定合法到账时点的 CashDividend、
ShareSplit、WithholdingTax、ShareReduction，以及 PaymentDebit、SubscriptionRefund 和
SharesWithCost 数值操作，尚未连接生产公司链。
金额为 i64 分、数量为 u32 股；行政股份变动不是买卖申报，不套用买入 100 股整数倍。
ShareSplit 沿用既有命名，仅表示已经可流通的送转正向到账，不增加当日买入的
frozen_today。ShareReduction 只从实际在手且可卖股份安全核减；在途卖股已从组端
shares 扣出，不能重复扣减，pending_sell_qty、在途票与 T+1 冻结保持原值。

这是账务数值原语，**不表示已支持任意拆缩股制度**。长期限售、lock_until、冻结股整体
缩股、混合来源归属、碎股分配与任意缩股方案合法性均不支持；本入口不接受 lock_until。
真清仓但仍有在途票的 ShareReduction 显式拒绝。正式公司命令及 restore 接线须先补齐
这些制度，或对未支持输入显式拒绝。WithholdingTax 仅借记已经算出的正分额，
只可使用实际可用 cash，不使用在途买单 purse；本批没有确定新税率、计税主体或舍入制度。

配股资金原语（2026-10-28 登记）：PaymentDebit 按输入顺序精确扣除当前 raw cash，
不使用在途 purse，也不以随后退款净额掩盖中间余额不足。SubscriptionRefund 只退回原实缴
本金，不计退款利息，不改变股份或成本。SharesWithCost 仅在上层确认已经到合法上市流通日
后增加股份与该账户原实缴总成本，保留 recovered，不再扣现金、不收二级市场交易费用、
不增加 frozen_today；长期限售仍不支持。三原语沿同一个 PendingAdmin 与 INFO 候选共同
prepare／commit／abort；数值通道接线不表示配股缴款、上市或回购生产流程已经启用。

依据沿用 ADR-0031、ADR-0038、ADR-0039、ADR-0043 及已有中国结算取证：
[上海分公司证券发行人业务指南 §2.5](https://www.chinaclear.cn/zdjs/yshsc/202503/bc6781d0a57340f598930115fe63e5c3/files/中国证券登记结算有限责任公司上海分公司证券发行人业务指南.pdf)、
[深圳分公司证券发行人业务指南 §2.5](https://www.chinaclear.cn/zdjs/dsz/202512/fb54cbc97d61460288d48d079bb9f72c/files/中国证券登记结算有限责任公司深圳分公司证券发行人业务指南.pdf)
关于送转新增股份上市及限售继承的适用边界。原件取证日期为 2026-10-06，记录见
[`agents/company-system/remaining-corporate-rules-research.md`](../agents/company-system/remaining-corporate-rules-research.md)。
本次只延续已可流通到账和既有 T+1 资源口径，没有重新联网核验官方原件，不声称新增完整
权益日程、税制、拆缩股或回购注销制度；公司链接线仍须核对届时现行规则。

## 尚未模拟

新股上市初期无涨跌幅限制、科创板/北交所的全部差异、停复牌、退市整理、盘中临停、
大宗交易、融资融券和真实券商的个性化佣金舍入均不在当前版本范围。界面不得把游戏结果描述为真实
投资建议或真实券商成交结果。

公司经营的收付不进入证券投资者资金池。工商年度所得税只在年末计提并尝试支付，未模拟
法定预缴及汇算清缴期限；商业借款、地产项目借款与银行定期存款采用显式合同到期日收付，
不足时保留未付事实并逐自然日重试，不自动续贷、补钱或触发停牌、退市及股东清算。
付款失败公开材料属于游戏经营风险事实，不等同于现实法定重大违约认定。完整简化及法源
限制见 [`company-accounting.md`](company-accounting.md) §2.6。

固定集团只支持单层、精确基点持股与唯一配对的单批内部商品交易；来源身份不完整、
存货成本池跨 item 歧义、同成员对多个内部销售批次、跨年旧未售内部批次及非工商买方
内部商品采购均显式不支持。内部未售成本按移动加权池均匀比例分摊，不采用 FIFO。
这些为游戏装配简化，不是现实上市公司合并规则；详见
[`company-accounting.md`](company-accounting.md) §8，不改变证券交易、T+1、费用或投资者资金规则。

## 快捷委托与定时自动（2026-10-05）

前端手数输入按100股转换为原 Intent 整数股；买入整手、可卖/T+1、零股及板块单笔上限复用既有校验。比例和全部使用可用整手，不自动突破单笔上限。可买能力扣除已挂单资源及成交额、配置佣金（含最低佣金）、0.01‰过户费；只作预检查，最终 engine 拒单/结算仍为权威。二进制费率乘法和银行家舍入沿用 Money::apply_rate。此轮未调整费率；现有费用来源的访问边界仍按上文记录。

最小/最大保留 Lowest/Highest 意图，B1/S1 在每次提交时取对应当前真实报价；不存在报价时明确等待。定时自动是会话内 Web 功能，不是交易所原生委托类型，也不写入日终存档。

**游戏简化：** 自动按压缩休市后的游戏秒累计，以公开 Snapshot 进度到期执行；默认一 tick 为一秒，自定义交易日按既有阶段比例映射。暂停无进度不执行。前端批次跨多个间隔时，只用最新权威资源执行一次，不补发过去时点的委托；因此高倍率/远程批量交付不保证每个游戏秒一单。间隔最小1游戏秒，亚秒请求暂不支持；1.5秒会落到到期后的下一可用进度。页面关闭、刷新、读档或 baseline 重新同步会停止自动。资源不足保持开启，未来到期再试；切股仍绑定原证券。取消所有只撤现有自有活动订单，保持自动开启。

本次数量语义依据核对日期：2026-10-05。沪市现行[《交易规则（2026年修订）》3.3.8](https://www.sse.com.cn/lawandrules/sselawsrules2025/fund/trading/c/c_20260424_10817739.shtml)规定竞价买入100股整数倍与不足100股余额一次卖出；[申报数量前端控制通知](https://www.sse.com.cn/lawandrules/guide/stock/jyglywznylc/tz/c/c_20230209_5716007.shtml)说明余股可单独或合并整手一次申报。本次仅核对数量条款，并沿用已登记的沪深/板块差异，不声称重验所有制度。

## std-thread runtime 的受理回执三面登记（2026-10-18，Sol 终审 §93 补登）

以下三面在 runtime（组线程/交易部）路径已实现，此前缺正式登记：

1. **竞价相位×市价申报的显式拒收回执**：集合竞价窗口内市价申报被组端
   受理校验拒绝，回执原因=`AuctionLimitOrderRequired`（竞价期只接受限价
   申报——与真实制度"集合竞价阶段不接受市价申报"一致）；旧管线等价
   面在市场层入口同判。
2. **相位校验先于资源校验的次序**：组端受理顺序为相位门禁→资源（现金/
   可卖）校验——保证相位类拒收不被资金不足误吞（回执原因可观测区分；
   Sol 复审修复 3 裁定）。
3. **组端 T+1/可卖不足的显式拒绝回执**：卖出超过"持仓−T+1 锁定−已挂
   卖"时，组端以 `InsufficientShares` 回执显式拒绝（不静默 skip）；
   T+1 当日买入冻结的解锁仍由日结相位统一承载。

## 散户经历反馈的成交事件面（2026-11-01 更新）

此前登记的 std-thread 迁移期缺口已有接线：组内按成交事实调用 `record_fill_dated` 更新散户经历，决策读取带日期的经历影响；清仓后的冷静期及经历行发布、Session 镜像和存档恢复已有专项测试。业务 hash 不含经历行域，不能仅凭 hash 对拍证明该面一致。本次更新只纠正原“成交反馈不演进、无回归锚”的过期总述，不代表完整验收已通过。

**同族补充（desk 费用累计口径，Sol 五审重送批统一）：** std-thread
runtime 交易部的买腿 purse 扣减与 charged 分项累计均为**边际**口径
（每笔=费用(累计后)−费用(累计前)，含最低佣金下限的差分；多笔部分
成交后 charged 望远镜式恰等于累计式名义费用 nominal(Σ)），与旧管线
ledger 的累计式口径（nominal(Σ)）及存档审计契约（"Buy charged==
nominal"、卖腿封顶不超累计成交额）完全一致——此前"每笔完整名义费用
叠加"的分段口径会使 charged 超过 nominal(Σ)，被 restore 审计
（fault2220/2228 族）拒绝，已随五审重送批修复统一。卖腿维持封顶
分配（名义费用→佣金→印花→过户费顺序分配、不超累计成交额）。

公司共同只读查询（B4e 恢复）：`company_capabilities` 投影公司现有财务事实、五类未完成方案与查询账户本人权利；`owner_rights_offerings` 只投影现有配股账簿及认购队列，不推进状态机或扣款。未知账户、未知公司、无公司局及公开配售占用超过额度均显式报错。缴款窗口以方案自身日期的闭区间为准，无公开配售额度用 `None`，已耗尽的公开额度用 `Some("0")`；已结算方案不再出现在当前方案列表。

**能力边界：** 只读查询通过不代表各类公司行为均可完整执行。native 现金分红和个人税收缴、受限配股、送转、拆缩股及 Simple 回购的具体接线和未支持范围见下文；串行／WASM、其他税制和复杂组合仍有明确限制，相关验证与独立复核状态也以各节为准。配股日程与沪深既有证据边界沿用公司领域模块及[配股实施台账](../agents/company-system/rights-and-repurchase.md)。
## 配股／增发纯候选的能力边界（2026-10-26 登记）

`rights_offering_day_end_candidate` 仅准备隔离的公司账簿、名册、个人税账与逐申请的
`PaymentDebit`、`FailureRefund`、`NewShares` 意图。`PreparedSubscription` 只表示
候选已计算，受理命令成功与组账户真实缴款成功是不同事实。本片未启用生产命令、
组账户付款／退款／新股入账或公司财务接线；后续须通过同一个行政事务取得真实回执，
逐 event/account/submitted_on 勾稽后安装候选，不能用 `CashDividend` 或
`WithholdingTax` 冒充配股资金操作。

本片官方材料核对日期为 **2026-10-26**：

- 中国结算深圳分公司[《证券发行人业务指南》（深业〔2025〕68号）](https://www.chinaclear.cn/zdjs/dsz/202512/fb54cbc97d61460288d48d079bb9f72c/files/中国证券登记结算有限责任公司深圳分公司证券发行人业务指南.pdf)
  第2.4.3（二）、（五）、（六）及第2.4.4（一）：R 日日终派发权证；认购窗口含 L 日；
  L+2 成功划款及失败退款均含利息，且经过结算参与机构／主承销商路径；新股上市另经批准。
- 证监会[第206号令《上市公司证券发行注册管理办法》](https://www.csrc.gov.cn/csrc/c101953/c7121921/content.shtml)
  第53条：配股量不超过配售前股本50%，认购不足70%或控股股东未履行认配承诺时，
  按发行价加银行同期存款利息退款；第59条规定向特定对象发行通常6个月、规定对象18个月限售。
  本片沿用既有数量门槛算法，不宣称完成发行条件、承销与各板块监管程序。

**明确简化／不支持**：候选复用既有领域日程：`settlement_on == listing_on`，成功新股
在结算日取得；仅退认购本金，未计算退款利息、登记费与承销商资金路径，也未模拟控股股东
履约承诺。深市上述日程有已核材料；沪市复用同框架属于游戏简化，尚缺对应操作时点原文。
`DirectedPlacement` 的具名对象与公开额度为游戏机制分类，不等同于真实法规发行类别的完整实现。

NPC 默认策略只在缴款首日日终对未显式申请的非玩家、非回购专户具名权利账户运行，
以最终组账户可用现金（已扣在途资源）认购，现金不足部分记 `waived_shares`；玩家仅显式申请。
队列按冻结顺序处理，截止日先处理同日申请再关窗，多事件共享同一账户可用余额，禁止重复花钱。
`NewShares.paid_amount` 保留对应认购的逐账户实缴成本。后续行政操作须一次增加净投入
`invested`，不得再次扣现金或增加二级市场佣金；本候选本身不改变账户成本。

长期限售的组端卖出权威尚未实现。本候选的 `validate_rights_offering_capability` 对所有非空
`lock_until` 显式拒绝，包括由恢复数据进入候选的方案；纯公司领域仍保留限售事实表达。
生产命令和恢复入口接线前必须复用该门禁，不能只在名册标 `Restricted` 却让交易账户可卖。
`lock_until=None` 的定向对象属于无长期限售的游戏简化，不代表满足第59条；真实长期限售支持须另批设计。

拆股／缩股领域账簿沿用整数比例及汇总取整的游戏简化，不将其视为现行 A 股统一取整规则。以真实 R 日快照计算后，若换算后的总发行股数为 0，`allocate_share_split` 与 `ShareSplitBook::register` 在冻结成功前显式拒绝，账簿校验、严格 JSON 与直接内存 `SaveSlot` 恢复使用同一约束。领域错误及直接内存恢复的 `Error::source` 保留事件、证券、方向、比例与换算前后数量；严格 JSON 拒绝不可执行回执，`decode_save_slot` 仍沿用现有通用解码错误边界，宿主继续脱敏展示。普通账户可换算为 0 股，只要发行人总发行股数仍为正；不补足 1 股、不自动产生现金替代。该约束维护既有 `ShareRegistry`／`CompanySpec` 正发行股本不变量，不由买入申报的 100 股单位推导，也不改变[ADR-0039](decisions/0039-rights-placement-split-scope-and-toggle.md)的功能范围。

## 分红与送转独立候选的支持边界（2026-10-26）

`session/corporate_actions/distributions_day_end.rs` 准备隔离候选；native civil 已接现金分红
与个人税收缴；受限 native 送转的盘前接线见本文末节。
调用前安装 R1 最终名册/税账；返回分红、送转、公司账面与发行人候选、逐笔税前股利、
税款退出、新股可用数量，以及包括具名 `External` 持有人的来源和回执。`Paid`/`Credited`
均是待提交候选状态，必须经组端真实行政回执逐笔勾稽后才能发布。公司 `Simple` 合成
股利按 ADR-0038 保留来源；外部股东付款不计入游戏账户现金注入。真实账户与除权须经 Admin 事务提交；长期限售仍未接线。

- 税务模式沿 ADR-0040：`FlatWithholding` 按配置比例对每位账户持有人（包括机构）
  的税前股利直接代扣，half-up 到分，不创建个人税账；具名外部股东无游戏账户，
  不代扣属于引擎边界。`AShareIndividual` 复用已配置个人税账，非个人保持
  `TreatmentNotConfigured`。`Exempt` 不自动开税账，仍保留 ADR-0040 允许的显式装配税账。
- 个人差别化税务依据[财税〔2015〕101号](https://fgk.chinatax.gov.cn/zcfgk/c102416/c5203902/content.html)
  第一、二条（2015-09-08 起执行）和其第三条继续适用的
  [财税〔2012〕85号](https://fgk.chinatax.gov.cn/zcfgk/c102416/c5204415/content.html)
  第二、三、六条（2013-01-01 起执行）：持股档位、转让扣收、FIFO、日终净增减与
  资金不足通知补足。候选按最终真实可用现金部分收缴，保留实收与未清税款；不含在途
  buy purse，不以默认零余额代替缺失账户。登记日冻结权利，登记后卖出不减少已登记股利。
- `cash_dividend_tax::day_boundary::TaxDayBoundaryFacts` 由 native civil shadow 接入 ordinary
  REG、个人税日及同日现金分红 capture，随唯一 INFO/Admin 固定成功尾安装。
  `civil_cash_distributions` 从同一次完整行政基线读取 raw cash，沿候选原顺序准备 cash／tax-only
  stages，并在同一个 pending/id 的最终 INFO 与镜像检查中勾稽原 intent、借用的原 typed operand、
  真实税阶段和完整最终账户；成功 Commit 后才随固定尾安装公司付款、`Paid`、payment 与
  collection 候选。零税额不制造零额 `WithholdingTax`，不借 buy purse 或补钱；当前已验证
  native 现金付款、简税代扣、真实转让后的个人收缴、恢复续行不重付和最终投影拒绝回滚，
  不代表 WASM 或完整送转链已完成。每证券一次
  消费 typed REG 完整自然日、最终确认全集与实际完成水位，零成交日和
  休市日也记税日零净额。每本税账从自己的 `opened_on` 完成 frontier 核完整 opening lots，
  其后每个 REG 自然日与税日的净额、取得和处置须逐项一致；当前同数量、同 lot 尾部不能
  替代历史勾稽。登记归属只接受无任何公司或单位 movement 的普通路径，完整物理与税
  lots 必须一致；physical90／tax100 显式拒绝，不按比例推导原权益。历史旧式无最终确认
  绑定的 REG、公司或单位 movement、未经核实的法定限售资格均不支持。本入口不确定
  公司变动的 R／R+1 税务日，不改变下述个人 `BonusShares` 拒绝门。
- 送转分配复用现有 `company::stock_distribution`，批准总量必须显式给出。
  中国结算沪业字〔2026〕33号指南 2.5.1（四）7、深业〔2025〕68号指南
  2.5.4（十）支持账户碎股降序、同数系统随机、依序补一股，但没有给出统一汇总
  floor/ceil 总量通则。现有汇总可行区间与可复现 seed 是游戏算法约束，不冒充法定
  总量推导。现有每股 micros 只支持深市参数的子集：沪市每股六位，深市每十股六位。
  现行深市指南已删除零碎股转现金内容；候选不自行添加碎股现金补偿。
- 回购专户失权，复用名册 `IssuerTreasury` 排除（深市指南 2.5.4（六）明确专户不参与
  权益分派）。正向新股意图直接增加可用股数，不生成当日买入的 T+1 冻结；受限 native
  路径在 E 前一自然日 civil 的同次 Admin 准备 E opening，成功后 E 首拍可使用新股。
  其他独立候选不代表生产已接线。尚无组端长期限售权威，
  新股 lot 若仍受限则显式拒绝，不以名册限售冒充真实账户不可卖。
- [国税发〔1994〕089号](https://www.chinatax.gov.cn/chinatax/n810214/n810641/c102061/c102062/c5172932/content.html)
  第十一条明确送股按票面金额作为股息红利收入；
  [国税函〔1998〕289号](https://zhejiang.chinatax.gov.cn/art/1998/5/15/art_8409_11735.html)
  第二条明确转增免税仅限股票溢价发行收入形成的资本公积，其他资本公积分配个人所得应税。
  当前 `CapitalReserveConversion` 只扣减混合资本公积总科目，不能从 kind 推断免税。
  对有个人税账的转增，候选要求逐事件显式 `StockPremium` 来源：非空证据、覆盖金额与
  本次实际转增事实精确相等；缺失、其他来源、空证据或金额不符均 typed 拒绝。原来源
  随候选输出，不能用字符串标签冒充已完成来源追溯。R4/finance 后续必须从真实权威来源
  构造事实，正式受理与恢复须统一校验；本片不实现非溢价计税或资本公积来源追踪。
  个人送股 `BonusShares` 在该候选明确 typed 拒绝：既有领域税基按新增 lot 的未来持期
  计算本次红利，不能替代原登记权益的持期与转让关联。沪2026指南 2.5.3（三）4 的公式
  是“每股面值×送股比例×缴税税率×待缴税股份数量”。原股长期持有者不能仅因次日卖出
  新股而被改收短期红利税。本片保留 Flat／无个人税账送股与有凭据的溢价转增候选，
  完整个税送股关联留后续独立实现；`Exempt` 显式个人税账同样受此能力门约束。
  现有同日“先市场净额、后送转新 lot”分列税账仍是游戏简化：同日卖出与送转的
  合并净额与 85号第三条存在差异，不能宣称完整还原逐日日终净增减。

原文核对范围、取得日期、来源摘要和未验证项见
[候选冻结记录](../agents/branch-completion/distributions-dayend-r2-2026-10-26.md)。

## std-thread 初始静点接管的有限边界（2026-10-28 登记）

初始 lazy handover 不推进市场 tick、不产生交易事件，不改变既有金额、股数、费用累计、
同价 FIFO 和收盘竞价公开可见性。旧簿 `OrderId` 必须能按既有 desk 编码原值表示；
同价旧受理顺序必须与本地 serial 递增顺序一致；旧公开 cutoff 下每张单的可见分类必须
在 desk serial 域保持一致。不能满足时，在 spawn 和权威替换前显式报错，保留旧权威，
不重编号、不假造可见性。旧全局已消耗 ID 上界也进入新 desk 的取号下界，避免复用。

这是迁移期的有限安全出口，完整合法旧簿 handover 仍未实现：第二股旧全局 ID 无法按
现有编码原值表示、旧身份与同价时间顺序冲突、cutoff 分类不等价或旧全局游标超出本地
编码域时均可能被拒绝。持久化转入 desk 后的 `Order.seq` 使用本地 serial，不承诺旧
`seq` 数字恒等；本片仅在上述条件内保持 FIFO 行为。没有新增或改写 A 股交易制度；
价格时间优先和竞价信息公开边界仍以本文既有官方依据及已登记的游戏简化为准。

## 个人信息 anchor 与普通显式投递边界（2026-10-28）

个人信息持有期使用逐股冻结 Calendar 的三态 `InformationTradingAnchor`：首会话前
休市为 `NoPriorSession`，开市日期为 `TradingDay`，已有会话后的休市日期为
`DayBefore`。后两者同 ordinal 的期限位置相同，历史先后仍区分；这不改变 T+1、
撮合或 `PlanBook` 的逐股交易日定义。

显式 `RuntimeDriver::deliver_information` 的订阅资格仍是当前持仓与 watchlist；
该旧入口尚未覆盖仅有 pending sell 或 active plan 的订阅，属于不支持边界。
serial 与 Admin 信息观察入口包含 active plan；不能据 typed anchor 迁移宣称三个
入口的订阅资格已经完全一致。

native runtime 的 `GameSession`／`ProtocolSession` 自然日日结在同一次 Admin 事务内准备
全部 live Group 的本人获知、信念与两类记忆裁剪。观察使用原自然日的真实逐股
Calendar projection 和本次报告的 `disclosure_instant`，不把已前进的次日当作披露日；
休市日不制造市场 tick、不消费交易 intent 或市场 RNG。公开 callback 与日终存档读取
同版候选，成功尾只安装同次镜像及预验证的公司、自然日时钟、披露游标和事件序号。

日终先按原 watchlist、持仓及 active plan 完成本人观察，再裁 watchlist／price_memory；
保护集包含在手股、pending sell 与 active plan，不裁 `history_reads`。即使本人 INFO
检查未到期或命中同一观察签名，日终裁剪仍在同一个 Group 候选内完成。首个休市日的
可逆投影拒绝保留原状态并允许重试；本 authority 已实际安装市场收盘后的 civil 失败
保留收盘并终结该会话。受限例外仅是尚无首个 fatal、civil 尚未提交时，真实更正 prepare
返回 typed `ReportCorrectionDayEnd` 的 `InvalidInput` 或 `BusinessCondition`：保留已安装
市场、retained 和旧存档候选，只允许取消坏更正后重试 civil，不再发送市场 `Start` 或重跑
收盘。`UnsupportedOperation`、`SystemState`、INFO／输出 callback 即使返回同型错误、
内部不变量以及已提交 civil 后的失败仍终结会话；不得按“不是 SystemState”自动扩大例外。
构造／restore 的静点标记不冒充实际市场收盘。

本条只说明普通自然日 INFO 入口；Correction queue 的完成、资本动作、税日 producer、
三宿主恢复流程与 WASM runtime 迁移仍按各自登记边界验收，不由本条宣称完成。

### 共同 Calendar 日终来源与逐股迁移（2026-10-28）

std-thread runtime 的 `DayEndCalendar` 明确区分 `MarketTick` 与 `ManualDayEnd`。
自动市场日终只消费本 authority 已成功提交的原始 `MarketTickContext`，并验证末拍完成、
当前日期、owner、bindings 与 tick layout；手动日终消费同 owner 的 `MarketDayContext`，
以 World 已提交 tick 通过同一纯 Calendar projection 验证，允许日内与末拍手动日终，
不依赖 Session 尚未安装的 `day`，不补造市场拍。`PreparedDriverCalendar::Static` 模式不发起日终，tick 0 不构成日终输入；
合法 Closed 日期的手动 DayArc 保留原 API 范围，逐股无当日事件，不补造 Tick。

`Group` 在退役 undo 或解冻前校验日历、所有活动公司 execution 与个人事件时间顺序；
休市股个人计划保持原值且 T+1 不解冻，开市股计划按本股交易日 ordinal 迁移。
公司计划尚未接通的 execution 无论本股是否休市都显式拒绝。真实 Drain 成交推进及余单
释放完成后，更新后的 `PlanBook` 经唯一 clone/swap 算法迁移，避免旧簿覆盖回流事实。
这些前置校验不改变 Drain、权益及其它原有失败的 undo/Abort 契约；不据此宣称公司回购
真实执行、完整授权或其它未实现规则已支持。合成沪深差异测试不代表官方未来交易日安排。

### 运行镜像同步与日终存档（2026-10-29）

`RuntimeSyncPolicy` 控制 std-thread runtime 的展示镜像安装时机。新局默认 `EveryTick`；
`DayEnd` 日内仅提交真实市场进度与逐拍事实，拒绝 Player ingress；`OnDemand` 在显式
`flush_now()` 或运行账户消费前同步镜像。三档均在市场日结完成后同步完整权威，再执行
自然日日结。现金单位仍为分、持仓单位仍为股，撮合、费用和 T+1 规则不随镜像策略改变。

公开运行 `tick/seq/day/phase` 表达已提交进度；`mirror_origin` 明示可滞后的镜像来源及相位。
显式 flush 不增加市场 tick 或事件 seq。只读日终候选查询不触发 flush，未完成的自然日不能
因镜像已经同步而冒充保存成功。WASM 串行管线只支持 `EveryTick`，不支持日内镜像解耦。

成员加入在旧 World 静点捕获完整账户、本人态、计划、在途委托、真实 RNG 与来源，
先准备新 authority，再登记 receiver；成功后只移动已验证值，并停止旧 worker，
不重新撮合、不 Drain 或重发历史。准备失败保留旧权威；无法完整回收时显式终结会话。

`ProtocolCheckpoint` 只支持无 runtime authority 的健康 serial 机械回放。它不复制 live World，
不能撤回 runtime 已提交的交易、Full／retained，也不能清除 terminal 后重建旧 driver。
`rollback` 对这些调用显式返回错误并保留当前状态；正常取消坏更正后的 civil 重试继续遵守上述受限例外。

## 公司资本行为登记接线（2026-11-01）

native 自然日日终的市场成交和税账候选现同时推进已批准的送转、配股／增发、拆缩股账簿的公告与登记；登记快照来自同一证券当日市场收盘后的名册，缺少名册、错过日期或账簿与公司事实不一致时整次候选失败。拆缩股仍是整数比例和既有取整规则的游戏简化，不代表现行 A 股统一制度。

登记本身不代表结算完成；配股、受限送转与拆缩股接线以以下各节记录的范围为准。本轮尝试读取中国结算深圳分公司《证券发行人业务指南》原件时连接超时；现行性未能于 2026-11-01 复核，仍仅可追溯上文记载的 2026-10-26 材料核对，不能据此声称本轮已核对现行制度。

配股缴款期的生产候选使用真实组账户在现金分红及扣税后的可用现金，通过同一 Admin 事务逐笔扣款；NPC 默认足额认购，现金不足部分弃配，玩家可在当前缴款日终提交显式认购申请。失败发行的结算日按实缴本金通过同一 Admin 事务退款，并在公司账面记录零发行；不计算退款利息。成功发行仅接通下述受限路径，其他不支持的成功发行会显式报错；不能把已缴款视为完整发行，也不能用于完整游玩验收。

串行／WASM 宿主尚不能推进未完成公司权益账簿，跨自然日会显式拒绝；native 的送转与拆缩股只开放下述受限路径，对不满足下述受限路径条件的成功配股在除权日首个市场操作前显式拒绝。缺少完整股份、opening 或除权锚时禁止开市，避免以旧单位或旧参考价继续撮合。失败配股仍可进入退款日。此门禁不代表相应结算已完成。

`Exempt` 或 `FlatWithholding`、无个人税账、全部股份流通、无库存股和限售、同证券无同日组合时，native 已接入面向全体股东配股的成功路径：缴款后以实际新增股数与登记股数的精确有理比例在除权日前的 civil 事务准备 Desk 参考价，最终按既有银行家舍入到分；上市日按实际认购将发行 movement 纳入同一 `ShareSettlementDay`，并用 `SharesWithCost` 在同一 Admin 提交真实账户股份与实缴成本、登记公司资本。新股不重复扣款；失败发行只退本金。该路径的生产旅程和恢复测试尚待集成环境执行与独立复核，不能宣称验收通过。`FlatWithholding` 仅对现金股息按配置代扣，不对配股新股再次计税；`AShareIndividual`、长期限售、同日复杂组合、公开配售和定向发行的成功闭环仍不支持。

native 玩家配股认购命令（2026-11-01）：仅受理上述受限的 `RightsToAllShareholders` 方案，且账户必须有本人登记权利、请求股数不得超过本人额度；每账户每方案仅接受一笔申请。成功回执只证明申请进入当前自然日结算队列，不预扣现金、不承诺获配；日终仍按账户真实可用现金逐笔缴款，余额不足部分弃配。待处理队列不能由公共 `SaveSlot` 保存，`GameSession::save()` 明确拒绝，须先完成日终结算后再从可恢复存档继续。串行／WASM、个人税账、非全部流通、库存股、限售及同证券同除权日组合仍不支持。此为游戏内认购流程，不等同于现实交易所／中国结算申报或缴款渠道。


## native 单证券 Exempt 送转盘前接线（2026-11-01）

本切片仅开放单证券、`Exempt`、无个人税账、全部流通且全部由真实游戏账户持有的独立送转。
R 的登记快照必须来自真实市场收盘后的 typed REG；E 仍是 R 次一交易日。
在 E 前一**自然日**完成经营、REG、现金权益及配股候选后，送转候选于同次 Admin 准备
Group 可用股份、Company 发行股本／账面资本、Desk 除权参考价与公开镜像。
首 Commit 前勾稽冻结分配、原 typed operand、完整账户与 Desk 请求，固定成功尾才发布。
保留累计 `invested_cents`／`recovered_cents`，不新增现金、费用或 `frozen_today`。

REG 新增必填可空的 `pending_opening`，只保存原 `ShareDayFrontier` 与一个 `FrozenOpeningMovement`。
物理 holdings 已含新股，civil `settled_on` 仍停在 E 前一自然日；不伪造 E 的收盘或零成交确认。
送转使用单位相同的 `NonTradingTransfer`，来源覆盖全部登记 lots，包括新股分配为零的 holder；
零分配不生成零数量 Admin 操作。E 首拍使用新可用股数及除权参考价，E 收盘以真实市场绑定消费 opening，
后续恢复不得重复入股或除权。Rust 完整 `SaveSlot` 同时检查公司资本、登记、唯一除权锚、pending／消费位置、
盘前真实账户数量与冻结状态；Web 同步严格结构，不迁移缺字段旧档。

仍拒绝多证券、`FlatWithholding`／`AShareIndividual`、个人税账、Treasury／External、长期限售、
同证券重叠公司行为、活动／待续行计划、未执行玩家委托、Drain 后仍冻结／在途或登记 lots 漂移。
送转与拆缩股不能组成同日组合；每户单笔入股超过 `i32::MAX` 显式拒绝，维护现有 Group undo 数量边界。
真实市场收盘已经安装后的后续失败沿原有 terminal／poison；未涉及新收盘的 precommit 拒绝只 Abort 本次 Admin。
部分 Commit 失败不得用 Session checkpoint 伪装恢复。

2026-11-01 实际重新读取上交所[发布页](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/c_20260424_10816482.shtml)
与其[2026 年修订附件正文](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/10816482/files/e3a3b98e36894edca77841837936e006.docx)：
自 2026-07-06 施行，4.3.1 要求 R 次一交易日除权，4.3.2 给出流通股份变动比例公式并要求行情前收显示除权参考价，
4.3.3 以该参考价计算涨跌幅。深市继续使用前文单列公式，不能用沪市口径代替；中国结算送转上市时序与
碎股分配沿前文已登记材料及简化，不声称本次重新取得中国结算指南正文。

本节记录代码接线范围；本轮 Cargo 编译、代表旅程执行及独立 Sol 复核由主任务统一安排，尚未据此宣称验收通过。

## native 单证券 Exempt 拆缩股盘前接线（2026-11-01）

整数比例拆股／缩股现复用上述真实 R 登记与 E 前一自然日的唯一 Admin 事务，
同时准备真实账户股份、Company 股本／面值事实、Desk 除权锚与 REG 跨单位 opening。
此能力是 ADR-0039 的游戏简化：整数比例、汇总取整及余数分配沿既有冻结回执，
不宣称是现行 A 股统一拆缩股制度；官方来源边界仍沿本节上文与拆缩股领域台账。
E 首拍使用换算后的可卖股数与参考价，E 收盘只用真实成交确认消费 opening，恢复不重复换算。

范围仍限单证券、`Exempt`、无个人税账、全部流通且全部由真实账户持有，
无 Treasury／External、长期限售、同证券重叠行为、活动计划、未执行玩家委托、
冻结或在途资源。登记 lots 漂移、超过 `i32::MAX` 的单户净变动或账户 `u32` 股数溢出显式拒绝。
缩股允许普通账户变为零股，发行人总股数必须为正；不补足一股、不补现金。
非零持仓保留累计 `invested_cents`／`recovered_cents`；零股沿现有 `ShareReduction`
清仓语义清零累计成本和回收额，不把被取整消灭的股份伪造为卖出成交。
同一 Admin 的 INFO 候选对归零账户结束当前持仓观察态及 active epoch，保留受挫与退出历史，
不生成卖出身份、盈亏或退出冷静期；Abort 不安装个人态候选。
严格恢复同时核验 Company、冻结换算、唯一除权锚、跨单位来源与 pending／已消费位置。
未安装真实收盘时的投影拒绝 Abort 整笔候选；已安装真实收盘后的失败沿现有 terminal 门禁。

本轮新增双方向首拍／恢复／真实收盘旅程与缩股至零、投影 Abort 代表测试；
编译、执行及独立复核由主任务统一安排，尚未据此宣称验收通过。


送转与拆缩股的同一 Admin INFO 候选现使用同次真实 Desk 的
`reference_price / expected_last_close` 作为统一价格单位换算比率，不按每户碎股取整结果
生成不同价格。当前持仓的 `entry_reference_price`、`peak_price_since_entry`、
`last_buy_price`、active `HoldingEpoch.last_own_observation.price` 以及所有本人价格记忆
（包括未持仓关注者的首次／最近／最高／最低锚）同步换算；各观察时钟、epoch 入场日期、
订单身份、受挫和退出历史保留。个人价格落分使用整数半偶舍入，若某个已有正价格锚换算后
不足一分或超出 `i64`，显式拒绝整笔候选，不补造价格。该记账单位转换不是一次新观察，
不新增成交、失败经历或冷静期；Abort 保留完整原个人态。

除权后的首笔真实成交前，市值和本人观察使用 `last_close` 中的除权参考价；
首笔成交后使用真实 `last_price`。native 的 `StockPublic::observation_price()`、
策略 `StockView`、Session 公开快照及行情进度投影使用该读面。
Desk／Session／`SaveMarketSnap.last_price`、成交回执及原始价格历史继续保留真实成交事实，
不为盘前换单位补造一笔成交，也不追溯改写 K 线或历史序列。

Desk 的 E 日空拍占位 K、当前新增连续 tick 样本及分钟收盘观察点同样使用上述参考价；
原来已经记录的价格／分钟历史保持原值。空簿开盘清算与收盘 Drain 的零成交占位沿用
同一读面，不增加真实成交计数。首笔真实成交仍按既有逻辑重建当日 OHLC，之后的新
观察点使用真实成交价。
## native Simple 回购执行接线（2026-11-01，待独立审查与统一测试）

本批代码接通 `ReduceCapital`／`CancelOnCompletion`、基准股份单位、同证券单活动方案。
批准仍沿原 Admin 一次注资；现场资格由当前 Session 的完整 Simple 公司行为声明与 finance、
真实公开库、待处理更正及本拍冻结 Calendar 生产，只读 cut 随 Start 传入 Group。
`None` 与 `NotModelled` 不视为允许。存在未实施完的发行而缺少邀请书／注册启动事实时显式拒绝；
没有待实施发行，或已真实完成入账的发行可证明不在实施区间。跨拆缩股、待入账送转及
无法解释的 Shock 不支持。模型外内幕事项／其他监管禁限没有 producer，明确作为 Simple
封闭事项模型的简化，不宣称能认证现实世界全部资格。

沪市指引第17—20条、深市第16—19条已于本日从上文官方原件重新读取：一般12个月／
维护价值3个月自最终方案批准日起按公历月数校验；重大事项未披露时限制下单；连续竞价
每天最多一笔整手限价单，价格不超过方案价、真实笼子与涨停下一档。现有 setup 仅接受
显式涨跌幅证券，不能把此路径外推到无涨跌幅日。当前实际执行只开放减资用途。
证据与摘要见[本批核验](../agents/repurchase-production/规则核验.md)。

真实 Desk 产生的 Accepted／Rejected／Fill／Return／Closed 经过原 Group consumer 校验，
Drain 后由同次 civil Admin 消费完整 tail 前缀。唯一公司 book 按日汇总真实成交与费用；
cursor 确认、尾流清理、精确回收、canonical 计划关闭与公司 binding 共用原 PendingAdmin。
窗口截止、数量不足一整手或预算精确耗尽后回收 `budget−gross−fees`，零余款不发零额操作。
正库存于完成后的首个日终按本方案实际 Treasury lot 注销，同步 ShareReduction、REG、
总股本、面值资本，不自动除权；零成交保持 Completed。前案库存未注销前拒绝后继交叠。

native 日终 restore 以公司 book／finance／binding／canonical Plan／cursor／专户现金和库存
交叉校验并重建执行投影，拒绝残留公司 execution；不保存或补造活动 child。WASM 真实执行
及 Web 全链验收不在本片验证范围。以上是代码接线范围，三个短真实场景已通过静态编译检查，仍交主任务统一执行，
独立 Sol 审查亦未完成；不据此登记全链验收通过。

2026-11-01 核对沪市[《交易规则（2026年修订）》4.3.2](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/exchange/c/c_20260424_10816482.shtml)与深市[《交易规则（2026年修订）》4.4.2](https://docs.static.szse.cn/www/lawrules/rule/trade/current/W020260424690713155663.pdf)，分别使用流通股份变动比例与股份变动比例。本实现仅支持全部股份流通的受限配股路径；银行家舍入仍为游戏简化。
