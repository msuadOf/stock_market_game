# luna10：计算接缝、长期状态所有权与原生分配器独立复核

## 基线与方法

- 任务指定基线 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960`；审计工作树 HEAD 为 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`，前者是后者祖先，三份 ADR 与本报告涉及的计算/状态所有权代码区间在两者间无差异。
- 已读根 `AGENTS.md`、`docs/principles.md`。按连续全文读完 ADR-0008（142 行）、ADR-0018（1390 行）、ADR-0020（47 行），共 1579 行；ADR-0018 从行 1 连续覆盖到 EOF。
- 静态检查生产入口、clone owner、归档读写和既有审计记录；未运行测试、编译或长测。未读交易所规则，且本批不主张改动 A 股制度。
- 0018 状态是 `proposed`。其完整 ObservationRoot/COW、冷热历史、反向计划索引、WAL/durable watermarks、旧令牌政策、2099 年后日历等只能作为提议或待裁决项，不能当成现行缺陷或直接实施授权。以下只核实既有明确性能承诺、accepted 决定及当前生产路径。

## 全文章节矩阵

| 文档原文章节 | 追踪主题 | 当前实现证据与判定 |
|---|---|---|
| ADR-0008 行 1–34：状态、上下文、GPU/Rayon 事实基线 | 当前负载下决定 offload 的依据 | 当前权威 tick 在 `session/pipeline/authoritative_tick.rs:20` 按 phase 调用统一 prepare/commit；线程并行走 Rayon，如 `session/pipeline/stock_stream.rs`、`local_admission.rs`。ADR 的 5–10μs/200μs 是文档中的历史讨论测量，不能外推当前构建/负载。未本轮复测。 |
| ADR-0008 行 35–80：D1–D5、N1–N3、ComputeBackend 草案 | Rayon、Rust 指标、并行撮合、GPU 阈值、抽象 seam | `compute.rs:25` 有 `ComputeBackend`；`compute.rs:59` 的 `CpuBackend` 用 `par_iter`；`compute.rs:107` 仅 `Cpu/Auto -> CpuBackend`，显式 `Gpu` 返回 `BackendUnavailable`。全仓生产 `GameSession` 未调用 `create_backend/decide_all`；会话策略由 `npc_tick_preparation.rs`、`plan_chain_candidates.rs` 等独立编排。故 seam 是库级 API，不是生产运行时切换。跨股票 task 有实际生产并行路径；未来 GPU、MC、流水线及 Strategy 数据化不属当前必做。 |
| ADR-0008 行 81–107：T1–T6 与收益排序 | 确定性、计算位置、指标批处理 | `indicators.rs:251` 有 Rayon `calculate_indicators_batch`，但 Web-WASM `lib.rs:435–439`、Server `routes.rs:632–647`、Desktop `lib.rs:60–64` 全调用单项顺序 `calculate_indicators`。指标在 Rust 三端统一计算已成立；跨多序列批量生产接线仍未成立，核对 G17。浮点技术指标不属于账户结算/撮合 authority；不能从其 f64 算法推断交易结算精度漂移。 |
| ADR-0008 行 108–142：备选、后果、关联 | 旧建议与已取代语义 | 共同 V/evolve_v 示例已在 ADR 状态注释中撤除；GPU 仅 future target。`positions Vec` 是旧方案/提议，当前账户、候选、reserved-resource 协议已迭代，不宜机械要求字面重构；Q09 保留为 ADR-0006 扩展与 sealed 策略注册、ComputeMode 旧接口与现行会话编排之间的文档裁决问题。 |
| ADR-0018 行 1–47：状态补充、导读、定义 | proposed 范围与世界/tick/版本词义 | 顶部明确 proposed，并引用 ADR-0019、0024 更新；不把未获产品裁决方案转成生产合同。 |
| ADR-0018 §1.1–1.4 行 48–126：世界、长期目标、观察截点、线程职责 | session 隔离、请求时点、生产并行所有权 | tick 公共入口先经 `failure.rs:50–77` 至 `authoritative_tick.rs`；候选状态在 `clone_for_tick_shadow` 建立、P9 才 `candidate_commit.rs:98` 替换。局部受理与股票 stream 已有任务完成通知。文档提议的玩家完整 CommitVersion/身份令牌、P0 后策略观察改成严格旧根尚未被接受；ADR-0017 当前语义优先。 |
| ADR-0018 §2 行 127–202：现状瓶颈清单 | 深拷贝、哈希、历史、计划、公司/回执、Web reducer | 普通 tick 仍建立完整 `CommittableSessionState` 候选，见 `session.rs:1328–1336, 4074–4166`。已优化处要准确区分：普通 tick 不做全局 JSON/FNV 哈希（`session/hash.rs:30` 为显式诊断入口，生产候选校验已不调用整局哈希）；账户按页 Arc COW（`account_book.rs:13–20,57–76`）；日 K 用 32 条 chunk 的 `AppendOnlyHistory`（`experience/shared_history.rs:8–18,106–137`）；回执防重 radix COW（`retail_projection.rs:25–37`）；公司 `operations`/`company_registry` Arc clone；短期 `price_history` 有界且日内分钟 vector 在 Civil 日界清空。PlanBook、ClosingEngine、PublicLibrary 仍被深复制，见本表末尾 R01。 |
| ADR-0018 §3.1–3.2 行 203–247：复杂度目标与物理限制 | 工作量随当前变化而非年龄；不可无损无限压缩/免费查询 | 当前热路径尚有历史数据依赖的深拷贝（G16，含本次扩展发现）；无永久冷存储/区间查询实现的结论仍是 proposed 范围，不报为 accepted 功能缺陷。 |
| ADR-0018 §4.1–4.3 行 248–368：CommitVersion、观察/执行拆分、权限 | 行情版本、P0 策略可见性、私有信息视图 | 0018 明确这是对 ADR-0017 的业务行为改动，且 proposed。现有生产采用 P0 后决策快照的现行语义；没有 `CommitVersion`/`ObservationToken` 完整版本根 API。不是遗漏已接受规则。没有发现本次代码更改 A 股申报、撮合或结算规则。 |
| ADR-0018 §5、5.1 行 369–493：目标流水线、COW 所有权 | typed patch、page COW、历史追加 | AccountBook 和 AppendOnlyHistory 是局部 COW/追加实现，不等于全态页 COW。session 每 tick shadow 中直接 clone `plans`、`closing`、`library`，且 plan roots 再捕获只读根（见 R01）。没有全量状态 MerkleRoot/typed patch 架构，因其为提议，不列为新增产品缺陷。 |
| ADR-0018 §5.2–5.3 行 494–567：分段历史、冷热、窗口 | 完整日 K、旧 K 查询、历史游标 | 新局完整日 K 保留；每 tick candidate clone 因 AppendOnlyHistory 共享旧 chunk，不复制过去全部 K。显式 `snapshot()` 在 `snapshot.rs:116–121,215–224` 可选地输出所有日 K；`save_snapshot_projection()` 在 `session.rs:2583–2590` 展开完整日 K，属于明确快照/存档投影的线性成本，不是普通 tick 热路径。持久冷热分层、分页查询和分时永久契约待裁决，不推成当前要求。 |
| ADR-0018 §5.4 行 568–637：长期计划休眠和唤醒索引 | 全部计划扫簿与历史生命周期 | `PlanBook` 有活跃 `(account, stock)` 索引，并且计划唤醒 caller 存在；但复制 `PlanBook` 会复制其 `plans: BTreeMap<PlanId, TradingPlan>`（含终止计划），既有 G16 仍成立。本文进一步找到公司报告/公告历史同样深复制，见新 R01。完整 Time/Disclosure/PriceTrigger 反向索引仍是长期目标，不误判缺陷。 |
| ADR-0018 §6–6.2 行 638–751：统一交易者、迟到/断线、机械 continuation | 玩家/NPC/计划同等交易路径、幂等 ID | 已接受 ADR-0017 局部受理规则与生产 pipeline 是当前契约；本提议的旧版本 token、外部 command_id、冻结策略以及跨 tick 输入边界并未完整落地，但仍待产品/宿主政策裁决。机械 continuation 当前以 typed 事实流转，不据此声称可见性规则已变。交易所分类仍由现行交易规则文档决定。 |
| ADR-0018 §7 行 752–800：账户冲突、股票并行、确定性范围 | 局部受理与真实共享依赖 | `local_admission.rs` 建现金/股份依赖；股票独立工作经 `stock_stream.rs` 驱动。事件编号/展示顺序不能回馈优先级，与 ADR-0017 当前实现一致。K7 跨 worker 强比较问题是 G39，非本批代码变更。 |
| ADR-0018 §8.1 行 801–861：增量摘要 | Merkle、版本化 hash、定期 oracle | 当前显式 `business_state_hash` 仍按 JSON 投影整权威状态，适合低频诊断/测试；普通 tick 已移除全局哈希调用。增量摘要是建议方向，未获接受的恢复方案不是缺口。 |
| ADR-0018 §8.2 行 862–957：WAL、checkpoint、durable watermark | 崩溃恢复与持久验收 | ADR-0010/0005 范围不承诺 WAL/崩溃恢复，0018 明确为条件提案。Engine `save/restore` 是 JSON SaveSlot 输入输出，不等于 WAL 或每 tick durable commit。协议 `end_civil_day_update` 在日界调用 `game.save()` 以构造 day-end candidate（`protocol/civil/session.rs:290–329`）；Server/Desktop 的 `SessionCommand::Save` 是显式存档命令（`server/actor.rs:1342–1356`、`desktop/actor.rs:1027–1039`）。这些调用不能冒充高频 WAL，也不能说整个宿主从不生成全档候选。 |
| ADR-0018 §9 行 958–996：生命周期与回收 | 旧页引用、缓存、tombstone | 当前无完整历史段冷存/旧 ObservationToken TTL；但 `AppendOnlyHistory` 旧块按 Arc owner 共享回收。根/版本句柄长存、旧观察私有状态重建期限是提议未决项。 |
| ADR-0018 §10 行 997–1074：验收矩阵 | 历史年龄、权限、收益、三宿主验证 | 当前存在 production entry performance 示例，但本轮未运行/量测。三宿主 calculate indicators 调用已静态追；没有 GPU parity/long-history稳定耗时结论。该矩阵是目标验收，不是当前测试通过声明。 |
| ADR-0018 §11.1–11.2 行 1075–1209：接受原则与待裁决政策 | 严格旧观察、历史种类、旧页面政策、WAL、私有历史期限、2099后、流动性 | §11.2.1/2/4/5/6/7 仍是政策选择，不实现。§11.2.3 局部冲突规则已确认但实现仍有迁移空间；沿用既有审计。§11.2.8 被 ADR-0024 更新：可减少资金、不补钱。没有交易制度新主张。 |
| ADR-0018 §12 行 1210–1275：分阶段迁移 | 基线/版本 API/整局 clone/历史/哈希/计划/统一输入/并行/宿主 | 该节自称迁移提议。只将相符的既有 accepted 长历史普通 tick 所有权承诺核对为 G16；WAL、历史分段、反向索引和新令牌等步骤不得因此视为当前要求。 |
| ADR-0018 §13 行 1276–1307：被拒替代 | 全量 clone、截断、actor、永远重放、单大 Vec、cache 决策 | 这些是目标架构论证；AppendOnlyHistory 提供小块旧 K 反证，但不解决其他有历史的 owner clone。 |
| ADR-0018 §14 行 1308–1353：后果与物理边界 | 待实现收益、存储成本、稀疏并行 | 没有完成 30/50 年稳定性或满核承诺；本报告静态确认 clone 的增长路径，不推断实际毫秒增量。资金规则遵 ADR-0024。 |
| ADR-0018 §15 行 1354–1390：关联与采纳条件 | 0017/0010/0016/Q7/calendar 联动 | 保持 proposed 与关联条件；Q7 为公开存档边界，不等于 WAL 获批。ADR-0017 作为现行 P0–P9 与交易规则语义来源。 |
| ADR-0020 行 1–25：问题与整轮数据 | 分配器测量和适用负载 | 表格完整记录 glibc、预加载 jemalloc、正式链接 tikv-jemallocator，正式版同输入指纹、单次 400 tick 数据。它只支持该机器样本的整轮改进，不支持 macOS/WASM/Windows 泛化或“极致并行”结论。任务要求不跑长测，本轮没有复验。 |
| ADR-0020 行 26–47：决策与平台边界 | 进程级 allocator 和 A 股语义隔离 | Cargo target dependency 为 Linux/macOS（`packages/engine/Cargo.toml:61–62`）；engine crate 根按相同 cfg 设置唯一 global allocator（`packages/engine/src/lib.rs:12–17`），WASM/Windows 不设置此 allocator。算法不改请求受理、A 股优先规则或线程数。静态代码与 ADR 对齐，未发现新平台错配。 |

