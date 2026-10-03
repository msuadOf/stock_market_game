# Luna09：三份单局多线程计划的独立全文代码审计

基线：`08e4fc7`。本记录只写审计结论，不改产品代码、不运行测试/长测、不执行 Git 写操作。

## 全文覆盖

- 已按顺序从头至 EOF 阅读 `docs/superpowers/plans/2026-09-24-single-world-multithreading.md`（187 行）、`2026-09-25-production-entry-and-thread-pool.md`（79 行）、`2026-09-26-ready-receipt-and-thread-boundary.md`（69 行）；合计 335 行。未以关键词命中片段代替阅读，矩阵覆盖三份文件中的目标、逐条实施单、历史结果、撤回实验、验收门槛和末尾状态。
- 已阅读根 `AGENTS.md`、`docs/principles.md`，并核对既有总账 `implementation-audit-2026-10-02.md`、工具/引擎复审和相邻 sweep08、sweep80、sweep81；G16/G39 的既有结论作为基线，不重编号。

## 逐步矩阵

| 原文范围 / 步骤 | 当前生产实现与调用链 | 结论 |
|---|---|---|
| 09-24：目标、tick 请求时点与来源修订（:3–23） | 三阶段 caller 经 `ReadyIngress::capture_sources` 取 NPC 与玩家队列，再捕获真实 roots；根结果按完成交付。账户资金/股份依赖在 `local_admission` 形成，股票 gate 提交至股票流。见 `ready_ingress.rs:33–58,72–90`、`local_admission.rs:97–175,223–245`。 | 目标语义与现行入口基本接通。09-24:64 是当日历史状态陈述（旧类序仍在），不能覆盖后续 09-26 状态。未发现新交易规则偏移。 |
| 09-24：生产改造顺序 1–5（:17–21） | NPC 请求随 tick 候选持久提交；玩家请求跨 step 队列取出；NPC/玩家组成初始候选，计划 roots 异步 ready；三个阶段共用 ready ingress 与 typed stock feedback；撤单走订单簿事实，整体失败由私有候选丢弃。相关接线见 `ready_ingress.rs:37–48,51–58,72–90`，三阶段 dispatcher 见 `continuous_tick_transaction.rs`、`pre_open_transaction.rs`、`auction_transaction.rs`。 | 请求 tick 边界、统一入口、业务拒单与致命失败区分及三来源真实负载，均有计划记录与当前代码路径。计划复核/受理前状态隔离和性能稳定性继续按下行所列未完成，不能从历史单次性能记录推出完成。 |
| 09-24：计划个人状态、生命周期只读/动作隔离、母单安装时点（:25–32） | 根个人状态被单账户任务取出再随结果安装，计划生命周期以动作批应用，母单随 P4 接受事实处理；ready root worker 自有 personal 与只读 snapshot，结果仅在 coordinator 消费时安装：`plan_chain_candidates.rs:304–340,342–378`。 | 历史逐批记录中的前置缺口已被后续实现接缝处理；仍须按真实 caller 看所有权复制（G16），不能将“只读 worker”视为零复制。计划母单/撤单时序已有当日条目和集成边界，未发现新增遗漏。 |
| 09-24：账户页组、日 K、P2 工作单、来源键及旧接口清理（:33–51） | `AccountBook`、经验与日 K 使用分页/追加共享；P2 工作单按到期观察筛选；候选身份不按键值重排；旧即时计划接口及旧串行消费器已清理。三份计划记载了失败的注意力堆、提前合批、账户观察并行、Market COW 等试验均撤回。 | 逐条区分已保留改动与明确撤回实验；不把撤回代码记成遗漏，也不把历史测试 PASS 当现行调用。账户/股票存储布局不是线程预算或交易优先级。 |
| 09-24：计划 1–5（宿主线程、P6、删串行回退、局部冲突、长期状态）（:51–60） | Web WASM 初始化 `navigator.hardwareConcurrency`/可选 `threadCount`；native 使用当前进程 Rayon；P6 账户并行 settlement；旧 fallback 清理；P3/P4 以账户 lane、股票 gate 和 typed plan 前驱协调；P9 私有 candidate 单点提交。线程预算不作请求配额。 | 步骤 1–4 主体接线存在；执行流水线和线程并发与描述相符。第 5 步“普通 tick 不随多年历史复制”仍未全部满足：RootReadContext 的全历史 PlanBook clone 明确见 G16。 |
| 09-24：验收与进度历史（:66–187） | 记录含生产构建/宿主运行、P6 失败原子性、旧路径清理、P3容量政策、P4双交易阶段逐股执行、P5/P6/P7/P9账本/回执/提交优化、Rust/JS负控修订及多组活跃负载性能测量。末段明确识别来源类序、全局密封号耦合和整轮性能，且撤回不正确的提前合批及堆/释放实验。当前 source identity 到资源 lane 的边界见上一行；性能原始数字均为文档历史证据。 | 逐项复核结论：有效测试守住交易状态、T+1、簿账守恒、失败回滚和必要依赖；对无关账户/股票不要求同序。阶段局部变快与一次 ABBA/绑定节点加速均不能升级为普遍性能保证。文中明确已撤回的负控/实验不得当作当前代码。没有可另立的新生产遗漏。 |
| 09-25：实施步骤 1–4、完成条件、阶段结果（:18–79） | 线程参数显式校验；三个入口统一 `ReadyIngress`/局部受理；玩家验收入队次序、NPC前一 tick形成次序、计划按实际 ready 到达登记；资金争用依账户记录、股票先后依股票 gate；连续交易股票私有 shard、逐股并行初始化/收尾、P4 sparse participants、P5账本所有权及 P4反馈逐事实投影。实现索引：`ready_ingress.rs:33–90`、`local_admission.rs:97–175`。 | 步骤 1–3 已接通；计划状态先取候选再应用以及计划专属反馈等待保持 typed 约束；容量不是业务排名。:71 的调度 lane 改造避免独立资源 lane 相互等待，不能因初版逐请求 worker 的中间实现再报缺陷。完成条件中的稳定多 worker 收益仍只由负载实测回答，报告自身承认样本波动。 |
| 09-26：当前事实、一次写完整范围 1–3 与验收（:3–21） | `ReadyIngress` 将 NPC/玩家候选先组成 initial，计划命令在真正 ready 后追加；`AccountReceipts` 提供账户局部真实次序；`build_resource_edges` 仅连接同账户 Cash 或 Shares lane，`run_stock_gates` 将就绪节点送股票边界。见 `ready_ingress.rs:37–58`、`local_admission.rs:98–125,159–175,223–245`。 | 接收时间不由账户号/计划号/输出编号生成。初始向量的来源拼接仍是代码事实，但账户局部 receipt 和股票实际 gate 决定可争用请求次序；独立项允许调度差异。09-26 状态说明比 09-25:9 的早期担忧更新。 |
| 09-26：股票边界、私有轮次、回执参与者、终检、P5/P4所有权工作（:29–69） | 现行股票流将每股状态交给 worker、同股逐次续行并发布 typed 反馈；私有订单簿/ledger 增量维护、轮末簿账检查，权威 P5/P9 完整检查及统一 settlement 留存；超大委托负载性能记录完整说明样本和限制。 | 每块描述的机制均能在 engine 正式 pipeline 找到对应接线；失败/撤单/部分成交仍走明确交易结果。代码/记录不承诺稳定整轮加速，也未发现跨层业务语义漂移。 |
| 09-26：状态总结及性能结论（:3、:25–27、:31–47、:49–57、:59–69） | 文档确认跨 worker 调度可令同 tick 受理结果不同；输入/配置/线程事实有记载，原始本地 `.tmp` 不是提交制品。当前 matrix 入口的 performance measurements 校验环境、运行参数和实际 runnable threads，见 `run-escrow-verification-matrix.mjs:102–127`。 | 记录中的单次数据和执行线程来源可解释但不能代替重复验收。未发现把线程数当请求限制或以订单号重建价时优先的新增实现问题。 |

