# Batch 029 独立反查

## 来源完整性

按计划读取根目录原文至 EOF；各源均连续全文读取，实测行数与 SHA-256 匹配：

| 源 | 行数 | SHA-256 |
|---|---:|---|
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/25.md` | 9 | `ad3fdb4f9da2067604420ec51e9d22dc589db27f6c80b4edc10828e15bf0cafa` |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/26.md` | 27 | `d9669a610a198be8a716e5176f0f1f2633db9cae52aa547a269b9540c783b908` |
| `agents/oop-refactor-audit/completeness-2026-10-03/session/parts/28.md` | 17 | `ebcc29de49f83f8859fc1fc19932b170cf0e4f484a2bf7659ad43fe879597d47` |

## 当前代码核验（baseline `43b1aa5`）

历史结论在当前版本大体得到证实，尤其 A03 已在生产代码中实现，不能因其原文描述为候选就误报未实现。

- **A03 candle owner**：`packages/engine/src/session/candles.rs:90-129` 的 `SessionCandleBook` 持有完整历史和活动 K 线；`131-189` 同时负责活动更新、真实首笔成交替换零量昨收占位、成交统计更新和日终归档。`session.rs:1143` 将其置于 `CommittableSessionState`；存档投影分别导出历史和活动 map（`session.rs:2583-2590`），恢复分别写回（`2811-2820`）。`GameSession` 仍控制更新/提交转接（`candles.rs:191-205`），所以 candidate 不应扩张为接管 tick 发布权的全局服务。历史报告提出的完整日线和活动日线归属合并，在当前实现不是遗漏。
- **候选边界**：应保留 tick candidate/发布边界归 `GameSession`；A03 现有 owner 包含 `DailyTradeStats::record_trade`（`candles.rs:170-178`、`275`），测试存在不等于完整边界验收。历史报告列举的零成交占位、首笔及后续成交、溢出、candidate 隔离、双字段存档恢复是合理待验证边界；本轮没有运行测试，不断言覆盖完整。
- **NPC attention**：`attention.rs:14-25` 的 `NpcAttentionState` 持有个体 RNG 与候选 tick；发现采样回写 RNG 在 `253-257`，而 `NpcAttentionScheduler` 仅持候选堆（`261-319`）。`GameSession::pop_due_npc_ids` 以权威个人候选过滤 stale entry（`321-329`）。未发现把队列接线并入共享注意力服务的遗漏。历史记录指出的 discovery/RNG 写回、stale-entry/popped 行为测试边界仍未由本次静态读取证明覆盖。
- **causal facade**：`session/causal.rs:16-37,40-56,65-91,94-120` 依据会话时钟、市场盘口、母单、公司注册表和诊断 collector 构造并记录事实；这仍是 `GameSession` facade，不是独立且具备完整事实 ownership 的对象。抽走 facade 而留下上述权威状态会造成责任拆分；没有新抽取候选。
- **账户分页 map**：`account_paged_map.rs:14-18,31-67` 是以 Arc 分页实现的通用账户状态容器，非 Account 领域服务；`70-84` 提供并行替换/释放。未发现需要升格对象的领域状态或生命周期。历史指出的并行更新失败部分修改风险属于独立行为审查事项，本批未证实为新缺口。
- **策略 sizing/state/technical**：历史 26 源文提供候选与旧源码定位；本次已对 baseline `43b1aa5` 完成三者的当前 owner、生产 caller 与 consumer 复核，详见下方“补充：策略代码生产调用链”。当前代码支持 sizing helper 为无状态共享计算、`StrategyState` 承担封闭快照验证/恢复、technical 指标由纯内核计算并经 observation adapter 接入机构决策链。三者都不因此改变 A 股交易语义，也不自动核销 G/Q。

## 补充：策略代码生产调用链

