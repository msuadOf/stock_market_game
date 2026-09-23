# escrow-parallel-engine - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** 股票模拟引擎改为并行架构并附实测提速证据——同一只股票内交易严格排队，不同股票和不同账户可并行运转；**密封操作（成交/撤单/拒单/竞价完成/日界）产生的资金释放与入账在下一密封批次才可用，报价过期释放是例外（在本批次截点前即可用）**；游戏行为除**明确登记的九条分歧**（时间粒度、托管预留口径、**卖单费用封顶于所得（你已裁定）**等）外保持 A 股语义。

**Why this approach:** 买单预留资金，卖单预留可卖股票；撮合只操作本股票的托管资产，不同步等待其他实体或获取跨实体业务锁。实现唯一的并行代码路径（不建串行参考版本），正确性风险由跨线程数、调度扰动、新旧语义投影对比及守恒检查共同控制。这些检查提供测试覆盖范围内的验证证据，不构成整个系统正确或绝无死锁的数学证明。

**What it will NOT do:** 不迁移旧存档（旧档会被明确拒绝）；不预设提速幅度（只报实测）；不承诺跑满全部 CPU 核心或绝对无死锁；**除分歧 #9 明确批准的"卖单费用实收封顶与卖单零现金预留"外，不改变 nominal 费用函数、税率、最低佣金累计口径、买方费用语义及 T+1、集合竞价、涨跌停等 A 股规则本身**；不引入消息队列或 actor 框架。

**Effort:** XL
**Risk:** High - 引擎 tick 全流程重构且无独立串行参考；语义漂移风险由 扰动门禁 + 语义投影语料 + 双账本资源守恒 + 保留语义测试 控制
**Decisions to sanity-check:** 分配截点 `seal_allocation_snapshot`（**post-P0 预算公式**，无独立释放项；卖单现金占用恒 0，预算只减买单占用）；envelope 为资源向量、双账本（封前/密封）分资源守恒、聚合=逐键方程求和；**卖单费用封顶于所得（用户已裁定，分歧 #9）：本腿实收 = min(累计应计 − 已实收, 本腿成交额)，分项按 佣金→印花税→过户费 优先级拆分，deliver ≥ 0、spent.cash 恒 0、卖单预留恒 0；接受差异双向如实登记（旧预留 > 可用现金时旧拒新收；旧预留 == 0 时零现金接受相等）**；**买单价格改善差额逐次释放**（同现行公开行为）；**终结转移一律清零**；收据来源 tagged（SealedIntent|P0Expiry|Auction|DayEnd）+ 每来源因果序号（时序按构造安全）+ 收据链校验；P3 产 `EnvelopeDraft`、溢出即类型化中止（无事件泄漏）；事件源索引唯一序数映射；策略状态 `StrategyState` 唯一权威（profile 派生、封闭注册表、无 downcast）；比较键双侧可派生（固定规则 + 穷举 match 编译器强制 + 逐 tick 身份/载荷多重集比较；帧内数组重排不构成新分歧）；任务 2 全绿骨架（后续实现批次按用户批准的整模块实现/集中验证顺序；进入共享集成基线前须测试与独立审查通过）；类序保留 `npc → player → plan_chain`；毒化仅类型化失败；双哈希回滚；worker 内拒单消耗 ID；跨 envelope 同 tick 撤单取消；不加自成交限制；WASM 保留多线程；不建串行参考（用户已定）。

Your next move: 由协调者核对已有成果与全部后代的在途工作，按 A 核心引擎、B 存档恢复、C 宿主交付、D 验证收尾四组件登记接口版本、隔离环境和验收场景；每组件仅保留一个实现 agent，main 持续集成，不重做已验收内容。

---

> TL;DR (machine): XL/High - escrow 预拨 + 阶段化 tick（全 shadow + 双账本 + 单点提交）+ 实体单写者并行；单一并行实现；数据流契约内嵌（EnvelopeDraft、统一收据键含因果序号、tagged 事件索引、StrategyState 枚举、买侧逐次释放 + 卖侧费用封顶（预留恒 0））；12 个实施任务 + 4 项终审；确定性=跨预算+扰动+语义投影语料三重门禁；产出含存档 v2、三宿主接入、实测性能证据与全新 K7 矩阵。

## Scope
### Must have
- 账户预拨托管（资源向量 envelope）→ 股票独立撮合 → 收据结算的完整闭环，含部分成交、撤单、过期、集合竞价、日界；**双账本转移方程完备**（封前 P0 账本 + 密封批账本，cash 与 shares 分别守恒，聚合 = 逐键方程求和）。
- **单一并行实现**：阶段化 tick 管线 + 阶段边界 rayon 并行（native 与 wasm 同一并行代码路径），不建独立串行参考版本。
- `step()` 返回 `Result`；**毒化仅覆盖类型化失败与不变量违规**（panic 不承诺恢复）；**全阶段 shadow 化（含策略影子竞技场）+ `commit_tick` 单点权威提交**；双哈希回滚判据；静默点存档 v2（含全量策略状态），旧档显式拒绝。
- 三宿主（web-wasm / server / desktop）致命错误映射 + TS 类型再生。
- 确定性三重门禁：跨线程预算（1/2/4/auto）+ 重复运行字节对等；**带收据的执行顺序扰动对等**（防空转 + 负对照）；**重构前语义投影语料对等**（每分歧定向场景 + 字段级允许变换 + seq 结构化比较器含旧侧派生比较键，排除 9 条已批准分歧）。
- envelope 双账本逐键守恒 + 账户聚合守恒（同公式）；实测性能证据（重构前后同机同种子对比）。
- 诊断基线迁移说明与全新 source-fingerprinted K7 矩阵（94 次执行 = 85 canonical + 9 determinism rerun；包含链校验 + 独立根目录核验脚本）。
- ADR-0017 + 分歧台账（9 条）+ **数据流契约**（见任务 2）先行。
- 每批完整 diff 的独立 subagent 审查及修复后复核；顶层任务汇总组件、接线与原有验收证据（AGENTS.md 大 A 语义门禁）。

### Must NOT have (guardrails, anti-slop, scope boundaries)
- **不得实现独立的串行参考引擎或双代码路径**（用户明确免除；预算=1 的并行运行即对照基准）。
- engine 内不得引入 channel/actor/线程所有权框架；跨实体不得加锁或阻塞发送。
- 不得做旧档迁移器；不得删除用户旧存档文件。
- 不得承诺或"验收"绝对无死锁、跑满核心、新旧输出整体字节一致、具体提速幅度——禁止出现在文档与提交信息中；性能只报实测。
- 不得静默增删自成交（STP）策略、T+1、集合竞价决胜、涨跌停；**除分歧 #9 批准的卖单实收封顶与卖单零现金预留外，不得改变 nominal 费用公式（佣金/印花税/过户费的应计函数、税率、最低佣金累计口径）与买方费用语义**。
- 不得用 grep/日志冒充测试运行证据；不得伪造性能数字。
- worker 不得分配全局 OrderId/seq；FIFO 不得取决于线程调度、锁竞争、hash 迭代序或墙钟。
- 不得宣称跨预算对等即"语义正确性证明"；分歧 #6 的 seq 差异必须用结构化比较器核验（含旧侧派生键），不得用通配忽略。
- 市场 tick 的业务权威状态只能由 `commit_tick` 提交（含 P0 过期与 P2 策略执行——一律 shadow）；类型化失败仅允许写入既定 poison/错误元数据，并验证业务哈希不变。自然日日结按已批准 CivilUpdate 契约走独立 `end_civil_day` 边界，不伪造市场 tick 或移入撮合提交。下文同类“单点提交”限制均指此市场 tick 边界，不豁免自然日更新自身的校验与错误交付。
- 基线采集只允许**新增**采集 harness（不改既有引擎源码）；业务重构不得早于语料封存。
- 策略决策语义不得因 `StrategyState` 序列化而改变（往返必须逐字段等价）。

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test runtime decision（2026-09-23 用户最新明确覆盖）：普通自动化测试单命令/单 case 均 ≤10s，超过即缩小 fixture、拆分或去掉重复准备，不得改名为长测试规避；Node 以 `--test-timeout=10000` + 整命令进程树 deadline 做技术门禁。Web 普通测试不得经过触发 Node 25 动态导入错误的 Corepack/pnpm 启动链；`scripts/run-web-tests.mjs` 直接用当前满足 `>=24.18.0` 的 Node 发现全部测试，按 CPU 预算最多并发 8 个真实进程 shard，批次与每 child 均 ≤10s，失败终止 siblings，零测试失败。确有必要的长验收才可例外，单 child 和单阶段共享 wall-clock deadline 均 ≤300000ms。根完整回归严格拆为冷构建、测试执行两个进程外阶段，各自独立 ≤300000ms：构建阶段原子密封源码指纹及 binary 路径/大小/SHA-256；执行阶段先校验密封 inventory，再按最多 8 binaries、总 CPU 不超卖（128 CPU 时每 binary 12 harness + 4 Rayon）并发执行，失败中止 siblings，同一执行期限保留 doctests 与上述 Web 分片入口，结束后再查源码漂移。K7 将前 299000ms 用于执行、结果校验与 manifest 发布，最后 1000ms 只用于终止进程树、等待 close 与清理 staged 文件；runner 内部第二截止约束异步收尾，正式命令另由进程外 supervisor 约束总 wall，避免同一 Node 事件循环阻塞计时器。禁止恢复旧 2h/6h 超时。所有可并行测试必须实际使用多进程/多核；冷编译与测试执行分开计时。K7 `before` 为已密封历史工具且 CLI 禁止重跑；当前 K7 先在独立 ≤5min 构建阶段产出绑定源指纹与二进制哈希的 fixture，after/sensitivity 再各以 ≤5min 直接执行预构建产物。
- Test decision（2026-09-20 用户最新明确覆盖）：本计划后续批次采用“整模块先实现 → 同批集中补齐测试、审核、调试 → 全部适用门禁通过 → 本地 commit”。这是本计划对仓库默认 TDD/小步提交顺序的任务级例外，不修改项目通用规则，也不追溯篡改既有证据。开工先列行为契约和验收场景，编码阶段允许只做语法/格式及必要编译检查；最终必须完成原要求的行为、失败和跨层测试，不能以语法正确充当验收。框架 = cargo test（Rust）+ pnpm types:check（TS 契约）。
- Evidence: .omo/evidence/escrow-parallel-engine/task-<N>/（attemptDir = .omo/evidence/escrow-parallel-engine/）。QA 命令须核对实际符号和用例数，筛选命令运行 0 个测试不算通过；记录真实退出码，不能用 tail/grep 的成功退出覆盖上游失败。
- **保留语义测试按五类清单管理（任务 2 冻结 `b_test_inventory` 入 manifest；任务 3 交付 `preserved-test-inventory.md` + hunk 级校验器）**：(a) 源码与断言**逐字节不变**的测试（校验器断言 diff 为空；费用/T+1/竞价/笼子/整手零股/Money 舍入的主体，买侧与终态语义）——必须在线程预算=1 与 auto 两档通过；(b) 预留数值/费用实收断言受 #9 影响而修改的测试（**与冻结清单逐一精确比对（文件/符号/#9 效应 ID/允许变换），缺失/新增/改类/扩大输出失败码（ADDED/MISSING/RECLASSIFIED/EXPANDED）并使任务 3 不通过**）；(c) 仅因存档 v2 / step() Result / 事件形状而重写的测试（**allowlist：仅 API/schema/Result/事件形状构造；触及预留数值/可用现金计算/接受断言/决策链输出/场景输入的 hunk 强制归 (b)**）；(d) **性能 fixture 缩减（PF1）**：只允许 `setup`、预热 tick、完整 market-minute 历史输入的缩减；每项必须绑定前置 revision、前后整文件 SHA-256、全部 hunk SHA-256、未变的原业务 assertion hashes 和新增覆盖契约 item/assertion hashes。没有这些覆盖锚点，或删除/弱化原断言、触及撮合/费用/T+1/竞价/价格限制/接受拒绝/策略语义的改动，均不得分类为 PF1；(e) **校验器**：`preserved_test_baseline_sha`（规范时点 = 任务 2 第 0 步、干净工作树验证后、任何既有源码/测试修改前；校验器验证提交存在且为祖先）对当前工作树 `git diff <sha> -- packages/engine/tests`，拒绝未预期未跟踪替换；**每个被修改的既有测试符号/hunk 必须有类别，(b)/(c)/(d) 须带 effect ID 与理由**——**不得使用无范围"不经修改"的总称断言，也不得只做文件名级清单**。
- 确定性验证分层：**调度一致性**通过跨预算、重复运行和带收据的扰动对比，验证已覆盖执行条件下的结果一致性；**语义正确性风险**由四类语料（对等：旧预留==0、无欠费前缀、无反馈；#9 隔离定向：预留、接受翻转及实收差异；受控存活卖单：枚举的 #9 费用/预留/余额及 #7 表示差异；现实压力：新引擎确定性/守恒/安全）、保留测试清单和双账本检查共同提供受限验证证据与验收门禁。有限测试不证明所有输入及调度下的整体正确性，两类证据不得互相冒充。**"零人工干预"范围 = 验证命令的执行与判定；F1-F4 后的最终完成声明确认是独立的人工确认点**。压力语料不提供历史行为保真的证据，不得作超出测试覆盖范围的声明。
- 回滚判据：`business_state_hash`（仅排除 poison/错误元数据，失败 tick 前后必须相等）+ `session_state_hash`（仅允许 poison 字段差异）；字段枚举入 ADR。
- 每组件改动由未参与实现的 subagent 独立审查完整 diff（大 A 语义、必要性、边界漂移），发现修复后复核；只有汇总四组件复核、接线复核及原有验收证据后才能完成（docs/principles.md + AGENTS.md）。

## Execution strategy
> 2026-09-23 四组件收敛版：执行单位固定为 A 核心引擎、B 存档恢复、C 宿主交付、D 验证收尾，每组件一个实现 agent，采用整组件编码、集中补测/审核/调试、通过即本地提交的流水线。任务编号 1–12 与 F1–F4 保留作验收索引，checkbox 和历史证据不重置。运行时阶段契约同时按下节及 ADR-0017 修订。

### Runtime contract: one allocation, incremental execution, one commit

