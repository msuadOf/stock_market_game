# 全文 EOF 独立复核：diagnostics 与 experience

- 复核目标：merge HEAD `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`（merge commit `08e4fc7`）；产品改动基线记录为 `b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 复核日期：2026-10-03。
- 本记录只审查 `diagnostics-result.md`、`diagnostics-review.md`、`experience-result.md` 的全文（含最后一行），并抽样追到声明的真实 owner/caller 与最终汇总证据；不运行测试，不据静态读取宣称构建或产品验收。
- 先读仓库 `AGENTS.md`、`docs/principles.md` 和 `docs/trading-rules.md`。交易语义判断以正式规则文档为准：当前明确模拟与简化范围如交易时段、板块差异、T+1、单位、费用和未模拟类别均不因这些 owner 重构记录而改变；本次没有新官方规则核验，也不把历史规则日期写成 2026-10-03 的新依据。

## EOF 与章节矩阵

| 文档（全文行数） | 全部章节/范围 | EOF 状态 |
| --- | --- | --- |
| `agents/oop-refactor-implementation/domain/diagnostics-result.md`（138） | 标题、基线与边界 1–19；N30 21–36；N31 38–55；N32 57–76；N39 78–93；N41 95–114；检查交接 116–138 | EOF 为第 138 行“完整回归及任意长期诊断验收未运行”。该限制仍需保留。 |
| `agents/oop-refactor-implementation/domain/diagnostics-review.md`（120） | 标题/状态 1–13；范围方法与版本绑定 15–52；三门核对 54–68；关键次序 70–94；接线发现 96–113；结论限制 115–120 | EOF 为第 120 行“这些均交给 root 的统一运行验证”。静态审查不等同运行验收。 |
| `agents/oop-refactor-implementation/domain/experience-result.md`（53） | 元信息 1–8；前置边界 10–15；动作与 caller 矩阵 17–24；N07 caller 细节 26–28；短保护/TDD 事实 30–47；独立复审/验证 49–53 | EOF 为第 53 行短验证结论；不声称完整回归通过。 |

## 逐项复核与候选

### diagnostics-result.md

- N30/N31：owner 和摘要在源码中仍成立：`RetailOrderLedger` 位于 `diagnostics.rs`，`SeedDiagnostics`/`StockRunDiagnostics` 与真实 `run_one_seed` 投影同处该模块；`run_price_volume_baseline` 仍以真实 Session steps/events 采集，而不是第二份撮合状态。报告口径中的成交份额、目标/可执行目标与账户资产应继续分开理解；与正式 A 股规则文档无冲突。本文称实现阶段未跑红绿，不能升级成实现者 TDD 运行证据。
- N32：`CausalReport::from_facts` 仍委托 `CausalReportBuilder`；`analyze` 真实逐 index 调用方向和 quote owner。`facts[index + 1..]` 与恢复扫描 `facts[index..]` 的起点仍体现观测窗口约定。二者是报告派生观察，不是交易所撮合或因果估计；原说明未将其冒充真实市场规则。
- N39：`PhaseTimingLedger`/Collector 仍走 `authoritative_tick` 的 precommit 校验和 commit 成功后的 `mark_committed`。源码 `current_runnable_threads()` 直接取 `rayon::current_num_threads()`，所以文中称它为 Rayon registry worker 容量、而非 OS runnable/CPU 利用率是准确且重要的限制；不得将字段名解释成真实运行线程观测。
- N41：feature-gated 真 caller 已核实：`decision_chain.rs` 调用 `record_plan_root` 后仍调用 decision trace；`execution/records.rs` 在真实成交后调用 `record_continuous_fill`；`auction_day_end.rs` 将 receipt before/after 交给 `record_auction_fill` 并保留错误映射。抽查 `CausalCollector` receiver 的 append/checked arithmetic/receipt chain 顺序与文中说法吻合。不能从 diagnostics facts 推导交易优先级或代替 authoritative receipt。
- 历史短测状态：各动作段描述的是“实施者当时未运行”；EOF 第 136 行及 `final-summary.md` 后续记录的是 root 最终冻结后的运行。二者时间点不同，不构成自相矛盾。最终汇总记载相关 104 个 case（首批 100 + N07 增量 4）通过；diagnostics EOF 的 21 个为本簇指定子集。它们都不代表全量回归通过，文件也明确写明未运行完整回归。
- 旧结论复核：实现语义、真实 caller 与“没有修改交易制度”的旧结论静态成立；但独立审查对最终源码版本的绑定有以下候选 C-53-01，故“最终完整 diff 已无待修发现”的复核范围声明不能不加限定地沿用。

### diagnostics-review.md

- 三门内容与源码及正式规则基线一致：没有发现交易制度语义漂移；金额/股数边界仍是原领域单位；双边 participant shares 对账不是单边市场成交量；诊断事实顺序不是跨实体委托排序。
- 接线发现 D-INT-01 的修复可由最终调用点直接证实：plan-root 不再访问 private `facts`/`decision` 字段，而委托 owner；continuous/auction callers 也分别使用 receiver。其“静态复核闭合”在 caller 范围内成立，不是 feature 编译通过的证据。
- **C-53-01（需重绑定复核，非已证实行为缺陷）：** 本文 2026-10-03 的最终版本表声称六个文件 SHA 与已复核版本一致，但在本复核 HEAD，`diagnostics.rs` SHA 为 `21738e1c…`（记录值 `198cda3f…`），`microstructure.rs` 为 `8d921113…`（记录值 `9f42ebb5…`）。其余四个文件 SHA 与表中一致。历史 `git log` 显示两个不匹配文件分别在该记录绑定的实现之后又有提交：`a40b127` 修改 diagnostics.rs 文档，说明自由调度重复运行报告可能不同；`2247f4f` 再改该文档，并将方向 owner 的 `side` 匹配改为 `side: Some(direction)`，明确 auction `None` 不参与方向链。后一个变化与现有规则“竞价无主动方向”及当前测试断言相符，静态未见 A 股语义问题；前者使“相同输入完全相同报告”的过时说法得到修正。它们看起来是有意的澄清/等价行为收束，但旧审查 SHA 并未覆盖这两个最终字节版本。应由未实施者针对这两处最终差异重新核验并更新指纹，之后才能把独立审查覆盖结论绑定到最终文件。此候选不要求回滚，也不据 SHA 差异臆断代码错误。
- 日期/证据边界：文档引用的官方规则核对日期沿用 `docs/trading-rules.md` 和历史审查记载；本次只读静态检查不构成重新访问官方材料。review 的“不运行 Cargo/测试”与 implementation/final-summary 的 root 后续运行分别属于审查者与统一 runner 的事实，不得合并成“reviewer 跑过”。

### experience-result.md

- N01/N40：`indicators.rs` 中 EMA recurrence 与 KDJ 状态累积仍被 `macd`、`kdj_ohlc`、`kdj_from_values` 原调用路径使用；period/window 与算式仍在原 caller。它们是技术指标的游戏计算，不是报价/成交规则。
- N07/N28：`RetailPositionDecisionContext` 的实际调用在 `behavior/decision.rs`；N07 的机构 dated observation 与 stale 清理在 `session/institutional_behavior.rs`/`session.rs` 调用生命周期 State API。六个 writer 与 state transition owner 的记录与 `PositionExperienceTransition` 文件存在及引用相符。target 与 executable delta、整手与余数、T+1 的区分应按正式规则和对应 ADR 解释；本文未声称该游戏策略参数是交易所制度。
- N34/N35：price memory 和 retention owner 确实存在于 `experience/price_memory.rs`、`experience/retention.rs`，条目淘汰使用 attention/touch 时钟属于游戏状态维护。没有找到其改变证券交易规则的主张。
- 历史测试结论：第 34、47、51 行分别划清“实施者未运行”和“root 最终冻结后运行”。`final-summary.md` 记载统一编译、类型检查及 104 个指定短 case；因此本结果第 53 行的“14 个指定短保护通过”可视为该批汇总的子集陈述，但不构成完整回归通过。本文另称 N07 六个用例移位、路径/filter 保持，属于代码组织事实，不应误读为六次独立测试结果。
- 旧结论复核：范围最小、engine 层隔离、未增依赖/存档字段和规则沿用的陈述未见反证。该结果引用的 `experience-review.md`/汇总验证说明不同时间点；本任务只读该结果、未重新独立复审其完整 implementation diff，故不能将本记录冒充 AGENTS 要求的改动者之外完整三门复核。

## 总结

未发现可从本次静态证据确证的 A 股规则回归，也未发现真实 caller 漏接。唯一保留项 C-53-01 是 diagnostics 两文件在既有 review SHA 之后有小幅变更，旧“最终版本已完整独立复核”绑定需重做；当前差异内容可理解为报告确定性边界澄清及不计竞价方向的表达收紧，不能仅凭 SHA 变化宣称产品缺陷。全量回归、官方规则重查和本次新增代码验证均未执行。
