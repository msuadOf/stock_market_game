# sweep72：stream 与 Session 独立复核记录全文回查

## 阅读与基线

连续全文读完 `pipeline/stream/implementation.md` 1–57 行、`session/caller-review.md` 1–38 行、`session/core-review.md` 1–83 行，均位于 `agents/oop-refactor-implementation/`，合计178行，到 EOF。已读根 AGENTS.md 与 docs/principles.md。生产源码绑定 b76ece3；合并树与该提交的 apps/packages/scripts/.github 无 diff。本次只新增记录，无产品修改、Git写、测试或编译。

正式ADR-0017实际受理/偏序、ADR-0025公共日终存档、ADR-0026个人经历规则优先。保行为owner重构不等于新实现分析能力、内存组织或完整运行验收；历史静态APPROVE与SHA不冒充当前测试结果。

## stream 实施记录：全部条款族

| 原文条款族/位置 | 当前caller→owner→consumer | 判定与反证 |
|---|---|---|
| 范围/依据3–9：A01 coordinator、R2-N08 admission、交易语义不变 | continuous transaction `session/pipeline/continuous_tick_transaction.rs:227`、auction transaction `auction_tick_transaction.rs:210`→drive_stock_stream；`ready_ingress.rs:108`→admit_ready_batch | 两目标进入当前真实pipeline，不是仅测试调用；未新增价格、数量、费用或T+1规则。 |
| owner表11–16：Coordinator七方法 | `stock_stream.rs:284`创建owner→`:285`enqueue→`:286`drive；方法`:314`/`:323`/`:372`/`:396`/`:408`/`:419`持available/pending/in_flight/channel | owner已实现且生产接线。不是因为有类型名就判完成，调度/回收由同实例消费。 |
| owner表11–16：AdmissionPlan五方法 | `local_admission.rs:75`prepare→`:79`run_stock_gates→`:80`finish_layout；prepare建立资源边/quote边；`:85`私有批内owner | 短批`:68`、无冲突`:76`快路径保留；只调度候选，不独占账户余额或最终结算。 |
| 生命周期18：payload先通知、过时PlanRoot、不搬finish | `stock_stream.rs:364`send成功才`:365`notify；`:440`PlanRoot分支只回调并continue，不读payload；`:396`才取完成股票；`:430`in_place_scope | 实现存在。通知失败忽略是已放弃私有tick的生命周期处理，不是吞掉已提交成交事实；channel无payload/重复book在`:401`/`:403`明确fatal。finish/Settlement仍在原transaction层，职责分离不是漏接。 |
| 生命周期20：批内边/observe/receipt顺序 | `local_admission.rs:101`逐candidate先observe，后构建偏序；`:223`gate、`:265`追加实际gate arrival边、`:274`拓扑布局 | 本地失败可能已有receipt推进，原文明确保持；权威tick由candidate事务失败回滚。不能给不同来源添加额外固定成交优先以满足旧自由调度字节要求。 |
| 验证22–35：characterization/损坏路径/filter/4worker/10秒 | `stock_stream.rs:549`通知缺payload、`:571`断开、`:587`过时root、`:613`重复完成、`:635`payload断开、`:651`drop；`local_admission.rs:635`cycle、`:680`混合phase、`:732`receipt overflow | 测试源码确实后续落盘，不把原记录“尚待统一执行”当仍没有测试实现；当前未执行，不宣称绿灯。错误case和测试先写历史不能由最终源码证明。 |
| 未完成门禁37–40：AuctionTickBoundary/主代理编译复核 | `auction_tick_transaction.rs:230`capture→`:231`finish_auction_shards；`stock_stream.rs:180`接boundary；auction_tests使用for_test | boundary caller已接，非现行漏实现。原“底层拆参数”是接口细化，仍一次终结；编译/独立复核属于历史验证债，后续报告可证明其当时结果，本次不重跑。 |
| 后续六文件42–55：state/getter/非法fixture迁移 | 六文件全部现存于`session/pipeline/`；Account test-only setter、Position restored parts及ParentPlan getter保留非法fixture；原Snapshot DTO未要求隐藏公开事实 | caller改成owner路径是必要适配，不能把缺少生产setter认成功能删除。`Account::fixture_set_strategy` `account.rs:167`直接调用restore_strategy；`Market::fixture_set_last_price` `market.rs:270`为test-only直接赋值。 |
| 编译02修正57：四处Position私有字段遗漏 | `pipeline/decision_resources_tests.rs:299`/`:488`/`:546`/`:623`均from_restored_parts→fixture_insert_position | 四处后续修正已在当前源码，不再登记历史编译错误为现行G；未自行重编译。 |

## caller-review：全部章节与负向fixture