- **单次截点**：P0、P1 各一次，post-P0 预算公式保持。NPC/玩家决策及计划链决策观测读取固定 `DecisionSnapshot`；策略在 shadow 中执行，权威对象不变。
- **增量操作流**：先按既定 `npc → player → plan_chain` 类序处理就绪候选；仅有真实结果依赖的计划链交错执行“生成下一命令 → P3 校验/分配 → P4 应用 → 私有结果反馈 → 续执行”，直至本 tick 全部根计划及其合法续执行排空。不再要求全 tick 所有 P3 在任何 P4 前完成，也不再限制 P3/P4 函数只调用一次。真实根入口覆盖 `PlanChainOperationBatch` 的全账户、Lifecycle、QuotePlans、AccountExecution，而非只循环外部注入的单个 driver。确定性分轮即可，不引入动态调度框架、业务锁或阻塞发送。
- **预算与身份跨轮持续**：同一 tick 的账户剩余 cash/shares 预算、数量约束状态、OrderId 游标、sealed index、chain generation index 和收据 outbox 持续维护；资源预算只能按批准校验规则扣减，P4 成交/撤单/拒单释放不回补。订单数量约束依据其既有规则使用私有操作结果更新，不能把数量槽位与 cash/shares 预算混为一谈。每轮可对就绪候选使用“账户校验草案 → 按 canonical 总序 checked 分配 ID”两遍法；P3 拒绝/Cancel 不耗 OrderId，P4 拒绝仍耗已分配 ID。sealed/chain 索引按已固定的根账户/计划遍历顺序连续生成，跨轮不重置、不回插早先序号，不依赖线程完成顺序。
- **反馈可见性**：continuation 按显式命令/密封身份接收 typed P3/P4 outcome（不得从展示事件顺序猜结果，即时全填可能没有 OrderAccepted）和最小私有执行事实投影（订单接受/拒绝/撤销/成交、活跃子单、剩余量、parent/PlanBook 对应状态），以保持撤旧→下新、两撤→下新等现有自适应行为。决策市场/账户观测仍是固定快照，不用可变 GameSession 重新算现金、可卖量或触发新一轮策略。复用既有事件/收据事实及投影逻辑，明确每种事实的唯一消费位置；P7 最终协调不能重复消费已反馈事实，不能复制一套结算/计划状态机。接口定稿必须逐一核查 continuation 的实际读写集合；遇到依赖 P6 余额或无法由既有事实投影满足的分支，记录具体场景并阻止受影响验收，不擅自扩大同 tick 资源可见性，无关模块继续。
- **每股连续 shadow**：股票状态从 post-P0 初始化一次，后续轮次接续同一状态；同股票按总密封序串行，已知无依赖的不同股票可并行。根计划及续执行流排空后，按原边界执行适用的竞价清算、rollover、日终终结和 PriceTick 等收尾各一次，不能每轮重放 finalizer。计划链只能消费已实际产生的结果，不能提前消费尚未进行的竞价最终成交。
- **统一结算与提交**：操作及收尾结束后 P5 一次规范合并全部 P0/P4 收据并校验，P6 一次按原 Buy-before-Sell 生命周期结算，P7 一次完成最终派生/事实对账及事件规范化，P8 一次检查，P9 一次提交。经验按既定每订单归并边界更新；不得按执行轮重复结算、重复费用或重复投影。分歧 #4 的撤单限制不因 continuation 扩大。
- **全 tick 原子性**：后轮 OrderId/密封序/chain 索引或收据索引溢出返回类型化失败。允许先前轮次已产生私有键控 envelope、簿变化和 outbox，但失败时整个 tick 丢弃；权威 cash/持仓/簿/计划/队列/RNG/游标/策略保持，只有既定 poison/错误元数据可变，零对外事件。初轮失败和后轮失败分别测试，不再承诺一切溢出必在首次 P4 前发生。
- **必需运行时场景**：真实会话遍历多账户、多根计划；撤旧→下新、两撤→下新；第一/第二撤失败不错误生成后继；新单接受并即时全填/部分填后 parent/PlanBook 恰一次更新；P0 释放可用而同 tick 其他释放不回补；跨股票预算竞争；后轮身份溢出/失败完整回滚；竞价/日界收尾恰一次；预算 1/2/4/auto 与扰动下操作、收据、最终状态一致。不得用只注入单个 driver 的测试替代真实根计划调度。

### Four-component execution organization

最终实施只保留以下四个组件。每个组件恰好由一个实现 agent 连续负责其完整范围；不得在组件内部再按 P 阶段、竞价/连续交易、宿主种类、测试或文档拆分实现席位，也不得派生孙 agent 实施组件内子模块。独立 reviewer 与验证执行者不属于实现 agent，且不得替实现者编写业务改动。

| 组件 | 唯一实现范围 | 原任务映射 | 最终验收与交付边界 |
| --- | --- | --- | --- |
| A 核心引擎 | 连续交易、竞价、日界与自适应计划链合一；P2 三来源、持续 P3 预算/ID、增量 P4、私有 continuation 投影、跨源收据、P5–P9 闭环与失败回滚 | Task 3–7 | 一个实现 agent 对完整 runtime diff 负责；真实多账户、多根计划、成交/撤单/跨 tick、竞价/日界一次性与失败原子性共同验收，不能将连续交易或竞价拆作独立实现批次 |
| B 存档恢复 | schema v2、全状态编解码、订单/账本/游标/策略/seen 校验、非空状态恢复续跑 | Task 8 | 一个实现 agent 完成核心引擎非空状态往返、原 save_contract 与 Task 3/6 的存档子场景，不依赖正式启用 |
| C 宿主交付 | web-wasm/server/desktop 错误映射、TS 契约、完整 TickFrame/TickBatch 与 CivilUpdate 接缝 | Task 7 的三宿主交付部分 | 一个实现 agent 同时负责三宿主，不按宿主拆人；以真实引擎接入、失败映射和跨宿主契约一致性验收 |
| D 验证收尾 | 确定性/扰动/守恒/语料比较、性能 harness、诊断审计、K7 核验、文档/证据及终审收敛 | Task 9–12 + F1–F4 | 一个实现 agent 负责工具、文档和证据接线；最终数值只取自 A–C 的稳定集成源，Task 9 历史见证缺口如实保留，F1–F4 仍由独立终审给结论 |

依赖分三种：**开发**只依赖明确接口和文件归属；**候选集成**可使用已登记的固定上游修订并记录失效传播；**验收/提交**要求实际依赖闭包已审已测。A–D 可以在各自边界就绪后并行推进，但组件内部始终单一实现属主；跨组件接线由 main 串行组装，不能借接线名义增加组件内实现者。

任务索引的最终门禁：Task 1/2 历史成果复用；A 覆盖 Task 3–7 的完整运行时；B 回填 Task 3/6 的存档子验收；C 完成 Task 7 的三宿主门禁；D 的 Task 9 最终矩阵等待 A–C 全部验收，Task 10 诊断迁移依 Task 9，Task 11 正式 K7 依 Task 10 与核验脚本稳定指纹，Task 12 与 F1–F4 只写已验证结论。

### Component ownership and review roles

协调者只维护四个实现归属：A、B、C、D 各一个实现 agent。若某实现 agent 结束，后续返修仍续派给该组件属主；确需换人时先完整交接并明确取消旧属主，任何时刻不得有两个实现者并写同一组件。实现模型/强度按实际工具能力登记，不能以模型标签新增实现席位。

main 负责契约、共享入口、唯一候选组装、跨组件接线、验收与串行 commit；独立 reviewer 负责完整 diff 的大 A 语义、必要性与边界审查；验证执行者负责全量构建、完整回归、矩阵和资源记录，不写业务代码。`session.rs`、`pipeline/mod.rs`、共享类型/入口发生跨组件冲突时由 main 维护单写窗口，组件 agent 不互抢路径。既有产物只按内容映射进 A–D 并复用，不恢复历史内部拆分调度。

### One-pass batch workflow

每批沿以下状态推进，实施和测试同属一个交付，但不强制先写测试代码：

`契约/场景固定 → 整模块编码与候选组装 → 集中补测并冻结 → 批量 review ∥ 批量运行 → 按失败清单调试 → 最终复核/回归通过 → 立即本地 commit`

1. **一次交清楚任务**：给实现者完整模块目标、API/字段、独占路径、依赖指纹、明确不包含项、行为场景、模型/强度和结束条件。复用已审设计，不要求每个函数或 wrapper 先获批准。领域行为未定仅阻塞具体相关分支；先做一次有界契约核查，避免重复探索全仓库。
2. **一次写完整模块**：以先快速写齐完整复杂逻辑为默认工作方式，允许连续写实现、类型、适配、错误分支及必要接缝；不要强制“写一个函数→测试→审核→再写下一个”。编码阶段只要求语法/格式及必要编译反馈，语法通过不等于编译通过。尚缺上游符号明确登记，不能伪造绿灯。main 同时成组接线，不能要求 worker 每个小补丁停下等测试或 reviewer。触及受保护既有测试仍先通过其映射门禁，禁止删弱断言。
3. **集中补测和初审**：模块范围写齐后一次补齐场景测试，整理完整 diff。可先冻结代码修订供初审，同时在另一个可写副本补测试；最终必须审查包含这些测试及返修的最终完整 diff。按依赖前沿收集当前可运行的检查，独立检查可并行；编译阻止执行的断言记未运行，不用一个失败掩盖其他可收集结果。
4. **批量调试**：main 去重全部发现，先处理编译/接口，再处理原子性、守恒、身份和状态，再处理其他行为与格式。不同文件/根因并行返修；同一文件由唯一属主逐项修；不为每条机械修复重新创建一整套冷构建和审查。修复后只重跑受影响检查，最终完成该批所需整体回归。reviewer 只审不替作者修。
   **允许临时裁剪调试**：先写齐完整复杂模块，再在隔离调试副本中通过临时注释部分调用、局部开关、stub 或缩小输入逐段定位。保留完整候选基线，逐项登记临时禁用的文件/符号、原因、替代行为和恢复条件；不得把裁剪代码当作新设计悄悄保留。诊断期间可以跳过尚在定位的路径，但标明“诊断结果，非验收”，不能据此删除/弱化原断言或将未执行项记为通过。按依赖逐段恢复并调通后，清空所有临时绕过清单，核对完整入口/错误分支/真实依赖均已恢复，再冻结最终修订跑整批适用回归及独立完整 diff 审查；这一恢复门禁不可省略。永久保留的正式测试替身只用于其声明的 unit test 范围，不能替代真实集成验证；不得把临时调试开关带进产品流程。
5. **批次关闭即提交**：整批适用门禁、完整独立复核和依赖闭包全部通过，main 下一安全操作执行本地 commit，记录 SHA 和证据，释放 WIP；不等顶层任务或下个模块完成。局部模块可以独立验收提交，但其提交树必须可编译可验证，不能遗漏仍未提交的依赖；无法切出安全边界则合并必要已审依赖或明确“已验收待提交”。

组件按完整能力边界定范围，不以行数凑量，不拆成逐 wrapper 审查，也不把四组件混成一个无限增长交付。一个组件冻结后不得不断追加新功能；初审/验证期间可继续另一已就绪组件。存在未决计划链分支时，可以冻结当前修订供诊断审查、验证其他场景，但不能将 A 核心引擎标为完整验收通过或提前启用。

### Main agent operating loop and nonblocking dispatch

main 的工作是持续集成并减少队列等待。首次接续只读当前 Plan 指纹、已有交付和在途工作，列剩余模块，不重做已经完成的模块。维护既有 notepad 中的一张工作面板：批次/修订、文件属主、依赖、状态/WIP、模型/强度、测试和审核结果、作业 ID、最近实质产出、阻塞属主及下一动作；不另建调度系统。

每轮在安全编辑边界按“收取结果 → 非阻塞续派 → 亲自写接线 → 冻结已写齐批次并派审核/V1 → 接纳通过版本并立即提交”推进。main 同时只写一个集成批次，其余代码由模块属主写；main 不长期做日志转发，也不包揽全部测试/组件。已有完整模块到手即组装，不等待所有模块一起完成。

| 条件 | 工具和动作 | 是否阻塞等子任务结束 |
| --- | --- | --- |
| 多个模块已就绪且资源/WIP 允许 | 连续调用 `collaboration.spawn_agent` 创建有完整交付边界的任务；拿到 ID 后继续下一派发或集成 | 否；不紧接无条件 wait |
| 既有 agent 本轮已结束，有新模块/返修 | `collaboration.followup_task` 续派 | 否 |
| 运行中的 agent 需同步契约/修复要求 | `collaboration.send_message`，继续其他工作；消息不能唤醒已结束者 | 否 |
| 模块 A 等审核/编译，B 可开发 | 在另一个源码副本写 B；V1/reviewer 继续 A | 否 |
| 只有局部依赖阻塞 | 列准确 API/文件/资源原因，继续其他模块、候选接线或返修 | 否 |
| 所有安全可执行工作均受阻，存在能解除阻塞的在途作业 | 记录等待对象/修订、解除属主、下步动作后 `collaboration.wait_agent`；后台命令用返回的作业 ID 和对应等待工具 | 是；单次最多 60 秒 |
| 无可执行工作且无在途解除者 | 报告具体缺口或所需决策，不无期限空等 | 不轮询 |

等待前必须检查候选接线、固定接口下游编码、测试夹具、修复、提交、A–D 四组件就绪任务与验证请求。连续两次同因等待无进展要看实际作业输出和依赖，不能只 `list_agents`。一次定位加一次修复仍不解的具体问题可交独立分析者做有界诊断，再由原组件实现者一次修完整边界；诊断者回避该组件最终独立审核。worker/reviewer 交付后无已分配工作则结束本轮，main 续派。

**并行度要求**：接口/文件/资源允许时，A–D 可各由其唯一实现 agent 并行推进；不得为提高并行度拆开 A 的连续交易/竞价/日界、C 的三个宿主或 D 的工具/文档。若某组件没有活跃实现，应说明已完成、具体依赖或 WIP 约束；不以重复测试或新研究任务装饰并行度。审核与验证可独立并行，重型作业数量另受统一验证资源限制。

接续当前会话时先发送本版内容指纹与规则差异，取得各实现者文件归属、reviewer 快照、V1 缓存/作业回执；未确认者不算已采用新版。旧作业安全收尾，失效结果如实标识，不盲目停机重启。此计划修改本身不代表运行会话已经迁移。

### Source isolation, WIP and acceptance

- **源码和构建隔离**：worker 在工作区 `.tmp/` 内的独立源码副本或 `.worktree/` 内的 Git worktree 开发（目录规则见下节）；main 从稳定交接组装一个当前候选修订。可以有多个不同批次副本，禁止多个作者并写同一候选路径。副本包含必要的未提交/未跟踪依赖，不仅复制 HEAD。A 的审核/测试用不可变完整输入，B 的写入在另一副本；不能边改 A 边跑 A 的测试。无法隔离时登记单写/冻结窗口，其他不冲突工作继续。
- **修复和证据归属**：返修生成新修订，保留失败记录；接口变化通知消费者，失效范围明确标记。旧结果不冒充新版，相关门禁重新运行。每批有“场景→测试符号/断言→实际结果”映射；编译失败、0 测试、仅 legacy 路径成功均不能算新实现验收。
- **边界场景**：覆盖真实非空数据、非默认配置、跨股票同局部索引、P5/P6 重复收据类型化拒绝、后阶段失败整 tick 回滚与正确错误上下文。只有已批准的投影重放可以幂等，不能推广为账本/结算静默跳过重复收据。最终断言不可因编码先行而省略，历史 TDD 证据不重写。
- **限制积压**：每条实现线最多 2 个已开始但未集成验收批次，包含编码/待验证/返修/待集成；联合批次在每个参与线计一次，内部接线/返修不重复计数。满额后处理已有批次；不能改名或换 agent 绕过。批次审核与验证已通过但提交受阻单列，不宣称已提交。
- **审查独立性**：每批至少一名未参与该批设计/实现者通读完整 diff，回答大 A 语义及依据、必要性/最小范围、遗漏测试/跨层漂移/复杂度三项问题；有效发现修复后复核。另一 reviewer 可并行审别批或高风险部分，不拼凑无人负责的整批 APPROVE。原任务完整 Acceptance/QA 和 F1–F4 不因模块提交而免除。
- **正式启用**：候选可替换 legacy bridge 做真实全链验证，但不能同一操作走两遍。用户路径的正式切换须新权威状态持久化/恢复、全部运行时和宿主门禁以及最终相关验证齐备；B 存档恢复的实现/验收不依赖正式启用。无关组件可先提交；不新增长期双业务实现，不提前把无法恢复的新状态交用户。

