# 批次 156：engine-foundation 历史候选复核

## 范围与依据

逐篇连续读取指定的三个历史候选源至 EOF，源文件行数和 SHA-256 与任务清单一致。当前代码基线为 `43b1aa5`；该提交只更新审计文档，engine 实现基线来自其祖先 `8cf34a1` 的 OOP 状态与行为聚合改造。另读本 worktree 的 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、ADR-0017、ADR-0019，以及实现审计总账、coverage index、核心账务复核。未运行测试或构建。

源文件里“候选设计，未实施”只说明当时状态；它们不是当前实现指令。当前决定以 `43b1aa5` 源码和之后的正式 ADR 为准。这里仅做静态回核，不对官方规则现行性作重新取证。

## 01：engine 基础与 Account

**旧的 Account A03 字段封装建议已由后续实现落实。** 当前 `Account.id/state` 和 `AccountState` 的账务/策略字段均为私有，公开查询走 `id/kind/cash/strategy/positions/position`；账户用 `Arc<AccountState>` 做 COW，`DerefMut` 已移除（`packages/engine/src/account.rs:94-138`）。恢复路径使用 crate-private `restore_balances`，策略恢复/替换使用 `restore_strategy`；NPC 策略更新仍经 `AccountBook::get_mut`，存档策略恢复也先取账本可变账户（`account.rs:140-154`、`session/pipeline/npc_state_projection.rs:195-207`、`session/persistence/v2.rs:427-438`）。生产调用因此保留 AccountBook 页面缓存失效路径。A03 属已实施历史动作，不应再登记为未修复或重复重构。

Account 的结算所有权、Money 分单位、T+1 持仓与 session 实收费收据口径仍有明确边界；ADR-0017、trading-rules 和核心账务复核未支持因对象重构更改交易语义。旧源指出的 `apply_buy` / `apply_sell` 文档漏列过户费仍可复现：`apply_buy` 文档写“成交额 + 佣金”，卖出文档写“扣两项费用”，实现实际均计算过户费（`account.rs:277-286,390-397,311-320,420-432`）。这是说明文字契约缺口，不是费率或结算代码缺陷；费率依据本轮未复核。

其余 01 retain 判断大体仍成立：无自有状态的行为 helper、指标函数、金额运算、sealed CalendarPolicy 和 CPU compute 边界不需为 OOP 包装。仍须分开看待源文件列出的非 OOP 风险：

- **可确认的 policy 摘要缺口：** `OfficialCoverageEntry` 声明 `source_digest` 用于绑定通知文本，但 `CalendarPolicy::compute_content_digest` 只编码交易所、年份、出处 ID 和日期区间，未编码 `source_digest`（`calendar/policy/coverage.rs:95-104`、`calendar/policy/validation.rs:79-105`）。恢复校验只要求摘要非空（`validation.rs:125-130`），故替换非空来源摘要不改变外层 content digest。该风险仍存在，属于政策完整性/来源绑定候选，非 OOP 迁移；不据此声称当前 A 股日历规则已被改写。总账的 G15 讨论模拟回退优先级，与此具体摘要遗漏并不等同；建议总审计判定是否作为独立新项。
- **反序列化不变量候选仍存在：** `CivilInstant` 仍 derive `Deserialize`，而 `new` 才检查 `second_of_day < 86400`（`calendar/date.rs:234-269`）。需继续追 SaveSlot/所有公开反序列化入口的可达性和后续校验，不能仅凭私有字段断言外部输入一定会绕过深度校验。
- **GameConfig 仍是公开字段 + derive Deserialize 的配置 DTO**（`config.rs:70-99`）。`new/validate` 存在，但公开字段可绕过构造校验；此旧观察未证明未校验配置可到达生产消费链。不要把候选不变量风险误报为 A 股规则变化，也不应只为对象化而私有化配置。

## 02：诊断、个人经历与指标

保留结论适用：`CausalCollector` 记录事实、报告与指标为投影/DTO、`AppendOnlyHistory` 是共享持久序列，经历/关注/价格记忆有账户级 owner，纯指标与启发式函数不需要生命周期类。它们没有要求新的对象迁移。历史审计亦明确只读诊断不参与撮合或资金结算，ADR-0013/0017/0026 的个人事实和实收费边界不能被对象重排改变。

源文件提及的以下风险经当前源码复核仍未消失，应作为候选缺口/既有边界留给总审计去重，不要归为本批 OOP 修改：

