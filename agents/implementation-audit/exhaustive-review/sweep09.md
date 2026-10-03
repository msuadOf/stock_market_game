# Sweep09：单局多线程、生产入口与 ready/receipt 全文复核

## 基线与完整阅读

- 代码基线为 `4ad5a2e298d086024e6b0069b98b4a6b195a0a01`；按父任务说明，其产品树与 `b76ece3` 相同。
- 已读根 `AGENTS.md`、`docs/principles.md`，并核对 ADR-0017 当前阶段契约、ADR-0018 待决范围、ADR-0019 容量废除决定与 `docs/open-questions.md`。本轮只新增工作记录，不修改交易实现。
- 连续全文阅读 `docs/superpowers/plans/2026-09-24-single-world-multithreading.md`：187 行；`2026-09-25-production-entry-and-thread-pool.md`：79 行；`2026-09-26-ready-receipt-and-thread-boundary.md`：69 行；共 335 行。较长输出截断后分段重读补全，包含全部进度、撤回实验及测量限定。
- 本轮仅静态追调用链，没有运行测试、矩阵、编译或长验收；历史文档中的 PASS 与性能数字不是本轮验证结果。

## 各实施步骤及后续承诺的现状

下表的文档简称分别为 Sept24、Sept25、Sept26；行号均对应以上三个完整文件。

