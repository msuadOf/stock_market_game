# 量价诊断文档独立复核

审查日期：2026-10-03。目标产品提交：`08e4fc7`；目标 HEAD：`a7c7ce3`；实际工作树 HEAD 与目标一致。只读生产代码审查；未运行测试、编译或长验收，未修改产品代码或 Git 状态。

按连续顺序读完 `docs/price-volume-simulation-gap-checklist.md` 305 行、`docs/diagnostics.md` 68 行、`docs/causal-diagnostics.md` 49 行，共 422 行；首次合并工具输出截断后，分别连续读完价量清单 1–160、161–305 行及另外两份文档全篇。开工前读根 `AGENTS.md` 与 `docs/principles.md`。审查范围涵盖条款、实际生产调用、CLI输入、指标来源、DEV查询、序列化及相关旧账候选。

## 条款矩阵

| 原文条款（文件行号） | 追踪结果与证据 | 结论 |
|---|---|---|
| 量价清单 14–22：权威撮合、测量先行、画面采样隔离、资金口径、基线能力 | 基线 runner 消费 `GameSession::step()` 事件并基于真实会话聚合，`packages/engine/src/diagnostics.rs:792–805`；订单簿/日K未由诊断写入。文档历史基线的当前实现不能由清单本身推断全部策略正确。 | 与原则相符；没有发现诊断制造成交或回写市场状态。 |
| M01–M06，47–55：收盘竞价、停复牌、公司行为、市价申报、类别规则、L2 | 诊断按竞价阶段分开量统计，`packages/engine/src/diagnostics.rs:558–608`；其它项是范围/规则差异或扩展建议，不是当前无条件交付项。 | 不新增产品工作；本次不改交易语义，也未联网复核规则。 |
| A01–A14，59–73：身份、学习、公司信息、风险、执行、挂单、锚点、规模、时间、风格与关注 | 文首说明旧基线已部分过期。个体判断通过策略链产生，`packages/engine/src/session/plan_chain_candidates.rs:376`；此局DEV追踪仍存在G37，见下文。 | 旧“尚未实现”不能整体恢复为待办；局部断点保留原有账项。 |
| A09独立账户与规模，89–109 | 文档记录的是历史规模验收，不等于当前规模/性能结果；本次不运行压测。 | 不能宣称当前规模门禁重验通过。 |
| C01–C08，115–123：参数、基线、共同V、真实数据范围、跨宿主、当日统计 | 基础指标与跨seed汇总在 `packages/engine/src/diagnostics.rs:267–300, 497–772, 1193–1490`；C06由 `docs/diagnostics.md:62–65` 明确排除真实数据校准；活动日K统计不从UI最近成交缓存推算。 | 基础工具存在不证明拟合真实A股；旧共同V叙述为历史文漂。 |
| 清单建议指标及顺序，128–154 | price-volume报告与causal报告分别覆盖基础统计和离线微结构事实；并非所有建议指标均已实现。 | “建议长期记录”不能升级成遗漏实现缺陷；长局/性能证据未复核。 |
| 行为计划 161–207：持仓、路径、市场观测、异质判断、经历、目标/执行/成交、可恢复诊断 | `docs/diagnostics.md:31–43`区分散户目标和订单成交；读取器 `packages/engine/src/session.rs:2240–2268`明确诊断切片不属存档/撮合状态。实际计划到订单的根trace关联不足为G37。 | 目标仓位不冒充成交；DEV trace的因果链仍有缺口。 |
| B01–B04，211–227：观测、决策、恢复、多seed验收 | 报告定义按目标、可执行量和订单生命周期分层；恢复后causal collector显式记录restart。实际自由调度不承诺同seed每次接受轨迹相同，`packages/engine/src/diagnostics.rs:349–352`。 | 统计守恒可核对；不可把seed单独作为受理轨迹重放标识。 |
| 对照场景、现场记录，236–261 | 场景是测试方向；现场缓存数字明确为旧错误口径证据。 | 本轮未执行场景，不把历史截图当当前统计。 |
| CLI输入投影，263–276 | `packages/engine/examples/price_volume_baseline.rs:29–34`先将完整JSON反序列化为`SaveSlot`，然后仅读取`slot.setup`。结构异常的忽略字段仍可令CLI失败。 | G21仍成立。 |
| 完成检查及规则/论文依据，278–305 | 诊断来源是实际撮合；清单第9节区分规则与经验研究。C06的当前不使用真实市场数据边界与末行历史校准措辞有文档张力。 | 未改交易语义，未核验外部规则来源；标记历史文漂，不作为实现授权。 |
| diagnostics报告定义，`docs/diagnostics.md:3–14` | `run_price_volume_baseline`确实不读墙钟/UI；不过“相同setup/seed/天数逐字段相同”与生产注释的自由调度受理边界冲突，另见D1。CLI全量SaveSlot解析与第13行“仅读取SessionSetup”也应理解为消费字段投影，而非输入解析投影。 | 报告器输入投影账G21成立；逐字段确定性措辞需收窄。 |
| 活跃度、竞价/连续节奏、收益、盘口、取消，`docs/diagnostics.md:18–30` | 指标字段在`diagnostics.rs:267–300`；Trade与日K对账在`:1209–1238`；静默时段成交显式错误`:607`；最长无成交在阶段/日界重置`:693–717`。 | 口径实现相符；取消字段明确包含日终失效。 |
| 散户判断/执行/参与归因/逐笔对账，`docs/diagnostics.md:31–48` | Retail目标缓存来自会话诊断投影；订单账本按已分配订单ID更新，`diagnostics.rs:950–1108`；参与双方量来自Trade并检查两倍市场量，`:824–920`；无样本使用Option。 | 未见把意图、挂单、成交混淆的代码证据。 |
| 跨seed统计与极端样本，`docs/diagnostics.md:50–58` | 重复seed检查、原始runs、极端样本与分位数/区间分别见`diagnostics.rs:364–384, 1358–1490`。可复现seed不固定并行订单受理轨迹。 | 报告统计存在；“可复现seed”需限于setup/seed及实际运行记录，不代表轨迹确定复放。 |
| 不使用真实数据及连续竞价无成交口径，`docs/diagnostics.md:60–68` | C06/ADR-0023边界清楚；连续竞价无成交指标排除竞价、静默和隔夜。 | 与A股时段统计边界一致；无新遗漏。 |
| Causal artifact/source facts与feature隔离，`docs/causal-diagnostics.md:3–10` | `price_volume_baseline.rs:35–53`按seed重新创建会话采集causal；`session/causal.rs:12–14`报告由collector facts汇总；采集字段受simulation-diagnostics控制。 | 两个报告在同一artifact内，但并非同一场会话（N13-1）。 |
| 提交、成交、撤销、中止、open及守恒，`docs/causal-diagnostics.md:14–20` | 提交/执行/终止facts来自真实流水，`session/causal.rs:65–96`；aggregate校验逐订单和双边成交，`diagnostics/causal/aggregate.rs:100–114, 140–202, 257–306`。 | 定义有实现支撑。 |
| 游戏分钟/civil秒、实际获知延迟，`docs/causal-diagnostics.md:21–28` | bucket与civil时间生成于`session/causal.rs:16–31`；获知时延从published/acquired事实计算，`diagnostics/causal/aggregate.rs:59–68`。午休缺陷在文中明确保留。 | 未发现伪造源时点；240为游戏分钟简化，不是240真实连续分钟。 |
| 方向持续、冲击、深度恢复/null，`docs/causal-diagnostics.md:29–39` | 执行方向、首个后续有效quote、深度下降和50%恢复在`diagnostics/causal/microstructure.rs:35–158`；无观测保留null/reason。 | 指标按描述为观察量，不作因果估计。 |
| submission当前决策归因及restore restart，`docs/causal-diagnostics.md:40–45` | 提交时从账户当前决策与linked plan查归因，`session/causal.rs:65–96`；restore发restart并明确拒绝推断前史，`session.rs:2986`、`diagnostics/causal/aggregate.rs:43–45`。 | 同局offline causal facts可关联真实订单；它不自动填充另一条DEV trace。 |
| full fact vector内存声明，`docs/causal-diagnostics.md:47–49` | feature开启时fact Vec顺序保留，`packages/engine/src/diagnostics/causal.rs:200`附近；文档提示大报告消费端承担内存预算。 | 属已披露离线成本边界；未测试大规模内存。 |