## 旧结论复核

- **G16 保留并扩充证据，不应核销。** 旧审计指出 `RootReadContext::capture` 深复制整个 `PlanBook`（`decision_chain/roots.rs:18–29`，核心为 `:25`），生产每轮有活跃计划 root 时由 `plan_chain_candidates.rs:290–339` capture 一次，再用 `Arc` 分发给 workers。Arc 只共享已复制的本轮快照，不消除该次复制。更直接的是普通 tick 本身执行 `CommittableSessionState::clone_for_shadow`，`session.rs:4074–4166` 对 `plans`、`closing`、`library` 逐个 `.clone()`（`session.rs:4151–4155`）；它们包含随运行增长的归档集合：`PlanBook.plans` 含终止计划（`plans/mod.rs:135–155`），`ClosingEngine.versions/restatements` 是完整 BTreeMap（`accounting/closing/mod.rs:95–101`），`PublicLibrary.reports/announcements/by_company` 是完整 BTreeMap（`information/public_view.rs:27–35`）。因此每个 tick 的候选 shadow 会深拷贝计划、结账报表版本/重述索引和公开报告/公告，当前业务状态若累计多年就与 G16 的“普通 tick 不随多年历史线性复制”同一缺口。建议更新 G16 原条目证据及覆盖对象，不单独重复编号；这是本轮新增有效发现。
- **区分已改进的历史所有权。** `candle_book.clone()` 通过 `DailyCandleHistory` 持有的 `AppendOnlyHistory` Arc 共享旧 chunk，每次封存追加至多复制一个 32 根尾块（`candles.rs:10–17,51–63`；`shared_history.rs:8–18,106–137`），所以旧日 K 本身不是当前每 tick 深拷贝缺口。账户页、retail receipt identity 也采用 COW。短期 `price_history` 按 `history_len` 裁剪（`continuous_tick_finalizer.rs:117–124`），`market_minute_closes` 是当日游标且日界重置，不是数十年向量。`save()`、完整 `snapshot()` / `save_snapshot_projection()` 会全量投影日 K、完整公司信息等，但这些显式导出/快照成本按 ADR-0018 §5.2、§10 不等同每 tick 成本；day-end 协议确有一次 SaveSlot candidate，需与每 tick hot path 分开报告。
- **G17 仍缺，旧结论成立。** 全仓入口搜索只有三宿主单项调用：Web-WASM `lib.rs:435–439`、Server `routes.rs:632–647`、Desktop `lib.rs:60–64`。`calculate_indicators_batch`（`indicators.rs:251–269`）确用 Rayon，但只在 engine 测试中被调用，没有 production caller。该差异是 D2 的批量/Rayon接线范围，不是 Rust 指标功能或 A 股计算口径未实现；是否值得将多资产 UI 查询合并成 batch 应由真实工作负载决定，不能虚构每个单指标请求可并行的跨序列工作。
- **Q09 仍待澄清，不能当功能缺失授权。** `ComputeBackend` 有纯数据化批量 `decide_all` API，但生产 `GameSession` 不经此工厂；显式 GPU 失败返回，未静默 fallback。ADR-0008 状态已删除共同 V 示例，positions Vec 是旧收益排序提案；ADR-0006 的 Strategy 可插拔与现有 sealed StrategyKind/策略构造体系之间的扩展语义仍需文档裁决。Q09 留作范围/文档待定，不要求现在接 GPU、不要求重建旧 V 或字面 positions Vec。
- **0020 allocator 结论与实现一致。** 正式全局配置已在引擎 crate，不只是测试或 preload 实验。目标平台条件排除 Windows/WASM；Cargo lock/实际 CI 构建覆盖与跨平台性能证据本轮未检，不能报告通过。单次数据的局限由 ADR 本身已诚实陈述。

