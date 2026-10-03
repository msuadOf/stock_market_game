# Session 与 Strategy 旧计划逐章复核（luna19）

## 范围与阅读记录

- 基线：产品 `08e4fc7`；当前审计 worktree `HEAD=a7c7ce3`。只读源码、决策和此前审计记录；唯一写入本文件。
- 按连续 `cat` 从首行读至 EOF：`docs/superpowers/plans/2026-06-29-session.md`（954 行）、`docs/superpowers/specs/2026-06-29-session-design.md`（250 行）、`docs/superpowers/plans/2026-06-29-strategy-impl.md`（913 行），合计 2,117 行；另读 `AGENTS.md` 与 `docs/principles.md`。按章节逐项对照实现，不以搜索结果替代全文阅读。
- 未运行测试、构建或性能验证；没有改产品文件或 Git 状态。结论为静态源码复核，不是大 A 规则法源复核。

## 逐章 / 任务矩阵

| 原文位置 | 需求或主张 | 当前实现对照与判定 |
|---|---|---|
| Session plan 1–34；spec §1–2（10–33） | 单一 `GameSession`，种子 RNG，事件/快照、玩家意图与日界；单步失败进事件而继续 | 已被后来的领域及事务决策重塑。当前 `step` 是 `Result<Vec<Event>, StepFatal>`；生产入口统一进入 phase dispatcher，任一 invariant 失败会 poison 并丢弃 tick shadow（`session/failure.rs:27–82`；`pipeline/authoritative_tick.rs:20–54`）。不得照搬旧版“运行错误全转事件、循环继续”。 |
| Session plan Task 1–2（36–322）；spec §3.1–3.2（35–100） | SplitMix64、六种旧事件、含 V 的快照类型 | `SplitMix64` 仍存在，但 RNG 已按 `tick/account` 派生隔离（`pipeline/npc_decisions.rs:151–179`）；旧 `VError`/`fundamental_value` 合同已删除。当前 `MarketSnap` 暴露真实盘口深度，`Snapshot` 含交易阶段及完整/活动日 K（`session/snapshot.rs:6–60`）。不要求恢复旧事件/API。 |
| Session plan Task 3（323–450）；spec §3.3–3.4（101–173） | `new()` 校验、玩家 0、生成 NPC、会话错误 | 当前新局执行完整 `SessionSetup::validate`、逐股 market、civil clock、公司注册/前史披露、人口/策略/流通股装配；玩家初始现金来自 config（`session.rs:1342–1465`）。比旧样例更广，当前账户构造与多股分配应按最新契约判定。 |
| Session plan Task 4（451–523）；spec §3.2 snapshot | 只读完整状态快照，可作首次连接/存档基线 | 当前 `snapshot()` 与 `runtime_snapshot()` 分开；前者含完整日 K，后者明确不复制历史（`session/snapshot.rs:110–125`）。这与“内存快照不是持久档”相容；两者均不应因旧设计里的共享 Snapshot 名称而混为存档。 |
| Session plan Task 5（524–795）；spec §4–5（174–196） | NPC + 玩家统一批次，预校验、撮合结算、V 演化、历史、日界和 seq；失败继续 | 当前按 P0–P9、typed receipt、shadow、一次提交；continuous 的账户验证与股票 shard 可增量交错，最后单次 finalization（`continuous_tick_transaction.rs:181–285`）。NPC 决策的纯 P2 输出不持有 session/router/orderbook（`npc_decisions.rs:88–115`），各账户独立 RNG（`:151–215`）。旧循环代码/单 RNG 顺序、拒单全部事件、V 阶段都被替代。 |
| Session plan Task 6（796–880）；spec §3.3/§7（101–158, 205–220） | 未知玩家报错；玩家意图入队后 step 执行 | `enqueue_player_intent` 校验账户存在且类型为 Player，再把 `(player_id,intent)` 加入待处理队列（`session.rs:2516–2532`）。意图之后由统一入口纳入固定观察、P3 校验及 P4 处理，不是旧 `route_intent` 直接执行。 |
| Session plan Task 7 / Self-Review（881–954）；spec §8–10（222–250） | 导出、TDD 测试、clippy、原始验收 | 作为历史计划记录，不足以代表当前验收；当前新增阶段、CivilUpdate、存档、公司与多策略路径都超出原 DoD。原测试矩阵里共同 V、`step -> Vec`、错误事件恢复等断言已过时，不能据“当时全绿”证明当前契约。 |
| Strategy plan 1–34；Task 1（35–207） | 多股视图、Intent 含 code、策略返回向量、Serde | 多股视图和 `Intent` 仍在，但字段已经演进为涨跌停/价格笼子边界、完成分钟序列和阶段时间；隐藏 V 不存在（`strategy/mod.rs:70–152`）。当前共用视图构建含历史日量、成交量倍率与盘口失衡（`session/views.rs:6–121`）。 |
| Strategy Task 2 ZiNoise（208–378） | 散户 ZI 到达/追势/买卖方向、随机报价 | 生产策略为 `StrategyState` 可序列化的封闭权威状态；实际 NPC P2 重建 `StrategyState`、调用带个人经历及风险输入的 `decide_with_experience`，并回写新策略态（`npc_decisions.rs:160–215`）。另有零售行为路径，不是旧 plan 中一个价差公式足以代表的实现。 |
| Strategy Task 3 Value（379–551） | 以共享 V/TargetPolicy 形成估值机构决策 | 共享 V 已由 ADR-0016 明确删除；机构只可使用个人 belief、信息与预期。Strategy API 明注不携带共同隐藏价值（`strategy/mod.rs:176–189`），当前机构决策另由个人信念链驱动。计划中的 ValueStrategy/TrackV 和低估确定买入旧测试不能恢复。 |
| Strategy Task 4 Momentum（552–699） | 以最近 N 个 tick 点算动量 | 当前 Strategy 明确禁止游资趋势把 tick 样本当时间跨度，改用标准交易分钟收盘价（`strategy/mod.rs:83–90`）；构建视图分别提供 tick 最近价与完成分钟价（`session/views.rs:21–34,109–120`）。这是重要跨层语义差异，不是漏掉历史向量。 |
| Strategy Task 5 factory（700–822） | 固定每类参数/可选采样的工厂 | 当前有 `StrategyFactory`、参数采样与持久 `StrategyState`，应继续以有效账户策略、种子和存档恢复契约核验。不可把旧样例所述同类参数相同或 `Option<Box<dyn Strategy>>` 当现状。 |
| Strategy Task 6 / Self-Review（823–913） | crate re-export、clippy、110 测试预期 | 仅是历史阶段验收。当前策略拆为多文件；这三份旧文件和旧断言没有证明现行代码的编译/测试结果。本审计未运行验证。 |
| Session spec §6–10（197–250） | 延后真实联机/存档、只需 Snapshot；V 测试、13 项测试矩阵 | 这些边界已由后续决策变更：真实存档由日终 CivilUpdate 候选负责，V 被删除，现有 `Snapshot` 不等于可随时落盘的 public archive。原矩阵只用于追溯需求演化，不能当当前待办清单。 |