- **Sizing owner 与生产 callers**：`strategy/sizing.rs:6-69` 是纯函数，无持久状态；`a_share_tranche` 被机构策略使用（`strategy/institution.rs:5,41-53`）；买卖 sizing 被零噪声/散户/动量策略分别调用（`strategy/zi_noise.rs:259-279`、`strategy/retail.rs:52-65,117-127`、`strategy/hot.rs:30-45,80-94`）。`strategy/mod.rs:51` 导出共享 helper；会话计划报价也直接按账户分配现金调用 `affordable_buy_qty`（`session/decision_chain/quote.rs:198`），其生产逻辑由 `decision_chain.rs` 管理，附近测试调用同一报价流程（`decision_chain.rs:2164-2185`）。因此把 helper 归属某个策略对象会让会话报价反向依赖策略实例，不成立。函数只计算提议数量/现金承担能力；账户层仍掌握冻结、同账户竞争及最终受理（`sizing.rs:30-31`）。A 股单位和零股边界仍需与正式交易规则及会话受理层一致；本轮没有重审交易所依据或执行测试。
- **StrategyState owner 与消费者**：sealed `ProductionStrategy` 限定三个生产策略实现（`strategy/state.rs:6-35`）；策略 payload 保持在各策略自身，`StrategyState` 是三种已登记状态的闭合可序列化快照，并负责校验/导出及重建（`state.rs:91-169`）。账户 `StoredStrategy` 缓存经验证的快照并提供生产状态视图（`account/strategy.rs:48-68`）；会话决策快照及 NPC decisions 将其用于候选决策与后续重建（`session/pipeline/decision_snapshot.rs:30,64-101`、`npc_decisions.rs:175-201`）；shadow clone 经账户账簿校验（`account.rs:211`、`session/account_book.rs:39-69`）；v2 存档验证与恢复通过 `into_strategy` 重建且校验策略观察概率一致（`session/persistence/v2.rs:315-339,393-409`）。职责是闭合快照的验证/恢复，不是策略 payload 或玩家账户领域的替代 owner。
- **Technical indicators owner 与生产 consumers**：`strategy/technical.rs:43-46,81,111,152` 定义输入/结果和 `sma`、`rsi14`、`atr14` 纯计算内核。`observation/technical.rs:49-98` 拥有观察输入日序/未来数据校验、排除无成交日的样本筛选，`103-114` 再调用各内核；生产 decision-chain 从 candle book 的完整历史取得数据并构造 bars（`session/decision_chain.rs:90-123`），在机构决策链组装并消费技术观察（`decision_chain.rs:690-725`）。生产调用路径同时保留“技术指标只用已完成日线”的边界；不把交易日历选择或行情 owner 搬进指标函数。技术指标描述的是观察输入/公式，不更改沪深撮合、价格、申报或结算规则。
- **G/Q 补充裁定**： sizing 有 A 股申报量语义，但它是已存在策略与 session 报价共同使用的数量建议层；没有证据证明其调用结构就是 G01–G68 中任一缺口的同一 owner/接受边界。其“账户最终校验”边界不能核销任何市场受理相关 G，也不应把对官方制度依据的独立规则复核混为本次 OOP 审查。StrategyState/technical observations 同样未找到总账直接映射。G15 是官方日历覆盖与模拟回退问题，非 sizing 或指标指标公式；engine 重点 G06–G09/G16/G28/G35–G38、Q02/Q11 仍沿其现行总账判断。所有其他 G01–G68 与 Q01–Q23 不因这些类型/测试存在而完成或变更状态。

## 全章矩阵与 G/Q

| 总账范围 | 本批关系及裁定 |
|---|---|
| 宿主/远程、市场显示/日历、工程/交互/发布、公司计划经营等现行总账章节（G01–G68 全域） | 三篇来源讨论 engine/session 内部对象责任，无可确认的直接 G 映射。按现行 `implementation-audit-2026-10-02.md` 和 `exhaustive-review/resolution.md` 各章状态管理；不得将对象存在或测试存在作为 G 项核销。|
| engine 重点：G06–G09、G16、G28、G35–G38 | `reaudit-engine.md:18-25,31-83` 仍将这些条目及 Q02/Q11 按各自现状处理；candle/attention/causal/map ownership 与这些缺口非同一 owner 或验收承诺，不构成核销或重复新 G。G16 历史复制、G28 集团披露、G35 经营闭环等仍以总账对应证据为准。|
| 其余 G01–G68 | 当前总账仍记载 G27 已核销，其余各项保留各自完成边界；本批不更改状态，不臆造逐项映射。全局分域参照 `implementation-audit-2026-10-02.md` 与 `exhaustive-review/resolution.md:5-18`。|
| Q01–Q23 | 未发现来源候选与开放问题的直接映射。现行问题清单及裁定参照 `docs/open-questions.md`、`exhaustive-review/resolution.md:47-59`；Q02/Q11 继续按 `reaudit-engine.md:77-83` 记录。不能将 OOP 结构审查视作问题解决。|

## 结论与范围

没有发现新的 OOP 动作，也没有发现当前代码反证旧的核心 retain 结论。A03 已实施且生产使用，只有明确的 `SessionCandleBook` 历史/活动日线 owner 补全值得记录；batch 28 所指 A03 已包含统计更新责任。历史文字与当前代码的区别以当前 baseline 代码为准。无交易制度规则改变；本次未重新核验交易所法规。未改产品代码、未运行测试/构建/回归、未执行 Git 写操作。