## G16：全历史 PlanBook 复制

- 旧总账 G16 仍有效，非重复新项。生产 caller 为 `ReadyIngress::first_ready_batch`/三阶段 `capture_roots` → `capture_ready_decision_chain_roots` → `PlanChainOperationBatch::start_ready_accounts` → `RootReadContext::capture`（`ready_ingress.rs:80,84`；`plan_chain_candidates.rs:304`；`decision_chain.rs:539`）。
- `RootReadContext::capture` 执行 `session.state.plans.clone()`（`decision_chain/roots.rs:17–29`）。`PlanBook` 是 `Clone` 的 `BTreeMap<PlanId, TradingPlan>` 及活跃索引（`plans/mod.rs:135–146`）；这不是 `Arc<PlanBook>` 共享，克隆会随保留计划历史规模复制 map/tree 与计划值。随后把只读 context 放在 `Arc`，只是让多个 root 共用已完成的复制（`plan_chain_candidates.rs:304–339`），不消除它。
- 触发条件是有 ready roots；同一批 roots 每 tick 只 capture 一次，不是每账户重复复制。事实是保留计划历史越长，相关 root tick 的 capture 工作随历史规模增长；尚无因此导致正确性错的证据。建议按既有 G16 跟踪所有权/共享只读版本并测不同历史长度的普通 tick，不删计划历史、不变存档语义。
- 历史 root worker 失败/后台 worker 部分提交的候选已由既有反证覆盖：任务只持 snapshot 与 personal，不写 authority；接收端关闭会丢弃结果，worker 成功结果由 candidate 消费者安装（`plan_chain_candidates.rs:324–340,371–378`）。不是新增原子性缺陷。