### Dedicated verification seat and build scheduling
> 2026-09-20 用户批准新增 V1；总上限 12 包含主 agent、全部子孙 agent 和 V1。新增的是验证执行职责，不增加实现线或放宽两批 WIP。下述规则统一约束前后各任务的 QA/验收命令：原有覆盖要求保留，执行属主与资源排程按本节；旧定向命令的 target 限定按下述等价映射处理。

- **执行边界**：主协调、A–D 四组件实现者和审查者自行执行限定文件的格式/静态检查、选定 target 的检查及定向 unit/integration tests；全包/全 workspace 构建、完整回归、完整宿主构建和最终矩阵统一提交验证执行者。组件属主仍负责设计场景、修复失败和解释结果；验证执行者不写业务实现，也不以命令绿灯签署独立语义 APPROVE。F1–F4 各自结论仍由独立终审者给出，F3 真实 Web 操作不委托验证执行者代替。
- **局部不等于廉价**：Rust 单元测试通常仍需编译整个 crate 及依赖，`cargo check` 也不是纯语法检查。按实际冷编译、链接和缓存状态判断开销，不能因命令带过滤词就绕过资源限制。模块单测采用 `cargo test -p engine --lib <实际过滤词>`，独立集成测试采用 `--test <实际 target> <过滤词>`；旧 QA 中未带 target 的过滤命令先定位全部匹配用例及其 target，逐项转换为 `--lib`/`--test` 并记录与原覆盖的等价映射；包含 doctest 或其他 target 时保留对应执行，不能只挑其中一个 target 冒充完整覆盖。无法确认等价范围时交 V1 运行原命令。原定完整回归命令不缩减范围。执行后检查目标用例确实运行且数量符合预期；格式/静态检查不替代最终行为测试及真实检查/修复记录；历史 TDD 证据仍如实保留。
- **生成型验证**：冻结基线保持只读；`types:check` 等会先生成文件的命令在从该基线建立的可丢弃验证副本运行。记录运行前后源码、生成文件与 lockfile 差异，按原检查契约判断；需要纳入交付的生成差异交属主作为新修订修复并独立复核，不回写冻结基线，不把运行中改变后的输入冒充原指纹。输出日志和构建产物写入登记目录。
- **统一构建契约**：V1 与协调者一次登记工具链、manifest/lockfile、features、target triple、profile、RUSTFLAGS、Cargo jobs、格式配置及对应源码指纹；实现者与 reviewer 沿用相同基准，确需变更则记录原因与失效证据。edition/style edition 从对应 crate manifest 与仓库 formatter 配置解析，不能凭默认值或 reviewer 偏好在 2021/2024 间切换。限定文件格式检查传入解析后的有效配置；全仓库格式门禁由 V1 按项目命令执行。禁止仅为隐藏 warning 改 RUSTFLAGS 或创建新 target 重编；保留原始日志，展示时摘要告警，不吞退出码。
- **冻结交审协议**：按“属主完成模块实现、场景清单和格式化，记录测试编写/运行进度与已知失败 → main 固定基线及完整批次 diff → 创建不可改的完整输入快照（或登记等效冻结窗口） → reviewer/V1 确认同一快照与配置 → 开始检查”执行。输入清单覆盖所需源码（含未跟踪文件）、模块注册、依赖清单/锁文件、构建脚本、工具链和格式配置，不只哈希本批两个文件。优先使用物理副本，复制在输入稳定窗口进行，复制前后核对清单一致，否则重新冻结。环境无法隔离而使用已登记冻结窗口时，原输入同样具备完整清单/指纹，直到验证结束禁止任何人写入该输入；不能同时在该输入开发 B，其他不冲突工作可继续。reviewer/V1 只对给定路径和快照标识验证，不临时从共享树补文件。开始及结束检查输入清单，除“生成型验证”明确登记的输出外不得变化。共享树后续独立改动不否定真正冻结副本，但接纳/合入时须核对依赖，变化则形成新的集成修订。
- **快照或格式分歧处理**：哈希不符立即拒收该输入，暂停相关批次放行；不能边改冻结目录边补跑并把结果改名为“合并快照”。确需合并则新建修订、完整输入清单及受影响门禁。格式结果矛盾先比较 rustfmt 版本、manifest edition、style edition、配置路径、工作目录及精确命令；在统一配置的可写副本由文件属主格式化一次，再冻结。V1/reviewer 不格式化共享文件。纯格式差异可经 diff 证明后缩小复核/重跑范围，但不能宣称旧哈希测试在新哈希上重新执行；原要求全量验证的里程碑仍保留。
- **工作区目录强制约定（2026-09-21 用户纠正）**：统一工作区根为 `/data1/baiyifan/workplace/stock_market_game`，所有路径先解析到此根下的绝对路径，不以当前副本的 cwd 推导另一套嵌套根。新源码/冻结/生成型验证副本放 `.tmp/<batch>-<revision>/`，Git worktree 放 `.worktree/<batch>/`，固定构建缓存放 `.tmp/build-cache/<owner>-<config>/`，进程临时文件放 `.tmp/process-tmp/<job>/`，验证日志放既有 evidence 或登记的工作区日志目录。除下条登记的已启动作业安全收尾外，禁止启动或续派向系统 `/tmp` 写入项目副本、构建缓存及验证临时输出的新作业，不能改用 `/var/tmp` 等目录绕过。每个构建/测试进程及其子进程显式继承工作区绝对路径的 `CARGO_TARGET_DIR`、`TMPDIR`、`TMP`、`TEMP`；工具另有临时目录参数时同步设置。启动前创建目录、核对 realpath 与可写性，符号链接不能把输出导回 `/tmp`；不得修改 HOME/CODEX_HOME 或全局配置来实现此规则。
- **旧目录接续**：仅规则切换时已启动并登记的在途作业可安全收尾，不续派旧路径作业，不在运行中搬动或删除其输入/缓存。下一次写入/验证前，由属主把仍需使用的旧 `/tmp` 候选迁入工作区、比对完整输入清单并更新交接路径；历史只读证据可继续引用原路径，路径迁移不冒充新版本测试通过。旧工作区外缓存也在无作业占用后迁入登记目录，兼容缓存尽量保留增量产物，不默认删除重建。复制副本时排除工作区 `.tmp/`、`.worktree/` 这些嵌套副本根以及已登记 target、`.review-targets` 和缓存目录，防止递归复制；确需的未提交/未跟踪源码和配置必须显式纳入清单，不随缓存排除。
- **并发验收与资源排程**：A–D 的独立冻结修订可以同时审核和验证，无真实依赖的验收不等待 A 或其他组件完成。统一验证执行者管理全量作业资源，但不是所有组件串行验收的锁；可启动多个后台作业并收取结果。首次无资源观测时以最多 2 个重型作业、其中最多 1 个全量构建为起点；必须依据工作区可用空间、CPU、内存和实际峰值调整并发及各作业 Cargo jobs。轻量检查/独立审核不因重型队列停工；性能测量按其要求独占以避免干扰。实际依赖、同一可写缓存、源码冻结窗口冲突才限制对应作业。
- **故障影响范围**：用户明确工作区不受此前 `/tmp` 配额问题限制，不把历史 EDQUOT 或缺少 quota 工具当作工作区验证的前置阻塞。旧 `/tmp` 作业失败只阻塞相关作业：迁移其输入及全部临时输出到工作区，完成路径/空间检查后即可重试，不等待查询旧 `/tmp` 配额；其他工作区作业继续。若工作区实际出现 ENOSPC/EDQUOT/OOM 等新故障，记录具体路径、设备、错误及受影响作业，由 V1 限流对应资源并定位；只有证据显示共享资源受影响才扩大暂停范围，不据假设全局停验收。
- **缓存复用与回收**：V1 登记固定 target 的绝对路径、属主、工具链/配置指纹和占用作业。相同配置的连续修订顺序复用缓存；并发副本使用不同可写 target，reviewer 独立复跑有自己的缓存。批次或 agent 结束不是清理理由，禁止默认“新建 target → 验证 → 删除”。确需回收时仅处理已确认无人使用、归属明确的过期构建产物，保留冻结源码/diff/日志/哈希；进程不可见不能证明无人使用，不能删除其他人的未交接产物。必要的增量/调试信息配置调整由 V1 登记，正式验收和性能 profile 不得悄悄降级。
- **异步申请与公平排队**：复用 evidence/notepad 和原生消息，不新建调度服务。请求包含批次/修订、冻结输入指纹、命令及期望用例、工具链配置、目录归属、是否需独立复跑、优先级与解除的依赖；提交请求后即可继续独立工作或交接结束，不为等 V1 回执阻塞；main 负责追踪未确认请求，收到 V1 作业/队列标识后更新摘要，未获资源放行不得自行启动重型作业。关键路径返修和接线验收优先，随后已就绪组件与其余门禁；不能无限插队饿死既有请求。等价且同指纹/配置/范围的普通请求合并；要求 reviewer 亲自复跑的独立验证不能由实现方日志替代。V1 为这些局部复跑分配资源，reviewer 自己执行；独立完整复跑由未参与实现的 V1 执行并供 reviewer 核对。
- **编码与验收分层**：组件先集中实现，再于同批补测和调试，整批独立复核；不要求每个组件先单独完成审核或全量回归才允许同批候选接线；有明确全量验收要求的批次/顶层任务仍等 V1 结果。V1 在相关稳定集成快照和原定里程碑执行完整门禁，不能用局部绿灯替代任务 2、任务 9、最终 K7 等原有完整要求。构建成功不代表测试通过；证据记录真实退出码、测试数量、源码/配置指纹与失效范围。失败交回对应属主返修，新修订重跑受影响门禁，旧版成功不得归给新版。
- **V1 生命周期**：协调者用非阻塞创建/续派分配 V1，重型作业资源只由一个当前 V1 管理，转交须移交在途作业和队列。无排队请求且无在途作业时结束本轮，有新请求由协调者续派；存在后台构建时继续收取结果并处理可放行请求，不将监护责任留空；不得让空闲 V1 长期 wait。所有席位遵循前述最多 60 秒阻塞及交付后重排规则；局部验证排队不阻塞其他轻量任务。运行中的长构建可后台继续，60 秒限制约束单次工具等待，不要求每 60 秒终止构建。


## Todos
> 编号是最终验收索引，不是串行开工队列。实现与测试属于同一交付，允许先完整编码再集中补测/调试；所有批次还须满足 Runtime contract 的跨轮场景及临时调试绕过恢复门禁。
<!-- 顶层任务编号与完成状态保持稳定；调度修订同步更新本节与 Execution strategy，历史执行证据不覆写。 -->
- [x] 1. ADR-0017：escrow 并行 tick 模型、数据流契约与分歧台账
  What to do / Must NOT do: 新建 `docs/decisions/0017-escrow-parallel-tick.md`（**Status: accepted，引用 #9 用户决策记录（见下方）**），内容 = 任务 2 中"数据流契约"全文 + 毒化分类 + 双哈希定义与字段枚举 + 事件变体表（每变体 → phase_rank/entity_tag/EventSourceIndex 来源/local_event_index 派生）+ 单一并行实现（无串行参考）+ WASM 保留 rayon。**分歧台账** 9 条：(1) 决策观测改读固定 `DecisionSnapshot`，计划链另消费 typed 结果和最小私有执行事实续跑（类序 npc → player → plan_chain 保留，资源可见性仍遵守分歧 #2）；(2) 分配截点 `seal_allocation_snapshot`：P0 过期释放（shadow 内、截点前，经 post-P0 预算公式唯一入账）本 tick 决策可用；密封操作（成交/撤单/拒单/竞价完成/日界）产生的释放次一密封才可分配；提交后快照立即反映释放；(3) worker 内拒单消耗预分配 ID；(4) 取消跨 envelope 同 tick 撤单；(5) SettlementError→下单时 IntentRejected 迁移；(6) 全局 seq 用于覆盖/去重/断线检测/重连与稳定重放，不表示业务因果；结构化比较器在每个 tick 内比较稳定事件身份、变体和业务载荷的多重集（保留事件计数，逐字段应用已批准分歧），基线侧保留 `comparison_event_key`；帧内数组重排不构成新分歧，同实体 FIFO、订单/收据身份及跨 tick 顺序仍须保持，seq 满足 TickBatch/CivilUpdate 全更新流覆盖契约；(7) 存档 v2 拒旧档；(8) 笼子校验移入股票处理阶段；**(9) 卖单费用实收封顶于卖出所得（用户 2026-09-17 会话明示裁定，决策记录见 `.omo/drafts/escrow-parallel-engine.md`）：每腿实收总费用 = `min(F_after − charged_before, 本腿成交额)`（F = 累计应计费用，逐字现有费用函数累计口径；charged = 累计已实收）；**分项拆分规则（确定性，不留执行者选择）**：每单持久化各分项（佣金/印花税/过户费）累计应计与累计实收；本腿先算三个分项应计增量，再按**固定优先级 佣金 → 印花税 → 过户费**在总封顶额度内逐项扣取（每项实收增量 = min(该项累计应计 − 该项累计实收, 剩余总额度)），保证三分项增量非负、总和恰等于总实收增量、各项不超其应计余额（Money 整数分位运算，无舍入歧义）；收据同时携带分项应计增量与分项实收增量；SettlementTotals 只消费实收增量。**卖单现金预留恒为 0**；**与旧引擎的差异集（按旧实现如实登记，session.rs:1142-1168/2717-2748）**：(a) 卖单 reserved_cash 数值（旧 = max(一股缺口, 全剩余缺口) 可为正——含低价大数量卖单如 1 元限价×100 股约 4 元；新 = 0）；(b) **接受结果：旧预留 > 账户可用现金时旧拒新收**（反向不存在——新引擎卖单无现金要求）；(c) 费用实收与净交付数值及关联事件/诊断字段——**触发条件 = 任一成交前缀发生 charged < F（欠费，含后续腿追收）或任一观察时点旧预留 ≠ 0**；**零现金接受相等断言仅适用于机械验证 `旧预留 == 0` 的卖单**；正常交易（无欠费前缀且旧预留为 0）下费用实收与预留逐笔一致。nominal 费用函数、税率、最低佣金累计口径与买方费用语义不变。同步更新 `docs/trading-rules.md`（占用措辞改为托管 envelope、截点释放语义；标注托管为游戏内简化、非交易所真实清算）与 `docs/open-questions.md`。Must NOT: 不得把托管模型写成真实交易所清算机制；**除分歧 #9 批准的实收封顶与卖单零现金预留外，不得改 nominal 费用函数/税率/最低佣金累计口径/竞价规则条目本身**。
