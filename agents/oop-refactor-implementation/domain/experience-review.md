# Experience / Behavior / Indicators 独立复核

- 日期：2026-10-03。
- Reviewer：`review_experience`，未实施本批源码；只写本报告。
- Baseline：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 权威范围：`../../oop-refactor-audit/challenge-2026-10-03/action-index.md` 的 domain-N01、domain-N07、domain-R2-N28、domain-R2-N34、domain-R2-N35、domain-R2-N40，共 6 个动作。
- 审阅方式：指定范围完整 baseline diff、未跟踪 `retention.rs` / `position_transition.rs` / `lifecycle/institutional_transition_tests.rs` 全文、相关实现/校验/现有保护测试，以及 N07 的跨 session caller；未运行 Cargo、测试、编译或 Git 写命令。

## 结论

最终冻结源码静态复核无未解决有效发现，N07 包含 domain-R2-E03 的六个内部 writer 组合。发现的 N07 生产观察接线遗漏已修复并复核。前轮只审两个新增 State receiver 的范围已补正；本终稿以最终一体实现为结论，不以 dated writer 原本属于 State 代替内部组合核销。此报告不表示编译、保护测试、完整回归或宿主验收通过。

## 有效发现及修复复核

### EXP-01：N07 新 owner 未接入真实机构观察（已修复）

- 首次定位：`packages/engine/src/session/institutional_behavior.rs:16` 的 `observe_institution_position` 仍持有旧完整转换，`session/decision_chain/roots.rs:246` 的生产调用仍走该 helper。新增 `RetailExperienceState::observe_institution_position_dated` 当时只有测试直接调用，未完成动作要求的生产写入归属。
- 行为影响：机构实际观察继续由 session 直接更新 legacy/feedback，新增 owner 和旧 helper 两份算法并存；动作不能据新增方法存在宣称已完成。
- 修法：保留 helper 签名，将其改为薄 policy adapter，调用 `observe_institution_position_dated(code, price, moment, policy.adverse_move_threshold_bp())`，不重排旧守卫或添加新条件。
- 再复核：已读取最终 helper 全部 body 与该文件完整 baseline diff，`institutional_behavior.rs:23` 现委托 owner；`roots.rs:283` 的真实机构持仓观察经原 helper 进入 owner。`session.rs:1582` 的 stale 清理也已调用 `clear_stale_institutional_holding`。新增 owner 的守卫、转换和 history 提交与 baseline 观察算法一致，threshold 仍来自被冻结的个体 policy。
- `institutional_behavior.rs` 中账户风险 latch 等其他动作的完整语义验收属于 session 组；本报告不代替其独立复核。

## 门禁一：大 A 语义与依据

已读 AGENTS、principles、architecture、open-questions、trading-rules，以及 ADR-0011、0012、0013、0021、0026。规则依据来自正式文档登记的上交所/深交所《交易规则（2026 年修订）》与上交所不足申报单位余额一次性卖出说明；文档记录 2026 版自 2026-07-06 施行、交易条款最近完整核对为 2026-09-22。此次静态复核没有再次访问官方网页，不把已有取证日期改称当日核验。

- `heuristics.rs:145` 的目标推导仍用 Money 分、数量股、fraction 比例；TryBuy/Add 按 100 股增量并保留既有零股余数，Reduce 保留余数而不扩大成 Exit。sellable 小于期望减仓时只截断 executable，T+1 全锁时保留 desired 与动作并将 executable 置零。现金、费用、申报数量、板块、涨跌幅与撮合校验继续由执行层承担，本 context 不冒充完整交易制度。
- 风险判断、止损/止盈、冷却、failure threshold、KDJ/MACD 均是已登记的游戏行为或指标模型。此批没有引入强制止损、资金补充、共同估值、板块默认值或可卖证明。
- `lifecycle.rs:12` 的机构失败仍依据真实买单身份及本人实际观察，以显式个体 threshold 确认；不增加零售连续失败计数、不套零售冷却。`clear_stale_institutional_holding` 清理 epoch 与五个 legacy 活跃持仓字段，保留观察/交易时钟、last_sell_order_id、cooldown、退出和失败历史；不伪造清仓成交。
- Watchlist 注意力与 PriceMemory 公开历史读取仍是独立事实。公共读取不创建本人未观察条目、不覆盖本人高低锚点，protected 集合仍由持仓/活跃计划提供。指标结果不成为权威成交价格。

## 门禁二：必要性与最小范围