## 旧结论复核

- **G21仍成立。** CLI第31行全量反序列化`SaveSlot`，随后第33行才取`setup`。因此格式错误的快照/订单/账户字段仍会在setup消费之前被Serde拒绝；正式恢复校验严格与CLI只需SessionSetup的工具契约是两个边界，修CLI投影不应放宽存档恢复。
- **G37仍成立。** 真实caller在`packages/engine/src/session/plan_chain_candidates.rs:376`；`decision_chain.rs:754–770`向`record_npc_decision_trace`传入空`events`。该函数`decision_chain.rs:800–810`只从OrderAccepted/OrderCanceled抽取order IDs，所以此root记录没有后续真实订单ID。`:812–817`的`budget_constraints`由终止计划状态生成，不能表示候选预算分配时实际拒绝/约束事实。独立causal collector能从提交和成交源记录订单，不是此DEV trace的consumer，不能核销G37。
- 该DEV只读查询路径仍在`session.rs:2252–2268`；读取已存trace不产生后续订单关系。只读/重复查询稳定性属于不同性质的保证。

## Sweep13候选复核

### N13-1：合并输出的两个报告没有共同运行身份

CLI先调用`run_price_volume_baseline`（`examples/price_volume_baseline.rs:33`），该API每个seed运行自己的`run_one_seed`（`diagnostics.rs:380–384`、`:792–805`）；接着CLI用相同setup/seed另起`GameSession`再跑causal（`price_volume_baseline.rs:35–53`）。seed不含实际并发受理轨迹，生产注释明确自由调度重复运行可能不同（`diagnostics.rs:349–352`）。最后只按JSON键将两组报告放进一个artifact（`price_volume_baseline.rs:55–57`），无run identity或两组实际事件量额校验。因此组合报告不能保证`price_volume.runs[i]`的指标可由同seed的`causal_runs[i]`订单/冲击事实解释。