**#9 决策记录（替代批准门禁）**：卖单费用封顶语义由用户于 2026-09-17 在规划会话中明示裁定（"印花税按比例扣……手续费固定的 5 块钱，要是扣完不够了，就扣完扣到 0 就好"），本计划以此为准；无实施中人工批准门禁；ADR-0017 直接以 `Status: accepted` 落地并引用本决策记录。

  Parallelization: 契约文档维护；既有证据保留，本版运行时修订须独立复核，不重采历史基线；详见 Four-component execution organization。
  References (executor has NO interview context - be exhaustive): docs/decisions/0000-template.md; docs/decisions/0008/0009/0010/0014; docs/trading-rules.md:1-54; docs/open-questions.md:98-105; docs/principles.md:73-82; .omo/drafts/escrow-parallel-engine.md
  Acceptance criteria (agent-executable): `ls docs/decisions/0017-escrow-parallel-tick.md` 存在且含 "Status: accepted"（引用 #9 用户决策记录）；`grep -c "^|" docs/decisions/0017*.md` 分歧台账 ≥9 行；`grep -c "seal_allocation_snapshot" docs/decisions/0017*.md` ≥1；`grep -c "business_state_hash" docs/decisions/0017*.md` ≥1；`grep -c "事件变体表" docs/decisions/0017*.md` ≥1；`grep -c "封顶" docs/decisions/0017*.md` ≥1；trading-rules.md 含 "托管" 且含 "简化"。
  QA scenarios (name the exact tool + invocation): happy — `grep -n "Status: accepted" docs/decisions/0017-escrow-parallel-tick.md` 且 `grep -n "决策记录" docs/decisions/0017-escrow-parallel-tick.md`（引用 #9 用户裁定）；failure — `! grep -E "真实清算|交易所清算机制" docs/trading-rules.md | grep -v 简化`。Evidence .omo/evidence/escrow-parallel-engine/task-1/adr-check.txt
  Commit: Y | docs(engine): ADR-0017 escrow 并行 tick 模型与分歧台账