- N01/N40：分别用私有 KdjAccumulator 和 EmaSmoother 收束一条短序列递推状态，未跨请求缓存或扩展产品 API。EMA 保留乘加顺序与 DEA 显式 0.2/0.8；KDJ 保留 seed 50、先 K 后 D 后 J、9 样本窗口与 flat RSV 50。
- N28：具名 context 只借用 code/market/account_risk 并保存本次持仓输入；风险筛选、风格优先级、arrival gate 与宽决策链未重排。各调用点传入原 weight/qty/sellable 值，信心拒绝复用 target_for，不拥有 RNG 或权威账本。
- N07：观察与对账写入归同一 RetailExperienceState receiver，内部组合私有 PositionExperienceTransition，借用同股 legacy 与可选 active epoch，收束真实 fill/observe、匹配初始化、同股本人观察和费用提交。State 继续拥有两张 map、时钟及退出/失败历史；没有物理合并、持久复制或强制键集相等。
- N34/N35：条目 receiver 收束观察/read 更新；私有 RetentionCandidates 只排序与选出未保护 code，两容器保留自己的时间输入、数据、恢复边界和实际 retain。未增加依赖或改公开字段可见性。
- 既有 serde/TS 字段、默认值、deny_unknown_fields、公开计算入口、batch 顺序校验和结果顺序保持。新增机构 owner 方法不新增接受旧档的迁移路径，也未对原 API 添加拒绝条件。

## 门禁三：边界、跨层与复杂度

已静态核对本批 14 个新增保护 test，其中 N07 的 6 个短 case 在 `experience/feedback/lifecycle/institutional_transition_tests.rs`，模块 filter 保持；这些 test 未在本 reviewer 任务中执行。

- N01/N40：32 样本逐点 to_bits 对照覆盖 26 样本以上 EMA，KDJ 第 10 样本低点滚出窗口；旧空输入、单样本、flat、JSON 字段、invalid OHLC 和极端有限值 NonFiniteResult 用例保留。批量计算仍先顺序校验全部输入、再 par_iter、再顺序验证结果。
- N28：TryBuy/Add/Reduce/Exit、已有零股、raw target 小于零股余数、部分可卖、T+1 锁定、信心 0/1 draw、Hold/Watch、零/负 equity、非正 last_price、缺少 risk observation/weight 已有代表性保护断言；所有 RNG 条件与调用点静态保持原顺序。上下文为模块私有，没有对外放开任意持仓写口。
- N07 机构本人观察：price 先于 civil_date → market_minute → trading_day，再检查双表存在；守卫错误前不写状态。测试核对回拨、缺单侧 map、800bp 门槛差一分、同买单只确认一次、本人观察时间、零售计数不变、stale 清理幂等和历史保留。
- N07 内部组合：六 writer 与额外的 legacy observe 确实调用新 transition，详见下表。原 legacy counter/cooldown overflow 的部分写入与 feedback 不提交未被改造成原子操作；初始化、实际费用累加、清仓、本人观察、退出和失败历史的提交点与旧流程一致。
- N34：read-count overflow 仍先写 last_public_history_read_minute，再 checked_add panic，last_touched/本人高低未更新。新增保护明确锁定这一旧失败面。价格错误与回拨守卫顺序保持；新私有条目方法不绕过集合正价入口。
- N35：按 `(minute, StockCode)` 降序确定性保留未保护前 8；超过 cap 的保护项永不驱逐，同分钟较大 code 保留。测试让 public-history touch 只影响 PriceMemory 淘汰，不改变 Watchlist attention 时钟；from_parts 仍拒绝条目晚于列表时钟。
- 真实接线只读核对：root 被接受观察 → watchlist.record_attention / price_memory.observe_price；持仓及活跃计划组合后 watchlist.prune；机构真实结算仍在 retail_projection 调 record_institutional_fill_dated。未定位 public-history-read 的生产 writer，报告不把测试调用冒称已有实际产品接线。

## 范围与证据限制

完整 diff 覆盖 `experience.rs`、`experience/**`、`behavior/{decision,heuristics,mod}.rs` 和 `indicators.rs`；其中本批有 7 个 tracked 改动文件，加 3 个未跟踪文件：`experience/retention.rs`、`experience/position_transition.rs`、`experience/feedback/lifecycle/institutional_transition_tests.rs`。相关类型、时钟守卫及入站校验读取用于行为对照；session 新路径只核对 N07、N34/N35 真实接线，不宣称覆盖其他 session 动作全部实现。

未执行测试，未证明 TDD 红阶段，也未验证三宿主运行、长期统计、实际 CPU 利用率或性能改善。新增保护的静态存在不能代替 root 集中构建/运行门禁；以后修改上述源码或 caller 时需要复核相应增量。

## N07 / domain-R2-E03 全量细项核销

完整对照权威 action-index 第 145–199 行、domain-R2-E03 正文与 baseline，以下不是仅有方法名的占位接线。

