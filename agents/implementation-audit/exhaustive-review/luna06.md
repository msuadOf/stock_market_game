# ADR-0011–0013 散户观察与经历链独立复核

- 审计基线：用户指定 `08e4fc7`；审计工作树 `.worktree/implementation-reaudit`。
- ADR 阅读：逐份连续阅读至 EOF，ADR-0011 103 行、ADR-0012 65 行、ADR-0013 51 行，共 219 行；另读根 `AGENTS.md`、`docs/principles.md`。没有用旧审计代替 ADR 原文阅读。
- 审计方式：静态追踪分钟构造、观察快照、散户策略、风险目标仓位、经历写入及真实成交回执调用；未运行测试、未更改产品或 Git 状态。
- 依据边界：此记录不重新取证 ADR-0011 中交易所规则；这些 ADR 明确把窗口与行为阈值定义为游戏假设。

## 结论

- 旧 G08 仍成立：散户实际观察和结算成交继续写 legacy 经历；dated 写入、失败影响日期衰减及相应输入没有接入散户生产消费链。机构 dated 能力是另一条消费链，不能替代散户。
- 旧 Q02 的代码事实仍成立：`record_public_history_read` 没有生产调用，只有定义和测试；公开历史“实际主动读取”的事件边界仍需产品/策略消费方明确定义，不能把公共观测缓存构建当成个人阅读。当前 `docs/open-questions.md` 没有名为 Q02 的该项，故这里复核历史审计标签与代码事实，不声称它仍是该文档中的开放问题。
- 没有确认 G08/Q02 之外新的独立遗漏。现有散户观察→目标仓位→订单→真实撮合→成交经历主链基本兑现，限定的 dated 生命周期契约是明确例外。

## ADR-0011 条款矩阵

| 原文范围 | 复核结论与代码证据 |
|---|---|
| 上下文、制度口径、参数为游戏假设（ADR 行 8–27） | 时间轴实现显式命名为游戏分钟；没有发现把其声明为连续竞价制度时长的生产用法。连续撮合收尾从总 tick 中扣开盘、收盘集合竞价后映射 240 桶（`packages/engine/src/session/pipeline/continuous_tick_finalizer.rs:60-72`）。本审计未重查官方规则。 |
| §1 市场时间映射、跨桶沿用最新价、宿主速度隔离（行 30–37） | 已接入：每个连续 tick 计算完成分钟数，并把新增桶按冻结的最新价逐桶追加（`continuous_tick_finalizer.rs:107-143`）；不依赖 UI、Publisher 或墙钟。 |
| §1 绝对分钟、无成交陈旧价、批量连续输入、精确历史（行 38–44） | Session 增量路径使用绝对日偏移追加分钟；量为 0 的行情更新不制造成交（`continuous_tick_finalizer.rs:112-143`）。批量转换要求 tick 从 0 连续递增并拒绝缺口（`packages/engine/src/observation.rs:180-236`）；窗口端点按具名跨度构造（`:261-315`）。 |
| §2 具名分钟/日窗口、真实活动日开盘、窗口过去信息及跨度（行 46–59） | 已接入：`market_price_path_observations` 用权威分钟、完成日历史和仅在活动日 K 有成交量时提供开盘价（`packages/engine/src/session/views.rs:124-160`）；分钟构造具名 1/30 分钟与日收益（`observation.rs:261-315`）。完成日连续性与精确端点逻辑见 `observation.rs` 校验/窗口函数。 |
| §3 等权市场、覆盖数、缺失处理（行 61–69） | 接入散户观察：只聚合存在收益的股票并计算覆盖，空值不补零（`observation.rs:318-365`）；散户快照从公共价格路径的 30 分钟收益构造游戏内等权背景（`decision_snapshot_capture.rs:538-563`）。 |
| §4 独立成本、持仓权重、账户净值和回撤（行 71–78） | 风险快照从账户现金、持仓、权威市价/成本逐户生成；非正成本过滤为不可用（`decision_snapshot_capture.rs:300-365`）。生命周期峰值来自个人经历状态，不从短窗口代替。 |
| §4 风险先于非风险动作；B01只读及执行约束（行 79–82） | 散户判断先选持仓风险，再进入趋势和随机到达逻辑（`packages/engine/src/behavior/decision.rs:67-120`）；限价子单仍检查可负担量、整手及可卖量（`packages/engine/src/strategy/zi_noise.rs:239-288`）。 |
| §5 权威分钟存档、重建派生值、共享行情/独立风险、宿主共用（行 84–87） | Session save/load 包含分钟历史；恢复校验股票键集、严格递增、正价和当日完整分钟（`packages/engine/src/session/persistence.rs:354-415`）。快照的公共行情一次构造，账户风险逐账户采集（`decision_snapshot_capture.rs:152-175, 300-365`）。 |
| 不做及后果（行 91–103） | 未发现 UI 分时来源、补造成交或代表账户替代。权威 `price_history` 仍保留；此处不是缺口。 |