- `RetailStockExperience`、`RetailExperienceState`、`ExperienceFeedback` 的字段仍为 public（`experience.rs:74-114`、`experience/feedback.rs:107-122`）。外部可直接构造/编辑这些存档事实，绕过 `RetailExperienceState` 的写方法校验；这与“存档支持编辑”的原则并不自动矛盾，但不能宣称类型本身封闭了不变量。历史字段公开观察成立，是否需限制写口应由存档/公共 API 契约裁定。
- `ExperienceFeedback::validate` 仍在 `latest_moment=None` 时用 `u64::MAX` 作为 `latest_day`，且事件顺序主要按 `trading_day` 核验（`feedback.rs:196-244`）。该恢复边界须结合实际 SaveSlot 深度校验入口评估，不能仅由单类型 validator 得出用户存档必可绕过。
- `record_fill_dated`、`observe_position_dated` 先委托 legacy writer，成功后才提交 feedback（`experience/feedback/lifecycle.rs:231-330,335-370`）。被委托的 `record_retail_fill` / `observe_position` 在 checked 溢出路径可能已更改 legacy 字段再返回错误；所以“feedback 未提交”不保证整体 `RetailExperienceState` 回滚。源文件已正确限定这不是 whole-object 原子保证。若登记为修复项，须按失败路径证明完整状态前后相等，并与已有经历链缺口去重。
- `AppendOnlyHistory::push` 的长度增量仍用普通 `len += 1`（`experience/shared_history.rs:112,136`）。数学上的 usize 溢出路径可指出，但本轮未证明可达容量，故仅作低置信度边界，不升为产品缺口。

历史记录指出的机构经历与散户 writer、双时钟语义差异继续有效；不得把机构专用 writer 当成散户链已接入，也不得把未成交意图登记为成交经历。指标计算保留输入顺序与错误边界，不把展示指标提升为权威交易状态。

## 03：市场、订单簿与证据投影

保留结论成立：`Market` 聚合单股价格和 `OrderBook`，订单簿拥有撮合队列及索引；`Money` 是值类型，Observation/Evidence 以 DTO 和纯投影为主，只有 `UpdateStreamProjector` 拥有跨调用游标状态。未发现对象拆分必要性。当前 `Market` 的直接价格写口已收为 crate-private/测试 fixture 路径；例如 `fixture_set_last_price/fixture_set_last_close` 只在 `cfg(test)` 下（`market.rs:269-276`），旧源中的 `set_last_price/set_last_close` 公共写口结论已被后续代码取代。`record_filled_order` 当前为 `pub(crate)`（`market.rs:419`），也不是旧源所述的 public API。

历史源揭示的局部错误原子性限制仍成立：`OrderBook::place_inner` 在后续 checked 运算、成交索引或挂单序号失败前可先更新 maker/盘口（`orderbook.rs:424-463`）；这不等于已有契约承诺的任意低层错误强回滚。真实 session tick 另有候选提交/回滚边界，不能把低层 public API 失败面说成生产 tick 已部分提交。该范围和处理结论与 `reaudit-core-contracts.md` 一致，不新增重复 G 项。

订单价格时间优先仍由簿内 price/seq 决定；Account 费用仍按 ADR-0017 实收收据，而非历史 A02 提议的 Account 重算。现行 `Market`/`OrderBook` 对象化没有引入第二权威账本、改变交易优先级或 A 股规则。本审计未运行测试，不把已有测试源码或历史验收结果表述为本轮通过。

## 去重与结论

指定三源的章节族依次为：`engine-foundation-01` 的总览、Account、behavior、calendar、compute/config 与迁移；`engine-foundation-02` 的诊断、因果聚合、个人经历/反馈/历史/关注与指标；`engine-foundation-03` 的 crate/market/money/observation/orderbook/evidence、调用关系和验证映射。Account 字段封装属于已完成历史实现；其它 retain/OOP 不迁移判断无须追加代码动作。fee 注释、来源摘要绑定、CivilInstant 反序列化、经历状态完整原子性等独立观察则保留为候选，须并入总账时先核查 reachability 与现有 G/Q 去重。现行总账 G01–G68 和 Q 项没有用这些旧“候选未实施”字样自动核销；其中已明示相关近邻项（G08/G15/G17/Q01/Q03/Q09/Q11）仍按各自精确范围处理，不能以近邻编号替代上述独立边界判断。

**门禁结论：** 本批没有产品代码改动、没有新的已确认 A 股语义变化，也没有执行测试。对象迁移意见以当前状态确认/反证为主；来源 digest 等候选必须由总审计决定是否纳入，不在本批擅自改代码或新增正式 G 编号。