- [x] 2. 阶段化 tick 管线（全 shadow + 双账本 + 单点提交 + 内建并行）、step() Result 化与重构前基线采集
  What to do / Must NOT do: **范围收窄（任务 2 = 骨架 + 基线，全部绿灯）**：本任务历史交付管线类型定义、阶段接口、shadow/commit 脚手架、step() Result 化与重构前基线；下列数据流已按本版修订，新增增量执行契约不因任务 2 已勾选而视为实现完成；**本任务提交时所有签入测试必须全绿**——任务 3-6 的语义测试由各属主任务按 One-pass batch workflow 的模块批次顺序实现和验证（红灯实现局限于各批隔离副本或受控写窗口，失败证据保留，不向共享已验收基线提交失败测试）。**第 0 步（任何既有引擎源码改动前）**：**首先记录 `preserved_test_baseline_sha` = 验证工作树干净且已提交后的当前源码提交 SHA（规范时点：任务 2 第 0 步、任何既有引擎源码或测试被修改之前；写入 manifest，供任务 3 的 hunk 级保留测试校验器使用）**；**并生成 `b_test_inventory`（(b) 类清单，冻结入 任务 2 指定的 baseline-corpus/manifest.json）**：**候选集 = 自基线提交机械枚举的并集**：(i) 全部断言或读取 `reserved_cash`、卖方费用预留、或由此派生的可用现金的既有测试符号；(ii) 全部实现 #9 场景族（**定义：任一成交前缀发生 charged < F（欠费/追收），或任一观察时点旧 sell_order_fee_reservation ≠ 0（含低价大数量卖单与其接受翻转：旧预留 > 可用现金 → 旧拒新收）**）的既有测试；(iii) 接受/拒单结果断言与决策链输出断言亦列为候选；(iv) 明确锚点候选 company_scenarios/constraints.rs:90-94——**经核对为排除项**（该测试把 `fee_reserve = 500` 作为请求输入写入计划软预算分配（allocation.rs:76-80）并断言 `allocated_cash`，非订单 `reserved_cash` 断言；其场景真实卖单限价 11 元、旧预留本已为 0，无 #9 差异——候选报告中记录此排除理由）；**强制锚点 = 机械枚举 (i) 中真实直接断言订单卖方现金预留的测试（若枚举结果为空则 b_test_inventory 不含此锚，任务 3 验收相应核验为空）**；每条目含文件、测试符号/断言标识、基线字节/哈希锚点、#9 效应 ID 与允许的断言变换；**冻结完整候选报告（每条附纳入/排除理由）先于任何重构**；新增采集 harness（只新增文件，不改既有引擎源码）：`packages/engine/examples/escrow_baseline_corpus.rs`（输出逐场景 JSONL 投影：余额、持仓、reserved_cash/reserved_sell_qty、带 ID 与到达序的未完成单、订单簿终态、成交流、费用、竞价结果、T+1、拒单原因、决策链状态、计划状态、提交后快照可见性、存档→恢复续跑、**每事件双侧可派生 `comparison_event_key` = (tick_index, variant_phase_tag, entity_tag, occurrence_ordinal_within_scope)**——**推导规则固定**：ordinal = 同 (tick, 变体, entity) 在公开事件流中的零基出现序；实体规则：股票侧事件（Trade、AuctionTick、AuctionCompleted、PriceTick）→ Stock(code)，账户侧事件（OrderAccepted、OrderCanceled、IntentRejected、SettlementError）→ Account(id)，会话级事件（DayBoundary、CivilDateAdvanced、CompanyDisclosurePublished、ResourceLimit）→ Session；Trade 的 maker/taker 为载荷而非实体；其中 CivilDateAdvanced、CompanyDisclosurePublished、ResourceLimit 共用阶段 6 的 Session 发出流序号域，不按变体各自计数；本句已同步至已接受的 ADR-0017 与最终 TickFrame 事实多重集协议，取代早期将 OrderAccepted、OrderCanceled 归为 Stock 的过时分类，不另设旧版身份域或转换层；**完整逐变体表为任务 1 ADR 交付物**，以 Rust 穷举 match 落地——编译器强制覆盖 Event 全部变体，杜绝遗漏；不依赖 sealed_index 等新概念）+ `scripts/simulation/collect-baseline-corpus.mjs`（固定 seed 矩阵 ≥10 seed × ≥2 场景，含竞价/日界 tick）；**封存前先跑比较键可行性测试**（每个旧事件都能唯一派生键 + 键碰撞/载荷互换负测试；完整事件记录仅在帧内重排应通过；删除、固定身份下交换载荷或未映射的载荷篡改必须失败；同实体 FIFO/订单身份/收据因果或跨 tick 顺序发生未批准变化仍必须阻断，不得用多重集比较掩盖）。语料清单冻结**每条分歧 ≥1 个定向触发场景 + 对照场景**，每分歧附字段/路径级允许变换表；**语料分四类并在 manifest 中逐场景标注 + 附逐语义表面构造矩阵**：(i) **对等语料**——**逐表面构造规则（manifest 必含，不得留给执行者）**：买侧表面（费用/T+1/笼子/连续竞价买腿）天然与旧引擎一致；**卖侧表面须同时满足三个机械可验条件**：①下单时旧 `sell_order_fee_reservation == 0`（机械断言；不满足的卖单——含低价大数量如 1 元限价×100 股（旧预留约 4 元）——不得入对等语料，接受差异归 #9 隔离场景：旧拒新收）；②每个实际成交前缀均无欠费（`charged_prefix == F(prefix)`，机械断言）——否则后续腿追收导致逐腿费用与旧不同；③无策略反馈回路；满足时费用实收、预留（均为 0）、净交付、接受行为与旧引擎逐笔一致（含零现金接受相等断言）；**显式不可全字段对等清单：竞价 rollover 存活卖单、跨 tick 部分成交在册卖单、含未完成卖单的存档恢复（若触及 #9 条件①②）——改由第 (iv) 类受控存活卖单对比语料承担，其语义断言主体仍由保留语义测试覆盖**；观察到任何 #9 接受边界或决策输出差异即失败；(ii) **#9 隔离定向场景**——单一卖单、无其他后续决策循环、有限枚举差异集：**预留数值（旧差额预留 vs 新恒 0）；接受结果（旧预留 > 可用现金时旧拒新收）；费用实收与净交付数值（旧累计全额口径 vs 新封顶+追收口径）及关联事件/诊断字段**；**封存前须含旧引擎投影、新引擎预期结果与精确差异表**（含 1 元限价×100 股旧预留约 4 元的接受翻转场景与三腿欠费追收场景）；(iii) **现实长跑压力语料**——仅校验新引擎确定性/守恒/安全性，不做新旧语义对等比较（**诚实边界：不证历史保真**）；(iv) **受控存活卖单对比语料**——构造：现金充裕；**反馈闭环的可操作化规则：恢复前后均不使用任何策略生成、计划链生成或状态依赖的意图——续跑输入 = 双侧完全相同的密封外生意图脚本；RNG 不消耗或显式比较 RNG 游标**（存档恢复本身是反馈通道：有状态策略、旧档 profile-only vs 新档全量 StrategyState、恢复顺序都可能引入差异，必须以此规则关闭）；**旧引擎投影在重构前捕获**；覆盖三个表面：竞价 rollover 存活卖单、跨 tick 部分成交在册卖单、含未完成卖单的存档→恢复→续跑；**比较时点：存档前立即、恢复后立即、每个续跑 tick 后**；**允许差异 = 该类各自枚举的 #9 字段——与任务 9 完全相同的穷举清单与机械等式**：预留数值、费用实收、净交付、费用影响下的事件/诊断载荷字段；**终结后余额机械规则：仅当 `最终 charged == F_final` 时要求相等，否则允许精确差额 `new_terminal_balance − old_terminal_balance == F_final − charged_final ≥ 0`（仅现金余额）**；**持仓成本链字段（对照实际实现 account.rs:352-357/364-373）：`invested_cents` 与 `recovered_cents` 均必须相等**（recovered 按成交总额（gross）累计、与费用无关；清仓时双侧均按原规则删除持仓）；另加 #7 存档表示差异；**必须相等**：成交结果（数量/价格/订单身份与 FIFO）、事件变体与载荷（modulo #6 seq 与 #9 费用字段）、续跑行为、**恢复顺序/待处理意图次序/RNG 游标/策略状态/决策链状态**（#7+#9 之外任何差异即失败；未触及 #9 条件的正常卖单其费用实收与预留亦必须相等）；**卖方费用语义矩阵行（必填）**：满足对等三条件的正常卖单在比较 tick 内被接受并全部成交、提交后比较（卖单现金分量恒 0）——旧/新在 gross/佣金/印花税/过户费实收（封顶不触发且无欠费，逐笔一致）/净交付现金/持仓变化/成交与事件载荷/终结状态上完全相等，且 ≥1 个多腿成交同 tick 终结场景（覆盖累计最低佣金口径一致性）；另含 ≥2 个 #9 对照（1 元限价×100 股接受翻转：旧拒新收；三腿欠费追收）归 #9 隔离差异表，不参与相等断言；分歧 #6 用结构化 seq 比较器（含双侧派生键）；**对等语料中级联 #9 差异一律判未映射失败（比较器不得实现或臆造因果溯源）**。性能基线：同机同工具链同 seed 同预算整跑 ticks/sec 与 RSS（不做引擎内插桩）。落盘 `.omo/evidence/escrow-parallel-engine/baseline-corpus/`。**语料封存之前不得开始业务重构。**然后实现**数据流契约（决策完整，执行者只翻译不选择）**：
  (i) 权威状态（仅 `commit_tick` 单函数可变）：accounts（cash/positions/T+1）、markets/订单簿/竞价队列、plans、envelope 账本、RNG 游标、next_order_id/seq/next_receipt_base、诊断与完整 `StrategyState`；策略持久状态仅以 `StrategyState` 为权威，profile 为派生表示，策略执行对象按 (vi) 转换，不另立并列权威。
  (ii) 阶段与可变域（**全部 shadow，P9 前权威 `self` 不变**）：P0 过期（shadow 簿移除**报价过期**挂单 + shadow envelope 释放，发出**封前账本**收据；**日界清簿属密封批**（P4 终结收据））→ P1 `seal_allocation_snapshot` + **预算唯一公式（post-P0 形式，无独立释放项，杜绝双重入账）**：`P1_available_cash = tick_start_cash − post_P0_live_buy_cash`（**卖单现金占用恒为 0**，分歧 #9 封顶模型下卖单无现金预留）、`P1_available_sell_qty(account,stock) = tick_start_sellable − post_P0_live_sell_qty`（live 一律取 **P0 之后的存量**；P0 释放的效果已体现为存量减少）→ 不可变 `DecisionSnapshot` → P2 决策/计划链驱动（策略由封闭 `StrategyState` 重 hydrated 到影子竞技场，P9 换回权威；固定 `DecisionSnapshot` 供决策观测，typed 命令结果和最小私有执行事实供 continuation，不允许重新读取可用现金快照）→ **增量 P3/P4 操作流**（详见 Runtime contract：普通候选按 npc→player→plan_chain 类序；真实全账户根计划及依赖续执行按既定 canonical 遍历生成下一命令，按显式命令/密封身份关联结果。P3 对当前就绪候选先以持续剩余预算产未键控 `EnvelopeDraft`/掩码，再按总序 checked 分配 ID；全 tick 的预算/数量约束/订单 ID/密封序/chain 索引不重置，释放不回补资源预算。P4 每股票从 post-P0 初始化一次并连续应用增量操作，股票内有序、独立股票可并行；反馈更新私有执行投影，保持同 tick 自适应计划链。全部操作流排空后执行适用竞价、rollover、日终及价格收尾各一次。**后轮溢出允许先前私有键控 envelope/簿/outbox 已变化，但整个 tick 丢弃，权威状态和游标不变，无事件外发**；不再要求所有 P3 在首次 P4 前完成） → P5 收据聚合（**统一局部键** `ReceiptLocalKey = (journal_rank, ReceiptSource, envelope_key, transition_ordinal_within_source)`；**唯一序数映射（如 EventSourceIndex 一样显式定死，不得用派生枚举顺序或自选比较器）**：`journal_rank: PreSeal=0 < SealedBatch=1`；journal 内来源 tag 序（仅含该 journal 合法来源）：PreSeal → `P0Expiry=0`；SealedBatch → `SealedIntent=0 < Auction=1 < DayEnd=2`；tag 内 payload（u32/u64）升序；再按 `envelope_key` 字典序升序；再按 source-local ordinal 升序——全局 `receipt_index` 由此前缀和分配；`ReceiptSource = SealedIntent(u64) | P0Expiry(u32) | Auction(u32) | DayEnd(u32)`——密封意图用其 sealed_index；P0 过期按 (stock_code, order_id) 升序编址；竞价完成/日终终结影响的**前 tick 挂单**无 sealed_index，用各自每股票内按 envelope_key 升序的确定性序数；journal ∈ {PreSeal, SealedBatch}；envelope_key 区分同一撮合买卖双方；**`transition_ordinal_within_source` 定义域 = `(journal, ReceiptSource, envelope_key)`——每源实例内、每 envelope 各自编号**：密封下单源（SealedIntent）实例 = 该 sealed_index 的撮合操作，其中**每个参与 envelope**（incoming 与各 resting 对手方）独立获得自己的转移序：本 envelope 的成交腿按匹配序 0..k；**`k+1` 位 = 同源独立后续转移，分两类**：**非终结类——auction-rollover（同一 Auction 源实例内用 k+1，envelope 继续存活）**；**终结类——cancel/expiry/reject/day-end（用各自源实例的序号并终结订单）**；**序号命名空间严格源内局部**：同一源实例内成交腿 0..k、rollover k+1；**跨源转移在新源实例内从 0 重新起编（不用 k+2）**——如 Auction 部分成交（fills 0..k + rollover k+1）之后跨 tick 的日终终结属 DayEnd 源、其序号为 0；**envelope 收据链的连续性由 before/after 相等断言保证（跨源、跨 tick）：`auction_rollover.*_after == later_day_end.*_before`**；**全额成交不产生额外零数量收据：最后一条正数量 Fill 本身即终结（携带 live_after=0 与残余释放，spent/deliver 照常），无 k+1 收据**；竞价完成源（Auction）实例 = (stock, tick) 的一次竞价清算，参与 envelope 各自在 (源, envelope_key) 内编号（成交腿后接 rollover（非终结，存活））；日终源（DayEnd）与 P0 过期源同理——**按构造保证每 envelope 的时序，废除 match_leg_index 与 transition_kind_rank 两个字段**；**每 envelope 收据链校验**：每张 `*_before == 前一张 *_after`，违反 → InvariantViolation；规范合并后按 ReceiptLocalKey 前缀和分配全局 `receipt_index`（tick 内连续、跨 tick 由 next_receipt_base 延续，P0+P4 共用一个连续区间，溢出 checked fail）；按 (envelope_key, receipt_index) 唯一性 + 双账本方程校验；收据带 `journal` 类型标记防止错用方程）→ P6 账户结算（按 AccountId par_iter；**双输入流**：①账户余额/持仓只消费 side 聚合的 `SettlementTotals`——仅正数量 Fill 收据的**费用实收增量（分项按优先级拆分后）**/gross/数量增量聚合，键 = (account_id, stock_code, side)，股票按确定序、同账户同股票 Side::Buy 先于 Side::Sell 应用（镜像现行 session.rs:3446-3462 的既有生命周期次序，:3475 注释），保护持仓删除/重建路径与 invested/recovered 成本基础；**②零售经验消费规范排序后的正数量 Fill 明细流——精确归并规则（不留执行者选择）：先按 `(account_id, stock_code, side, order_id)` 聚合（数量/ gross/费用取该单收据之和），再按精确总键 `(account_id, stock_code, side_rank, order_id)` 排序、每订单恰好调用一次经验更新（镜像现行 session.rs:3469 每单边界），在 Buy-before-Sell 账户结算成功之后应用**；收据顺序仅用于链/审计校验，不作账户生命周期应用序；**收据路径唯一结算 API = apply_settlement；release/rollover/reject 类收据不产生交易结算调用**（零数量/零金额会被 apply_settlement 拒绝——它们只更新 envelope/预留状态）；apply_trade_batch 被排除——它会按批重算费用、破坏累计口径与最低佣金一次性）→ P7 最终派生与审计（核对 continuation 已消费事实，只应用尚未消费的计划事实，禁止重复计划同步；含诊断/状态投影和存档候选；整理 outbox，并按**事件变体表**总键生成稳定事件 seq）→ P8 双哈希检查（`business_state_hash` 与 `session_state_hash` 回滚判据；展示排序键的 phase_rank 不等于执行阶段编号）→ P9 `commit_tick`（唯一权威提交；失败 → 丢弃全部 shadow、置 poisoned、返回 StepFatal）。
  (iii) 总密封序键 = `(source_class, class_sub_key)`，source_class 序：**npc → player → plan_chain（保留现行实际次序）**；npc = (account_id, npc_local_index)；player = (player_queue_index)（全局 FIFO）；plan_chain = (chain_generation_index)。OrderId 只分配给 P3 接受的 Place；Cancel 与 P3 拒绝不占 ID；P4 被拒消耗其 ID。竞价 `arrival_seq` ≡ 订单 ID ≡ `Order.seq` ≡ 展示 ID ≡ 持久化序。 计划链候选可按 typed 前置结果增量追加；chain_generation_index/密封序在整 tick 全账户根计划域内连续且唯一，不按轮次或单 driver 重置，顺序由原确定性账户/计划遍历定义。
  (iii-bis) **逐 tick 帧与批量传输契约**：后端必须完整执行、结算并提交一个 tick，之后才生成 `TickFrame { tick, events, timeseries_payload, seq_from, seq_to }` 并开始下一个 tick。`timeseries_payload` 明确携带该 tick 的分时线、竞价和绘图数据；前端不得依赖事件数组顺序重建这些数据。高加速只允许传输层把多个已完成帧按 tick 严格递增装入 `TickBatch`，不得合并 tick 语义、跨 tick 重排或丢弃中间帧；批次可只在最后附一份完整 runtime snapshot，作为批次结束后的权威状态。前端可逐帧处理，也可一次吸收多帧后只渲染一次；这是前端内部性能策略，不改变协议。帧内 `events` 是该 tick 的事实集合，数组位置和展示 `seq` **不表示业务因果或执行先后**；`seq` 只用于覆盖范围、去重、断线检测与稳定重放游标。后端仍使用确定性键生成稳定字节输出：`(phase_rank, entity_tag, EventSourceIndex, local_event_index)`；消费者不得据此推断跨实体业务顺序。新旧语料比较以 tick 为单位，对事件按稳定身份与载荷做集合/多重集对比；除同一实体 FIFO、同一订单/收据身份字段及显式业务状态外，不要求旧数组相对顺序一致。当前前端中依赖事件顺序构建图表、提示、日志或自动下单的路径必须在后续实现中迁移为按 `TickFrame.timeseries_payload` 或事件类型聚合处理；远程覆盖继续要求 tick 与 seq 区间连续，但不把帧内数组位置解释为业务顺序。
  (iv) envelope 键 `(account_id, stock_code, order_id, side)`；**资源向量** `ResVec { cash: Money, shares: u32 }`；金额逐字复用 session.rs:1112-1169 helper；密封时对部分成交挂单按累计 filled_value 重算差额。
  (v) 收据模式：每 envelope 每 tick 可发多张：`{receipt_index, journal: PreSeal|SealedBatch, ReceiptLocalKey, envelope_key, kind: Fill|CancelRelease|ExpiryRelease|AuctionRollover|Reject, cum_filled_qty_before/after, cum_filled_value_before/after, remaining_qty_before/after, limit_price, spent: ResVec, released: ResVec, live_after: ResVec, commission_delta, stamp_tax_delta, transfer_fee_delta（**卖方为实收增量；另携 nominal_commission_delta/nominal_stamp_delta/nominal_transfer_delta 应计增量与 charged_before/after 累计审计字段**）, deliver_qty, deliver_cash}`；`deliver_*` 为净结算交付，不入守恒方程；P6 的 SettlementTotals **只消费实收增量**。
  (v-bis) **双账本转移方程（P4 计算 delta，P5 分资源校验）**：
  - 封前账本（每 envelope）：`tick_start_live == P0_released + P1_live`。
  - 密封批账本：P1 已存在者 `P1_live == ΣP4 spent + ΣP4 released + commit_live`；P3 新建者 `created == ΣP4 spent + ΣP4 released + commit_live`（含同 tick 新建且日界终结：走密封批 SealedBatch 日终收据）。
  - cash 与 shares 分别守恒。
  - **卖单现金腿（费用封顶模型，分歧 #9）**：每腿实收总费用 = `min(F_after − charged_before, gross_delta)`，其中 `F = F(cum)`（累计应计，逐字现有费用函数累计口径），`charged` = 该单累计已实收（归纳起点 0）；**分项拆分**：按固定优先级 佣金 → 印花税 → 过户费在总封顶额度内逐项扣取（每项实收增量 = min(该项累计应计 − 该项累计实收, 剩余总额度)，非负、总和恰等于总实收增量、不超各项应计余额）；`deliver_cash = gross_delta − 实收总费用 ≥ 0` **按构造恒成立**，`spent.cash = 0` **恒成立**；**卖单 envelope 的 cash 分量恒为 0**；**每张卖方 Fill 收据前后校验的不变量链**：`0 ≤ charged_before ≤ F_before ≤ F_after`；`charged_delta = min(checked_sub(F_after, charged_before), gross_delta)`；`charged_after = charged_before + charged_delta ≤ min(F_after, cum_after)`；`deliver_cash ≥ 0`——违反即 InvariantViolation。**欠费追收语义**：早期前缀封顶后（charged_before < F_before），后续大额腿在 F_after − charged_before 内追收历史欠费（即使 F_after < cum_after）——与旧引擎逐腿费用在此情况下必然不同（归 #9 差异集）。归納不变量：`charged_after ≤ cum_after` 恒成立。零成交点：cum=0 不产生结算收据。契约测试：封顶激活/不激活边界、**欠费追收（1元/1元/10元三腿）**、分项分配（所得不足且 ≥2 分项应计非零时按优先级拆分、各项不超余额、总和恰等）、跨多腿累计封顶、部分成交后撤单（终结 released.cash=0、charged 历史保留入审计与存档）、**旧 Momus 舍入跳变场景（commission_min=0、限价 0.01、cum 499.98、剩余 4）在新模型下净额恒 ≥ 0 且无需现金预留**、F 及各分项单调性边界测试（half-even 舍入上下邻点、最低佣金切换点、跨 tick/竞价累计）。
  - **买单现金腿（逐次释放）**：`spent.cash = gross_delta + commission_delta + transfer_delta`；`live_after.cash = buy_order_reservation(config, limit, remaining_qty_after, cum_filled_value_after)`（逐字 helper，买单成交价 ≤ 限价，helper 为健全上界）；`released.cash = max(0, live_before.cash − spent.cash − live_after.cash)`——**价格改善差额逐次释放**（与现行引擎 reserved_cash 随成交重算缩小的公开行为一致；不设"留存至终结"）。
  - **终结转移（全额成交/撤单/过期/拒单/日界）**：`live_after = ResVec::ZERO`；`released = live_before − spent`（checked）；卖单 `released.shares = 剩余股份`，买单 `released.shares = 0`；`remaining_qty_after` 仅作审计字段。
  - 竞价 rollover = 非终结转移，用上述公式（卖 `released.shares = 0`）。
  - **数值锚点（任务 3/5/9 必含，默认费率）**：①卖单三腿低额成交（腿 1/1/10 元；累计 1/2/12 元；逐腿应计：**F(1)=F(2)=5.00**（印花税 0.0005×1/2 不足半分舍 0）、**F(12)=5.01**（0.0005×12=0.6 分 half-even 进 1 分；过户费舍 0））：第一腿 charged=min(5.00−0,1)=1、deliver=0；第二腿 charged=min(5.00−1,1)=1、deliver=0（累计 charged=2）；**第三腿 charged=min(5.01−2,10)=3.01、deliver=6.99（欠费追收含印花税进位）**；全程 spent 恒 0；旧侧按其累计费用口径：净额 −4/+1/+9.99（差异表条目）。②买单限价 10 元买 200 股、100 股以 9 元成交：初始预留 2005.02、成交消耗 905.01、重算剩余预留 1000.01、**立即释放 100.00**。③对账：订单全部收据 `Σ deliver_cash − Σ spent.cash == 该订单账户现金净变化`（卖单 spent.cash 恒 0）；分项实收之和每腿恰等于总实收。
  (vi) 策略执行：封闭可序列化 **`StrategyState` 枚举**（每现有具体策略一变体，含 `InstitutionMomentumStrategy`）为**唯一权威**——profile 降级为派生字段（构造与存档时校验一致性，现行 profile 断言保留为派生校验）；`from_strategy/into_strategy` 经**封闭构造注册表**（判别 match → 具体类型，无 downcast）转换，**行为相关字段往返逐字段等价**（决策语义零变化）；测试专用策略仅存在于 cfg(test) 变体/适配器，生产会话在构造与存档处显式拒绝未知变体；P2 前全部策略经 `from_strategy` 进入影子竞技场，P9 `into_strategy` 换回。
  (vii) 毒化分类：`StepFatal::{InvariantViolation(描述+定位), Internal(状态哈希不等)}`——仅类型化失败与检出违规；**panic 不承诺恢复**。双哈希回滚判据同前。
  (viii) 存档合法态：市场 tick 中途禁止存档，commit_tick 成功返回后为合法静默点；初始、restore 后及完整 CivilUpdate 后的独立静默点按任务 8 逐场景对账，不擅自允许或禁止。
  `step(&mut self) -> Result<Vec<Event>, StepFatal>`；poisoned 后 step/save 显式 Err。native 与 wasm 同一代码路径。Must NOT: 不得另建串行参考或执行模式 cfg 分叉（`grep -rn "cfg(.*parallel" packages/engine/src` = 0）；不得用 catch_unwind；**不得改撮合公式与 nominal 应计费用公式/税率/最低佣金累计口径——实收口径仅按分歧 #9 的封顶与追收规则改造**；P0/P2 不得改权威状态；基线采集不得改既有引擎源码；语料未封存不得重构；ADR 未定稿不得重构（第 0 步除外）。
  Parallelization: 已封存基线/骨架复用；新增运行时行为统一由 A 核心引擎实现 agent 完成并验收，其他组件只消费固定契约；详见 Four-component execution organization。
  References: packages/engine/src/session.rs:1977, :85, :164-333, :2018-2089, :2175-2209, :2981-2986, :407-408, :3720-3728, :3913-3927; packages/engine/src/strategy/mod.rs:165-247; packages/engine/src/account.rs:87-101; packages/engine/src/compute.rs:24-86; packages/engine/src/session/decision_chain.rs:124-143; apps/web-wasm/Cargo.toml:25-26; package.json:18; packages/engine/examples/baseline_fixture.rs:57-79
  Acceptance criteria (agent-executable): `cargo test -p engine` **全绿（含本任务新增测试）**；`grep -n "fn step" packages/engine/src/session.rs` 为 Result 签名；`node scripts/check-wasm-threading.mjs` 通过；`ls .omo/evidence/escrow-parallel-engine/baseline-corpus/manifest.json` 含 9 分歧场景 + 对照 + 允许变换表 + 比较键推导规则 + **可行性测试与负测试通过记录** + **`preserved_test_baseline_sha` 与 `b_test_inventory`（文件/符号/哈希锚点/#9 效应 ID/允许变换，含 constraints.rs:90-94 的排除理由记录与真实卖方预留断言锚点——或空锚点核验）**；骨架 golden 冒烟（固定 seed ≥3 场景 × ≥50 tick，预算=1，仅管线穿透无行为断言）落盘；**基础毒化脚手架测试**（骨架级：注入类型化失败 → Err、business_state_hash 不变、poisoned 二次调用拒绝——完整分阶段回滚归任务 7）；**StrategyState 往返测试**（每变体逐字段等价 + profile 派生一致 + 未知变体生产拒绝——影子执行语义归任务 3-6 后的全量验证在任务 7）；任务 3-6 语义测试不在本任务验收内。
  QA scenarios (name the exact tool + invocation): happy — `cargo test -p engine step_phases -- --nocapture`；failure — `cargo test -p engine poison -- --nocapture`：类型化 envelope 超支 → Err(InvariantViolation)、二次 step Err(poisoned)、save 拒、business_state_hash 不变。Evidence .omo/evidence/escrow-parallel-engine/task-2/cargo-test.log
  Commit: Y | refactor(engine): 阶段化 tick 管线与内建并行执行

