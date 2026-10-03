# Sweep19：session 与 strategy 历史实现逐章核对

审计工作树：`.worktree/implementation-reaudit`，读取时 HEAD 为 `4ad5a2e`；任务给定 `b76ece3`，本记录依据当前代码，不把两个提交冒充同一提交。只新增本记录，未修改产品、未执行测试或 Git 写操作。已读根 `AGENTS.md`、`docs/principles.md`，核对相关 ADR 与开放问题。

## 连续全文读取范围

| 文档 | 行数 | 实际连续读取 |
|---|---:|---|
| `docs/superpowers/plans/2026-06-29-session.md` | 954 | 1–330、331–660、661–954，含代码、验收与 Self-Review |
| `docs/superpowers/specs/2026-06-29-session-design.md` | 250 | 1–250，含 §10 风险 |
| `docs/superpowers/plans/2026-06-29-strategy-impl.md` | 913 | 1–330、331–660、661–913，含工厂延期说明与 Self-Review |

合计 2117 行。以下状态只表示静态代码核对，不声称历史测试命令、clippy、build 或完整长验收已在本轮通过。

## Session design 逐章状态

| 原文章节与行号 | 状态 | 当前证据与演进 |
|---|---|---|
| §1 背景与动机 :10 | 已有 | `packages/engine/src/session/failure.rs:73` 实际调用权威执行入口；不是仅存在类型或测试骨架。 |
| §2 决策 :18 | 已有，部分被替代 | 玩家队列 `session.rs:2518`；逐 tick 主干 `session/failure.rs:29`。后续 ADR-0017:34–45 的 P0–P9 和并行受理替代旧「NPC AccountId 顺序后玩家」及失败后继续。 |
| §3.1 RNG :37 | 已有 | `session.rs:129–155` 完整 SplitMix64；新局 seed `session.rs:1386`；当前 NPC 每账户独立流 `pipeline/npc_decisions.rs:151–190`，随机数继续注入 caller。 |
| §3.2 Event/Snapshot :52 | 已有，投影演进 | `session/snapshot.rs:116–180` 有市场与玩家账户快照；公开投影不公开全部 NPC。完整存档另有 `session.rs:2537` 及 `:2636` RNG 游标，不要求把全 NPC 资产向 UI 泄露。 |
| §3.3 Setup/session :101 | 已有 | `session.rs:1348–1386` 验证 setup、构造多市场与玩家；`:1840–1870` 真实调用工厂并注册 NPC 状态。 |
| §3.4 Error :159 | 已有 | `session.rs:2527–2529` 明确 UnknownPlayer/NotPlayer；`session/failure.rs:54–82` 致命失败阻断并 poison。 |
| §4 step 流程 :174 | 已有，算法被后续替代 | `session/failure.rs:70–76` 统一权威事务；`pipeline/npc_decisions.rs:160` NPC 生产调用；历史窗口 `pipeline/continuous_tick_finalizer.rs:121–123` 真正 push/pop。receipt Settlement 取代旧逐腿 apply_trade。 |
| §5 错误分级 :191 | 被批准的新事务模型替代 | 原 `step()->Vec<Event>` / SettlementError 后继续不再是当前义务；`step()->Result` 与整候选丢弃符合 ADR-0017:39、:45。 |
| §6 模块边界 :197 | 已有，集合竞价已推进 | 存档 I/O 仍由宿主承担；内存 save 非 I/O。集合竞价不再是本项目未来缺失项；测试 `tests/session.rs:1201` 验证竞价事件及市价拒绝。 |
| §7 测试矩阵 :205 | 有对应测试，未运行 | `tests/session.rs:6/:19/:28` RNG；`:1254` 装配；`:2193` seq；`:2207` 固定 fixture；`:2219` 日界；`:2235/:2259/:2296/:2317` 入队/非法账户/资金拒绝。V 测试已被范围替代。 |
| §8 布局 :222 | 已有，拆分演进 | `session.rs` 拆为 `session/` 子模块；导出 `packages/engine/src/lib.rs:61`，不按旧单文件路径缺失误报。 |
| §9 验收 :234 | 静态主干可确认；运行验收未执行 | 导出、实例状态、Money 与错误路径可查；历史计数 ≥125 不作为当前正确性门槛，未宣称命令全绿。 |
| §10 风险 :243 | 已实现或替代 | SplitMix 常量正确；历史窗口确实滚动；旧隐藏 V 可见性由 ADR-0006:231–244 明确删除；最坏情况资金预算由 escrow 契约接替。 |

## Session plan 各任务核对

| 任务 / 原文 | 状态 | 代码反证 |
|---|---|---|
| Task1 RNG :36 | 已有 | `session.rs:129`，`tests/session.rs:6`。 |
| Task2 类型 :152 | 已有，扩展 | `session.rs:159` RejectionReason；`strategy/mod.rs:135` Intent；`lib.rs:61` 公共导出。 |
| Task3 new :323 | 已有 | `session.rs:1364` 验证；`:1368` 多股；`:1378` 玩家；`:1840` NPC 工厂。原计划示例错误字段 `setup.cash_per_npc` 不要求复刻。 |
| Task4 snapshot :451 | 已有，公开/内部区分 | `session/snapshot.rs:116` 完整公开投影；`session.rs:2537` 内部 SaveSnapshot；旧 snapshot 兼作全状态存档已拆开。 |
| Task5 step :524 | 已有，P0–P9 替代 | `session/failure.rs:73` 实际生产入口。原 :713「Cancel 暂不处理」不是现状；当前 Intent 有 Cancel，撤单及释放进入流水线。 |
| Task6 enqueue :796 | 已有且修正原示例边界 | `session.rs:2523–2531` 校验真正 Player 并存 `(player_id,intent)`，不会把存在的 NPC 偷换成 AccountId(0)；测试 `tests/session.rs:2296`。 |
| Task7 导出/回归 :881 | 导出已有；本轮未运行验收 | `lib.rs:61`；未把历史 commit 或 checkbox 当成测试结果。 |
| Self-Review :942 | 历史风险已核对 | 确定性不再要求自由调度跨线程整局字节相等；机构低估无对手必成交的旧测试设想也不构成正确真实撮合要求。 |

