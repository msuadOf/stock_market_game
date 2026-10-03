# sweep06：ADR-0011–0013 全文与散户生产链复核

审计目标为 `b76ece3`。工作目录 HEAD 为 `4ad5a2e`；已执行只读 `git diff b76ece3 -- packages/engine/src apps/web/src/save apps/web-wasm/src`，结果为空；三份 ADR 的目标提交 diff 也为空。因此下述生产代码与原文行号适用于目标提交。没有修改产品代码、Git 状态或运行长测试。

已连续阅读全文：`docs/decisions/0011-market-time-observations-and-position-risk.md` 103 行、`0012-retail-observation-to-target-position-loop.md` 65 行、`0013-retail-experience-memory.md` 51 行，共 219 行。另已读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`，核对 ADR-0021 与 ADR-0026 的修订范围。本目录没有更深层 `AGENTS.md`。

## 结论

本分片未发现需要新编号的独立遗漏。**G08 仍成立**，并且范围应覆盖散户失败日期、20 日影响衰减以及依赖 dated 生命周期的忍耐/风险压力读取。**Q02 仍成立**，实际主动读取公开历史的留痕没有生产调用；它不能靠共享公共分钟观测的构建自动核销。原始 B01/B02/B03 主链已有，不能因后续 K5 漏接将其全部重新列为未实现。

下表的“已实现”表示已找到权威生产调用与相应代码边界；不表示本轮执行测试通过。历史性能数值保留为历史测量，没有重跑或冒充当前基线。

## ADR-0011 逐章节、逐条款状态

| 原文与条款 | 状态 | 当前证据及排除理由 |
|---|---|---|
| 上下文：秒级点数不冒充市场分钟，逐户独立，行为参数不是制度（10、15、18 行） | 已实现的建模边界 | `packages/engine/src/observation.rs:20` 明确 240 个游戏桶；`session/pipeline/npc_decisions.rs:177` 从 tick/account 产生个体 RNG；`decision_snapshot_capture.rs:162` 逐接受账户构造快照。未使用代表性散户。 |
| 制度依据：连续竞价与收盘集合竞价分离（21 行） | 已声明且代码分离 | `continuous_tick_finalizer.rs:59` 从总 tick 扣除开、收盘集合竞价；`observation.rs:20` 明确游戏桶不等同制度时长。不新增交易制度判断。 |
| 决策§1 第1–3项：240 桶、tick 映射、多分钟补全、速度独立（30、33、37 行） | 已实现 | `observation.rs:242` 算比例；`session/pipeline/continuous_tick_finalizer.rs:69` 由连续竞价完成进度计算；138 行逐桶追加同 tick 最新价；没有 UI、Publisher 或墙钟参数。 |
| §1 第4–5项：绝对分钟、无成交仍记录、批量输入从0连续、增量不重建（38、40 行） | 已实现 | `observation.rs:180` 批量生成器，213 行检查 expected tick；`continuous_tick_finalizer.rs:116` 无成交以量0更新行情，137 行按绝对 day offset 追加。 |
| §1 第6项：精确端点、历史不足显式不可用（42 行） | 已实现 | `observation.rs:545` 精确 binary search，537 行构造 unavailable return 与 available span；损坏时间序列由428/448行校验显式拒绝。没有补零收益。 |
| §2：1/30分钟、真实开盘日内收益、30分钟高低区间、5/20/120/250完成日（49–54 行） | 已实现 | `observation.rs:261` 构造全部具名窗口；`session/views.rs:149` 仅 volume>0 的 active candle 提供开盘价；没有成交的昨收占位不会成为真实开盘。 |
| §2：过去信息、连续完成日序、报告可用跨度（56 行） | 已实现 | `observation.rs:448` 校验连续完成日序；569 行按实际 completed days 取端点；`session/views.rs:148` 提供尾部完成日，保留绝对日序。 |
| §3：等权、涨跌平家数、缺失覆盖、全缺失 None、非真实指数（63–68 行） | 已实现 | `observation.rs:319` 只累计 Some 收益并报告覆盖，空集合返回不可用；`decision_snapshot_capture.rs:538` 构造全股票30分钟等权背景。没有市值指数替身。 |
| §4：个人合法成本、净值与权重、溢出/非法值拒绝（72–75 行） | 已实现 | `observation.rs:365` 计算账户风险、381 行累加全部持仓市值；`decision_snapshot_capture.rs:307` 读取权威账户现金/持仓并传入成本，354 行过滤非正净成本。 |
| §4：参考权益、历史权益峰值、持仓生命周期峰值（76–78 行） | 已实现原始B03 | `experience.rs:138` 记录权益高点；`position_transition.rs:126` 记录本人观察持仓高点；`decision_snapshot_capture.rs:347` 更新该账户经历并取 peak_price_since_entry，不用短窗口高点。 |
| §4：风险先于追涨/抄底/噪声，执行仍受库存/T+1等（79 行） | 已实现 | `behavior/decision.rs:67` 先选择风险持仓，337 行之后才选非风险观察标的；`behavior/heuristics.rs:204` 区分目标与可执行差额；真实子单继续经过交易链。 |
| §5：分钟权威存档及单调/唯一/正价/长度校验（84 行） | 已实现内部权威契约 | `session.rs:2635` 保存、2826 行恢复；`session/persistence.rs:354` 校验股票集合、384 行校验完整 minute keys 和正价格。ADR-0025 的公开日终存档范围不允许据此要求公开日内存档。 |
| §5：公共共享、个人独立、诊断不消耗RNG、三宿主共用engine（86–87 行） | 已实现主链 | `decision_snapshot_capture.rs:159` 同 tick 构造一次 behavior_market；`npc_decisions.rs:179` 读取各自 account risk 与 experience；宿主依赖同一 engine。`tests/session.rs:4132` 已有诊断非权威重放测试。 |
| 不做§四项（91–94 行） | 保持边界 | B01只读计算没有撮合副作用；分钟由权威成交价而非 UI/Pub 帧生成；没有造成交或合并自然人。 |
| 后果：粗采样、有限窗口、旧price_history兼容（98–103 行） | 已实现/非新增待办 | `session/views.rs:146` 有界250日；`continuous_tick_finalizer.rs:117` 仍保留 price_history，因此不存在未获兼容决定就删除的缺口。 |

## ADR-0012 逐章节、逐条款状态

| 原文与条款 | 状态 | 当前证据及排除理由 |
|---|---|---|
| 背景（10–16 行） | 已有替代主链 | 秒级 legacy 方法仍存在不等于主链继续用它；权威调用为 `npc_decisions.rs:179` → `strategy/zi_noise.rs:192` → `behavior/decision.rs:11`。 |
| 决策#1 风险不被非风险arrival_rate门控（20 行） | 已实现 | `behavior/decision.rs:67` 风险检查先于363行历史不足分支与后续 arrival 分支。 |
| #2 30分钟/5日、InsufficientHistory、竞价基础需求诚实（22 行） | 已实现 | `behavior/decision.rs:363` 缺30分钟时明确 InsufficientHistory，即使有独立基础买卖也保持该理由；不拿秒级价格补出趋势。 |
| #3 全市场等权背景（25 行） | 已实现；行业出现后不自动重写旧散户 | `decision_snapshot_capture.rs:538` 与 `behavior/decision.rs:47`。该 ADR 的“当时没有行业”是历史上下文，不单独创造当前必须接入行业信号的承诺。 |
| #4 60%持仓与全市场发现、独立RNG（27 行） | 原始路径已实现，B03扩展关注 | `behavior/heuristics.rs:31` 保留60%持仓优先；36 行在非持仓优先分支额外优先已关注列表，48 行仍可全市场发现。ADR-0013新增恢复关注集合，故不把旧40%参数当作永远禁止关注集合的要求；70%关注倾向为实现参数，不用ADR-0026冒充散户替代依据。 |
| #5 六动作、目标/期望/可执行、T+1意图、各风格非永久禁令（29 行） | 已实现 | `behavior/mod.rs:20` 六动作，57 行输出结构；`heuristics.rs:204` T+1执行边界；`decision.rs:149` 各风格纪律分支。 |
| #6 删除硬上限和股票数下限（33 行） | 明确被ADR-0021修订且已实现 | 修订原文已直接指向ADR-0021；`heuristics.rs:157` TryBuy四分之五步幅、Add叠加到1，目标不读取股票数。不能把已删的旧上限重新列待办。 |
| #7 个体子单、真实费用/现金、整手、保留零股、判断不保证成交（37 行） | 已实现 | `strategy/zi_noise.rs:239` 目标转换，258/274 行按个体 order_size_mean 限制；`heuristics.rs:176` 买入/Reduce保留零股，真正Exit可清仓；空订单可保留判断。 |
| #8 仅审视股票协调工作单、不轮到不撤、不可撤时抑制新目标（41 行） | 已实现 | `strategy/zi_noise.rs:207` 输出 reviewed_stocks；`npc_state_projection.rs:250` Retail使用ReviewedStocks scope，255行进入工作单 reconciliation；299 行记录真实查看。 |
| #9 瞬时判断不存档，B03记忆另存（44 行） | 已实现原始契约；K5漏接见G08 | PositionDecision不是新增权威记忆；`session.rs:436` 与 `experience.rs:106` 保存个人经历。仅结构可存档不能证明后续日期事件已写入。 |
| #10 末尾250完成日、非正净成本不可用但权重保留（46 行） | 已实现 | `session/views.rs:146`；`decision_snapshot_capture.rs:354` cost.filter；`observation.rs:402` 可用成本才算收益，与独立equity_weight分开。 |
| 可证伪假设与验证（52–59 行） | 有测试代码，本轮未运行 | `tests/behavior.rs` 覆盖成本/仓位/风格/T+1/历史不足与子单；`tests/observations.rs:22` 采样密度；`tests/session.rs:4167` 分钟恢复、4233 行压缩日桶完整。当前并发重放承诺受现行 `docs/open-questions.md` Q8 的实际输入轨迹边界约束，不能按旧同seed文字要求自由调度整局字节相同。 |
| 后果（63–65 行） | 主链已有，明确不保证持续成交 | 持久关注/经历后由ADR-0013补充；不能按本ADR“不宣称完成”的历史范围凭空重开这些已落地项。 |

## ADR-0013 逐章节、逐条款状态

| 原文与条款 | 状态 | 当前证据及排除理由 |
|---|---|---|
| 决策：逐户初始/峰值权益、失败数与个股经历，私有数据不发布（10 行） | 原始B03已实现 | `experience.rs:106` 独立状态；`tests/session.rs:1847` 公开snapshot不含NPC、存档保留；接受观察才更新个人状态。 |
| 仅实际结算/接受观察写经历，未持仓观察无伪造入场（14–15 行） | 原始B03已实现 | `retail_projection.rs:322` 仅处理已结算receipt；519行调用真实fill writer；`npc_state_projection.rs:299` 审视后observe_stock；`experience.rs:184` 只更新观察分钟，无入场/成交参照。 |
| 同订单跨tick去重，补仓保留失败/高点，清仓120分钟，持仓+8列表（17 行） | 原始B03已实现 | `position_transition.rs:73` buy ID判重、92 行sell ID判重，82行peak.max；106行清仓、112行120分钟；`experience.rs:264` 持仓保护与列表剪枝。`tests/experience.rs:43` 专测部分成交去重。 |
| 参数与边界：5%不利观察、120分钟、失败重试、盈利卖出减一（21 行） | 原始B03已实现 | `position_transition.rs:141` price*100<=buy*95；95行盈利减一；`heuristics.rs:126` 读取失败数形成重试概率。此5%仍受48行“保持不变”保护，机构个体门槛不能覆盖散户。 |
| 旧“当前不设置时间衰减”（21 行） | 明确已被44行取代 | 不以旧不做条款核销新20日衰减；真正生产接线缺口见G08。 |
| 权威存档、完整账户集合、价格/时间/生命周期/峰值/冷静期、WASM Map、诊断（23 行） | 原始B03已实现；dated完整性需G08 | `session/persistence.rs:470` 散户集合、581行逐经历校验、752行feedback.validate；`apps/web/src/host/serde-normalize.ts:88` 转account Map；Rust宿主保持JSON对象键。字段存在且被验证不代表生产产生dated经历。 |
| 后果与20,000户测量（27–37 行） | 测量实现与历史证据存在 | `tests/session.rs:1767` 同名成本report；文中数值仅是2026-09-10本机历史结果。本轮未跑压力测试，不宣称当前成本通过。 |
| 计划契约：20交易日无新受挫减一、保留亏损事实（44–45 行） | **未完整实现：既有G08** | `experience/feedback/inputs.rs:13` 纯函数存在；但 Retail 主链 `heuristics.rs:122` 直接读 consecutive_failed_buys，缺as_of，未调用failure_influence；观察与成交均走legacy无日期 writer。 |
| 计划契约：忍耐/信心/风险压力、失败日期、双时钟（46–47 行） | **生产漏接归入G08，不新编号** | `feedback.rs:42` ExperienceMoment、56行FailureEventRecord；`feedback/inputs.rs:25` is_long_stuck与后续风险读取存在；`decision_snapshot_capture.rs:347` 仍observe_position，`retail_projection.rs:519` 仍record_fill_with_order；Retail入口 `npc_decisions.rs:188` 只传market_minute。因此不能核销完整K5记忆反馈。 |
| 计划契约保持：5%、部分成交去重、120分钟、持仓+8、真实事件、参数假设（48–51 行） | 原始行为仍存在，不被ADR-0026覆盖 | ADR-0026:41明确机构衰减不依赖散户计数；48行明确机构不套用散户固定冷静期/不利门槛。机构dated writer `retail_projection.rs:597` 不能证明散户519行已切换。 |

## 追到权威生产端点的调用链

1. 分钟：连续竞价真实成交与冻结最新价 → `continuous_tick_finalizer.rs:108` 更新权威日K → 138 行补齐已完成市场分钟 → `session/views.rs:127` 完成分钟与尾部250个日K构造观测 → `decision_snapshot_capture.rs:159` 为接受散户共享公共行情。
2. 个人风险/经历：`decision_snapshot_capture.rs:133` 接受注意力 → 170 行capture个人账户 → 347 行legacy observe_position、参考权益/个人峰值 → `npc_decisions.rs:179` 个体seed策略 → `zi_noise.rs:192` → `behavior/decision.rs:67` 风险先行 → 非风险趋势/历史不足 → `heuristics.rs:145` 目标/差额 → `zi_noise.rs:239` 真实可负担限价子单 → `npc_state_projection.rs:250` 对审视股票工作单协调 → 299 行实际观察记忆 → 权威订单簿撮合/Settlement → `retail_projection.rs:519` legacy真实成交经历。
3. G08漏接端点：上述个人观察不调用 `feedback/lifecycle.rs:335` observe_position_dated；真实Retail成交不调用231行record_fill_dated；买入信心不调用 `feedback/inputs.rs:13` failure_influence。三个端点缺任一个都不能宣称散户20日衰减闭环。
4. Q02范围核对：`session/decision_chain/roots.rs:323` 为本人候选记录 observe_price；`experience/price_memory.rs:168` record_public_history_read 的所有命中仅为定义或测试，未发现生产调用。根候选确实实际消费公开历史时才应记录该事件，不能在 `views.rs:127` 给全账户批量补经历。本条沿用Q02，三份ADR自身未新增专门的公开历史读取硬条款。

## 排除的新候选

- “所有散户观察仍用旧秒级20点”：排除。`Strategy::decide` 的legacy便利路径存在，但权威 `decide_with_experience` 明确接入具名分钟行为；不能凭遗留函数存在判断生产漏接。
- “没有真实开盘仍算日内收益”：排除。`views.rs:154` 以真实volume>0门控active open，测试在 `tests/session.rs:4197`。
- “压缩日漏桶/靠UI补历史”：排除。finalizer逐已完成桶增量追加，与Publisher独立，测试在 `tests/session.rs:4233`。
- “散户缺少逐户经历/关注集合”：排除。legacy事实与持仓+8均已落地；后续dated反馈仅部分落地，已有G08准确覆盖。
- “机构ADR-0026完成就等于散户20日衰减完成”：排除此核销理由。它明确是机构个人经验补充，生产writer按Retail/InstitutionalFacts分支区分。
- “每次公共30分钟观测都缺少个人public-history-read”：排除该过度扩展。Q02须限定本人实际主动读取，避免制造未发生的观察。
- “旧单股上限未实现”：排除。ADR-0012:33与ADR-0021:6、20有明确修订，当前目标步幅已有。
- “历史20,000户数值等于当前完整验收通过”：排除。只确认report实现和历史结果存在，本轮未验证最新性能。

## 验证边界

本轮为静态全文/调用链审计，执行只读搜索与目标提交diff，没有运行测试。检查过现有测试对应边界；G08需要真实GameSession散户日期写入、跨20交易日衰减及无新受挫边界测试，Q02需要实际主动历史消费留痕测试，不能仅凭纯函数已有测试核销。本文由父agent统一纳入完整diff独立复核。