- [x] 3. 托管 envelope 账本：双账本逐键 + 聚合守恒（同公式）
  What to do / Must NOT do: 逐 envelope 双账本守恒（封前/密封批方程，cash 与 shares 分别；`(envelope_key, receipt_index)` 唯一）；**账户聚合守恒 = 逐键方程求和**（分资源）：`Σ tick_start_live + Σ created == Σ P0_released + Σ P4 spent + Σ P4 released + Σ commit_live`（任务 9 聚合断言用同一公式）；`cash ≥ 0`、`t1_locked ≤ qty`；静默点公开 reserved_cash/reserved_sell_qty 语义不变（**除分歧 #9 明确的卖单现金预留恒 0 与病态小额卖出费用实收封顶外**）。**交付保留测试四类清单 `preserved-test-inventory.md` + hunk 级校验器**（见验证策略：(a) 逐字节不变（**校验器断言 (a) 类条目 diff 为空**）/ (b) #9 预留数值/费用实收断言修改（**与 任务 2 指定的 baseline-corpus/manifest.json 中冻结的 `b_test_inventory` 逐一精确比对：文件、符号/断言标识、#9 效应 ID、允许变换；缺失/新增/改类/扩大任何一条即失败并输出机器可读失败码 ADDED/MISSING/RECLASSIFIED/EXPANDED**）/ (c) 存档 v2/API 形状重写（**allowlist 校验：仅允许枚举的 API/schema/Result/事件形状构造变更；任何触及预留数值、可用现金计算、接受/拒单断言、决策链输出、场景输入的 hunk 必须归 (b) 并匹配冻结清单**） / (d) **校验器**：以 `preserved_test_baseline_sha`（规范时点：任务 2 第 0 步、工作树干净已提交验证之后、任何既有引擎源码或测试修改之前记录；校验器须验证该提交存在且为实现提交的祖先）直接对当前工作树比较 `git diff <preserved_test_baseline_sha> -- packages/engine/tests`（覆盖未提交变更）并拒绝未预期的未跟踪替换文件；**每个被修改的既有测试符号/diff hunk 必须归入 (a)/(b)/(c) 之一，(b)/(c) 须带分歧编号与理由——仅文件名清单不够**。**增量门禁**：D 验证收尾组件建设/修复清单与校验器，A/B/C 各组件属主提交其精确映射；既有校验器可用时先运行并修复真实残余。修改受保护既有测试或合入相关组件前，必须验证当前 (b) 类集合与 `b_test_inventory` 完全一致方可继续；任何新增或扩大 → 任务 3 不通过，修正计划与清单后经独立审查再继续（触及 #9 语义扩大时须回规划会话向用户报告））。Must NOT: **不得绕开 F/fee_delta 另造 nominal 应计函数**；不得让 envelope 见计划软预算；不得只做聚合；不得标量混合 cash/shares；不得用与逐键不一致的聚合口径；**不得修改任何既有测试而不入清单**。
  Parallelization: A 核心引擎统一实现账本与守恒；开工依固定接口及受保护测试增量门禁，存档子验收由 B 存档恢复回填；详见 Four-component execution organization。
  References: packages/engine/src/session.rs:1112-1169, :2531-2574, :2611-2649, :3555-3627; packages/engine/src/session/snapshot.rs:32, 82-147; apps/web/src/app/LocalRefreshViews.tsx:26; packages/engine/tests/session.rs:2625; packages/engine/tests/auction.rs:644-832; packages/engine/tests/session.rs:501-515
  Acceptance criteria (agent-executable): 守恒测试覆盖：跨 tick 部分成交、同 tick 成交→撤单、多对手方多腿、价格改善、最低佣金阈值跨越、**卖费封顶测试组（(v-bis) 数值锚点①三腿（默认费率）：charged 1/1/3.01、deliver 0/0/6.99（含印花税 half-even 进位）、欠费追收、全程 spent 恒 0；封顶激活/不激活边界；分项分配：所得不足且 ≥2 分项应计非零时按优先级拆分、各项不超其余额、总和恰等于总额度；跨多腿累计封顶；部分成交后撤单：终结 released.cash=0 且 charged 历史保留入审计与存档（运行时审计先验；存档/恢复子验收由任务 8 完成后回填，之前任务 3 顶层不勾选）；竞价多腿封顶 + rollover）**、**接受翻转测试（#9）：1 元限价×100 股 + 可用现金 < 旧预留（约 4 元）→ 旧拒新收（隔离场景断言）；旧预留 == 0 的卖单零现金接受相等（对等断言）**、**Momus 舍入跳变场景（commission_min=0、限价 0.01 元、cum 499.98 元/剩余 4 股）在新模型下净额恒 ≥ 0 且无需现金预留**、**买单数值锚点②：限价 10 元 200 股、100 股以 9 元成交 → 预留 2005.02/消耗 905.01/重算 1000.01/立即释放 100.00**、全额成交、撤单、报价过期（封前账本）、日终过期（密封批）、**同 tick 新建且日界终结走密封批方程**、**新建即成交 `created == Σ spent + Σ released + commit_live` 数值断言**、竞价不成交/部分成交 rollover（买卖各含未成交与部分成交的显式方程数值断言；卖单 rollover 现金分量为 0）；P0 过期后本 tick 重分配数值断言；**封顶不变量链测试**（每张卖方 Fill 收据：`0 ≤ charged_before ≤ F_before ≤ F_after`、charged_after ≤ min(F_after, cum_after)、deliver ≥ 0；随机成交切分/价格扫描性质测试含确定性 seed 与用例数声明；**F 与三分项单调性边界测试：half-even 舍入上下邻点、最低佣金切换点、跨 tick/竞价累计**）；**终结转移测试**（partial-fill-then-cancel/expiry/reject/full-fill 各自唯一守恒方程：live_after=0、released=live_before−spent、卖单 released.shares=剩余股份、released.cash 恒 0；**全额成交无额外零数量终结收据——最后一条 Fill 即终结（live_after=0 + 残余释放），断言收据数 == 成交腿数**）；**收据键与链测试**：同一撮合买卖双方两收据、多腿竞价 partial→rollover→日终终结链（**断言精确键：Auction 源 fills 0..k + rollover k+1；DayEnd 源终结收据序号 0；跨源 before/after 连续（rollover 的 *_after == 日终的 *_before）**）、**前 tick 挂单成交腿 + 本 tick 密封撤单链**、P4 拒绝新 envelope 收据、**一单 incoming 吃多张 resting + 单 resting 多腿成交 + 其后密封撤单终结**（断言精确局部键/收据索引/链序）、多股票 receipt_index 确定、**同 tick 跨来源（P0Expiry 与 SealedIntent/Auction/DayEnd 混合）的精确 receipt_index 断言（按唯一序数映射逐位核对）+ 负测试：禁用规范排序（如改用枚举派生序）必须使 receipt_index 校验失败**、before/after 首尾相接（断裂 → 毒化）；现有 reserved 测试按**四类清单**处置（(a) 类逐字节不变者必须原样通过；(b) 类 #9 数值断言修改逐处注明台账条目，不得静默改数；校验器确认无清单外修改）。
  QA scenarios: happy — `cargo test -p engine conservation -- --nocapture`；failure — `cargo test -p engine conservation_cross_leak`、`cargo test -p engine conservation_duplicate_receipt`、`cargo test -p engine conservation_p0_double_release`、`cargo test -p engine conservation_negative_release`（checked 减法违反 → 毒化）、`cargo test -p engine conservation_receipt_chain`（收据链断裂 → 毒化）。Evidence .omo/evidence/escrow-parallel-engine/task-3/cargo-test.log
  Commit: Y | feat(engine): 双账本资源向量 envelope 账本与守恒

- [x] 4. 校验拆分：账户阶段 vs 股票处理阶段
  What to do / Must NOT do: P3 账户阶段拒单：现金/可卖（含 T+1 与未完成卖单）/整手与零股一次性/未完成单数量上限（各轮两遍法，跨轮持续预算与约束状态）；P4 拒单：价格笼子（对手一档→本方一档→最新价/昨收，102%/98% 与十档取宽）与涨跌停闭合区间；worker 产生 IntentRejected 在展示层合并；P4 被拒消耗其 ID。Must NOT: 不得把笼子留在 P3；P3 预算不得被同批释放回补。
  Parallelization: A 核心引擎的唯一实现 agent 连续完成账户校验与增量股票处理，验收含初轮及后轮溢出整 tick 回滚；详见 Four-component execution organization。
  References: packages/engine/src/market.rs:102-171, 201-225; packages/engine/src/session.rs:2684-2714, 675-684; packages/engine/tests/market/price_limits.rs:8-126; packages/engine/tests/session.rs:170-234
  Acceptance criteria (agent-executable): 拆分测试（P3 拒绝不耗簿不出 ID；P4 笼子拒绝耗 ID 出 IntentRejected）；P3 竞争测试（含跨股票）；ID 分配测试（交错 Cancel/P3 拒绝/接受/P4 拒绝 → ID 连续只归接受者、next_order_id 推进 == 接受数）；**近溢出扩展测试**：初轮及已有 P4 私有成功后的后轮分别让 next_order_id/密封序/chain 索引近上限，checked 返回 StepFatal::InvariantViolation；后轮临时键控 envelope/簿/outbox 全部丢弃，business hash 不变、session hash 仅 poison 差异、权威计数器/订单簿/队列/RNG/计划状态不变、**无事件外发**；现有笼子/涨跌停测试按四类清单全部通过：(a) 类逐字节不变；仅已批准的 (b)/(c) 变更通过原校验门禁，不能借 API 调整改变笼子/涨跌停规则断言。
  QA scenarios: happy — `cargo test -p engine price_limits -- --nocapture`；failure — `cargo test -p engine cage_reject_consumes_id`、`cargo test -p engine p3_budget_contention`、`cargo test -p engine order_id_overflow`。Evidence .omo/evidence/escrow-parallel-engine/task-4/cargo-test.log
  Commit: Y | feat(engine): 校验拆分与 worker 内拒单事件

- [x] 5. 收据驱动结算与分配截点语义
  What to do / Must NOT do: P4 产出收据；P6 按账户分组，**仅正数量 Fill 增量聚合为 SettlementTotals（键 (account_id, stock_code, side)），同账户同股票 Buy 先于 Sell 应用**（镜像现行 session.rs:3446-3462/3475 生命周期次序）；先校验（唯一性 + 双账本方程 + journal 标记）后结算；**结算 API 唯一指定：`apply_settlement`；`apply_trade_batch` 从收据路径中排除**（它会按本批 gross 重算费用、对跨收据/跨 tick 订单无法保持累计口径并会重复计最低佣金；该函数仅为既有调用方保留，执行者不得二选一）；**release/rollover/reject 收据不触发交易结算调用，仅更新 envelope/预留状态**；分配截点语义同台账 #2（post-P0 预算公式）；T+1 不变；提交后快照立即反映释放。Must NOT: 不得按单笔 fill 计最低佣金；不得让结算写账户以外实体；P6 不得重算 P4 delta；**不得在收据路径调用 apply_trade_batch**；**不得以收据顺序替代 Buy-before-Sell 生命周期应用序**；**分侧规则（替代一切净额拆分旧口径）：买方 `spent.cash = gross_delta + 买方费用增量`、`deliver_cash = 0`；卖方 `spent.cash = 0` 恒成立、`deliver_cash = gross_delta − 实收费用 ≥ 0`；P6 只消费实收分项增量，不重算任一值**；对账（锚点③）覆盖两侧（对账测试）。
  Parallelization: A 核心引擎的唯一实现 agent 完成收据与结算接线，完整流后一次 P6，实际接入后验收；详见 Four-component execution organization。
  References: packages/engine/src/account.rs:214-269, 322-375, 378-457; packages/engine/src/session.rs:3384-3444; packages/engine/tests/account.rs:183-419, 204-229; packages/engine/tests/session.rs:2625; apps/web/src/app/LocalRefreshViews.tsx:26
  Acceptance criteria (agent-executable): 现有累计费用测试按四类清单全部通过：(a) 类逐字节不变；仅冻结 (b) 类允许 #9 实收/预留断言变换，(c) 类仅允许已登记形状适配，均须通过原校验门禁，nominal 费用规则不变；截点测试组（过期本 tick 可用 / 其余次 tick / 提交后快照可见）；**P0 预算精确性测试**：零空闲现金 + 单一到期买予約 → P1 预算恰好该值（非 0 非 2×；股份侧镜像；账户现金不变）；跨 tick 拆分成交费用连续性；**P4↔P6 对账测试（(v-bis) 数值锚点③）**：每订单 `Σ deliver_cash − Σ spent.cash == 该订单账户现金净变化`，且卖单 envelope `created/P1 现金 == Σ spent.cash + Σ released.cash + 终态(0)`（恒 0 == 0）——用锚点①②的数值场景验证；**分项对账：每腿 SettlementTotals 收到的三分项实收增量之和恰等于该腿总实收增量，且账户逐项扣费（account.rs:335-339 口径）后的现金变化与总对账一致**；**精确快照测试（双检查点）**：锚点②场景成交提交后，`账户现金 == tick 前现金 − 905.01`（结算扣款）且 `reserved_cash == 1000.01`；**预留释放 100.00 不产生额外现金入账**（释放仅缩小 reserved_cash；P6 结算后快照现金与结算完成时现金相等）；P0 仅释放预留的场景保留"账户现金不变"断言；**同 tick 买卖同股对等测试**：同账户同 tick 买入并卖出同一股票（含卖单清空 tick 前持仓的场景），断言精确的现金/数量/t1_locked/**invested_cents/recovered_cents/散户经验状态**（Buy-before-Sell 生命周期保护）；**经验双流对等测试**：同账户同股同方向多订单 + 同 tick 买卖双向的经验状态精确对等——**断言经验更新调用次数 == 订单数（每订单恰一次、按精确总键 (account_id, stock_code, side_rank, order_id) 排序）**；**零结算排除测试**：cancel-only/expiry-only/拒单/无成交竞价 rollover 的 tick 不产生任何 apply_settlement 调用（零数量/零金额断言）。
  QA scenarios: happy — `cargo test -p engine allocation_cutoff -- --nocapture`；failure — `cargo test -p engine same_batch_no_visibility`。Evidence .omo/evidence/escrow-parallel-engine/task-5/cargo-test.log
  Commit: Y | feat(engine): 收据驱动结算与分配截点语义

