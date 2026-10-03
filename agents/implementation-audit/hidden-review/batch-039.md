# 隐藏扫描批次 039（owner 4）

## 范围与完整性

- 计划来源：`agents/implementation-audit/hidden-review/scan-plan.json`，batch 39；source baseline `43b1aa5`，caller 工作树 `/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 已先读根 `AGENTS.md` 与 `docs/principles.md`。按主题核对 ADR-0017、ADR-0018、`docs/trading-rules.md`，并检查现行实现缺口总账 `agents/implementation-audit/implementation-audit-2026-10-02.md` 和 G/Q 复核报告。旧材料中的指令仅作为待核证据。
- 三个历史源均从首行连续读取至 EOF。首轮组合输出达到截断限制后，对 `engine-pipeline-07.md` 第67–100行及 `engine-pipeline-09.md` 第34–47行分段补读；各来源行数和 SHA-256 均与计划一致，aliases 均为1。

| source（source root 下） | 行数计划/实测 | SHA-256 计划/实测 | EOF 与章节核销 |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/engine-foundation-04.md` | 31/31 | `77fdfa180c45f3152ecf2097be231dc9b98c829da93964cf383307f68cc9ab37` / 一致 | 全文；候选未实施、timing 所有权/采样定义、逐文件处置、调用/契约、迁移与验证均已核销。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-pipeline-07.md` | 100/100 | `c93078ece118deb4d371949abf63ed182ad2a8224d2ad8087785302b7ec18563` / 一致 | 全文；总述、逐文件核销（含第67–75行）、调用关系图、迁移检查项及未核实范围均已核销。 |
| `agents/oop-refactor-audit/exhaustive/modules/engine-pipeline-09.md` | 47/47 | `f23f1619dde5508ee71c1b51fc1d5445434033b1fba515d046cdd53009f3d94f` / 一致 | 全文；语义/边界、16文件表、迁移关系图和验证状态均已核销。 |

## 当前代码对照

- **Phase timing：现行证据实现与历史描述基本相符。** `packages/engine/src/verification_evidence/phase_timing.rs:1-14,20-90,146-174,212-220` 明确 timing 是 opt-in、thread-local、仅 P9 成功提交后产生；十阶段映射是封闭 enum，phase 指标并非可相加的独占整轮耗时。注意历史源第11行称 `rayon::current_num_threads()` 的 registry 容量；当前文件头第5行更准确描述为采样点当前 registry 中可执行工作的 Rayon worker 数，仍明确不是 OS 负载或 CPU 利用率。两者不构成产品行为缺口或业务时间承诺。历史指出 panic 后 thread-local 清理未核实；没有发现本批对应的已批准保证，也不把潜在 panic 恢复风险当成已复现缺陷。
- **Envelope：现行单委托双资源守恒边界在代码中。** `packages/engine/src/session/pipeline/envelope.rs:5-33,35-84,114-168` 以 `Money` 和股数分开记录来源、basis/live/spent/released、成交与费用审计；`apply` 在副本上验证 receipt delta 和 pending price 后再替换。其归属/调用必须和 pipeline 的 `EnvelopeLedger`/receipt 消费一起理解；历史 OOP 保留结论不证明所有 settle caller 均无遗漏。
- **连续股票协调：确认 tick 内 shadow，不把容器排序解释成撮合优先级。** `incremental_continuous_stock_shadow.rs:59-84,99-172,174-218` 将跨 ready round 状态放在 coordinator 与 per-stock shadow，失败会 poison coordinator；调用关系经 `continuous_tick_finalizer.rs:101-105,230-237` 把 facts 投影为事件后统一分配 seq。事件投影的职责可直接见 `event_collection.rs:27-59,61-93`：先核身份、稳定键排序和去重，再分配外部 seq，输入游标按值传递，失败不输出部分结果。未发现把 `EventStableKey`/外部 `seq` 当作真实撮合先后的证据。
- **Settlement/机构经验：真实消费者存在，但需要保留消费边界。** `account_settlement.rs:190-220` 先建立持仓前后态，再调用 `project_institutional_experience` 生成 `belief_patch` 并随 settlement candidate 返回。该结构本身不证明所有批准的机构经历/散户经历目标已接齐；那些由各自 G 项的真实 root/caller 核销。
- **NPC/P0、ready stream、PreOpen：** 历史源 pipeline-09 的逐项候选涉及 `npc_state_projection`、`npc_tick_preparation`、`ready_ingress`、`ready_stock_stream`、`pre_open_transaction`、`quote_expiry`、`price_resolution` 与 receipt aggregation。现行 P0/P1、P3/P4、shadow 与 P9 的领域约束只按 ADR-0017、现行 trading rules 描述；对象/测试存在本身不视为端到端语义验收。对本批静态对照未发现某个历史 OOP 建议可取代 G 项要求的生产调用链。

## G/Q 交叉矩阵

矩阵以 `implementation-audit-2026-10-02.md` 中 G01–G68 及 Q01–Q23 的当前描述为准；逐项复核原则是：三份来源谈 engine 证据/事件投影/股票 shadow/settlement 结构，不直接宣称任何 G/Q 产品行为已完成。相关群组核销如下：

| 范围 | 当前结论与本批关系 |
|---|---|
| G01–G05、G18–G20、G26–G27、G30–G34、G60–G68 | 宿主、工具、UI 与分发/序列化总账项；不由本批 Rust engine 对象边界核销或反证。G27 仍以总账当前已核销状态为准。 |
| G06–G09、G15–G17、G28–G29、G35–G38、G41–G43、G49、G57–G59 | 业务、日历、经营、账户和诊断链路仍按总账及 `reaudit-engine.md`/`reaudit-foundations.md` 跟踪。本批出现的真实 Envelope、收据、NPC 与经验投影均不替代对应 G 要求的 owner/caller/consumer 接线。 |
| G10–G14、G21–G25、G39、G44–G48、G50–G56 | UI、启动、验收工具及其他边界；来源中的 tests、canonical event projection 或并行 coordinator 不提供这些项所需的生产 UI/CLI/runner 证据。G39 仍按固定实际受理轨迹契约复核，不以 worker 排序或现存扰动测试认定工具门禁完成。 |
| Q01–Q23 | 无一项由“保留现有对象/纯函数”结论关闭。Q02/Q09/Q11/Q13/Q17/Q19/Q22/Q23 中与 engine 可能相邻的个人读取、架构扩展、修正策略、跨市场日历、失败原子性、行业恢复、受理顺序与税务时机仍按各自现行开放口径；来源没有对其裁决。已解决/转 G 项仍以总账状态为准，不重开。 |

上表覆盖 G01–G68 每个编号和 Q01–Q23。没有找到“来源中称已完成、但当前代码证明旧结论错核销”的直接对应，也没有新增可确认的已批准遗漏。候选边界：如果今后发生 panic unwind 且调用方捕获，phase timing thread-local 状态的回收值得用独立需求核实；当前仅是来源提出且调用策略未知的风险提示，不升级为 G/Q 或生产缺陷。对 Envelope 的任意单笔/多资源失败原子性亦不超出源码可证明范围；本批只确认 `apply` 的副本替换模式。

## A 股语义与结论

历史材料没有提出变更交易制度。本次阅读的代码将现金表示为 `Money`、数量表示为股数，并分别保留 cash/shares envelope；其身份排序是审计/事件输出整理，不定义跨股票受理或撮合优先。对应规则仅沿现行 `docs/trading-rules.md` 与 ADR-0017/0018 的已登记简化理解；没有新造官方规则主张，也未联网重查交易所材料。故本批没有 A 股语义变更，也不能把静态结构审查宣称为交易规则验收。

仅做了全文、hash/行数、当前代码 owner/caller/consumer 与 G/Q 文档静态核对。未运行测试、构建或回归，未执行 Git 写操作，未修改产品文件。