## ADR-0012 条款矩阵

| 原文范围 | 复核结论与代码证据 |
|---|---|
| 背景（ADR 行 8–18） | 当前生产决策已由标准分钟风险输入驱动；不能以仍保留的 legacy `decide` 入口推断它是 Session 主路径。 |
| #1 风险先于行情且不受 `arrival_rate` 门控（行 20） | 已实现于 `behavior/decision.rs:67-120`；该风险评估发生在普通行情信号和随机到达前。 |
| #2 标准 30 分钟/5 日与不足历史语义（行 22） | 观察由 `views.rs:124-160` 构造并传入散户决策；趋势不足在策略中保留 `InsufficientHistory`，无秒级 fallback。旧审计已核此分支，本次没有找到相反生产调用。 |
| #3 等权市场、无行业伪信号（行 25） | `decision_snapshot_capture.rs:538-563` 提供等权 30 分钟数据，没有看到散户虚构行业观测。 |
| #4 持仓优先与全市场发现、独立抽样（行 27） | `behavior/heuristics.rs:25-53` 先按账户 RNG 抽持仓优先分支，再可从关注股票或全市场选股；仍逐户调用确定性 RNG（`npc_decisions.rs:160-190`）。 |
| #5 六种动作、理由、目标/期望/可执行差额与 T+1（行 29） | 目标判断/执行数量分离；风险以个人成本和权重计算（`behavior/decision.rs:69-120`），卖单转换受 `sellable_qty` 限制（`zi_noise.rs:271-285`）。 |
| #6 单股硬上限/股数下限修订（行 31–35） | 当前目标步幅不按证券数计算，并允许加仓目标到全账户权益（`behavior/heuristics.rs:157-163`）；无旧上限遗漏。 |
| #7 子单、费用现金、整手/零股、不保证成交（行 37） | 买入转换调用 `affordable_buy_qty`，卖出调用 `a_share_sell_qty`（`zi_noise.rs:255-285`）；成交与否由订单簿回执决定。 |
| #8 已审视股票的旧单协调规则（行 39–42） | Retail 输出 `reviewed_stocks`（`zi_noise.rs:203-208`），Projection 使用 `ReviewedStocks` reconciliation scope，并只对真实 review 记录 `observe_stock`（`npc_state_projection.rs:249-303`）。 |
| #9 瞬时判断不存档、经历另存（行 44） | 判断是当次派生输入；经历另有权威存档结构。保存字段存在不等于 dated 生产事件已经接通，具体见 G08。 |
| #10 250 日、非正成本不可用但权重保留（行 46） | 日窗截取保留末尾 250 个连续日及绝对序号（`views.rs:146-148`）；非正成本被过滤而权重独立计算（`decision_snapshot_capture.rs:349-365`）。 |
| 可证伪假设/验证与后果（行 49–65） | 静态调用与数据路径支持契约；未运行测试，不报告通过。集合竞价/无对手方零成交不属于故障或应注入的成交。 |

## ADR-0013 条款矩阵