| 原文位置 | 当前判定/证据 |
|---|---|
| 元数据1–4、结论6–12 | 追加10文件旧diff范围，不冒充全Session复核。唯一生产player capture `session/player_candidates.rs:10`通过mem::take取`state.pending_player`；真实caller `pipeline/ready_ingress.rs:41`接candidate composition。take一次移出保全队列顺序，没有capture时分配OrderId/seq或撮合。当前并发受理仍由就绪偏序决定，FIFO捕获不等于跨账户固定交易优先。 |
| 非法fixture14–19：None strategy/首错/poison/溢出/state-only撤单 | Account setter `account.rs:168`直接restore_strategy，AccountBook get_mut写边界失效缓存（`account_book.rs:131`）；Market `market.rs:270`直接赋值可保留i64::MAX溢出fixture。`session/failure.rs:20`require_healthy→`:80`poison_failed_step→`:85`save拒绝；没有自动默认strategy/clamp。测试迁移不新增容错能力。 |
| envelope fixture20：重复identity/名义费用/实收历史/拒绝ledger不变 | 当前`session/envelope_projection.rs`及`envelope_projection_hydration_tests.rs`保留escrow事实验证；nominal fee与seller charged debt分开是ADR-0017真实部分成交费用语义，不能把二者强行相等当漏实现。 |
| 执行与绑定22–38：未亲跑/compile05告知/10文件SHA | 原文明确只读，告知不是亲跑；10项content绑定属于历史文件版本。本次按当前b76ece3查production，不重写原SHA、不将测试计数51/190当语义充分覆盖证明。 |

## core-review：全部章节与后续发现闭环

| 原文条款族/位置 | 当前证据与判定 |
|---|---|
| 引用/范围1–6、结论8–11 | 当前`session.rs:1126` facade poison、`:1131`state、`:1135`CommittableSessionState；`:1335`clone_for_shadow、`:1340`commit_from真实调用，无Deref恢复字段暴露。N04测试补齐见后文，不因原审查提出覆盖缺口就重复列G。 |
| 大A语义15–21：时钟/费用/个人成员/依据 | 时间投影owner仍在`session/observation_clock.rs`；账户/Market字段迁移保留T+1与单位。personal participant仅已有信念机构，attention全NPC仍在；G07散户能力装配漏接不能被本次“没有默认补信念”核销。官方依据沿用既有登记，本轮未联网扩时效。 |
| 必要性23–29：state+COW/四map/相关owner适配/observation迁移 | `session.rs:1328`shadow重置facade hooks，`:1340`只提交state；`account_book.rs:69`clone_for_shadow保留生产strategy校验。个人四字段从participant投影存档`session.rs:2665`至`:2688`，restore`:2962`直接构造participant。不存在四份第二mutable authority。RootReadContext已有独立owner但历史PlanBook复制G16仍在，不能因clone_for_plan_roots旧helper删除核销。 |
| 边界31–39：hash/scheduler/RNG/restore/take-install/checkpoint | `session/hash.rs:141`information→`:149`belief→`:157`watchlist→`:165`price_memory各BTree投影，`:118`scheduler收集排序；`personal_state.rs:69`take、`:90`install拥有一次root私有状态。restore先`session.rs:2720`validate，再`:2962`装配，`:2977`holdings reconcile不改memory。日结失败`session.rs:2088`checkpoint→`:2094`恢复，未新增panic捕获。已有memory容量/淘汰漏接仍见sweep21 S21-C01，不因聚合owner等价核销。 |
| 发现41–49：空机构fixture无法证明非空COW/两机构/duplicate | 该发现后续补测试，当前`personal_state.rs:132`两机构fixture，`:139`take/修改四成员/install→authority两机构与shadow另一个完整JSON不变；不是只有空map测试。 |
| 补测51–56：四case+一partial case | `personal_state.rs:202`逐四SaveSlot map比较与hash；`:229`duplicate attention先错、`:242`duplicate participant watchlist错；`:259`partial-write边界case，`:287`catch_unwind仅测试。生产没有新增panic捕获；原部分install写序仍保留。 |
| 保留证据债58–60：混合人口/旧基线hash | 原文明确未强制新增混合人口case，且restore hash不证明旧基线hash。`tests/extraction_replay.rs:3`当前受控单worker重放与后续锚说明可复用，但本轮未跑旧基线comparison。混合散户/游资attention-only既有构造保证与G07当前正式能力承诺须分开，不用测试豁免取消产品能力。 |
| 执行/内容指纹62–83：mut getter cfg(test)、AccountBook、11文件SHA | getter仅测试不等于生产无更新路径：`PlanPersonalState`公开于session内部的四私有字段由root独占修改，再install。AccountBook页cache失效/fixture事实保留。11项SHA绑定不扩大63项manifest范围，本轮不宣称全部manifest已审或编译通过。 |

## 残余反证与总账结论

- “stream owner仅声明、ReadyAdmissionPlan无production caller”被真实continuous/auction/ready_ingress路径反证，新增候选0。
- “N04非空个人owner没有隔离测试”被personal_state两机构完整四成员COW与restore逐map测试反证；原静态发现已闭合，不重开。
- “编译02 Position私有字段写漏仍存在”四处已经迁from_restored_parts；不按旧编译日志登记当前缺口。
- receipt observe后失败、通知接收器关闭弃私有工作、duplicate install部分写入均为明确保留的内部失败语义，不是已提交市场静默丢事实。保留完整交易回滚/首错语义，不能改成来源固定排序。
- 后续承诺主要为统一执行/独立审查/旧基线hash兼容证据，属于验证范围；本轮未运行测试，不宣称新绿灯。
- G07、G16、G39保持原分类；sweep21 S21-C01个人价格记忆未prune及恢复capacity检查仍成立，本批不新增重复条目。旧Q无分类变化，未启动未来产品。