## 新候选复核与反证

### R01：G16 的普通 tick 深复制范围还包括结账与披露归档

- **证据链：** 公共 `GameSession::step()` → `failure.rs:27–77` → phase prepare → `candidate_commit.rs:98` 最终替换；每一 phase candidate 创建时会经 `pipeline/shadow.rs:10` 调用 `clone_for_tick_shadow()`。后者在 `session.rs:4074–4166` 逐字段重建状态，`closing.clone()` 和 `library.clone()` 位于 `:4151–4155`。被 clone 的 `ClosingEngine` (`closing/mod.rs:95–101`) 是 `BTreeMap<VersionKey, Vec<ReportSet>>` 加全量重述登记；`PublicLibrary` (`information/public_view.rs:27–35`) 是所有 report/announcement 及 company 反向索引。其记录按年报/公告逐步增长，类型 derive 的普通 Clone 会遍历并复制集合，不像 operations 的 `Arc::clone` 或 DailyCandleHistory chunk clone。
- **反证边界：** 一年仅少数报告/披露使单次成本未必主导整轮；当前可能被市场、20K账户等固定规模淹没，未测现行 wall-time。显式 `save()` 和日界 SaveSlot 输出有意可为 O(历史)；proposal 并不要求任意历史查询/导出固定成本。`ClosingEngine` 的哈希投影缓存只缩短诊断 hash，不改变其普通 `Clone`；`PublicLibrary` 的 content_digest 也不使记录 clone 为常数时间。
- **判定：** 有效、现行、符合普通 tick 已接受的年龄无关复制目标；纳入 G16 的同一根因范围（状态 owner 没有共享有界页），建议不另造重复 G。应更新总账里只列 PlanBook 的证据，并将 closing/library 增入 G16 验证 fixture。不能借机要求增量历史 API、WAL 或 COW 采用某种具体设计。