## 最新契约复核

- **P0–P9：** 最新 ADR-0017 §阶段契约规定 P0/P1 一次、P2 驱动依赖计划、P3/P4 按真实依赖增量交错、ReceiptAggregation 至 P9 各一次（`docs/decisions/0017-escrow-parallel-tick.md:32–45`）。P0 的 quote-expiry 在连续阶段释放、其他阶段不释放（`quote_expiry.rs:89–127`）；P1 从 post-P0 envelope 计算预算而不重复加释放（`decision_resources.rs:92–145`；ADR-0017:36–39）。统一入口按连续/竞价/盘前 phase 准备候选，验证后只调用一次 prepared commit（`authoritative_tick.rs:20–54`）。与较早审计中“来源类型固定优先”“所有 P3 先于 P4”或“同 seed 不同 worker 必须整局字节相等”的旧结论冲突时，以 ADR-0017/0018 最新明确文字和实际受理链为准；不发现本轮新 P0–P9 反例。
- **日终存档：** ADR-0025 明确市场 tick、普通 snapshot、引擎 rollback checkpoint 与用户持久存档不是同一对象；持久写入只接受完整 CivilUpdate 后冻结的候选。Civil session 在成功 CivilUpdate 后取得 SaveSlot 并发布为 `day_end_save`（`protocol/civil/session.rs:283–330`）；公共恢复另拒绝非日终/含日内请求及挂单的档（`apps/web/src/save/day-end-candidate.ts:6–27`，协议恢复测试见 `protocol/civil/session.rs:534–578`）。Web 写入命令须经过 day-end gate（源码位于 `apps/web/src/app/useSaveCommands.ts`）。底层 `GameSession::save()` 可用于引擎内存检查点/验证（`session/failure.rs:85–89`），单凭 public 方法存在不能指称用户可日内持久化；没有找到新写盘绕过证据。
- **V 删除优先：** 旧 session 设计把共同 V、机构可见性、`VError` 与 `evolve_v` 写作核心；ADR-0016:12–32、:56–72 后续明确替换共同 V，当前 Strategy API 也明确说明 shared V 删除（`strategy/mod.rs:176–189`）。不把缺少旧 V 类型、快照字段或旧 V 测试登记成缺口；价值/方向来自个体 belief 与公开/本人信息。策略把日内 tick 和分钟序列分开也符合这个迁移。
- **A 股语义与差异：** 旧 plans/spec 的统一 tick 循环和市价/涨跌停校验顺序不再是裁决依据；当前边界是 ADR-0017、trading-rules 与各证券类别策略。此轮只核对交易顺序/账户资源语义未被旧计划误导；未查交易所新法源，不能声称重新认证大 A 规则。

## 旧结论复核与新候选

- 复读既有 `agents/implementation-audit/reaudit-engine.md`：G06/G07/G08/G09/G16/G28/G35/G36/G37/G38 与 Q02/Q11 的范围和限定仍适用于其记录的 OOP 变更基线；本轮静态查看的 P2/P9、策略调用及 save/civil caller 没有反证可核销这些缺口，也没有证据把它们扩大成新缺口。尤其 G08 不能以机构经历链替代散户路径，G28 不能以存在合并报表算法替代日终生产披露，G37 不能以诊断查询只读性替代 trace 订单关联。
- 旧总审计 R02/R10/R09 当前要按最新契约窄化：P0–P9 已存在生产权威链；session 与存档调用链已存在，但并不代表无 G 项或旧 spec 全兑现；共同 V 是正式删除决策，不是 R09 仍待实现功能。
- **未发现新的确定性产品缺口。** `session/views.rs` 的 limit observation 含显式 invariant panic 分支，但目前 session/setup 校验和市场状态维护为其前提；静态证据不足以把该分支确认成可由合法用户输入触发的新 panic，不登记候选。普通 `GameSession::save()` 与持久写盘候选已有 Civil protocol / Web gate 分层，不能把内存 checkpoint 误报为日内保存。
- 本轮结论仅补全文档核对与实现证据；没有修改产品、没有运行测试，也没有重新做独立 diff 审查或官方规则取证。不可据本文件单独宣布整批改动完成。