| Writer | 实际 transition 与保持的成功点 |
|---|---|
| `record_fill_with_order` | 原 price/cost/数量转换守卫后 `from_maps` → `record_retail_fill`；原 legacy 写体已搬入，计数、同订单去重、清仓字段与 cooldown checked_add 次序不变 |
| `record_fill_dated` | 保持先 price/cost、时钟、epoch、读取原买卖身份，再调用 legacy writer；仅成功后推进 clock、必要的新 epoch、`record_own_observation`、`cooldown_until`、退出与失败历史 |
| `record_institutional_fill_dated` | price → 当次费用非负 → 数量转换 → 时钟 → epoch → 历史费用存在/非负/checked_add；成功后 `record_institutional_fill`、必要的新 epoch、clock、`record_institutional_observation`，最后移除清仓 epoch 并登记退出；机构不写零售计数/cooldown |
| `initialize_holding_dated` | 保留 current price → reference → 三时钟 → epoch 缺省守卫；`reset_initial_holding` 按原 default 构造覆盖同股 legacy 并返回匹配 epoch；然后才推进 clock、安装 epoch，不产生买单身份或成交事实 |
| `initialize_institutional_holding_dated` | 先完整调用前一 initializer 成功，再通过同股 transition 写 `Some(Money::ZERO)` fee seed；拒绝时旧 fee 属性及其他状态保持 |
| `observe_position_dated` | 原 price/时钟/epoch 守卫和失败确认读取后调用 legacy `observe_position`；其真实写体由 `observe_retail_position` 执行；成功后 clock、同股 `record_own_observation` 和 failure history |

此外，新增 `observe_institution_position_dated` 的 legacy 观察由 `observe_institutional_position` 执行，再按原 clock → 本人观察 → failure history 顺序提交。stale 清理保持原两表移除/五字段清理，不调用会伪造退出的 writer。

`from_maps` 取一条原 `entry.or_default` legacy 和 `epochs.get_mut(code)`，epoch 为 Option，不新增双表同时存在的守卫。初始化可覆盖清仓 legacy；零售观察与机构 fill 仍接受 active epoch 存在但 legacy 缺行；清仓后只删 active epoch，legacy 的历史行继续存在。机构本人观察仍沿用它原有的双表存在守卫，不能把 fill 的接受集误推广到观察。公开 serde/TS 字段与 map 关系校验未改变。

新增测试明确锁定：retail observe 与 sell counter overflow 已写 legacy 时间而 feedback 不动；清仓 cooldown overflow 已清 legacy 活跃字段与写 sell identity，但 epoch/退出历史未提交；机构初始化的金额/时钟/已有 epoch 拒绝与费用 overflow 均不改状态；retail 与机构无 epoch、非法数量转换首错差异保持；机构缺 legacy 的 fill 原接受、累计实际费用、清仓 legacy 保留、再入场 fee 重启而历史不删除。

## 最终源码 SHA-256

哈希由 reviewer 在实施方最终 freeze 后读取磁盘计算，包含所有必需未跟踪源码。`behavior/mod.rs` 未产生 diff，仍记录其范围指纹。后续任何内容变化须让对应旧验证证据失效并复核增量。

| 文件（相对 `packages/engine/src/`） | SHA-256 |
|---|---|
| `experience.rs` | `3dcf703c42bde1a4d4bbd2a2049e433084e491d9db49bb9a9b6142dc03fd8bd2` |
| `experience/position_transition.rs` | `4ea4f7fe1b955c306a7809e0da501ab8fa4eb3ad2c4e2ab10153b19825c8baa1` |
| `experience/feedback/lifecycle.rs` | `cb62433d343add8cdf277cb17eaf50b231ec60941059edb8af2e8c95b9eaeb8c` |
| `experience/feedback/lifecycle/institutional_transition_tests.rs` | `d944b95a942d7adcf33d618590f92b8ee9b14427c9c8f1137cb616d97fa3cb5e` |
| `experience/price_memory.rs` | `1846bd1160e07390d4d06d1181c137b2fdcad5dbad79a6fc7b203790617767e0` |
| `experience/watchlist.rs` | `45499ac5e83f017f7ce5565c82d483864b47404a5ce522db81b5fcdee937f8e8` |
| `experience/retention.rs` | `d8caa2a4fe891bcf38fa749d082f4b5cabb7986f6ebd9a2d3e8caa1a2c307d2c` |
| `behavior/decision.rs` | `7f7b8712a6255d72fdfcc6d23bd5872e3981fb1f2d0b807371782be4d9272c67` |
| `behavior/heuristics.rs` | `dc40fdd7f787fa63c0d3863b49e216820c8880bde1cdbfea6545373f7578b742` |
| `behavior/mod.rs` | `3ab36b115949f950f7748b6e590dfc6e705e3daa28a8ecdda06f72fb06b340e3` |
| `indicators.rs` | `2df29d2f0cca57aeb0c8c846ad1f7cbe9f1422e5b0cb0bf4c5dad700b59e7305` |