- [x] 6. 股票状态机：密封序、撤单/替换与集合竞价
  What to do / Must NOT do: 每股票 worker = 确定性顺序状态机，从 post-P0 shadow 簿初始化一次，按跨轮总密封序接续逐操作应用；竞价完成/rollover/日终和价格收尾仅在该 tick 全部根计划及依赖操作排空后按适用边界各执行一次；余单转移复用原 envelope 与原 ID、按 (v-bis) rollover 显式方程；NPC 替换 = 同账户 cancel + place 两密封操作；跨 envelope 同 tick 撤单取消（分歧 #4）；竞价：市价单拒、09:15-09:20 可撤/之后不可撤、收盘竞价全程不可撤、SH 中间价/SZ 价优申报量差+昨收决胜保留；竞价原子结算类型化失败 → 毒化。Must NOT: 不得改变竞价决胜规则、可撤窗口、余单转移语义；不得批量化丢失中间状态。
  Parallelization: 竞价/日界与连续状态机同属 A 核心引擎，不内部拆分实现者；最终验收含真实接入及 B 存档恢复子场景；详见 Four-component execution organization。
  References: packages/engine/src/session/auction.rs:6-247, 249-583, 619-812（:340 arrival_seq、:629 余单序）; packages/engine/src/session.rs:1981-1984, 2817-2847, 3155-3182; packages/engine/tests/auction.rs:285-469, 644-832, 870-898
  Acceptance criteria (agent-executable): 现有 SH/SZ 决胜、余单不交叉、可撤窗口测试按四类清单全部通过：(a) 类逐字节不变；仅已批准的 (b)/(c) 变更通过原校验门禁，不改变竞价制度断言；状态机前后态测试（同账户多次替换、撤后下、下后撤、09:20 边界、收盘竞价撤单、SH 舍入、SZ 决胜、不成交/部分成交 rollover、意外交叉）；排序键测试（玩家交错全局 FIFO；npc→player→plan_chain 类序；跨账户跨来源同价 FIFO 夹 P4 拒单 + 存档恢复后竞价余单序一致；恢复子验收在任务 8 完成后回填，此前任务 6 顶层不勾选，但不阻止已满足运行时前置的任务 7）。
  QA scenarios: happy — `cargo test -p engine auction -- --nocapture` 与 `cargo test -p engine sealed_order -- --nocapture`；failure — `cargo test -p engine auction_settlement_poison`。Evidence .omo/evidence/escrow-parallel-engine/task-6/cargo-test.log
  Commit: Y | feat(engine): 股票状态机与竞价/撤单密封序语义

- [x] 7. 失败模型（全 shadow + 单点提交 + 类型化毒化）与三宿主接入
  What to do / Must NOT do: 全阶段 shadow（含策略影子竞技场、跨轮执行投影与根计划驱动状态）；按 Runtime contract 覆盖后轮失败和事实唯一消费；`commit_tick` 唯一权威提交；`StepFatal::{InvariantViolation, Internal}` 仅类型化失败；panic 不承诺恢复（宿主文档明示）；毒化会话 step/save 显式 Err；业务失败仍是事件。三宿主映射 ADR-0010 HostFailure：apps/web-wasm/src/lib.rs、desktop actor、server routes；`corepack pnpm types:check` 再生 TS 类型。Must NOT: 不得把业务拒单或 panic 当 StepFatal；不得用 catch_unwind；不得让宿主静默吞 StepFatal。
  Parallelization: A 核心引擎负责失败模型，C 宿主交付的唯一实现 agent 负责全部三宿主；分别按完整运行时与宿主证据验收，不等待 3/6 存档子验收，也不阻塞 B 存档恢复；详见 Four-component execution organization。
  References: docs/decisions/0010-unified-host-protocol-and-local-refresh.md; packages/engine/src/session.rs:1965-1979, 3699-3759, :2018; apps/web-wasm/src/lib.rs:61-65（按符号重解析）; package.json:16-19
  Acceptance criteria (agent-executable): `corepack pnpm types:check` 通过；`npm run lint` 通过；毒化门禁：P2/P4/P6 各注入类型化失败 → Err、business_state_hash 等于 tick 前、session_state_hash 仅 poison 差异、事件流无部分应用、save 拒、**权威策略对象哈希不变**。
  QA scenarios: happy — `corepack pnpm types:check`；failure — `cargo test -p engine poison_per_phase -- --nocapture`。Evidence .omo/evidence/escrow-parallel-engine/task-7/types-check.log
  Commit: Y | feat(web,server,desktop): 类型化致命错误映射与类型再生

- [x] 8. 存档 v2 与静默点契约
  What to do / Must NOT do: `SIMULATION_POLICY_ID` 升 v2；SaveSlot 增逐键 envelope 账本（ResVec live/累计值）、订单归属、每单累计 filled_value/费用（**含各分项累计应计与累计实收——欠费追收跨 tick/存档续跑必需**）、`next_receipt_base`、权威投影去重状态（当前 `RetailProjectionSeen`，保留已批准身份键）及其与 `next_receipt_base` 等既有账本/收据游标的一致性，不另造 seen 私有游标、**全量 `StrategyState`（非仅 profile）**、poisoned=false；市场 tick 内禁止 save，成功 commit_tick 后为合法静默点；旧档显式拒绝；恢复后 next_order_id/seq/next_receipt_base/rng_state/策略状态连续且竞价余单序一致。开工时须先对账合法静默点范围：分别核查成功市场 tick 后、恢复后未 step、完整 CivilUpdate 后及初始会话的 save 契约、既有测试与 ADR；ADR 中该市场 tick 限制不得被擅自解释为允许或禁止这些独立边界。记录逐场景依据；无法由既有批准契约消除的分歧明确上报，受影响验收保持未完成，无关存档字段工作继续，不删除或弱化既有测试来消除歧义。Must NOT: 不得写迁移器；不得在毒化状态产出存档。
  Parallelization: B 存档恢复由一个实现 agent 完整负责；持久化字段/静默点契约固定即可与 A 并行，验收依真实新状态往返和续跑，回填 3/6，最终任务 9 前完成；详见 Four-component execution organization。
  References: packages/engine/src/session.rs:85, 381-457, 3701, :407-408, :3720-3728, :3913-3927; packages/engine/src/session/persistence.rs:5-79, 487-529, 656-705, 1190-1203; packages/engine/tests/session.rs:457-515; packages/engine/tests/save_contract/main.rs:117-189
  Acceptance criteria (agent-executable): save_contract 按 v2 重写通过（decode/restore/续跑字节连续）；旧 schema fixture 拒绝测试；恢复后竞价余单序与 receipt_index 连续性测试；非空投影去重状态存档/恢复后身份集合等价及既有游标连续，按已批准投影重放契约验证不重复副作用、后续合法新收据正常消费，且无序号复用；损坏或与账本/收据游标不一致的状态必须显式拒绝，不能默认清空；**恢复后策略行为等价测试**（同 seed 续跑决策一致）；同时交付任务 3 charged 审计历史存档/恢复和任务 6 竞价余单恢复顺序的证据，供两任务回填其完整验收。
  QA scenarios: happy — `cargo test -p engine save_contract -- --nocapture`；failure — `cargo test -p engine save_rejects_legacy`。Evidence .omo/evidence/escrow-parallel-engine/task-8/cargo-test.log
  Commit: Y | feat(engine): 存档 v2 与旧档显式拒绝

- [ ] 9. 确定性三重门禁 + 守恒 + 性能证据 harness
  What to do / Must NOT do: (a) 调度一致性：①预算 1/2/4/auto 全事件流 + SaveSlot serde 字节相等；②同 seed 同预算重复运行字节相等；③带收据扰动门禁（提交/收集器边界独立置换账户分片序、股票分片序、完成序/结果向量序；扰动收据记录并断言 pre-canonical 序各维度确实不同；≥2 非空账户分片 + ≥2 非空股票分片 + 多完成序；负对照：禁用任一 canonical 合并必须失败；**多股票多腿扰动下 receipt_index 逐字节一致**；权威状态/收据/事件/存档字节与不扰动一致；覆盖竞价/日界；双档恢复续跑相等）。(b) 语义投影语料对等：**按任务 2 四类语料契约执行**——对等语料（三机械条件：旧预留==0 + 无欠费前缀 + 无反馈；含零现金接受相等断言）做全字段对等；**#9 在隔离定向场景与受控存活卖单类中按各自枚举的差异集映射**（单一卖单、无其他后续决策循环、有限枚举差异集：预留数值（旧差额预留 vs 新恒 0）、**接受结果（旧预留 > 可用现金时旧拒新收）**、费用实收与净交付数值（旧累计全额口径 vs 新封顶+追收口径）及关联事件/诊断字段）；**受控存活卖单对比语料**（现金充裕 + **反馈闭环规则：无策略/计划链/状态依赖意图，续跑 = 双侧相同密封外生脚本，RNG 不消耗或显式比较游标** + 旧引擎投影重构前捕获；覆盖 rollover 存活、跨 tick 部分成交、含未完成卖单的存档恢复续跑，比较时点 = 存档前/恢复后/每续跑 tick 后；**允许差异 = 该类各自枚举的 #9 字段（预留数值、费用实收、净交付、费用影响下的事件/诊断载荷字段、**终结后余额的机械规则：仅当 `最终 charged == F_final` 时要求余额相等；否则允许精确差额 == `F_final − charged_final`（相对旧侧）**）+ #7 存档表示**；必须相等 = 成交结果（数量/价格/订单身份与 FIFO）、事件变体与非费用载荷（modulo #6 seq 与 #9 费用字段）、**`invested_cents` 与 `recovered_cents`（recovered 按成交总额累计，与费用无关——account.rs:352-357/364-373）**、续跑行为、恢复顺序/待处理意图次序/RNG 游标/策略与决策链状态；**含定向校验"低额部分成交（仍有持仓）→ 撤单 → 存档恢复"：现金差额 == 未实收费用（F_final − charged_final）、`invested_cents` 与 `recovered_cents` 双侧相等（recovered 按成交总额累计，与费用无关）、清仓场景双侧均按原规则删除持仓；合法费用/余额差异必须通过映射，非费用字段篡改必须失败**；含卖方费用语义矩阵行与多腿同 tick 终结场景）；**压力语料仅校验新引擎确定性/守恒/安全，不做新旧对等（诚实边界）**；**对等语料中任何级联 #9 差异判为未映射失败**（比较器无反事实执行能力，不得发明因果溯源）；分歧 #6 结构化比较器（按 tick 比较稳定身份、事件数/变体/载荷多重集，逐字段应用允许变换；帧内数组重排通过；seq 按 TickBatch/CivilUpdate 全更新流保持覆盖与连续性，同实体 FIFO/订单与收据身份另行核验）；三负语料（删除事件/固定身份下交换载荷/未映射载荷篡改）必须判未映射失败。(c) 守恒：每 tick 后逐键双账本 + **聚合（同任务 3 公式）** + cash≥0 + t1_locked≤qty。(d) 性能：与基线同条件对比整跑 ticks/sec 与 RSS；新引擎另报各阶段 wall 与线程 runnable 采样；只报实测。Must NOT: 不得用 CPU 占用率冒充吞吐；扰动不得空转；不得把调度一致性当语义证明；对比条件不一致不得报比值。
  Parallelization: D 验证收尾的唯一实现 agent 统一实现工具、测试和证据接线；完整确定性/语料/性能验收依 A–C 全部证据；详见 Four-component execution organization。
  References: packages/engine/tests/diagnostic_parity.rs:63-71; packages/engine/tests/company_scale.rs:179-191; .omo/evidence/escrow-parallel-engine/baseline-corpus/; scripts/simulation/baseline-run.mjs
  Acceptance criteria (agent-executable): harness 输出 PASS/FAIL 汇总；证据含 determinism.sha256（四预算 × 重复 × 扰动 + 扰动收据）、corpus-diff.md（每差异→台账 + 字段级映射 + #6 结构化比较器输出 + 旧侧派生键）、perf-report.json（环境 manifest）。
  QA scenarios: happy — `cargo test -p engine escrow_determinism -- --nocapture`；failure — `cargo test -p engine perturbation_gate`、`cargo test -p engine corpus_unmapped_diff`（含三负语料必须失败）。Evidence .omo/evidence/escrow-parallel-engine/task-9/
  Commit: Y | test(engine): 确定性三重门禁与性能证据

- [ ] 10. 诊断再基线
  What to do / Must NOT do: 重新基线 engine_error_events 期望（旧基线 55-98/seed 仅历史记录）；更新 causal diagnostics 生命周期定义（submitted = filled + canceled + open + aborted，含 worker 拒单）；`cargo clippy -p engine --features simulation-diagnostics --examples -- -D warnings`。每处期望变更携带 `// 分歧 #N` 注释；独立 subagent 逐 hunk 核对。Must NOT: 不得无说明改期望数字。
  Parallelization: 诊断审计同属 D 验证收尾，不另拆实现者；最终期望迁移依任务 9 证据，完成后放行 11 最终矩阵；详见 Four-component execution organization。
  References: packages/engine/src/diagnostics.rs:596-606（按符号重解析）; .omo/evidence/resolve-blockers-wayland/task-7-after.txt
  Acceptance criteria (agent-executable): **固定边界诊断审计**：记录任务 9 完成提交与任务 10 完成提交的 SHA，`git diff <sha9> <sha10> -- packages/engine/src/diagnostics.rs packages/engine/tests`（仅限诊断基线期望相关文件），新增/变更的期望断言行必须携带 `// 分歧 #N`；输出结构化清单（原值/新值/分歧编号/对应语料证据路径）——**不得依赖无范围 working-tree diff**（已提交后为空 = 空通过，未提交则误报）；diagnostics 测试全绿；独立审查报告逐 hunk 映射；clippy diagnostics 通过。
  QA scenarios: happy — `cargo test -p engine --features simulation-diagnostics -- --nocapture`；failure — 区间 diff 中存在无 `分歧` 标记的期望变更行 → 任务不通过（清单与审查双确认）。Evidence .omo/evidence/escrow-parallel-engine/task-10/cargo-test.log + divergence-audit.md
  Commit: Y | test(engine): 诊断基线迁移