## G39：K7 跨 worker 完整产物相等门禁

- 旧总账 G39 仍有效，非本次新增。矩阵 runner 对每个非负控条目冻结首个 artifact vector，后续预算/repeat 逐字相等（`run-escrow-verification-matrix.mjs:583–595,768–775`）；独立 contracts checker 对每个 worker budget/repeat 与 `1/0` 的 artifacts 及 execution coverage JSON 相等（`escrow-verification-contracts.mjs:130–162`）。矩阵具有不同 worker budget；它没有要求这些运行重放同一实际受理轨迹。
- 当前 engine 受理允许不同独立账户/股票由真实 lane/gate 调度决定进入顺序，不承诺跨运行同一全局顺序（`local_admission.rs:159–175,223–245`；三份计划 09-24:3–5、66–69，09-26:9–16）。因此以上完整 state/event/receipt/save artifact 相等门禁会把契约允许的合法差异误报 determinism drift。
- 保留负控和守恒、账户现金/股份、同股价时、typed 计划依赖、身份唯一/连续、失败隔离。可复现要求应限于固定同一受理事实的 replay；跨 worker 自由调度则分别核对不变量与每次实际结果。不得通过删除失败负控/对账断言规避 G39，也不得用全局排序修工具门禁。
- 与既有 `reaudit-tools.md:32–40` 及 `sweep08.md:69` 结论一致；Sept24:154 已移除“股票归并”旧负控，但当前完整 artifact 比较仍存在，前者不能核销后者。

## 旧结论复核、新候选与反证

| 结论/候选 | 原文和当前代码证据 | 处理 |
|---|---|---|
| 旧结论：G16 未完成 | Sept24:60要求减少执行 shadow 全量复制；总账 `implementation-audit-2026-10-02.md:67` 与 `sweep08.md:69`定位 PlanBook；实际 `RootReadContext::capture` 仍 clone BTreeMap。 | 维持 G16；精确限定为历史 PlanBook 的捕获复制，不泛化成所有 history 均已线性扫描。 |
| 旧结论：G39 未完成 | Sept24:154 只撤销旧 canonical merge负控；现行 runner/contracts 两处仍比较完整跨 budget/repeat artifact。 | 维持 G39；保留不变量与负控，只改自由调度一致性语义。 |
| 来源 vector 顺序可能复活 NPC/player 优先 | 09-26:9 自述初始 vectors 顺序；代码 receipt observe 后现金/股份 lane 按 AccountReceipt 排序，跨账户同股顺序由实际 gate 到达，候选编号不参与（`local_admission.rs:98–125,159–175,223–245`）。同账户跨来源会否真实同时存在也受玩家账户与 NPC 账户划分限制。 | 现有证据反证此前把 vector concat 直接等同交易优先的候选；不新增。跨来源跨 lane 的更多边界由局部受理短测覆盖策略，而不是测试完整 artifact 等值。 |
| 同股 order_id / sealed index 作为价时优先 | Sept24:51、:153 和当日规则核对说明 `AuctionOrderSnap.order_id` 用于身份，集合竞价队列位置决定实际到达；连续簿 gate接收顺序传递。 | 不新增 A 股语义问题。没有重新联网查交易所规则；本任务复核仅确认此重构不混淆身份与同股时间。 |
| 性能不足本身是确定代码 bug | 三份计划完成条件要求整轮对照，但所列活跃测量多数单次且自述波动，09-25:25及09-26:3明确目标需持续验收。 | 作为性能验收未闭合保留，不虚构新生产代码遗漏或稳定加速承诺。 |
| 线程数/容量削成请求配额、非玩家/ NPC命令在账户状态采纳前产生账单变化 | 计划明确反对条数限额；计划根观察与交易提交分开，失败丢弃候选。 | 在当前代码/矩阵中未找到新增反证；不扩成新候选。 |

## 结论

在本审计范围内没有发现 G16/G39 之外的新确认遗漏。G16 是真实生产 capture 的数据规模问题；G39 是当前验收工具把合法自由调度差异误作错误的问题。局部受理、同股顺序、typed 续行和失败原子性没有显示新增 A 股语义缺陷。本文为静态代码/契约审计，没有重新运行定向测试、整轮性能测量或 K7 矩阵，不把计划内历史通过记录写作本轮验证结果。