| 原文范围 | 复核结论与代码证据 |
|---|---|
| 逐户权威经历、隐私及仅接受观察/实际成交写入（ADR 行 5–15） | 散户经历独立存于账户状态。观察快照只处理注意力调度实际接受的账户（`decision_snapshot_capture.rs:103-171`）；只有投影认可的 Fill receipt 才进入成交聚合（`retail_projection.rs:341-385`）。 |
| 同订单部分成交去重、补仓保留经历、清仓冷静期、持仓+8（行 17） | legacy retail writer 保有 order ID、生命周期峰值和清仓冷静期逻辑（`packages/engine/src/experience.rs:214-262`、`experience/position_transition.rs`）；实际跨 tick 按订单聚合后写经历（`retail_projection.rs:374-385, 518-528`）。关注集合在审视后保留持仓并剪枝（`npc_state_projection.rs:294-303`）。 |
| 5%受挫、120分钟、无时间衰减旧条款及参数性质（行 19–24） | 传统散户路径仍通过 legacy transition 处理原始受挫计数/市场分钟冷静期（`experience.rs:171-180, 214-262`）；旧“不衰减”已由 K5 更新取代，不能据此认为当前符合新要求。 |
| 权威经历校验、序列化及 20,000 户历史成本（行 26–39） | 经历字段有存档与校验，但日期生产接线仍缺；ADR 中性能数值是历史结果，本审计未运行压力测量。 |
| K5 日期修订：20 交易日衰减、失败日期、忍耐/风险压力、双时钟（行 41–51） | `ExperienceMoment` dated writer 与 `failure_influence` / `is_long_stuck` 已存在（`experience/feedback/lifecycle.rs:231-370`、`feedback/inputs.rs:10-54`），但全仓生产调用检索只发现分配体验委托对输入函数的使用，没有 Retail Session caller。Retail 观察仍调用 `observe_position`（`decision_snapshot_capture.rs:345-348`），真实成交仍调用 `record_fill_with_order`（`retail_projection.rs:518-528`），决策传入的只有市场分钟（`npc_decisions.rs:179-190`）。因此 K5 dated 消费对散户生产仍断链，归旧 G08。 |

## 旧结论复核与新候选排查

### G08

旧结论成立，且生产调用证据相互闭合：

1. 接受的散户注意力观察会逐户构造快照并调用 legacy `observe_position(code, price, market_minute)`（`decision_snapshot_capture.rs:162-171, 345-348`）；即使历时日历信息可由 Session 获得，该调用未传 `ExperienceMoment`，dated own-observation 与 failure-event 日期不会写入。
2. 撮合 receipt 只在 `ReceiptKind::Fill` 被聚合（`retail_projection.rs:347-385`），但 Retail 分支调用 `record_fill_with_order`，非 `record_fill_dated`（`:512-528`）。机构分支单独调用 `record_institutional_fill_dated`（`:529-607`），不能替代 Retail。
3. `failure_influence(as_of)`、`is_long_stuck(..., as_of)` 定义/调用链在散户 Session 未找到调用者（全仓搜索分别只见 API、分配 experience 委托与测试）；散户启发式仍直接消费 `consecutive_failed_buys`（`behavior/heuristics.rs:109-143`）。测试覆盖纯函数不等同真实会话接线。
4. 当前 Retail 成交/观察路径仍保留 legacy 5%受挫和 120 市场分钟行为。G08 应只指向缺少日期和 K5 消费的散户链，不要求重复实现机构日期/衰减，也不把整个 B03 标成未实现。

### Q02

`record_public_history_read` 生产调用搜索无命中；观察根的个人行情记忆调用 `observe_price`（`packages/engine/src/session/decision_chain/roots.rs:312-337`），独立 public-history writer 只有定义和测试。专业历史数据可能作为技术输入被实际消费，但这不意味着每次公共缓存构建是个人主动阅读。计划关于主动读取记忆的要求与消费方/事件边界仍应明确；沿用 Q02 历史边界，不提升为本轮新的 G。

### 额外遗漏及反证

- 没有发现分钟漏桶：tick 映射完成桶、沿用该 tick 权威最新价并按绝对交易分钟追加；Session 保存校验要求精确包含每个已完成分钟（`continuous_tick_finalizer.rs:125-143`、`persistence.rs:377-415`）。
- 没有发现散户风险落在随机到达后：风险持仓评估在非风险行情路径及随机到达之前（`behavior/decision.rs:67-120`）。
- 没有发现目标意图伪造成交：策略只产生合法意图；经历只由实际 Fill receipt 进入投影（`retail_projection.rs:347-385, 518-528`）。
- 没有发现机构链可核销散户 G08，或“每次共享行情计算”即可核销 Q02 的依据。
- 本轮未发现额外独立候选。以上是静态代码审计，不代表测试、性能或当前交易所规则已重新验证。