- [x] 11. 全新 K7 矩阵重跑（包含链契约 + 独立核验脚本）
  What to do / Must NOT do: 从新 source-fingerprinted 根目录执行完整 K7：MATRIX_SEEDS=[1,2,3,4,5,7,11,19,23,31]（seed 6 有意排除）；after 17 + sensitivity 77 = **94 次执行（85 canonical + 9 rerun）**；两套件各自独立新根目录。为满足验证时限，使用**语义代表性有界 fixture**：primary=5 个自然日、64 retail、30 ticks/day（开盘竞价 3、收盘竞价 2）；cross-year=8 个自然日、32 retail、20 ticks/day（开盘竞价 3、收盘竞价 2，2030-12-27 开始并跨年）；仍保留五股票、三类 NPC、连续交易、开收盘竞价、自然日/休市、四行业跨年覆盖，不声称完整市场规模压力。runner 先在独立 300000ms deadline 内用多核构建 fixture，必须从 Cargo JSON 解析 workspace 内实际产物，并封存二进制 SHA-256 与编译时嵌入的 source fingerprint；随后直接执行该二进制，不再使用 `cargo run`。runner 必须按检测到的 CPU 总预算真实并发 seed 子进程，单 child 和 after/sensitivity 各自整批共享 wall deadline 均硬限 300000ms，其中执行/校验/发布截止为 299000ms，最后 1000ms 只用于终止进程树、等待 close 和清理 staged 文件；正常异步超时时清理结束后才向调用方报告，正式命令同时置于进程外 300000ms supervisor 下，旧 2h/6h 运行永久 invalid。包含链校验（raw 字节哈希 == per-seed checkpoint entry.sha256 且 source identity 一致；aggregate 含完整 entry 且 checkpoint_digest 自校验；determinism receipt 绑定 canonical+rerun 同 identity）。步骤：(1) 读 runner 与其测试抄录实际 CLI 面（禁止假设旗标）；(2) 新增并测试 `scripts/simulation/verify-k7-root.mjs <root>` walk 全部条目逐条校验，缺失/错配退出码 1；(3) 待任务 10 合入且核验脚本改动纳入稳定源码指纹后，运行两套件并核验两根目录；(4) 命令原文、构建/执行分步 wall 时间、并发/线程资源策略与核验输出落盘；(5) 追加 append-only 证据日志。Must NOT: 不得删除 10-seed 矩阵、7 个 unique sensitivity 配置、9 次 rerun，或丢弃零值/错误/谎报 C06；不得复用旧根目录；不得用未经测试验证的旗标；不得把有界 fixture 冒充完整市场规模性能测试。
  Parallelization: K7 核验脚本与负测试同属 D 验证收尾；最终 94 次 K7 依 10 及所有相关工具代码纳入稳定源码指纹；详见 Four-component execution organization。
  References: scripts/simulation/baseline-run.mjs; scripts/simulation/baseline-run.test.mjs; .omo/evidence/resolve-blockers-wayland/task-7-after.txt
  Acceptance criteria (agent-executable): 最终矩阵包含任务 10 和本任务核验脚本的相关改动，并绑定固定源码指纹；任务 10 稳定前的试跑不计最终证据；94/94 完成；after 与 sensitivity 各自 wall <300000ms，manifest 记录 `max_concurrent_child_executions > 1`（当检测到的 CPU 预算允许）且进程观测证明真实多进程；`node scripts/simulation/verify-k7-root.mjs <after-root>` 与 `<sensitivity-root>` 退出码 0；manifest 封存；证据日志追加。
  QA scenarios: happy — 两根目录核验退出码 0；failure — `node --test scripts/simulation/baseline-run.test.mjs` 仍绿 + 破坏一条 entry.sha256 后 verify 退出码 1 自测。Evidence .omo/evidence/escrow-parallel-engine/task-11/k7-manifest.json
  Commit: Y | test(engine): K7 escrow 源指纹证据链（包含链契约）

- [x] 12. 文档最终同步
  What to do / Must NOT do: architecture.md（阶段管线、数据流契约、毒化分类、双哈希、事件变体表、StrategyState）、trading-rules.md 终稿、open-questions.md、README（含 panic=进程级故障明示）、AGENTS.md 与 testing.md（普通测试 ≤10s、必要长测 child/整批 ≤5min、真实多核、构建/测试分开计时）；只写已验证结论。新增 `scripts/check-doc-symbols.mjs <docs...>` 提取文档引用符号并 grep 引擎源码输出缺失清单。Must NOT: 不得写入未实测性能数字；不得宣称绝对无死锁/满核/具体提速；不得保留或建议 2h/6h 长测上限。
  Parallelization: 文档、符号检查、证据与 F1–F4 收敛均属 D 验证收尾，不再拆分实现者；代码工具纳入最终指纹后跑矩阵，终稿依 9–11 最终证据；详见 Four-component execution organization。
  References: docs/architecture.md; docs/trading-rules.md:1-54; docs/open-questions.md; .omo/evidence/escrow-parallel-engine/task-9/perf-report.json
  Acceptance criteria (agent-executable): `node scripts/check-doc-symbols.mjs docs/trading-rules.md docs/architecture.md` 缺失 = 0；`grep -n "seal_allocation_snapshot" docs/trading-rules.md docs/architecture.md | wc -l` ≥2；规则核对日期为执行当日。
  QA scenarios: happy — check-doc-symbols 退出码 0；failure — 人为加入不存在符号 → 脚本列出（自测）。Evidence .omo/evidence/escrow-parallel-engine/task-12/doc-check.txt
  Commit: Y | docs(engine): escrow 并行模型文档同步

### D 验证收尾状态（2026-09-23）

> 用户在 2026-09-23 明确将剩余重复验收收敛为一次多核 smoke，不再为每个任务重复全量回归。
> 这只替代重复执行方式，不允许把旧源码结果、失败、零测试或未覆盖路径写成通过，也不豁免
> Task 9 的历史见证真实性。当前 HEAD 的一次多核整包 library smoke 已通过
> （785/785）；preserved gate、竞价/存档集成与宿主 smoke 随后在同一当前源码上通过，
> Tasks 3–8 已收束。原始输出及映射见 `task-3-8-smoke.md`。

| 项目 | 状态 | 依据 / 阻塞 |
|---|---|---|
| Task 9 | BLOCKED，保持未勾选 | 九个可构造表面通过，但历史旧引擎零现金、多腿正常终结 witness 不存在；不得补造。见 `task-9/historical-witness-audit.md`。 |
| Task 10 | BLOCKED，保持未勾选 | 当前诊断与 clippy 检查已有通过记录，但 Task 9 未完成，且不存在真实的 Task-9-complete → Task-10-complete 两提交边界，不能伪造区间 diff。 |
| Task 11 | PASS | 提交 `f22241797385540c8910ee042d1072a6aa50a4c9` 后的正式根共享源码指纹 `5d3c51b2...668e`；after 17/17、sensitivity 77/77，独立 root verifier 均退出 0。 |
| Task 12 | PASS | 符号检查、panic 边界、10 秒普通测试 / 300 秒必要长验收政策与文档同步均已有通过记录；文档继续明确 Task 9/10 阻塞，不写未验证性能结论。 |
| F1 | CHANGES_REQUESTED，保持未勾选 | 要求 12 个任务全部 PASS；Task 9/10 明确阻塞，因此尚不能通过最终合规审计。 |
| F2 | 实质审查已有 APPROVE，形式门禁保持未勾选 | 独立 engine/save/host/K7/Task 9 审查均未发现未闭环 A 股语义问题；但 Final verification wave 规定在全部 todo 后执行。 |
| F3 | 验收证据 PASS，形式门禁保持未勾选 | 当前 HEAD 已一次性用 2 workers 跑完 Playwright 五场景（5/5），五张截图、五份 trace、原始日志和机器可读结果均已冻结；但 Final verification wave 规定在全部 todo 后执行，Task 9/10 未完成前不勾选。 |
| F4 | 实质审查已有 APPROVE，形式门禁保持未勾选 | 独立组件与集成候选审查未发现 Must/Must-NOT 越界；仍受 final wave 前置条件约束。 |

### 任务 2 协议补充：CivilUpdate（2026-09-18 用户确认）

任务 2 的逐 tick 协议使用有序 `TickBatch | CivilUpdate` 联合。CivilUpdate 包装
现有自然日日结完成结果，不推进市场 tick，但连续覆盖其实际消耗的全局 seq。
TickBatch 内帧的 tick 与 seq 连续；跨 CivilUpdate 时由整个更新流维护 seq 连续。
消费者必须完成屏障后才应用后续 TickBatch，禁止把自然日事实塞入已完成的 tick 帧。

AfterClose 必须让完整当日分时与竞价图完成，并同步权威收盘 K 线、当前行情；
BeforeOpen 必须同步行情、证券资料、公开信息版本或披露 ID 与 K 线，使重连客户端
无需旧事件数组即可恢复。一次日结同时满足两种边界时显式表达两种屏障目的，
不重复日结、不伪造事件。普通休市日前进如有需要使用明确的 CivilAdvance。
宿主提供收盘后、开盘前暂停偏好，默认均 false；先交付屏障再停止自动推进，
恢复不重复屏障。隔夜委托延后，任务 2 不改变委托有效期、撮合和竞价业务规则。

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
  What to do: 独立 subagent 逐任务核对验收证据与计划条文（References/Acceptance/QA/Evidence 真实存在且命令可复跑）；重跑关键验收命令抽样 ≥3；**核对 #9 决策记录（任务 1 引用的用户 2026-09-17 裁定）与 b 类清单/校验器证据完整**；输出逐任务 PASS/FAIL 表。
  Acceptance: 全部 12 任务 PASS；任何 FAIL 列出证据缺口。
  Evidence: .omo/evidence/escrow-parallel-engine/F1/compliance-audit.md
- [ ] F2. Code quality and A-share semantic review
  What to do: 未参与实现的 subagent 审查完整 diff：大 A 语义（价格时间优先/T+1/竞价/笼子/费用）、必要性、最小范围、边界测试遗漏、跨层语义漂移、不必要复杂度（docs/principles.md:73-82 + AGENTS.md 门禁）；有效发现修复后再次复核。
  Acceptance: 审查报告结论 approve 且复核确认修复闭环。
  Evidence: .omo/evidence/escrow-parallel-engine/F2/semantic-review.md
- [ ] F3. Real manual QA
  What to do: agent 驱动 `corepack pnpm --filter web dev` + Playwright 五场景（每场景含初始状态、操作步骤、UI 断言）：①连续竞价阶段下限价买单 → 订单出现在挂单列表且可用资金扣减；②09:15-09:20 竞价窗口撤单成功、09:20-09:25 撤单被拒并显式展示 AuctionOrderNotCancelable 错误；③竞价窗口提交市价单 → 显式错误提示（非静默）；④存档 → 刷新页面读档 → 持仓/挂单/余额与存档前一致且续跑正常；⑤资金不足下买单 → 显式 IntentRejected 错误展示。控制台无静默吞错。截图与日志落盘。
  Acceptance: 五场景全部通过且断言命中。
  Evidence: .omo/evidence/escrow-parallel-engine/F3/manual-qa/
- [ ] F4. Scope fidelity
  What to do: 独立 subagent 将 diff 与 Scope Must/Must-NOT 逐条对照（无串行参考路径、无 channel/actor、无迁移器、无 STP 变更、无编造性能/满核/无死锁断言、市场 tick 业务权威状态仅 commit_tick 提交（按 Scope 明示的 poison 元数据与 CivilUpdate 独立边界核验）、基线 harness 只新增未改既有源码、StrategyState 往返不改变决策语义）。
  Acceptance: Must 全满足且 Must-NOT 零违反。
  Evidence: .omo/evidence/escrow-parallel-engine/F4/scope-fidelity.md

## Commit strategy
> 用户 2026-09-20 明确要求：每趟模块/联合批次写齐、审核、调试跑通后马上本地 git commit，不等全部顶层任务完成；这替代原“TDD 小步提交”的执行节奏。不自动 push、合并 PR 或发布。

- **提交责任**：共享 checkout 的 index/HEAD 只由 main 串行操作；多个 subagent 可以并行写代码和验证，不能同时在同一 checkout 暂存/commit。各自独立 worktree/分支的提交可并行，但须登记基线与属主，main 汇入前重新核对集成验证及冲突；不得覆盖其他在途改动。
- **提交门禁**：完整模块实现＋临时注释/stub/调试开关已恢复并清单清零＋同批必需测试＋相关检查/回归通过＋未参与实现者的完整 diff APPROVE 均齐备，main 下一安全操作即执行该批本地提交。原顶层里程碑全量矩阵尚未要求运行时不为局部提交强行补跑，但该模块必须有独立可编译、可验证的提交边界；存在缺失依赖则提交同批已审依赖闭包，不提交只有语法检查或已知失败的半成品。
- **准确提交范围**：执行前读 `docs/git/AGENTS.md` 与日常流程，核对分支、HEAD、既有暂存内容、批次清单和已审指纹；不得对共享脏树使用 `git add .`/`git add -A` 把历史或其他 agent 的改动包进去。只提交已授权、可归属的本批文件/hunk及必要已审依赖；同文件混入他人修改且无法安全区分时登记提交阻塞，继续无关工作，不强行暂存。不得清空他人 index、reset/clean 或回滚他人改动来制造干净状态。
- **版本核对**：提交树必须对应已验证/已审输入，不能仅因工作树测试通过就提交缺少未提交依赖的子集。形成提交后核对 commit diff/源码指纹与清单，记录 SHA、分支、测试及审核证据；后续改动失效的证据另行标记。只有测试/审核完成但提交被阻塞的批次明确记“已验收待提交”，不能声称已 commit。
- **格式与边界**：Conventional Commits，scope 取 engine/web/server/desktop/docs/test；一个交付可用一个聚焦提交，涉及原要求分开的引擎重构/语义迁移、宿主或文档时用一组有序且可验证提交，不为凑“一次性”混入无关工作。顶层任务仍须全部组件、接线及原验收通过才可勾选；本地 commit 不等于正式启用或任务全部完成。

## Success criteria
- 12 个任务索引全部完成、验收命令通过，且 A–D 四组件各自完整 diff 的独立审查闭环；**分歧 #9 决策记录（用户 2026-09-17 裁定）被 ADR-0017 引用，b 类清单与校验器证据完整**。
- 保留语义测试按四类清单管理：(a) 类逐字节不变测试在预算=1 与 auto 两档全绿；(b)/(c) 类修改全部有台账映射且通过校验器核验。
- 确定性三重门禁全绿（含 receipt_index 扰动稳定、负对照有效、三负语料必须失败、帧内完整事件记录重排通过且稳定身份/载荷多重集一致；跨 tick 顺序与同实体 FIFO 不变）；语义投影语料差异 100% 映射到 9 条台账（字段级 + #6 结构化比较器含旧侧派生键 + **四类语料契约与任务 2 字段级规则完全一致：#9 隔离类与受控存活类各自映射其枚举的 #9 差异（预留/费用实收/净交付/费用载荷字段/仅现金余额的终结差额机械规则）+ #7 表示差异；invested_cents 与 recovered_cents 恒等**）；双账本逐键分资源 + 聚合（同公式）守恒全 tick 成立。
- 增量计划链按 Runtime contract 通过真实多根计划、后轮溢出、持续预算、事实恰一次消费和一次边界收尾场景；P5–P9 各一次。
- 回滚判据成立：初轮与后轮类型化失败后 business_state_hash 不变、session_state_hash 仅 poison 字段差异、权威策略对象不变。
- 临时注释/stub/调试开关全部恢复，完整路径的最终修订经适用测试和独立完整 diff 审查通过；每个已验收且具备安全提交边界的模块已本地提交，受阻项如实列明。
- StrategyState 往返逐字段等价；存档 v2 含全量策略状态且恢复后行为等价。
- 性能为同条件实测对比并落盘（无预设阈值、无幅度承诺）。
- K7 94 次执行（85 canonical + 9 rerun）包含链校验封存于新 source 指纹根目录，独立核验脚本退出码 0。
- F1-F4 全部 APPROVE；分歧台账 9 条与数据流契约在 ADR-0017 可审计。
