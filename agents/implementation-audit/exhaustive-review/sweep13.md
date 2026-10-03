# Sweep13：量价报告与因果诊断全文审查

审查日期：2026-10-03。产品基线：`b76ece3`；读取时工作树 HEAD 为 `4ad5a2e`，对本记录涉及的诊断生产文件及三份文档执行 `git diff --stat b76ece3 HEAD -- …` 无差异。只读审查，未执行 Rust 编译、测试、长验收或产品变更。

已读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`、`docs/architecture.md`、ADR-0023。三份指定文档逐行连续读完：`docs/price-volume-simulation-gap-checklist.md` 305 行，`docs/diagnostics.md` 68 行，`docs/causal-diagnostics.md` 49 行，共 422 行。部分初次合并输出被截断，随后按连续行段补读至末行。本记录仅为审计证据，不把研究建议提升为获准实施要求。

## 条款覆盖台账

| 原文与条款 | 状态与当前实现证据 |
|---|---|
| 量价清单 14–18：权威成交、统计优先、采样与真相分离、资金可解释、无预画验收曲线 | 诊断直接收集 `session.step()` 的 Event；`packages/engine/src/diagnostics.rs:792`。Trade 量额逐笔累积 `:509`，日 K 事实 `:612`，对账 `:1209`。指标不回写价格。资金不机械守恒按 ADR-0024 的现行边界；不能因此要求补现金。 |
| 24–41：旧 V、DriftUp、身份与注意力等历史基线 | 文首已经声明历史；当前 `decision_chain.rs:753` 与 `plan_chain_candidates.rs:371` 接入个人 root 和计划。旧 V/母单措辞不能据此重开恢复 V。本文未对整个策略算法独立核销。 |
| M01，47：开盘/收盘竞价分列 | 已有统计：`diagnostics.rs:558` 按 phase 分别累计，字段 `:281`；不把开盘/收盘量混作连续量。 |
| M02–M06，48–52：停复牌、公司行为、市价类型、类别阶段、深度视图 | 研究/扩展范围，按该文 147 和现行 trading-rules 判断；不能因清单旧“尚存差距”就新增当前必做项。现行公司行为不执行，市价保护限价为登记简化；完整 L2 是条件研究。 |
| A01–A08、A10–A14，60–73：策略扩展、学习、公司信息、风险、执行、挂单寿命、锚点、ETF、时间窗口、风格、关注 | 历史机制清单，已实施局部不能外推全部。公司/个体预期已接线；旧“尚未实施”与文首 24–27 不一致。与主账 G07/G08/G09/G37/G38 相关的局部断链保留，研究性跨身份组合、ETF、社会传播等不新开实施授权。 |
| A09，68、89–109：独立账户规模、恢复与同 seed | 20k/50k/100k 为历史验收描述，不能把 2026-09-10 的 402.25 秒记录当现行 deadline 通过。相同 seed 不保证自由调度的实际成交和整局字节一致；现行 `docs/open-questions.md` Q8 明确边界。规模证据与当前长期性能门禁不是同一事实，沿用主账验收债。 |
| 81–85：持续意图、紧迫度、个人经历、关注、跨股预算 | `plan_chain_candidates.rs:371` 安装个人 root 结果，`:376` 记录诊断；实际提交归因 `session/causal.rs:65`。存在计划/执行接线不等于 DEV trace 能关联实际订单，G37 下详述。 |
| C01/C02，115–116：U 型与设计参数 | 持续敏感性研究；真实市场数据校准已经 C06/ADR-0023 排除，不登记为等待授权。 |
| C03，117：风格化事实基线 | 基础指标已实现，见 `diagnostics.rs:267` 字段、`:497` 事实收集、`:1193` 汇总。冲击与寿命由 causal 扩展实现，不应误报完全没有。报告尚不能证明拟合真实市场。 |
| C04，118：移除共同 V | 原文“待实现”为历史文漂，文首声明已删除；不登记恢复旧 V。 |
| C05，119：多 seed、分位数、极端样本 | `diagnostics.rs:364` 拒重复 seed，`:380` 保留输入顺序，`:1358` 汇总极端样本，`:1476` 分位数和均值区间。自由调度下“可复现 seed”只能定位某次记录，不足以独立重建受理轨迹。 |
| C06，120：真实行情校准不适用 | 当前范围；`docs/diagnostics.md:62` 同步。虚拟前史不等于真实数据校准。 |
| C07，121：三宿主/发布频率一致性 | 回归门禁，本文追到查询入口和 feature 隔离；未运行完整三宿主 E2E，不宣称验收完成。不能由同 seed 自由调度整局相等替代实际受理事实重放。 |
| C08，122：当日累计统计来源 | Rust 活动/闭合日 K 事实与逐笔对账已有；Web `apps/web/src/host/protocol/wire-values.ts:75` 接收 trade_stats、`:79` 无损 turnover 字符串，`apps/web/src/mobile/kline-sync.ts:17` 使用 K 线统计。不从最近成交缓存重新汇总。 |
| 128–134：建议长期指标 | 当前 StockPriceVolumeReport 包含基础活跃度、价格、盘口；causal 包含寿命/方向/冲击/恢复。跳空分布、涨跌停持续时间、逐户收益集中度、方向衰减曲线、完整 CPU/发布性能等不全在该报告中；这些是“建议”，不冒充已承诺完成，也不列成新产品缺口。 |
| 140–154：顺序与讨论状态 | 历史顺序（含决定是否补 M01）已落后于当前实现；不是当前待办顺序。 |
| 161–207：风险、时间尺度、市场观察、风格、记忆、判断/成交区分、可解释与恢复 | 判断缓存生产投影 `pipeline/npc_state_projection.rs:209`，真实订单统计 `diagnostics.rs:950`；retail 诊断读取只读 `session.rs:2243`。机构 root 与散户 legacy memory 的区别仍需主账 G08，不以本报告计数核销其衰减。 |
| B01–B04，211–227：历史 checkpoint | B04 的“目标/可执行/提交/成交/撤销/中止/未结”字段分别实现 `diagnostics.rs:212`、`:231`；不是所有新增机制已经完整验收的证明。自由调度测试现改为实际事实对账（`tests/diagnostics.rs:84`），旧“同 seed 复放”用语需限定输入轨迹。 |
| 236–246：对照场景 | 作为测试设计方向保留；本轮未运行这些场景，不能把源码有测试等同于本轮通过。 |
| 253–261：第 84 日截图 | 明确是旧缓存口径事故证据，不把 14 笔当全天真相，不据此归因 NPC 数或资金。 |
| 265–276：CLI | G21 仍缺；命令另漏 feature，见文漂 D2。 |
| 280–286：完成检查 | 统计/守恒遵守；“相同 seed”和快档不改变交易结果必须适用现行受理轨迹边界，不能以自由调度整局字节相等为门禁。 |
| 292–305：官方规则与经验论文 | 区分现行制度与研究，保持边界；本轮不改交易制度，因此未重新访问规则网站。 |

## 量价报告逐项核对

`docs/diagnostics.md:18–45` 的活跃度、阶段分量、十区间、收益及相关、盘口、全部取消/接受、目标差额、订单 ID 生命周期、双边参与量、量额对账均有生产实现：`diagnostics.rs:267`、`:497`、`:724`、`:950`、`:1193`。静默期出现 Trade 会显式报错 `:607`；最长连续无成交在非 Continuous/有成交时重置 `:703`，日界再重置 `:717`。没有双边盘口、无成交占比和常量相关使用 Option，不把无样本当零。全部取消仍混合日终失效，与文档警示一致；causal 的终止分类单独实现不改变此字段定义。

跨 seed `:52–58` 的分位数、均值 95% 区间、单 seed 退化、重复拒绝和完整 runs/极端样本有实现（`diagnostics.rs:364`、`:1358`、`:1476`）。`docs/diagnostics.md:3–5` 逐字段相同承诺已经不符合 `diagnostics.rs:349–352` 的自由调度边界，是文漂，不要求改生产实现去固定调度。

## 因果分类与 DEV 查询调用链

`docs/causal-diagnostics.md` 全部定义已对应：

| 原文条款 | 实现与反证 |
|---|---|
| 4–6：不可变 sequence facts、feature 隔离 | `diagnostics/causal.rs:200` 顺序追加，`session.rs:1156` feature 字段，`session.rs:11` feature 模块。业务/存档 hash 排除 collector（`session/hash.rs:58`），独立 offline feature 可以存在，不能误报成发布私有资产。 |
| 14–20：提交/成交/撤销/中止/未结和双边守恒 | `session/causal.rs:65` Submitted，`causal/aggregate.rs:114` Fill，`:166` Termination，`:202` Execution 与双边实际价量对账，`:278` 总量守恒和两倍市场成交校验。验证拒绝不凭空造 Submitted。 |
| 21–25：市场分钟与 civil 时间 | `session/causal.rs:16` 240 游戏 bucket，civil 取生产观察时刻。不是 240 真实连续分钟。 |
| 26–28：获知时延与 inherited lunch 缺陷 | `causal/aggregate.rs:59` 实际 published/acquired 差值，负值错误。原文已经明确 inherited decision clock 缺陷；沿用主账时间边界，不为诊断修饰事实。 |
| 29–34：方向持续/冲击非因果估计 | `microstructure.rs:39` 只消费 side Some 的执行，逐股串链；`:81` 找首个后续有效同股双边 quote。跨过中间 transition 有文档说明，不能误报成因果识别算法。 |
| 35–39：深度损失/50% 恢复/null | `microstructure.rs:111` 每次深度降低均登记，`:122` 从当前 fact 起找 50% 门槛，因此合法 0 分钟；找不到用 null/reason，未伪造恢复。 |
| 40–42：submission 的当前 execution observation | `session/causal.rs:75` 取 linked plan，`:88` 取当前账户 decision；company 来自 issuer，普通单不强行制造 plan。此链与下列 DEV trace 空 order_ids 是不同能力。 |
| 43–45：restore restart | `session.rs:2986` 恢复后 ObservationRestart，`aggregate.rs:43` 显式 RestoredObservation；不把旧档发明为原始诊断来源。 |
| 47–49：完整 fact vector 的内存限制 | Vec 全保留 `causal.rs:200`，已有明确离线使用警告；无限局的成本没有被声称解决，不新报同一边界。 |

G37 仍成立：真实生产 root receiver `packages/engine/src/session/plan_chain_candidates.rs:376` 调用 `record_plan_root_diagnostics`；`decision_chain.rs:764–769` 给 `record_npc_decision_trace` 的 events 仍为空切片。后者 `:800` 仅由 Event::OrderAccepted/OrderCanceled 抽取订单，因此 `order_ids` 不可能获得随后实际订单。`budget_constraints` `:812` 仍取已终止计划状态，不能代替真实分配约束事实。离线 causal collector 有真实 Submitted/Execution 不能核销当前局 DEV trace 的 G37。

查询 caller 完整链已追：`apps/web/src/App.tsx:82` DEV lazy import；`:655` 能力门禁；`dev/NpcDecisionInspector.tsx:40` 调 host；Worker `host/worker-host.ts:299` → `wasm-worker.ts:277` generation/feature → `apps/web-wasm/src/lib.rs:455` debug+feature export → `session.rs:2252` 只读。Remote `host/remote-host.ts:291` → `apps/server/src/routes.rs:561` debug+feature + authorized session + generation actor；Tauri `host/tauri-host.ts:248` → `apps/desktop/src-tauri/src/lib.rs:187` → actor。WASM 动态诊断包仅 DEV+显式 flag `wasm-worker.ts:111`，release 走正常包 `:122`。API 只读的重复查询测试不能证明订单关联，未发现本链新的泄露缺口。

## 新候选

### N13-1：同一 CLI artifact 的量价与 causal 来源为两次自由调度运行

依据：量价清单 `:205–207` 要诊断解释来自实际决策/实际成交，`docs/causal-diagnostics.md:8` 合并输出两报告。当前 `packages/engine/examples/price_volume_baseline.rs:33` 完整跑量价报告；`:36–53` 随后用同 setup/seed 新建 session 再跑 causal；`:56` 仅拼 JSON，无共同 observation/run 身份和跨报告实际量额核对。`diagnostics.rs:349–352` 及 `tests/diagnostics.rs:84` 明确自由调度两次可能不同。因此同 seed 对应的 causal 订单/冲击不能可靠解释 price_volume 同 seed 已记录成交。

反证检查：两份报告各自内部守恒都实现，`docs/causal-diagnostics.md:9` 确实说 causal 新建 session；所以不声称每次必然错配，也不声称任一内部对账已坏。缺口是组合 artifact 缺少区分独立运行或同实际来源的关联契约，建议根审查后决定新增编号；可用单次实际会话同时派生或明确分开 run identity，不要求固定自由调度。

### N13-2：新增 causal JSON 绕过已承诺的 u64 无损编码

依据：`docs/diagnostics.md:14` 和量价清单 `:274` 承诺潜在超 JS 安全整数的 u64/seed 是十进制字符串。旧 `PriceVolumeRunReport.seed` `diagnostics.rs:177` 有 serializer。新增 `CausalReport` `diagnostics/causal/report.rs:45–52` 的 seed 和总股数是裸 u64，derive Serialize 没有十进制编码；`OrderLifecycle` `:31–38`、FactTime/sequence 也同样。CLI `examples/price_volume_baseline.rs:56` 直接 json! 后输出，不再转换。`u64::MAX` 被 CLI parse 测试明确允许，故同 artifact 的 `price_volume.runs[0].seed` 是字符串，`causal_runs[0].seed` 是 JSON 数字，JS 默认 JSON.parse 不能保全该 seed。

反证检查：Rust serde_json 自身能保存 u64 数值，不是 Rust 运算溢出；也不声称普通小 seed 已损失精度。`tests/diagnostics.rs:278` 无损测试只对旧 PriceVolumeBaselineReport 序列化，未涵盖 CLI 新包装/causal_runs，不能据此核销。建议新增诊断输出跨语言精度边界缺口。

## 既有 G21 与文漂

G21：`examples/price_volume_baseline.rs:31` 仍反序列化完整 SaveSlot，然后只用 setup；无关字段的结构错误会提前拒绝，与清单 `:276` 忽略无关快照/订单/账户字段冲突。这里是 CLI 投影契约，不应据此放宽正式 SaveSlot 恢复深校验。

D1：`docs/diagnostics.md:3–5`、`:57` 的逐字段相同/可复现 seed 以及旧清单 68、224、246、284 的无条件同 seed 说法，应对齐实际受理轨迹边界；生产注释与测试已经更正。

D2：量价清单 `:269` CLI 命令未加 `--features simulation-diagnostics`；`packages/engine/Cargo.toml` 的 `[[example]] name = "price_volume_baseline"` 明确 required-features，按原命令 cargo 会拒绝运行；`docs/diagnostics.md:10` 正确。属于文档命令漂移，无需扩大产品范围。

D3：清单文首说明历史；但 A03/C04/B02/M01 顺序、母单/V 描述仍夹杂“未实施”，需保留历史性质避免派生重复待办。全文末行 `:305` 仍要求目标 A 股数据校准，须与 C06 和 ADR-0023 区分历史研究语境。`docs/causal-diagnostics.md` 全英文与现行中文文档原则不符，为文档规范债，本轮仅记录。

结论：新增两个独立候选 N13-1/N13-2；复核 G21/G37 仍未修复；记录 D1–D3 文漂，不重开已取消真实市场校准、旧 V、未获准 ETF/L2/复杂资金迁移，不把重复 seed 自由运行整局一致视为正确验收要求。