| 原文承诺 | 当前状态 | 生产调用与代码证据 |
|---|---|---|
| Sept24:17、Sept25:7：前 tick NPC、玩家下一 tick 消费、候选失败不能丢队列 | 已实现主链 | `pipeline/npc_tick_preparation.rs:113` 在私有候选形成 `PendingNpcBatch`，记录 `observed_tick`、账户、意图及依赖；`:151` 消费时检查 tick 与依赖。`pipeline/candidate_commit.rs:59` 在提交前准备下一批；`pipeline/ready_ingress.rs:34` 在本轮候选取 NPC 与 player 队列。不是即时路由。 |
| Sept24:18–19、23、25–31；Sept25:19、35–37：个人根只读准备、共享变更延迟、撤单期间重算与新母单安装 | 已有生产接缝，未发现新的遗漏 | `plan_chain_candidates.rs:290` 将个人状态取出，`:304` 捕获共享观察，`:324` 启动独立根任务；`pipeline/ready_stock_stream.rs:64` 在对应股票 typed 事实返回后投影或 `advance_after_typed_outcomes`，随后继续取根/续步；auction 的同类路径为 `:100`。已完成的私有候选内生命周期应用不能仅因存在写入就判“过早提交”。 |
| Sept24:19、56、58；Sept25:5、19；Sept26:15–16：三阶段共享入口及逐股反馈，不等无关股 | 已接线 | B1 `pipeline/continuous_tick_transaction.rs:188`、B2 `auction_tick_transaction.rs:173`、盘前 `pre_open_transaction.rs:197` 均捕获 `ReadyIngress`，并通过 `rayon::join` 重叠初始化，再构造 `ReadyStockStream`；`stock_stream.rs:430` 的调度在 `:447` 取单股结果、`:449` 即投影并入队续步，没有先等所有股完成反馈。 |
| Sept24:56、58、127、152–153；Sept25:67、71；Sept26:15、25：同账户资源按接收事实，同股按实际入口；来源与编号不授优先 | 已实现当前资源及 gate 模型 | `pipeline/local_admission.rs:20` 将 PreviousCommit、BetweenTicks、ReadyThisTick 区分为真正就绪阶段；`:34` 保留来源内部接收 ordinal；`:106` 只为 Cash/同股 Shares 分组；`:136` 普通 Cancel 不加资源边；`:171` 按账户 receipt 建资源边；`:220` 并行运行 gate；`:267` 合并股票 gate 与资源/报价依赖。向量仍可由 NPC/player 拼接，但股票冲突经过 gate，不能据拼接形状直接认定来源优先仍在。 |
| Sept24:19、56；Sept25:19、49；Sept26:16：typed 依赖最小等待、可独立账户继续 | 已实现主链 | `ready_stock_stream.rs:166` 只登记已接受且未反馈的候选；`:191` 只保留真正未完成的账户/股票 routes；`:206` 拒绝残缺反馈。`stock_stream.rs:716` 有另一股运行时本股续步测试，`:763` 有单 worker 不互锁测试，`:794` 有多个外部 coordinator 不耗尽 worker 测试。 |
| Sept24:20、Sept25:7、Sept26:37：已成交撤单明确业务拒绝、部分成交撤余量、致命错误整轮丢弃 | 已实现主链 | `orderbook.rs:121` 提供 `OrderAlreadyFilled`；`pipeline/continuous_matching.rs:1337` 对真实撤单反馈转换，区别其他拒绝；`ready_stock_stream.rs:73`/`:109` 将业务事实交计划；`incremental_continuous_stock_shadow.rs:349` 拒绝失败 coordinator 收尾。事务错误上浮到单一 tick 候选边界，业务拒绝是 outcome，不是以成功默认值掩盖致命失败。 |
| Sept24:53、75；Sept25:18、29；Sept26:17：Web 可选线程数、默认 hardwareConcurrency、错误直达；原生默认 Rayon 池 | 已实现入口 | `apps/web/src/host/worker-host.ts:241` 传可选参数；`thread-count.ts:2` 校验正安全整数与 WASM u32 表示上界；`wasm-worker.ts:108` 解析，`:129` await 初始化，`:130` 才发送 ready。Server `apps/server/src/actor.rs:1041`/`:1416`、Desktop `apps/desktop/src-tauri/src/actor.rs:777`/`:1082` 调同一 `step_frame`。线程参数没有用于截断业务实体；不因原生没有自建 threadCount 配置而开新缺口。 |
| Sept24:54、77、118：P6 受影响账户并行、零售只处理触及账户、整体失败隔离 | 已实现主链 | `pipeline/settlement.rs:106` 账户任务 `into_par_iter`；`pipeline/account_settlement.rs:190` 以实际 `positions_before` 集合组织私有投影，而非整会话全账户复制。真实 stock transaction 为 `stock_execution_transaction.rs:174` 后进入 settlement；独立的原子 API 与 tick candidate 接口不能混为重复复制缺口。 |
| Sept24:55、79–100、102–103、115、128、155、163、172、182；Sept25:73：旧同步/串行/即时计划路由清理 | 旧要求已核销 | 当前入口均为上述统一 pipeline；`StoredStrategy` 与候选状态走生产可恢复类型。历史进度中的“仍需删除旧消费器”“仍需删旧直接路由”已被 Sept24:172、182 与 Sept25:73 后续完成记录覆盖，不能重新登记。旧兼容测试退役不自动变成漏实现。 |
| Sept24:51、153、156：auction arrival_seq 改 order_id，时间优先保留队列位置 | 已核销 | 当前 `AuctionOrderSnap` 使用 order_id；本股 P4 当前实际受理顺序由 local gate/operation 输入提供。全局 OrderId 仍用作引用与费用/回执关联，不代表同价时间优先。 |
| Sept24:60、70；Sept25:11、20、61：长历史普通 tick 所有权与根观察复制 | 部分未完成，归 G16 | `decision_chain/roots.rs:18` 的 `RootReadContext::capture` 在 `:25` 深复制整个 `PlanBook`，`plan_chain_candidates.rs:304` 每批根仍捕获该副本。Arc 只共享副本，不消除这一次历史复制。未发现本轮需要新增编号的独立契约。 |
| Sept24:33–43、78、140–150、159、179–183：账户分页、追加日K与个人经历、有限观察窗口、NPC工作单复用 | 有结构性实现；性能幅度另验 | 本轮核对当前快照/队列及根观察主链与现有 G16。日K、个人经历共享追加与有限观察是后续进度已落实的范围；没有将显式 save/完整快照遍历历史当作违反普通 tick 性能契约。其余冷热分层、反向索引、WAL 等仍属 ADR-0018 待决范围。 |
| Sept26:31、33：连续股票初始输入核对、私有状态并行初始化、并行日终与收盘 | 已实现 | `pipeline/continuous_matching_adapter.rs:11` 生产输入准备，`:36` 按股 `par_iter`；`stock_stream.rs:91` 逐股并行初始化，`:109` 逐股并行 finish。内部 `finish_for_tick` 仍含循环，但真实 caller 给每个 coordinator 单股输入，因此不能按内部循环误报全股串行。 |
| Sept26:37–39：私有股轮次避免重复初始全簿快照与逐单账本复制，独立API保留原子性 | 已实现当前边界 | `incremental_continuous_stock_shadow.rs:390` 使用 owned market/ledger，envelopes 为空；`continuous_matching.rs:282` 后区分 private round，`:305` 原地插入新 envelope；独立 public ledger 操作仍保留自身校验和复制，不构成本条回归。 |
| Sept26:43–45：成交投影只取入场与真实成交参与者 | 已实现 | `continuous_matching.rs:1056` 初始化只载入 incoming，`:1126` 对首次出现的 counterpart 按 key 查询并 clone 对应项，没有每笔克隆全部 live envelope。 |
| Sept26:55–57：初始全检、私有轮转换校验、股票终检对应、P5/P9 权威完整核验 | 已实现 | `continuous_matching.rs:604` 私有轮不再 complete audit；`incremental_continuous_stock_shadow.rs:454` 在发生操作的股票结束时做 `validate_private_market_ledger`；`receipt_aggregation.rs:69`/`:92` P5 输入输出 complete evidence；`candidate_commit.rs:71` P9 重新核验/rebase。没有因删除重复终检放弃权威守恒核验。 |
| Sept26:61–63：P5 只 clone 一次权威 ledger candidate，其后原地事务 | 已实现真实 caller | `stock_execution_transaction.rs:166` 直接调用 `apply_owned_receipt_transaction(ledger.clone(), …)`；`receipt_aggregation.rs:63` 接管 candidate、`:72` private insert、`:91` private apply。独立 `apply_receipt_transaction` 在 `:51` clone 是为自身原子性，不能以读到这个 helper 就判生产仍多层 clone。 |
| Sept26:67–69：P4 反馈移交盘口、按事实关联回执、真实回执清生命周期 | 已实现并进一步调整为 market delta | `incremental_continuous_stock_shadow.rs:417` 接管 output.market，`:436` 给 projection `market_delta`；`continuous_matching.rs:610` 提取本股变更；`adaptive_plan_chain.rs:713` take delta 并在 `:717` 应用到 candidate 盘口，`:720` 按 sealed 身份分组实际回执。当前采用增量投影，不因代码与旧“直接移入整盘口”措辞不同判功能缺失；此段也明确不承诺消除所有 P4 clone。 |
| Sept24:154：K7 不再跨自由调度强制 artifact 字节一致 | 未完成，归 G39 | `scripts/simulation/escrow-verification-contracts.mjs:130` 的 `compareArtifacts` 经 `:160`/`:176` 对多个预算与扰动比较完整 artifact；矩阵 runner `run-escrow-verification-matrix.mjs:583` 与 `:774` 仍比同一 reference。存在真实 caller，且未固定实际受理轨迹；不是只剩未引用旧 helper。 |
| Sept24:21、68–71；Sept25:21、25；Sept26:21、47、69：活跃三来源完整 step、1/多 worker交替、CPU/线程与独立复核 | 工具已有；稳定性能/长期宿主实测属验收证据债 | `packages/engine/examples/production_entry_performance.rs:97` 指定 Rayon pool，循环推进完整 engine tick，`:205` 检查三来源实际活动，`:217` 输出 input fingerprint、`:223` pool容量；Sept26:47 有固定输入文件和复测入口。不能从 pool 容量推断 CPU，不把未复跑性能数据登记成缺实现。 |