**反证与范围：** causal报告第8–10行明确说每个causal run会新建会话；两份报告各自有内部成交对账，故不声称每次都实际不一致，也不声称内部守恒损坏。缺失的是artifact层对独立运行的显式身份/关联合同，或同一实际会话派生两种报告的保证。这是有效且独立的新候选，建议根审查编号，不能擅自固定自由调度或用seed伪称事件轨迹标识。

### N13-2：causal输出中潜在超JS安全整数保持JSON number

`docs/diagnostics.md:14`对报告承诺所有可能超过JS安全整数的u64采用十进制字符串；旧报告字段有`serialize_u64_decimal`（`diagnostics.rs:169–190`等）。因果DTO直接derive Serialize：`diagnostics/causal/report.rs:27–59`的`OrderLifecycle`/`CausalReport`包含raw u64的source_sequence、数量、seed、information_delays序号；相关fact time/sequence也使用u64。CLI第56行直接serde_json输出包装对象。因而合法输入`u64::MAX`会在`price_volume.runs[].seed`作为字符串，而`causal_runs[].seed`是JSON number，JS解析可能失精。不能从Rust serde_json可无损解析/输出u64推出所有消费者安全。

**反证与范围：** 小seed通常不会丢精度；此候选不声称Rust内部算术溢出，也不要求存档/API全局改序列化。报告承诺文本覆盖了diagnostics artifact，causal新增字段未沿用其u64策略，故N13-2仍是有效独立候选。应给因果报告潜在大u64字段补十进制编码并测试完整CLI包装输出的安全整数边界。

## 其它文档差异与边界

- **D1，确定性表述漂移：** `docs/diagnostics.md:3–5`称相同setup/seed/日数必须逐字段相同；`diagnostics.rs:349–352`明确自由调度实际受理轨迹可变化。清单`284–285`和行为测试场景`246`的无条件“同seed结果一致”也应以相同实际受理轨迹为前提。此处不要求改变自由调度生产实现。
- **D2，运行命令缺feature：** 清单`269`命令缺`--features simulation-diagnostics`，而example在`packages/engine/Cargo.toml`配置required-features；diagnostics文档`10`命令已含feature。属于文档命令漂移。
- **D3，历史基线混入现行语气：** 清单`24–27`声明旧V/母单描述非现行说明，但A03/C04/B02等若干历史措辞和第7节顺序仍可被读成当前状态；第305行要求以目标时期目标板块A股数据校准，与C06/ADR-0023当前不使用真实市场数据有历史语境差异。需保持历史性质，不得据此重开真实市场校准、共同V或扩展策略实现授权。
- `docs/causal-diagnostics.md`为全英文，而根约定要求新增/维护文档中文；本次授权仅审计，不直接改文档，作为规范性差异报告。
- 行为诊断数字包含股票/报价/股数/资金单位，但本轮没有发现单位换算或交易规则被该报告改写；未进行外部现行规则核验，也未运行测试，不能声称大A语义或构建已验收。

## 结论

G21与G37经当前提交复核仍未修复；Sweep13的N13-1、N13-2均有当前生产证据，反证不足以否定。未发现其它独立于既有账项与上述两候选的必需实现遗漏。D1–D3及文档语言差异为文档一致性问题；本审查无生产改动、无测试运行、无规则网站核验。