## Strategy plan 各任务核对

| 任务 / 原文 | 状态 | 当前证据与反证 |
|---|---|---|
| Goal/Global :5/:11 | 主干已有；额外边界候选如下 | 多股 `strategy/mod.rs:72–151`；纯逻辑调用 `pipeline/npc_decisions.rs:179`；构造错误 `strategy/factory.rs:23` Result。 |
| Task1 多股骨架 :35 | 已有 | MarketView/SelfView/PositionView/Intent 在 `strategy/mod.rs:98/:108/:117/:135`；无 Pass，空向量表示不动作。 |
| Task2 ZiNoise :208 | 已有且扩大多股覆盖 | `strategy/retail.rs:23` 随机选股，非旧草案首键；测试 `tests/strategy.rs:2353` 覆盖全股票。当前最高/最低符号报价符合 ADR-0022，不要求恢复旧价±tick。 |
| Task3 Value :379 | 个人信念替代生产 TrackV；Fixed/DriftUp 内核保留 | ADR-0006:236 明确删除隐藏 V；`strategy/value.rs:37–42` BeliefInstitutionStrategy 壳走计划链，其 decide 为空属于明确路径隔离；`value.rs:16` Fixed/DriftUp target 共用函数仍在，市场分钟替代 private ticks。测试 `tests/strategy.rs:1117/:1204` 恢复与 tick 密度不漂移。不能以壳返回空认定机构没有交易。 |
| Task4 Momentum :552 | 已有，分钟趋势替代 tick 计数 | `strategy/mod.rs:87–89` 完成分钟窗口；测试 `tests/strategy.rs:1318/:1364/:1439/:1511` 涨买/分钟语义/跌卖/非法参数。 |
| Task5 Factory :700 | 已有且补齐原明确延期差异化 | 原 :807/:912 承认 v1 同类参数相同，非当前缺口；`strategy/factory.rs:54` RetailStyle 采样、`:130` 机构规模采样、`:143` position_step 采样、`:159` 游资阈值。测试 `tests/strategy.rs:1550/:1767/:1894` 个体差异。 |
| Task6 导出/回归 :823 | 导出已有；本轮未运行验收 | `lib.rs:30` 导出当前策略公共类型；测试 `tests/strategy.rs:2123`。旧 ValueStrategy 名称删除有明确替代依据。 |
| Self-Review :890 | 已核对 | 原 Option 工厂 `.ok()?` 吞错风险已被 Result 修正，`:2113` 有非法参数测试；不重新登记已修复项。 |

## 公共日终档与内部 checkpoint

旧设计 `session-design.md:29/:201` 的 snapshot 基础不是公共日内保存的现行批准依据。ADR-0025:12–16 明确公共持久存档只在自然日日结生成；ADR-0017:45 的合法 quiet-point 指内部事务状态，不覆盖公共策略。

当前 `session/failure.rs:85` 的 GameSession::save 捕获完整内部状态（含 `session.rs:2636` RNG 和 `:2654` 队列），用于校验/恢复；`session/protocol/civil/session.rs:100–138` 公共 restore 要求日结完成且无活动委托/待输入，`:175–183` 公共 save 只返回冻结 day_end_save。`:150` verification restore 明确隔离，不创建公共存档。这组代码是「没有公共日内档」已实现反证，不能登记缺失或违规。

## 新候选：公开 Factory 的零 ticks_per_day 边界未显式报错

原 `strategy-impl.md:13` 要求非法参数显式 Err、策略计算不 panic；公开 `StrategyFactory::build_for_market_day` / `build_for_market_day_with_ordinal` 当前 `strategy/factory.rs:28/:38` 接收 ticks_per_day，`:45` 仅有 `debug_assert!(ticks_per_day > 0)`。直接调用并传 0 时 debug 构建会 panic；release 路径 `strategy/sampling.rs:9` 用 0 作浮点除数，正观察频率得到概率 1，未返回 StrategyError。现有非法参数测试 `tests/strategy.rs:2113` 只测试既有配置，未找到这个公开入口零值测试。

反证/影响界限：`GameSession::new` 在 `session.rs:1364` 调 setup.validate，`:965` 已有 ticks_per_day 非零校验，因此不能断言正常新局会走此缺陷。它是可单独调用的公开 API 防御契约候选，是否进入 G 总账由主控裁定；本轮未写复现测试，未运行 debug/release，只据明确分支及 IEEE 浮点运算判断。

除此候选，三文主干没有发现可以独立于既有 G06–G09/G38 等编号的新产品断链。既有个人信念、经历、分配遗漏仍应保留其总账状态，不能因为旧三策略主干已在就核销它们。