## 新候选的反证与边界

1. **“NPC/player 在 initial 拼接所以仍固定来源优先”未成立。** `ReadyIngress` 保留拼接并不直接授予冲突胜负，`admit_ready_batch` 进入资源 lane 与股票 gate；同账户 PreviousCommit→ReadyThisTick 有真实形成时点依据。跨账户同股依实际 gate 登记，旧文本当前状态被 Sept26:25 的完成段覆盖。
2. **“P4 仍先等全股票”未成立。** `StockStreamCoordinator::drive` 在单股返回时就调用 progress，不需全部 drain；最后 finish 的全股屏障是在全部 continuation 排空之后做统一终检和提交，不是计划续步入口屏障。
3. **“P5 仍 clone 多层”未成立。** 生产 stock transaction 直接用 owned helper，独立 session 原子 API 的 clone 不在这条调用链形成同样成本。
4. **“auction 初始化串行”不单独登记。** `stock_stream.rs:167` 的 `auction_shards` 的确串行初始化，但 Sept26:31 的明确新承诺限定连续交易股票边界；同一股后续处理与 auction finish 仍使用 Rayon。若追求更多并行，需测量收益，不能将并行优化空间冒充已承诺缺功能。
5. **“原生 actor 未 spawn_blocking”不单独登记。** Sept24:53 明确先保留保护 Tokio I/O 的 Fastest semaphore，待计算移出 I/O 执行器再一并调整；当前原生 actor 同进程 Rayon 的事实不因没有该架构迁移而失效。宿主响应性/调度边界由宿主审计另核对。
6. **“独立计划唤醒/反向价格索引不存在”未成立或待决。** `decision_chain.rs:541` 将 observed_accounts、开盘策略与 active_plan_requires_review 合并，`:559` 读取活跃计划；`:597` 检查真实价格/本人资料条件。独立唤醒条件已有，反向索引不是该三文档已完成部分，更不能据同名类型缺席判漏功能。
7. **撤回实验不重开。** Sept24:41、126、158、160–161、165、173–178、185 与 Sept26:49–51 明确记载否决/撤回；对应未保留的优化和实验专测属于主动退出，不能新增产品任务。

结论：本次三份计划的实施承诺核对没有确认 G16、G39 之外的新 G。G16 是真实长历史所有权缺口；G39 是现行验证入口契约缺口；稳定速度、默认调度利用率、多年场景与打包 GUI 的实测仍是验收证据边界。本记录不宣称穷尽其他文档、当前性能达标或所有交易边界已运行验证。
