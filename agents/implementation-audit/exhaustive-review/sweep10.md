# 历史实现穷尽复核 sweep10：计算、allocator 与长期时间线

## 基线、读取范围与结论

- 产品基线为父任务指定的 `b76ece3`；读取期间工作树 HEAD 为 `4ad5a2e`，只读 `git diff --name-only b76ece3 HEAD -- packages apps` 无输出，产品代码一致。
- 已阅读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`。指定三份 ADR 均按连续区间读取全文：ADR-0008 为 **142 行**，ADR-0018 为 **1390 行**（1–240、241–480、481–720、721–960、961–1200、1201–1390，无跳章），ADR-0020 为 **47 行**，合计 **1579 行**。下表行号均为当前工作树。
- 已追踪生产 caller，不能以测试调用、类型存在、旧任务勾选或某个性能报告代替现行生产事实。未运行测试、编译、GPU 或长时间性能验收；本文件不宣称运行通过。
- 未确认独立于总账的新增 G。**G16、G17 仍缺**，补充 G16 两条实际复制路径；**Q09 仍待明确**。ADR-0018 未接受的 COW、HistoryStore、观察令牌、WAL、水位、旧版本期限、2099 年后制度不转成现行漏实现。ADR-0020 虽仍标 proposed，所写平台 allocator 选择已在代码实现。

## ADR-0008 逐章状态

| 原文章节与行号 | 原文要求及当前判定 | 当前代码与 caller 证据 |
| --- | --- | --- |
| 状态、上下文、关键事实 1–33 | accepted；历史的微秒值和 WGSL 编译值为讨论基线，不是今天负载的验收证据 | `packages/engine/src/compute.rs:25` 留下纯 decide seam；当前 NPC 会话实际路径另走下述 `run_npc_decisions`，不能从旧值推出当前加速 |
| §1 D1，41 | Rayon CPU 并行已有；CpuBackend 为库级实现，生产会话也实际使用 Rayon | `compute.rs:59` 实现 `CpuBackend`，77 行 `par_iter`；`session/pipeline/npc_tick_preparation.rs:95` 使用 `rayon::join`，`npc_decisions.rs:160` 的生产 `run_npc_decisions` 在166行对到期账户 `par_iter`；`queue_npc_for_next_tick` 113、135行调用准备路径 |
| §1 D2，42；收益排序100；备选D113；后续131 | **部分实现，G17**：MACD/KDJ 已迁至 Rust，Rayon batch 尚未接生产 | `indicators.rs:237` 单项函数包含 MACD、price KDJ、OHLC KDJ；251、258行 batch `par_iter`；全仓 `calculate_indicators_batch` 搜索仅定义及同文件测试。WASM `apps/web-wasm/src/lib.rs:437`、Server `apps/server/src/routes.rs:646`、Tauri `apps/desktop/src-tauri/src/lib.rs:64` 均调单项函数 |
| §1 D3，43；收益排序101 | 跨股票并行撮合已接生产；整体加速须另验收 | 连续竞价 `continuous_tick_transaction.rs:210` 调 `continuous_shards`，250行调用 `finish_continuous_shards`；盘前 `pre_open_transaction.rs:219`、257行；集合竞价 `auction_tick_transaction.rs:194`、231行。`stock_stream.rs:95` 初始化 `into_par_iter`，120、190行并行收尾，360行 `scope.spawn` 执行各股 `shard.apply(operations)`，不只是测试重叠 |
| §1 D4，44；排序103；后续132 | GPU Monte Carlo 是未来方向，未列现行 G | `packages/engine-gpu/src/lib.rs:44` 的 `decide_all` 在53行直接委托 CPU；未找到 Monte Carlo 生产计算链；ADR明确需要独立 spec/ADR，不据不存在真实内核报现行必做 |
| §1 D5，45；§3 57–78 | seam 存在；按配置切换权威会话的承诺与现行协议关系待明确，归 **Q09** | `compute.rs:25` trait、94行 `ComputeMode`、107行 `create_backend`，Cpu/Auto可建CPU，Gpu在110行显式拒绝。全仓搜索 `ComputeBackend/CpuBackend/create_backend`：生产会话不调用该工厂； `SessionSetup` 无 compute 字段。`compute.rs:8` 的旧注释声称 setup 控制，不是实装证据 |
| §2 N1/N2/N3，47–55 | GPU decide/三缓冲当前不做；N3 数据化为 GPU 前提或独立重构 | CPU strategy 数据已有 `StrategyData` 入参，但没有获批要求现在实现 GPU pipeline；5000 NPC 仅是重新评估触发条件，不等于自动要求新增 GPU。当前三来源流水线属于 CPU 局部冲突路线，不把同名 pipeline 当三缓冲实现 |
| 技术细节 T1–T6，81–90 | 架构说明与未来 GPU 确定性边界，不新生功能任务 | CPU权威撮合与 Money 仍由 engine；GPU适配器探测不等于整数WGSL内核。浏览器多核路线仍是 Rayon，而非 GPU 必做 |
| 收益排序，94–104；后续130–134 | D2/D3 已分别核对；positions Vec 与旧 deepNormalize 前提变化列 Q09 | 现行会话按账户和持仓 map、协议校验传递，不能为恢复旧字面 Vec 方案直接重构；排序不增加独立于D2/D3的要求 |
| 备选，108–114；后果118–134；关联138–142 | 与D1–D5同项合并；真实GPU、Strategy GPU布局为未来 | CPU seam保留和Rust指标主干已实现，批量接线缺口仍是G17；不得重复计数或把历史待落地状态当今天均未实现 |

### G17 的三宿主端到端调用

页面注册 calculator：`apps/web/src/app/useSessionHostLifecycle.ts:111` 将 `host.calculateIndicators` 交给图表。Worker `apps/web/src/host/worker-host.ts:67` 请求 `calculateIndicators`，`wasm-worker.ts:310` 调 WASM 单项入口；Remote `remote-host.ts:261` 在263行向 `/api/indicators` 发送一个 `IndicatorInput`；Tauri `tauri-host.ts:259` 在261行 invoke 单项 command。Rust三入口最终都进入 `calculate_indicators`，没有可隐藏在适配器中的批量生产 caller。因此缺口是accepted D2的批量生产接线与收益验证，不能写成指标完全未提供，也不能为凑并行强行给单股负载制造批量工作。

## ADR-0018 逐章状态与范围排除

| 章节、原文行号 | 当前实现与判定 | 代码证据／范围依据 |
| --- | --- | --- |
| 状态、导读，1–47 | **proposed**，整体结构未获批准；局部后续决定单独核对 | 5行状态；9行要求永久粒度与持久水位决定前不转accepted；14–18行明确正文建议非完整授权 |
| §1.1–1.4，48–126 | 单session、多核、跨tick队列与合成前史方向已有；长期稳态尚未完成 | `npc_tick_preparation.rs:113` 在P9前生成下一tick NPC队列，140行记observed_tick；`ready_ingress.rs:40` 取NPC队列、41行取玩家候选。107行后范围改由ADR0023限制为合成前史；不要求真实市场导入。没有每NPC常驻线程 |
| §2，127–201 | 历史列举需按当前代码更新，不能把旧行号/旧实现全部重开；剩余PlanBook复制属G16 | `session.rs:1328` 委托状态clone；4155行仍 clone PlanBook；`roots.rs:25` 再 clone。已完成日K与个人历史用共享chunk；普通tick的旧全量JSON hash移除不是尚缺。Web累积复制另见下文范围排除 |
| §3.1，203–227 | **G16 已接受局部目标仍缺**，不能用proposed掩盖后续明确接受的所有权优化 | 后续实施计划 `docs/superpowers/plans/2026-09-24-single-world-multithreading.md:60` 要消除执行shadow整局复制及历史增长扫描，70行要求固定受影响实体、变化历史长度验收。实际两个 PlanBook clone 均保留所有终止历史 |
| §3.2，228–246 | 物理存储与查询边界，不新增无限存储/固定大查询耗时保证 | 当前显式保存序列化全部日K；本身不违反每tick不复制全部过去的目标。磁盘、查询量、策略真实活跃量须分开，不从128核推导每tick满核 |
| §4.1–4.3，248–368 | 完整CommitVersion/ObservationToken/P0观察分离仍属建议；身份隔离由现行视图处理 | `ready_ingress.rs:26` 明示root和执行setup读post-P0；`roots.rs:6` 内部root context没有epoch/root hash端到端令牌。`npc_tick_preparation.rs:140` 仅保存observed_tick不代表完整版本结构；347行权限说明不要求把整个GameSession开放给策略。§4.2在342行明确接受才改ADR0017，不能列现在缺严格pre-P0观察 |
| §5，369–446 | CPU准备／股票任务／计划续行／统一结算已有；拟议HistoryAppend原子目录链无生产实现 | `continuous_tick_transaction.rs:188` 捕获ReadyIngress并进入股票工作，`stock_stream.rs:360` 单股任务，`ready_ingress.rs:97` 收取就绪续行；现行发布仍由完整候选P9完成，不宣称StateRoot模型已实装 |
| §5.1，447–493 | 部分共享页/chunk已实现；整套StateRoot/page-COW待接受；G16只记录已授权复制目标 | `session/account_book.rs:69` 账户shadow路径；`experience/shared_history.rs:8` chunk大小32，28行clone共享tail；`session.rs:4149`、4150行公司Arc共享不能被文字clone当全部公司历史深拷贝。仍有PlanBook复制，不以出现Arc核销G16 |
| §5.2，494–545 | 完整日K内存留存已有；HistoryStore冷热/按年月定位/NeedHistory预取尚未实现，属提议 | `candles.rs:13` DailyCandleHistory采用AppendOnlyHistory，51、62行追加不截权威完整历史；37行recent和58行只裁剪技术缓存；`shared_history.rs:64` get沿前驱chunk定位，不是年份索引。生产源搜索HistoryStore/NeedHistory/ResolvedHistory/HistoryReader无对应结构。§5.2自己写尚未实现，永久粒度未裁决 |
| §5.3，546–567 | 活跃/终止历史分离部分已实装；永久逐笔、盘口、trace、精确防重归档尚未决定 | `plans/mod.rs:140` 权威完整plans和145行活跃by_account_stock并存，217行active_codes仅索引活跃；165行显式save复制完整plans，不是普通计划全簿扫描。全套归档机制不依据该proposed表单列G |
| §5.4，568–636 | 2026-09-26确认策略观察节奏为局部范围；反向公告/价格/时间唤醒索引为长期目标 | `ready_ingress.rs:80` 捕获ready根，`plan_chain_candidates.rs:304` 创建root snapshot；`roots.rs:484` 候选包含active_codes。原文580–582行明确不能把活跃索引当完整反向结构。未发现新增独立缺口证据，不要求新增每计划常驻线程 |
| §6、6.1，638–730 | 跨tick与统一受理已经接入；旧页面政策、完整令牌、可靠验收未接受 | `ReadyIngress::capture_sources`统一队列；`npc_tick_preparation.rs:149` 队列在可丢弃shadow消费。建议SubmittedIntent含epoch/hash/capability不等于现行结构必需。可靠验收需§11.2.4，不能把日终存档说成每命令WAL |
| §6.2，731–750 | 机械typed续行已有；若读新市场作新投资决策才需实证为缺口 | `ready_ingress.rs:55` 阻挡unfinished routes、57行加入ready续行；97–119行只收就绪root/链。范围已由ADR0017 continuation承接，不凭缺提议对象名断言失败 |
| §7，752–799 | 局部冲突与跨tick时点经用户明确，生产股票并行与完成通知已有；自由调度≠固定总序 | `stock_stream.rs:360` 股票任务产round并发回传，365行通知；`plan_chain_candidates.rs:324` 独立root任务；`ready_ingress.rs:107` 对当前ready按局部受理入队。全轮加速／彻底消除准备等待未实测；K7未固定受理轨迹问题沿用G39，不另编号 |
| §8.1，801–861 | ordinary tick未全量hash；Merkle增量摘要为提议，不恢复旧JSON/FNV每轮检查 | `session/failure.rs:73` 生产P0–P9入口；`session.rs:1339` 候选提交；新MerkleRoot/hash schema不据缺同名符号开G。G16不因hash移除自动完成 |
| §8.2，862–957 | WAL、durable/published/committed水位、可靠admission、崩溃恢复明确条件提案 | 803–807行说明ADR0010当前不引WAL；`protocol/civil/session.rs:73` checkpoint和79行rollback是进程内状态，19–27行共享内存副本而非磁盘日志；175行public save只返回最近day_end_save。Web `save/day-end-persistence.ts:20` 串行写捕获日终candidate，不能等同durable每tick |
| §9，958–995 | 生命周期、旧私有token保留期限、清理政策为建议；未批准永久私有旧版本 | `protocol/civil/session.rs:11` 日内history，13行冻结日终档；没有完整历史版本根供用户持有。页面防重map与日内复制是当前事实，但纳入提议归档目标不直接新增G |
| §10，997–1073 | 13项验收矩阵是实施前提议，不能从代码测试存在宣称已达到30/50年稳态 | 与已获接受目标重合的history-age复制验收归G16、并发轨迹验收归G39；未运行长期性能、热内存、GPU、真实三宿主测量。T+1/价格时间/守恒本身仍以现行交易基线为准 |
| §11.1，1075–1108 | 建议整体原则尚未接受，尤指pre-P0观察、冷历史、摘要升级 | 不用原则编号把提议偷转任务；局部接受的顺序和后续所有权目标分别由§11.2.3及后续实施计划证明 |
| §11.2.1–2，1111–1145 | 永久历史粒度、旧页面请求政策待裁决 | `docs/open-questions.md` Q7仍把完整交易历史/数据库/cloud列Stage2产品选择；不误把单槽JSON存档完成当永久minute/盘口归档完成 |
| §11.2.3，1146–1149 | 真实冲突规则已由用户决定，现有局部生产链已读；准备等待仍未完整验收 | 不用整份proposed状态排除用户明确的局部决定，也不把早期10.701秒单次结果当当前稳定性能；现行代码caller见§7，G39审计入口另由工具复核负责 |
| §11.2.4–7，1150–1200 | WAL范围、强/易失发布、旧私有保留期、2099后规则均待裁决 | `calendar/policy/mod.rs:57` default_v1，62行runtime_max_end=2099-12-31；当前边界诚实，不报缺未来官方制度预测 |
| §11.2.8，1202–1208 | ADR0024核销Q12；允许投资者资金减少，不新增资金循环 | 不将缩量/零成交当长期架构失败或要求给NPC补钱；会计经营与投资者资金隔离依现行公司规则 |
| §12，1210–1274 | 9步迁移为proposal，批准局部已按上表单独核对 | 日K共享与活跃计划已有；运行时ownership G16仍缺；冷持久化、Merkle、观察令牌、WAL不得全部加入G。普通tick候选与公开日终save不同语义 |
| §13，1276–1306 | 6个备选排除用于说明设计限制，不新增功能清单 | 当前AppendOnlyHistory为链式有界chunk而非整年Arc<Vec>；显式存档全量允许，不能等同每tick重做历史；业务分片非每实体常驻actor |
| §14.1–3，1308–1352 | 收益、成本、物理上限均条件性；没有已达稳态证据 | 1322行明确收益待实现待测量；G16保留，而128核常满、无界RAM、保证永远有成交、任意大查询固定耗时均不应制造缺口 |
| §15，1354–1390 | acceptance同步门禁尚未触发 | ADR0017 pre-P0变更、Q7、可靠恢复、未来规则与旧token期限仍需产品裁决；不要求为了审计把proposed标accepted |

### G16 新补充的生产证据

1. `GameSession::step` 位于 `session/failure.rs:29`，73行进入 `execute_authoritative_tick`；候选的 `clone_for_tick_shadow` 在 `session.rs:1328`，1335行进入RuntimeState `clone_for_shadow`。**4155行 `plans: plans.clone()`** 的目标 `PlanBook` 在 `plans/mod.rs:139` derive Clone，143行是 owned `BTreeMap<PlanId, TradingPlan>`，终止计划留在这个完整map，故该clone随终止历史数量增长。这处不是 `Arc` 指针clone。
2. 生产三阶段ReadyIngress捕获roots，`ready_ingress.rs:80` 调会话ready根；后续 `plan_chain_candidates.rs:290` 启动root账户，**304行先执行 `RootReadContext::capture(session)` 再 `Arc::new`**。capture在 `decision_chain/roots.rs:25` 又clone整个PlanBook。Arc只让同批root共享已经形成的完整副本，不让副本生成免复制。活跃账户为空时301行早退，因此不能声称每个零活跃tick必走第二条；第一条shadow复制仍在。
3. 宿主内存checkpoint的 `protocol/civil/session.rs:22` 又调用 `game.clone_for_tick_shadow()`；`intraday`在23行用AppendOnlyHistory共享，不生成整份文件存档。候选和宿主失败边界不同，不能以保留必要原子性为由认定复制已优化，也不能简单删除边界。
4. `PlanBook::serialize`在 `plans/mod.rs:165` clone所有plans是显式保存路径，**不另开G**；未来优化目标是避免普通tick反复复制，而不是删终止计划或削弱可查询/恢复事实。未测复制字节或耗时，证据只支持增长路径存在。

### ADR-0018 范围下的现行事实，未新增 G

- Web `apps/web/src/host/protocol/reduce.ts:59` 每更新复制accepted map，60行追加身份；CivilUpdate在95行仍保留该map。79行对日内帧展开复制。该现行长期增长事实与ADR0018 §2/§10一致，但三份指定ADR中它仅属尚未整体接受的稳态/归档方案；总账已有 G31 是当前股票历史引用隔离问题，不能把accepted map增长并入G31假装同一原因，也不能自动开现行G。保留为未来架构与热内存验收证据。
- `AppendOnlyHistory::get` 在 `experience/shared_history.rs:64` 沿前驱chunk找index；是完整内存留存，不是磁盘冷热或时间区间索引。旧日K不再360截断，但这不证明热RAM有界，也不证明多年历史minute可查询。现行accepted JSON/day-end存档与拟议永久HistoryStore契约不同。
- 完整token和pre-P0策略观察尚未接线，但原文明确与ADR0017现行post-P0观察冲突，需acceptance同步修订。不能以新提议要求回退已接入的跨tickNPC请求或改动执行budget。

## ADR-0020 全文三章状态

| 章节与行号 | 当前状态 | 代码／caller 证据 |
| --- | --- | --- |
| 元数据1–5、问题7–24 | 文件仍proposed，单次20股/20,007NPC/400tick测量为历史证据 | 不从allocator存在推导所有平台同样加速，也不要求重新跑长测试才能确认配置已实现 |
| 决定26–37 | **配置已实现**：Linux/macOS engine全局jemalloc，WASM和Windows不链接 | `packages/engine/Cargo.toml:61` cfg linux/macos目标依赖，62行 `tikv-jemallocator="0.6"`；`packages/engine/src/lib.rs:15` 同一cfg，16行global_allocator，17行Jemalloc。Server `apps/server/Cargo.toml:26`、Tauri `apps/desktop/src-tauri/Cargo.toml:31`、WASM `apps/web-wasm/Cargo.toml:19` 都直接依赖同engine；前两者按目标继承，WASM cfg不成立。原生test/examples也共享engine配置 |
| 边界39–47 | 更换allocator不增加业务线程/容量/撮合路线；跨平台性能仍需分别验收 | 当前allocator仅在crate根静态选择；生产NPC/股票/账户仍走原engine路径。macOS/Windows/browser真实活跃多核性能未运行，不宣称已经证明。并行正确性与失败回滚不是仅靠jemalloc声明证明 |

## 新候选及排除依据

| 候选 | 最终处理 | 理由 |
| --- | --- | --- |
| PlanBook双路径完整复制、protocol checkpoint继承复制 | 增强 **G16**，不新编号 | 后续已明确接受ownership目标；已有G16根因完全覆盖，补caller和零活跃边界 |
| Rust指标batch无人生产调用 | 保留 **G17** | accepted D2当前部分完成；三宿主单项结果已有，不重开指标算法 |
| ComputeMode没有SessionSetup配置和会话caller | 保留 **Q09** | accepted seam与后续删除V/个体编排关系尚未明确，库级seam确实保留；不能未经裁决改生产策略路径 |
| GPU适配器有名称但内部CPU fallback、无Monte Carlo | 排除现行G | ADR0008 N1/N2明确当前不做，D4明确未来独立设计；不要误称GPU已执行，但也不要求现在补内核 |
| 缺HistoryStore/Merkle/WAL/durable/token/旧私有版本重建 | 排除现行G，保留提议范围 | ADR0018状态与§8/§11明确需裁决；局部后续ownership已另用G16承接 |
| Webaccepted无限增长与单日intraday反复复制 | 记录现行事实／未来长期验收 | 在指定三ADR中只有proposed稳态约束，不偷转新G；与G31当前股票引用隔离不同 |
| jemalloc配置没实现、Windows/WASM错误依赖 | 排除 | 两处平台cfg一致，production宿主链接engine；文件proposed不代表代码不存在 |
| allocator所有平台加速、30/50年稳态、128核常满 | 不宣称完成／列验收边界 | 未运行测量，旧单次数据不足；真实活跃量与历史年龄需分别控制 |

本批只增加工作审计记录，不变更任何交易、存档、API或A股单位；不引入真实行情或隐藏现金流。独立完整diff复核由父任务统一执行，本文件不自称替代该门禁。