### R02：不把其他归档路径误报为热路径

- `save_projection` 的日 K、公司 report/announcement、计划序列化会全量遍历，`GameSession::save()` 可由用户显式 Save 命令调用；这是保存合同允许的增长成本，未单独新增 G。协议日终会从 `game.save()` 建候选（`protocol/civil/session.rs:311`），这是 CivilUpdate/day-end 路径，现有 ADR-0018 §2 亦明确日终仍展开帧。建议基准分别覆盖普通 market tick 与 day-end/save，而非把一次日终写档当普通 tick 每轮增长。
- `business_state_hash()` 仍是 JSON/FNV 全历史哈希，但其调用是显式诊断/测试；普通生产 P8/P9 不算全量 hash。不能把旧历史描述直接复制成当前问题。
- WASM、Server、Desktop indicator handler 对单次图表输入的计算为顺序单序列调用，延迟与单序列长度有关；单次计算没有足够的输入批次供 `par_iter` 使用，需批次 caller 才能实现 D2 并行价值。不能为证明 G17 任意把多个相互独立的玩家请求强制串成批处理等待，这需先量测 UX/吞吐收益。

## 结论

旧审计关于 G16、G17、Q09 的分类未被反证推翻。G16 证据范围需扩展到 `ClosingEngine` 与 `PublicLibrary` 普通 tick 深拷贝；G17 仍是指标 batch 无生产 caller；Q09 保持待裁决。ADR-0018 未接受方向仍不是必做项；本轮没有核实新的 A 股制度事实，不作大 A 语义变更主张。没有执行性能或测试验收，不能声称当前速度目标/allocator 跨平台性能已通过。
