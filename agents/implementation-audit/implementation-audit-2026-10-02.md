# 历史需求到生产代码的实现审计（2026-10-02）

## 1. 基线、范围与判定

本报告核对历史需求与生产实现，补充并优先于 [旧缺口盘点](../../docs/implementation-gaps.md) 的完成度描述；保留旧证据，不把 A01–A11 整项重开，也不将审计视为实现授权。

当前固定产品基线为 `c0ab429`；按用户最新要求在新建的 `.worktree/implementation-audit-final`、`codex/implementation-audit-final` 分支续核，不等待也不带入主工作区正在修改的未提交产品或正式文档。`c0ab429`仅在`43b1aa5`上更新审计，产品代码与此前基线相同。原1110来源与各阶段记录保留自己的内容指纹；本轮追完此前未核实的恢复边界，不把旧PASS或HEAD相同当作所有工作树当前字节相同，也不将本审计说成覆盖后续并行改动。

此前234个来源路径包含229个跟踪Markdown、3份历史草稿及2份删除文档最后版本；其指纹在当前工作区未变，但这个集合不完整。使用包含 Git 忽略文件的清点后，又找到 `agents/oop-refactor-audit/` 下857个来源路径（681份不同正文），另有1份遗漏的 build 状态记录；`git log --all` 还发现18份仅在历史分支可达的文档。扩展集合共1110个来源路径、933份不同SHA正文，逐路径指纹、正文别名和批次映射见 [补扫路径清单](hidden-review/path-index.json) 与 [历史版本清单](hidden-review/history-plan.json)。新增227批、6批历史记录及1份build状态均已完成逐源EOF与指纹校验，待补读、缺少凭证及指纹不符均为0；统一映射见 [完整来源索引](hidden-review/expanded-source-index.json)。这个完成范围不包括未入集合的外部来源或未知程序缺陷。

按用户指定，扫描任务使用独立 `gpt-6-luna medium`，每个只负责1–3篇来源，连续全文读至EOF；截断处补读，完全同SHA正文共享阅读但保留全部路径。原234份已映射80份阅读记录；新增681份正文分为227批，历史分支另分6批。曾尝试50个reader并发，遇到429与文件描述符耗尽，失败批次保留待读并由原reader重试，服务恢复后错峰增加并发，不以换模型或截取摘要绕过全文要求。每章承诺归入生产已接、现行缺口、待定契约、未来/取代、文档漂移或验收证据边界，并反查当前caller、state owner与consumer。第2、3节给出已裁定状态；逐篇证据见 [覆盖清单](coverage-index.md)、[原来源清单](exhaustive-review/source-index.json) 及 [原裁定记录](exhaustive-review/resolution.md)。旧R/S/H记录保留自己的基线，不改写历史验收。

“生产已接”仅针对所列契约，不保证整模块无缺陷。确认缺口、待定需求、未来范围、文档漂移与验收证据分别登记，不用未勾选框、旧符号消失或纯函数测试证明生产功能缺失或完成。静态审计阶段没有修改游戏代码，只做源码差异与文档静态核对，未运行游戏测试、构建、浏览器、完整回归、性能矩阵或发布流程。后续按用户授权在同一 worktree 实施，逐项状态和短测证据写入对应行，批次记录见 [补缺实施](../implementation-gap-implementation/README.md)；不改写此前静态审计的验证范围。此前发布脚本与工作流契约的4个定向短测文件通过，属于上一基线复核的结果，实施阶段没有重跑，不将其他任务的验收记录写成本次通过。

本次未重新联网核验制度，沿用文档中登记的 A 股规则及简化；金额为分、数量为股，界面手数仅作换算。DCF和个体策略参数是游戏模型，不冒充交易制度或真实市场校准。本批完整diff已由非作者独立复核，三项门禁通过，见 [覆盖与复核记录](coverage-index.md#独立复核记录)。这是指定基线和文档集合的静态审计，不是程序没有未知缺陷的保证。

### 最新决定优先

ADR-0023：开局前虚拟历史，内部 day=0 起实际撮合，不使用真实行情校准。
ADR-0024：现金池可以减少，不补钱、返费或保证成交。
ADR-0025：仅成功自然日日结保存，启动/明确换档才读档；内部候选回滚不是公共日内存档。
ADR-0026：机构个人阈值、真实经历、暂停买入和本人观察恢复，不自动强卖。
ADR-0019：当前单局重点，不恢复任意订单条数配额。
ADR-0027/0028：运行时宿主选择、无 Node 部署、七个手动入口、有效标签 Release/Pages；普通 commit/PR 不自动 CI，签名暂不做。2026-10-03最新决定进一步明确发布仅构建、打包、核验和部署；CI仅保留为手动开发诊断，发布及产品构建不运行测试、lint或smoke。
ADR-0018 整体仍为 proposed；只有已被后续接受的具体实施目标才列为现行缺口。

## 2. 已确认的现行缺口

静态审计确认79项：原G01–G39除第5节核销的G27外有38项，此后G40–G68共29项、G69–G79共11项，Q19纠正恢复可达性判断后转G80。实施阶段已补齐 G06、G15、G54–G55、G69–G71、G74–G78、G80 共13项，剩余66项；以下保留编号、原需求与证据，并在对应行登记修复，不把历史发现删除。G78包含同一恢复owner的未来待办日期边界；BeliefBook的双profile身份候选仍保留Q25，不虚构必需全等契约。分类及拒绝项见 [扩展裁定](hidden-review/README.md) 与 [固定基线续核](renewed-check/README.md)。未来产品、仅未运行的验证和主工作区半成品迁移不列为本分支漏实现。
“未接线”指模块/类型可能已有，但生产路径没有完成承诺；“行为错误”不能靠补一个空接口解决。

### 2.1 宿主与远程链路

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G01 | 浏览器远程 HTTP/WS 可用且鉴权；ADR-0005 §6、ADR-0027 | 浏览器 WS query token 与服务端 header 认证仍不一致；`remote-request.ts` → `remote-host.ts` → Server `routes.rs`。 修复须保留鉴权，不得通过开放私有路由绕过契约。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G02 | 三宿主实际倍率；ADR-0005 §5、UX-CONTRACT 模拟控制 | Remote `readSpeedMetrics` GET 仍不附凭据；真实 UI 轮询迁至 `useSpeedMetricsPolling.ts`。  当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G03 | remote push/pull 都可持续取帧；ADR-0005 §6、ADR-0010 | Remote 切 pull 后仍没有 `GetFrame` 发送循环。 服务端测试手动拉帧不证明浏览器适配器已接线。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G04 | baseline 仅初始化、读档、显式重同步；ADR-0010 §统一更新 | `App.tsx:362` 暂停后继续再次 `host.start`，Remote/Tauri 重送旧 baseline；Worker 的 generation 守卫原已存在。 缓存不随 delta 推进，重新交付可能回退状态及游标；这里的继续不是从磁盘读档。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G05 | 心跳失活处理与重连；ADR-0005 §6、ADR-0010 宿主能力 | Server 无 Pong 截止判据，Remote 意外 close 仍转 fatal 而非自动恢复；换刷新模式后旧socket的迟到onerror缺身份守卫，会调用共享fail并关闭新连接。手动start可以重建连接，但不等于自动恢复或旧连接事件隔离。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G18 | Worker uiFrame 背压；ADR-0010 宿主能力/不做 | `WasmTickLoop.publish` 每步发完整更新，仍无消费者驱动的有界背压。 定时让出执行机会不等于消费背压；任何合并不得丢弃协议要求保留的提交帧及交易事实，未实测卡顿。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G19 | Tauri 固定高倍率 tick 在 Rust 聚合后约 16ms 发布；ADR-0010 | Desktop 固定倍率仍 `run_cycle(1)` 后直接 emit；Fastest 批次不等于固定倍率聚合。  当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G40 | 三宿主有副作用控制命令等待宿主确认；ADR-0010:60 | `EngineHost.start/stop/setSpeed` 为void，App暂停/继续立即更新running和成功提示；Desktop `set_running/set_speed/set_pause_preferences` 仅mpsc入队，IPC返回不证明actor已应用。订单CommandQueued仍只表示入队，不能改成成交确认。见 [宿主全文](exhaustive-review/luna01.md) 与 [actor](exhaustive-review/luna62.md)。 |
| G53 | 恢复响应必须推进generation；历史task30:26–27 | `worker-host.ts:79` 的restore helper只验正安全整数，load直接采纳nextGeneration，缺大于原值的守卫。真实WASM绑定正常自增已实现；这里是异常响应校验遗漏，未声称正常恢复失败。见 [Worker](exhaustive-review/luna29.md)。 |
| G66 | 已接受Remote请求保留完成或显式错误出口；ADR-0010、错误处理原则 | dispose忽略后续消息且不reject已登记写请求；刷新单槽waiter可被内部ResyncRequired覆盖，遗失读档恢复链原await。UI交易及保存hook真实等待这些Promise；确认中断应说明结果未知，不能假称请求必未执行。见 [Remote](exhaustive-review/luna60.md) 与 [调用](exhaustive-review/sweep61.md)。 |

### 2.2 策略、个人信息与估值

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G06 | 已补齐：五年权益现金流折现；公司计划 K5/K5a，`docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md:150` | FCFE每期按对应年数折现，终值仍折现5年；逐步整数半偶到分。恒定FCFE=17,820,000分、r=10%、g=gt=0，独立逐期推导及修复结果均为178,200,000分；悲观/乐观与真实披露→个人信念gold同核。原证据见[策略与公司](reaudit-engine.md)，修复短测与独立推导见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G07 | 身份不决定分析能力，散户可分析基本面及五路权重；公司计划 K5、任务17/26，ADR-0016 | 基本面认识和五路信号仍只装配到机构链；Retail 仍走 ZiNoise/retail 内核。 不是要求所有散户都估值，也不是赋予全部散户机构执行复杂度。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G08 | 失败日期与每20交易日无新受挫衰减；ADR-0013 计划契约修订、公司计划 K5 | 散户观察/成交仍走 legacy writer，dated writer/衰减没有进入对应消费链；不重做已有机构衰减。 不能用已有机构 ADR-0026 衰减核销散户链路，也不应重复实现机构衰减。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G09 | 本人已知完整年报及可用中期更新；公司计划 K5a:148 | `decision_chain/roots.rs` 仍只将年报投递到信念更新，中期补充链缺失。 中期材料须作为补充或修订，不能直接当全年报告。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G20 | 新局从熵取种、测试可固定；ADR-0005 §4 | `useSessionHostLifecycle.ts:90` 普通新局仍使用固定 `DEFAULT_SEED=42n`。 存档 RNG 和固定 seed 测试注入已存在；不要求自由并发同 seed 整局字节一致。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G42 | 个人价格记忆按持仓∪活跃计划＋8修剪，恢复验证同一边界；K5、任务19/25 | `roots.rs:486` 仅prune watchlist，PersonalPriceMemory::prune无生产caller；恢复以全市场股票数＋8设限且条目必须属于全市场，未验证未保护条目最多8个。默认5股不展露此边界；个人认知上限不属于被撤销的世界配额。见 [个人记忆](exhaustive-review/luna21.md) 与 [计划](exhaustive-review/luna03.md)。 |
| G43 | 淡出股票不再自动获知/分析/建立新计划；K6候选范围、任务25 | `root_candidate_codes` 无条件加入所有belief.entry_stocks，关注驱逐后无持仓/活动计划且未重新发现的旧股仍进入观察、报告获取和新计划候选。缺本次候选资格过滤，不能删除本人历史已知材料来冒充淡出。见 [生产root](exhaustive-review/luna03.md)。 |
| G69 | 已补齐：计划恢复保持非零且可表示的期限；输入校验原则、历史strategy D02 | 开户、TradingPlan serde、PlanBook与完整Session validator共用期限校验；先减一再求最后有效日，MAX/1合法，零期限及越界显式拒绝。新增直接API及完整SaveSlot定向短测，不改变A股订单有效期。原问题见 [期限原文复核](hidden-review/batch-068.md) 与 [裁定](hidden-review/candidate-resolution-01.md)；实施证据见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |

### 2.3 行情显示与日历边界

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G10 | 连续竞价一分钟成交量；DESIGN:88、UX-CONTRACT:47 | `market-chart-projection.ts` 仍以每帧累计量差替换分钟量，而非累加该分钟量。 同分钟累计100→200→200最终会只留0；首个连续点还可能带入竞价累计量。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G11 | 新日清旧分时且保留新日已到采样；UX-CONTRACT:81 | 普通TickBatch分时合并只按日内槽位；CivilUpdate虽重建历史，相邻交易日AfterClose＋BeforeOpen屏障仍携带旧日全帧，新日未覆盖槽可能保留旧日点。纯空屏障和初始化清理已有，未完成的是正常相邻交易日隔离。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G12 | 分时量涨红空心、跌绿实心；UX-CONTRACT:54 | 连续点方向固定 true、竞价按非空判方向，涨量柱仍非红色空心。 这里审计价格涨跌展示，不把 buy 字段称为真实主动买卖方向。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G13 | 逐笔展示最近成交；DESIGN:126、移动QA | `MobileIntradayProjection` 仍对最新优先成交数组取 `slice(-7).reverse()`。 100条成交带不是全天流水，不得以无限积累修补方向错误。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G14 | 逐笔时间对应真实成交；DESIGN:126 | 逐笔仍共用当前 `tradeTime`，不取各笔成交时间。  当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G15 | 已补齐：官方年度覆盖替代模拟回退；`docs/simulation-calendar.md:59` | 对存在Official覆盖的交易所/年度，非周末且不在覆盖休市区间的日期为Trading，不叠加模拟假日；其他交易所/年度保留fallback，周末不开放。合成Fixture短测覆盖上述边界，默认空official表不变。原证据见[基础](reaudit-foundations.md)，验证见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G75 | 已补齐：日内时刻恢复与构造器使用同一秒域；`CivilInstant`输入契约 | serde经过CivilInstant::new校验，0与86399合法，86400及u32::MAX拒绝；公开秒域不再可由私有字段派生serde绕过。原问题见 [原文复核](hidden-review/batch-139.md) 与 [裁定](hidden-review/candidate-resolution-04.md)；短测见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |
| G76 | 已补齐：冻结日历政策内容身份包含官方出处摘要；政策来源绑定契约 | policy content digest新增独立source_digest分量；只修改出处摘要即改变身份，旧digest与新摘要组合在validate拒绝。默认official空表身份不变；测试使用合成出处，不使用真实行情。原问题见 [来源审读](hidden-review/batch-156.md) 与 [裁定](hidden-review/candidate-resolution-04.md)；短测见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |
| G44 | 零成交量如实为零；DESIGN:88/110、移动QA:3 | `market-model.ts:693/772` 的分时/K量对0执行Math.max(1,...)，组件画出正高度量柱。应保留真实零量槽位；非零量最小可见高度不应套到零值。现有测试冻结正高度不核销真源契约。见 [绘图](exhaustive-review/luna02.md)。 |
| G46 | 桌面五档标签对应真实报价rank；DESIGN、UX盘口 | engine asks按低价优先，`LocalRefreshViews.tsx:113` 却把asks[0]标卖5，一档时也标卖5；移动五档映射正确。缺桌面档号/展示顺序一致性，不改撮合。见 [盘口](exhaustive-review/luna02.md)。 |
| G47 | 竞价null指示价槽不绘价格线；DESIGN:88、UX:50 | 投影先过滤null，再拼一个polyline，两个有效价之间的null槽被直线跨越。整段null空态已有，缺的是连续有效片段分隔。见 [竞价绘图](exhaustive-review/luna02.md)。 |

### 2.4 工程、交互和发布

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G16 | 普通tick不随多年历史线性复制；`docs/superpowers/plans/2026-09-24-single-world-multithreading.md:60` | 全历史PlanBook复制仍在RootReadContext::capture；普通shadow另深拷贝ClosingEngine版本/重述及PublicLibrary报告/公告。Arc封装不消除原副本。缺所有权优化，不删历史、不强定COW/WAL；未测幅度。见 [当前历史owner](exhaustive-review/luna10.md) 与 [策略](reaudit-engine.md)。 |
| G17 | Rust指标与Rayon生产加速；ADR-0008 D2 | Rust 指标已有，三宿主仍只调单项函数；Rayon batch 仍仅测试调用。 缺批量生产接线，不把指标功能整体重开；是否值得对现有负载并行须先测量。 当前路径、行号及调用链见[基础](reaudit-foundations.md)。 |
| G21 | 基线CLI只取setup、不验证其余档字段；`docs/price-volume-simulation-gap-checklist.md:265` | `price_volume_baseline.rs:29` 仍完整反序列化 SaveSlot 后才取 setup。 工具输入投影与公共 SaveSlot 深度验证是不同契约，不应放宽业务读档校验。 当前路径、行号及调用链见[工具](reaudit-tools.md)。 |
| G22 | 表单即时/字段级错误；`docs/error-handling.md:114` | 表单错误迁至 `useTradingCommands` 后仍只有全局 notice，缺字段关联。 错误并未静默吞掉；缺的是即时、字段级反馈。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G23 | 进入详情聚焦返回、返回聚焦原列表；UX-CONTRACT Flow ledger | 详情进入/返回仍只变状态，没有对应导航焦点恢复。 交易底页焦点管理不能代替详情导航；后续需浏览器短验收。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G24 | 信息标签切换保持滚动；UX-CONTRACT:69 | 信息 tab 仍调用 `scrollIntoView`。  当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G25 | 返回/切股至少44px点击热区；DESIGN:86 | 返回/切股横向点击区域仍小于约定 44px。 可见图标可以小，但热区应满足契约；本轮未做像素测量。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G26 | 手动开发CI的前端warning作为错误；`docs/tech-stack.md:23` | Web lint 仍裸 `oxlint`，未设置 warning 失败门槛。 Rust -D warnings 已有；仅指手动开发CI，不要求恢复普通 commit/PR 自动运行或发布链路lint。 当前路径、行号及调用链见[工具](reaudit-tools.md)。 |
| G45 | 自选详情返回原列表身份；UX导航/Flow ledger | `mobile-ui-state.ts:71` 打开详情强制primaryTab=market，back只清detailCode，自选返回后变为行情。列表内部缓存存在不能替代原主页面身份，独立于G23焦点。见 [导航](exhaustive-review/luna02.md)。 |
| G48 | 中文页面语言与辅助文本一致；UX:8/10、ADR-0007 | HTML固定lang=en且无运行时修正；AG Grid sortable表头未配置locale，实际使用英文排序辅助文本。两处均需对应中文界面，不能用中文列名核销内置提示。见 [页面](exhaustive-review/luna02.md) 与 [Grid](exhaustive-review/sweep41.md)。 |
| G49 | 持仓成本及浮盈承接半偶到分语义；account spec:23/63 | Rust cost_price半偶到分，Web原始除法/toFixed显示成本并以未舍入净投入算浮盈。200股、净投入200100分、现价1001分时，Web显示成本10.01元/浮盈1元，Rust成本10.00元/浮盈2元；是小额跨层模型漂移，非Q01安全整数争议。见 [账户消费](exhaustive-review/luna15.md)。 |
| G50 | React渲染异常进入可见详情/反馈出口；错误处理规范 | `render-app.tsx` 无ErrorBoundary或root错误callback；main异步启动catch不覆盖后续React render/layout-effect异常，宿主fatal出口也不覆盖UI自身异常。见 [错误链](exhaustive-review/luna14.md)。 |
| G51 | Desktop释放失败显式上报；错误处理规范 | `tauri-host.ts:165–175` 清fatalCallback后丢弃stop_session的Promise，dispose调用方无法接IPC rejection。unlisten运行时行为是版本限定补充，结论仅依明确stop_session失败出口。见 [释放](exhaustive-review/sweep14.md)。 |
| G52 | 协议断言保留已知actual/expected上下文；错误详情规范 | 前端generation/cursor不一致仅生成通用ProtocolError文本，coordinator没有补上已知实际值/期望值，复制反馈缺复现事实。三宿主真实engine fatal已有context，不泛称全部错误无详情。见 [协议错误](exhaustive-review/luna14.md)。 |
| G54 | 已补齐：Money公开解析拒绝完全无数字输入；Money spec/Task4 | Money::from_yuan_str显式拒绝“.”、“+.”、“-.”及带空白形态，保留“.5”“12.”等含数字的既有合法输入。影响限定公开库API，不冒称UI/存档此前已接受。原证据见 [解析](exhaustive-review/luna18.md)，短测见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G55 | 已补齐：公开Strategy构造/工厂统一拒非法参数；策略spec/防御原则 | Momentum构造器拒非有限阈值，params.validate与Factory沿用同一入口；机构原始margin在采样/钳位前校验，零tick日显式InvalidParam。新短测验证非法参数拒绝、margin/零tick不消耗RNG及合法三类实例可建。原证据见 [策略入口](exhaustive-review/sweep20.md) 与 [工厂](exhaustive-review/sweep19.md)，实施见 [策略输入](../implementation-gap-implementation/strategy-input.md)。 |
| G56 | Pages区分owner根站点和项目路径；ADR-0028 | distributions仅看repository.name的.github.io后缀，没有与owner匹配；非owner同后缀项目被误编为根路径。当前stock_market_game不触发，不推翻其发布验收。见 [Pages](exhaustive-review/luna12.md)。 |
| G64 | 桌面行情选股有键盘等价入口；UX/设计辅助功能 | MarketGrid仅onRowClicked选择，suppressCellFocus禁用单元格焦点；移动原生按钮在桌面隐藏。静态缺选股入口，不声称所有键盘操作失效或已跑浏览器。见 [桌面Grid](exhaustive-review/sweep41.md)。 |
| G65 | 主导航/行情分类选中状态程序化公开；UX辅助功能 | 主导航与行情分类只有active CSS，没有向辅助技术提供当前/选中状态；图表/信息tab已有aria-selected。具体角色及属性由实现按组件语义选，不硬指定错误role。见 [状态](exhaustive-review/luna02.md)。 |
| G67 | 外部baseline持仓必须有行情，估值缺项显式失败；防御/资产契约 | baseline解析未校验持仓代码属于markets，portfolio selector与组件以缺价??0计算零市值并低估资产。runtime-delta账户更新已有引用守卫；缺口限定不一致外部baseline与估值fallback，不泛称所有协议漏验。见 [资产](exhaustive-review/luna15.md)。 |
| G68 | 合法非默认局证券使用当前setup交易规则/选项；存档编辑、交易规则一致性 | 读档安装activeSetup，快捷涨跌停仍查DEFAULT_SETUP，下单选项仍取STOCK_LIST，非默认证券缺规则/选项。不同主板/ST类别未必改变现行限价，不能用“同code改category必算错”作证；缺当前配置消费。见 [表单](exhaustive-review/luna02.md) 与 [裁定](exhaustive-review/resolution.md)。 |

### 2.5 补充逐章核对发现

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G28 | 固定集团合并报表进入完整交付；ADR-0016:97、公司计划K3/任务12–13/完成条件 | 日终仍发布Standalone，未接固定集团合并公开链；等额往来申报仍不保证账面上界；跨行业完整合并产物还按原始科目码汇总/联合分类，保险与工商/地产同码异义会触发DuplicateClassification拒绝报告。 合并算法和五产物测试已有；单层等简化不取消普通集团报告承诺，也不要求默认每家公司有子公司。Task12 F6要求调用方从真实账簿派生申报，现配对校验仅确认双方等额，不保证未超账面余额；重复成员对双抵及销售金额上界已修，不能混报或声称默认游戏已发生超额抵销。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G29 | 无NPC时跳过初始流通盘分配；initial-positions spec§2.4、计划Task3 | ByKind 正流通盘且零 NPC 仍在 setup 校验阶段拒绝，空 NPC seed 早退不可达。 这是可配置新局边界，不声称默认20,007户受影响，也不应凭空分给玩家。 当前路径、行号及调用链见[基础](reaudit-foundations.md)。 |
| G30 | 图表窗口按钮可见键盘焦点；DESIGN:82、UX-CONTRACT:56 | 图表窗口按钮仍使用未定义 `--msd-focus`。 按钮名称和 SSR 存在不能证明键盘焦点可见。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G31 | 图表只随当前股票历史刷新；UX-CONTRACT:88 | `MarketChartProjection` accessor 每批重建数组，当前股票未变化也更新引用。 Provider 拆分未保证数据引用隔离；未实测性能幅度。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G32 | 昨收中轴与0.00%位置一致；DESIGN:90 | 昨收中轴与图形中点仍使用不同高度坐标系。 当前样式推导中两中点相差时间轴23px的一半，未做浏览器像素验收。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G33 | 移动端字号随容器宽度响应；DESIGN:64/127 | 后置 px 字号仍覆盖关键报价响应式规则，不是全部页面不响应。 局部响应式能力旧基线已存在，不算本次修复；320/390/430px仍需代表性验收。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G34 | 交易底页遵守减少动态效果偏好；DESIGN:104、UX-CONTRACT:95 | reduced-motion 规则仍未覆盖详情外的交易底页。 本轮未运行浏览器观察动画。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |


### 2.6 公司与计划生产闭环补漏

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G35 | 工商折旧、所得税及跨行业商业债务支付进入经营闭环；公司计划K3/任务8、14 | 工商折旧/所得税/商业债务支付调度仍未补齐；银行DEP本息结付、地产借款支付handler也缺经营caller。合法零额月折旧还会生成被拒的零金额凭证，子账月份不推进。已有利息计提、月年封账不重开。 开局已有固定资产寿命与商业借款；缺的是相应处理与支付，不是股东分红或给投资者补钱。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G36 | 四行业可自定义并经公共报告查询跑通；公司计划K3:104、任务26 | 新局仍固定 Industrial，非工商 `books_mut` 不支持；行业不适用冲击仍进入 active 至公告链。 银行/保险/地产经营及报告内核已有，不等于会话装配/封账/披露闭环；不声称默认工商局触发非工商 panic。sample_company_shock 不接收行业类型，银行/保险可能得到不消费的 ProductionInterruption/AssetImpairmentSignal；披露器直接读 active 状态按开始日期公布，丢弃日报告并不能阻止公告。获批只记录的 CreditDeterioration 另列，不视作同一缺口。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G37 | DEV因果记录关联实际订单ID和计划变化；公司计划任务35 | DEV 根诊断仍传空交易 events，无法关联随后真实订单 ID/计划变化。新增诊断查询只读/重复查询稳定测试未补上真实订单关联；DEV入口和隔离已有，不得伪造成交或向产品快照泄露私有状态。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G38 | 本人预算区分已有计划续行与新机会；公司计划K6:165/任务22 | 两个生产预算请求仍全标 ExistingPlan，新机会优先级未获实际分类输入。 仅是同一账户软预算分类，不得扩为跨账户撮合优先；AllocationExperience 默认值不单列缺口，避免重复扣减上游已处理的个人信心。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G41 | 真实支付失败/逾期状态持久并公开风险材料；K2:92、任务8/14 | 经营层生成CompanyDayReport.payment_failures，但`session.rs:2105` 日结caller丢弃返回值，权威状态/存档/公开公告不消费失败记录。合法现金不足继续经营已有；缺的是事实保留/获知，不补钱或造违约。见 [日结](exhaustive-review/luna03.md)。 |
| G58 | 每贷款人独立授信按对应未偿校验；公司商业合同 | IndustrialBooks::borrow/available_credit取全部loans.outstanding_total，却与指定lender额度比较；A/B各授信1000，A借满后B借1也被拒。默认单lender不触发；这是合法公开工商API边界，独立于G35支付调度。见 [授信](exhaustive-review/luna31.md)。 |
| G59 | 有明确保障期限的保险不在期后新造事故赔案；K3/公司会计 | `operations/insurance.rs:68–75` 只以remaining门控服务释放，赔案日程没有coverage_end门控，合同结束后仍周期创建并立即支付新claim。保障期内已发生未付赔案期后支付仍应允许；该经营边界不因G36缺会话装配而核销。见 [承保期限](exhaustive-review/luna55.md)。 |
| G70 | 已补齐：工商开局库存子账与对应总账逐科目对账；公开构造契约、历史company D01 | seed_inventory补核工业报表同真源的1403/1405/5001；存在但缺seed的科目按零子账余额对账，不平账补钱。custom chart没有对应科目不强行添加，其余seed仍逐项核。原证据见 [开局审读](hidden-review/batch-037.md) 与 [裁定](hidden-review/candidate-resolution-01.md)，短测见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G71 | 已补齐：经营调度器恢复保持唯一身份及序号耗尽显式错误；保存恢复纪律 | from_parts与serde拒绝跨日期重复ScheduledDueId，完整SaveSlot解码沿用同一守卫。submit先checked_add，MAX游标显式SequenceExhausted且队列/游标不变；MAX-1仍可分配，不混用A股委托ID。原问题见 [调度审读](hidden-review/batch-037.md) 与 [恢复裁定](hidden-review/candidate-resolution-01.md)；短测见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |
| G73 | 保险组反序列化保持经营不变量；外部存档校验原则 | `ContractGroupState`自定义Deserialize仅搬字段，InsuranceBooks/CompanyOperations恢复不补子账语义校验；既有behavior测试允许损坏released_revenue并观察后续释放先过账、再部分更新后报溢出。缺的是恢复拒绝不一致状态，不据此要求所有合法保险操作强事务，默认工商局与G36行业装配边界仍分开。见 [恢复裁定](hidden-review/candidate-resolution-01.md)；不等于ClosingEngine的Q17。 |
| G74 | 已补齐：P3预检失败后coordinator按既有契约停用；typed failure状态机 | Continuous及Auction身份预检错误调用既有fail锁存；两项短测验证失败后next_ready_batch与finish均拒绝，不改变正常撮合或宣称默认交易曾错序。原证据见 [预检原文](hidden-review/batch-141.md) 与 [裁定](hidden-review/candidate-resolution-03.md)，短测见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G77 | 已补齐：合并往来金额与工作底稿保持既有恒正契约；DTO及公开消除API | precheck_balance拒绝非正声明并携成员、对手方及金额；两种申报顺序均拒零/负配对，真实AR/AP正额仍生成1条底稿，成员账套不变。独立于G28集团接线、账面上界及行业分类。原证据见 [历史对照](hidden-review/batch-175.md) 与 [正额裁定](hidden-review/candidate-resolution-05.md)，短测见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G78 | 已补齐：自然日时钟恢复保持到期业务身份、耗尽错误及日期适用范围；CivilClock保存契约 | from_parts拒绝重复DueBusinessId并按冻结policy查询每条pending日期，完整SaveSlot恢复进入同一守卫。注册先checked_add，MAX游标显式DueSequenceExhausted且零变更，空队列游标0接受集合不变；与G71是不同owner。原问题见 [原裁定](hidden-review/candidate-resolution-06.md) 与 [日期续核](renewed-check/clock-bounds.md)；短测见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |
| G79 | 工商授信计算错误不得伪装无授信；显式错误与外部状态校验 | `available_credit`约定None为无授信，却用`.ok()`折叠贷款加总/减法错误；IndustrialBooks及完整公司恢复不深验贷款状态，外部档可注入无法加总的本金。缺明确错误出口及恢复不变量，不等于G58按lender归属计算，也不泛称正常借款必溢出。见 [公开API复核](hidden-review/batch-176.md) 与 [裁定](hidden-review/candidate-resolution-06.md)。 |
| G80 | 已补齐：完整存档恢复校验银行ECL政策；外部输入及行业政策契约 | validate_company_domain对Bank账套调用EclPolicy::validate，错误携公司ID与Bank ECL上下文；完整SaveSlot短测验证合法政策可恢复、两表空值/非法权重及PD拒绝。保持库级serde接受集合及issue_loan次序，不冒称G36四行业闭环已实现。原问题见 [续核](renewed-check/bank-restore.md) 与 [独立裁定](renewed-check/independent-review.md)；实施见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |

### 2.7 验收工具契约

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G39 | K7按现行并发受理契约验证；Sept24多线程计划:154 | K7仍比较不同worker完整artifact；after/sensitivity的baseline-run rerun与root verifier还要求自由重跑stdout SHA完全一致，均未固定同一实际受理轨迹。新增Session/规模测试已区分立即恢复等价与自由调度后各自对账，未修这些工具门禁。应保留守恒、价时、依赖及失败负控，另验同一受理事实的重放；未运行矩阵。 当前路径、行号及调用链见[工具](reaudit-tools.md)。 |
| G57 | 诊断输出潜在大u64无损十进制字符串；diagnostics/量价CLI契约 | causal_runs的seed、qty等字段直接Serialize为JSON数字，CLI包装未转字符串；合法u64大seed已能超JS安全整数。旧price_volume转换已实现，不能核销新增causalDTO边界；独立于Q01 Money范围。见 [序列化](exhaustive-review/luna13.md)。 |
| G60 | 性能工具通过现行启动选择进入游戏；性能README/正式命令 | 默认Vite为非e2e模式，启动只显示选择页；market-ui-report先等.app-root却没有确认启动操作，正式命令旅程不可达。修工具步骤，不改产品启动选择政策。见 [旅程](exhaustive-review/luna48.md)。 |
| G61 | 性能工具整体deadline及异常资源收尾闭合；测试/清理规则 | UI性能正式入口裸node、CDP挂起无整体外部期限，cleanup一处失败跳过后续；性能harness sampler提前reject却先等child close，未即时终止owned child。正常结束清理和失败不生成PASS已有，不核销异常树收尾。见 [UI工具](exhaustive-review/luna48.md) 与 [采样](exhaustive-review/luna64.md)。 |
| G62 | 进程外deadline终止整个嵌套进程树并诚实报告；AGENTS/testing | POSIX各层runBoundedCommand均detached，外层只kill直接PGID，孙进程另建组可逃离终止；事件循环正常计时不证明阻塞时收敛。Windows taskkill异步不验结果；run-with-deadline只请求kill或等直接child关闭，却称process tree已终止，hard timer还可先于close拒绝。需区分请求、已确认退出和未确认状态，不声称每次超时都留后代。见 [树监督](exhaustive-review/sweep64.md) 与 [文案裁定](hidden-review/candidate-resolution-02.md)。 |
| G63 | 普通Rust case独立10秒硬上限；AGENTS/testing | run-full-regression只给binary共享长阶段剩余期限与test-threads，没有普通case独立watchdog；最新验收记录也明确承认。整批PASS不证明逐case满足，ignored必要长用例仍按长验收分类。见 [runner](exhaustive-review/sweep81.md) 与 [发布复核](exhaustive-review/luna77.md)。 |
| G72 | 验收artifact先验证canonical containment再读内容；现存validator契约 | matrix `validateArtifacts`先realpath/readFile，再判断canonical路径属于output；新结果及PASS复用均调用它，拒绝之前已读越界目标。仅登记matrix读取次序，不把Pages开发smoke也按同类技术自动列缺口，不提供安全复现场景。见 [原文复核](hidden-review/batch-080.md) 与 [裁定](hidden-review/candidate-resolution-02.md)。 |

## 3. 候选项与契约冲突：不能冒充已确认漏实现

Q01–Q09、Q11继续保留；Q10已转G39，Q19经恢复链复核转G80并保留原编号追溯。Q12–Q25记录范围/规范冲突、错误优先级和条件边界；编号均为本审计内部ID，与docs/open-questions.md不是同一命名空间，不能把待定方向当作确认缺口。

| ID | 事实与证据 | 处理边界 |
|---|---|---|
| Q01 | Money 仍是 transparent i64 JSON整数，Web parseMoney 仍要求安全整数；配置只校验现金非负。当前边界见 [基础复核](reaudit-foundations.md)。 | 跨端支持范围未统一登记。需选择共同范围或无损编码，不直接要求删掉Web精度守卫，也不以默认金额小证明无风险。 |
| Q02 | 个人技术数据已用于候选，但 PersonalPriceMemory::record_public_history_read 仍无生产调用；根观察只记行情观察。当前链见 [策略与公司复核](reaudit-engine.md)。 | 确认未接调用，但应按“实际主动读取”而非每次共享缓存构建记账；实际消费边界需明确，不能伪造未观察经历。 |
| Q03 | 日历/会计文档要求冻结 RegulationProfile；当前 setup 与恢复均强制校验 simulation_policy_id。实现见 [基础复核](reaudit-foundations.md)。 | 未发现允许跨政策恢复却被覆盖的路径。不能仅因缺同名结构判缺功能；应明确ID与冻结规则集合的关系。 |
| Q04 | UX-CONTRACT 要求固定应用标题，useMobileUiController 仍在详情展示股票标题。见 [界面复核](reaudit-ui.md)。 | 这是文档/交互选择冲突，不擅自把当前标题行为认作交易错误。 |
| Q05 | 任务目录已有递归发现 scripts/**/*.test.mjs 的并行runner及历史32文件验收；根test和手动CI仍未接入该runner。待定的是正式持续维护入口，不是完全没有发现代码；获批测试helper不必接生产。见 [工具复核](reaudit-tools.md)。 | 验收工具持续覆盖策略需单独收口；不恢复已退役工具，也不把测试重新接入最新决定已排除测试的产品发布链。 |
| Q06 | 初始持仓 spec 要求 ByKind 比例和约等于1、类内随机；当前允许正有效权重归一，散户另作 eligibility/Pareto 分配。见 [基础复核](reaudit-foundations.md)。 | 容差及分布的最新批准依据未定位；先明确当前政策与旧spec关系，不要求为旧算法回退。独立于G29零NPC校验矛盾。 |
| Q07 | 前端 aggregateCandles 仍按5/20交易日分组，UX只列周期名。自然周/月及合成历史衔接口径未裁决。见 [界面复核](reaudit-ui.md)。 | 当前已有图表，但是否应按公历周/月及合成负时间历史衔接需明确；不能宣称已核实真实周月口径。 |
| Q08 | 移动 MA 与分时均价仍由前端推导；MACD/KDJ由Rust提供，均价代码明确不是撮合均价或VWAP。见 [界面复核](reaudit-ui.md)。 | MACD/KDJ已由Rust返回；需区分允许的展示派生与权威指标，尤其均价口径。不是凭此证明伪造行情，也不能写“全部指标均来自Rust”。 |
| Q09 | ADR0006扩展承诺与sealed策略注册、ADR0008 ComputeMode/positions Vec旧路线与现行会话协议的关系仍未澄清。见 [基础复核](reaudit-foundations.md) 与历史 [R04](coverage/r04.md)。 | 库级接缝不等于生产后端切换；扩展方式与优化范围待文档明确，不自动授权重构或GPU。见R04/R18。 |
| Q10（已转G39） | 二次核对fixture→runner→生产step，确认未冻结实际受理轨迹。 | 不再作为未定产品方向；保留编号记录分类变化，运行结果仍待验收。 |
| Q11 | Correction/CreditDefault 原语存在，生产 InstitutionDecisionRoot 仍只分发 NewMaterial/HorizonExpired；更正年报走普通λ修订。见 [策略与公司复核](reaudit-engine.md)。 | 更正年报仍会走普通λ修订，不是完全不更新；没有Correction/CreditDefault专门分发。需明确何时选直接重估与真正违约信号，不能把CreditDeterioration信用恶化自动当违约，不能要求所有公告一律重估。 |
| Q12 | CLI将两个真实自由调度运行的price_volume/causal报告装入同一JSON，无共同run身份。见 [诊断](exhaustive-review/luna13.md)。 | 文档允许causal独立新会话，未明确两份报告必须解释同一次成交；需定来源标识/用途，不把拼装本身判成G。 |
| Q13 | 政策保存沪深分别official覆盖，CivilClock按首只股票取exchange；默认政策v1同轨。见 [日历](exhaustive-review/sweep04.md)。 | 混合局异步交易日是否支持需明确；不能与G15覆盖替代问题混淆，也不声称当前默认官方日历有差异。 |
| Q14 | 工商CreditDeterioration会增加准备，到期应收仍全额回款，未独立延期。见 [经营](exhaustive-review/luna03.md)。 | K4“客户延付/信用恶化”的替代范围需明确，不能把斜线承诺自动当两个独立模型；已批准只记录行业假设保留。 |
| Q15 | 精确颜色token静态计算的小字号白底对比与AA目标冲突。见 [颜色](exhaustive-review/luna02.md)。 | 需明确文字与图形/品牌色的作用范围；尚未computed-style验收，不擅自改token或弱化AA。 |
| Q16 | 旧“负成本显示-而非xx%”混淆金额与收益率；当前显示负净成本金额。见 [账户](exhaustive-review/luna15.md)。 | 非正净成本收益率不可用不等于必须隐藏负成本金额；G49舍入/浮盈公式是已确认的另一问题。 |
| Q17 | ClosingEngine::correct先过账/记录重述，再生成报告；合法派生汇总溢出可Err且留部分状态。见 [更正](exhaustive-review/luna23.md)。 | Journal批次原子已有，未找到整个更正/报告API失败零状态变化的明确保证或Session生产caller；保留真实边界，不擅自要求所有底层操作强事务。 |
| Q18 | Release collector不独立强制各平台最低格式组合，但归档producer已拒缺格式。见 [制品](exhaustive-review/luna77.md)。 | 是否要求最终collector重复校验语义组成需明确；没有正常生产绕过producer的证据，不称当前Release已漏包。 |
| Q19（已转G80） | 新局默认Industrial不代表完整存档恢复拒绝Bank variant；恢复整体安装CompanyOperations，未校验EclPolicy。见 [银行续核](renewed-check/bank-restore.md)。 | 纠正原降级理由；缺口限SaveSlot外部恢复，不要求改变库级serde/issue_loan错误次序，也不称默认新局已具备完整银行产品。 |
| Q20 | WASM NEXT的u32句柄计数会环绕，登记insert可覆盖仍存活句柄。见 [绑定](exhaustive-review/luna40.md)。 | 需明确耗尽时拒绝/重新分配策略；本轮只静态记录，未跑数十亿次或证明普通旅程触发。 |
| Q21 | 非正固定价可先报日限/price cage/资源拒绝，而非OrderBook InvalidPrice。见 [校验](exhaustive-review/luna16.md)。 | 已显式拒绝；多重非法条件的Market/Session错误优先级未规定，正常UI先挡非正价，不可断言公共路径一律LimitExceeded。 |
| Q22 | 来源拼接顺序、时段AccountReceipt及实际接收轨迹之间仍需核验；见 [草稿](exhaustive-review/luna49.md)。 | PreviousCommit/BetweenTicks是就绪时间窗，NPC向量在前不证明交易来源优先；只有真实竞争错序证据才能升级，不能恢复全局固定来源排序。 |
| Q23 | 年度所得税重复直接调用会把已记税费计入再次税前并追加亏损池；现无经营caller。见 [税务](exhaustive-review/luna54.md)。 | G35期末接线需明确计提时机/一次性与失败重试；当前公共API幂等及准入尚无完整约定，不冒充默认游戏已算错税。 |
| Q24 | Tauri第二个listener注册失败可能留下第一个；create_session成功后的初始化失败退订listener却未stop_session。见 [初始化裁定](hidden-review/candidate-resolution-04.md)。 | 真实资源失败路径与G51的dispose错误不同；失败初始化的资源归属/回收契约须明确。Worker同步postMessage异常仅延迟到既有有界timeout清理，不一并称永久泄漏，也不因旧测试冻结就宣称所有初始化回收完备。 |
| Q25 | BeliefBook的owner/key与StrategyState确定性身份均已校验，但未交叉核对同账户两个profile。见 [身份续核](renewed-check/belief-identity.md)。 | 创建时一致、字段影响计算不证明恢复必须全等；允许认知与执行风格不同还是必须统一身份需澄清，不限制个体AnalysisProfile/机构阈值，本轮不新增确认G。 |

## 4. 最小验证缺口与现有测试入口

下表主要列后续修复的代表性验证方向；G27的既有行为测试已在上一基线短测中通过，本轮未重跑，其余条目不据测试源码宣称通过。普通case和整命令遵守10秒上限，不借审计启动全回归。

| 覆盖ID | 已有测试入口 | 还需验证的真实边界 |
|---|---|---|
| G01–G05 | `apps/server/tests/ws.rs`、`apps/web/src/host/remote-host.test.ts`、`apps/web/src/host/tauri-host.test.ts` | 浏览器可发凭据、speed授权、pull持续拉帧、推进后暂停继续不重置游标、失活恢复；模拟socket/人工Bearer测试不能代替浏览器契约。 |
| G06/G09 | `packages/engine/tests/fundamental_beliefs/gold.rs`、`packages/engine/tests/fundamental_beliefs/failures/guards.rs` | 独立逐期折现手算；本人已知中期材料更新且未获知材料不影响预测，保留年报/中期口径差异。 |
| G07/G08/Q02 | `packages/engine/tests/analysis_profiles/invariants.rs`、`packages/engine/tests/experience_feedback/reads.rs`、`packages/engine/tests/technical_memory/memory.rs` | 真实GameSession散户消费档案与日期衰减；实际历史读取留痕；不能仅重测纯函数。 |
| G10–G14 | `apps/web/src/app/market-chart-runtime.test.ts`、`apps/web/src/mobile/market-model.test.ts`、`apps/web/src/mobile/mobile-component-render.test.ts` | 生产hook同分钟多帧/跨日/竞价转连续，最新七笔不同tick，涨跌颜色；已有日K增量用例不代表分时链已覆盖。 |
| G15 | `packages/engine/tests/calendar/exchange_days.rs` | 官方覆盖既能增加也能取消模拟休市；周末规则、无官方覆盖回退分别保留。无需联网行情。 |
| G16–G19 | `packages/engine/src/indicators.rs` 的测试、`apps/desktop/src-tauri/src/actor.rs` 的测试、生产性能入口 | 固定活跃工作量增加终止计划，检查复制工作；并行/发布/有界背压或等价机制进入真实生产链，性能结论另需测量。 |
| G20/G21 | 新局启动路径、`packages/engine/examples/price_volume_baseline.rs` 的seed测试 | 普通新局取种与测试注入分开；setup合法但无关存档字段异常时符合CLI契约。 |
| G22–G25 | 移动组件测试、`apps/web/e2e/mobile-layout.spec.ts` | 字段关联、进入/返回焦点、滚动保持及实际点击热区；未执行本轮视觉矩阵。 |
| G26 | `scripts/ci-workflow.test.mjs` | 手动开发CI中的warning退出状态；不把lint加入发布链路。 |
| G27（已核销） | `scripts/publish-release.test.mjs` | 上一基线复核通过正常发布二次SHA查询及上传期间标签变化保留draft的既有测试；本轮未重跑，不等同于线上发布验收或原子标签锁。 |
| G28/G29 | `packages/engine/tests/consolidation/`、`packages/engine/tests/industry_reports/`、`packages/engine/tests/session.rs` | 固定集团报告由真实日终生成并公开、无集团明确不适用；零NPC/正流通盘/ByKind与Random对照，仍拒绝非法权重。 |
| G30–G34 | 移动组件及chart runtime测试、`apps/web/e2e/mobile-layout.spec.ts` | 可见焦点、未变股票保持图表引用、中轴坐标一致、容器字号及reduce偏好；不能仅SSR或源码字符串断言。 |
| G35–G38 | `packages/engine/tests/industrial_accounting/`、`packages/engine/tests/company_operations/`、`packages/engine/tests/diagnostic_parity.rs`、`packages/engine/tests/plan_allocation/` | 代表性月结折旧/所得税/商业债务支付、四行业自定义会话日结查询、DEV真实订单关联、新旧买计划有限现金竞争；纯处理器测试不替代生产入口。 |
| G39 | `scripts/simulation/escrow-verification-contracts.test.mjs`、`scripts/simulation/run-escrow-verification-matrix.test.mjs` | 区分固定受理事实重放与自由调度，合法局部顺序差异不误判，同时仍拒绝资金/股份/价时/依赖错误。无需本轮运行完整K7。 |
| G69–G71/G73 | `packages/engine/tests/plans.rs`、`packages/engine/tests/industrial_accounting/`、`packages/engine/tests/company_operations/failures/scheduler.rs`、`packages/engine/tests/save_contract/` | 非零/可表示期限含MAX/1，库存科目无seed对账，重复调度身份及耗尽错误，保险损坏子账serde拒绝；分别核直接API与完整恢复，不伪造合法经营或正常局故障。 |
| G77 | `packages/engine/tests/consolidation/` | 配对相等的零/负额仍须显式拒绝，合法正额和已有配对/账面上界负控保留；不以禁止编辑合法资产代替校验。 |
| G78/G79 | `packages/engine/tests/company_operations/`、`packages/engine/tests/save_contract/`、工商账套短fixture | 时钟重复ID、耗尽及未来pending日期超政策边界显式拒绝，合法未耗尽/范围内日期继续注册；授信计算失败与真正无授信分开，保留合法编辑资产事实。 |
| G80 | `packages/engine/src/company/bank/behavior_tests.rs`、`packages/engine/tests/save_contract/` | 完整Bank变体存档对空表/非法权重拒绝，合法政策及可编辑资产保留；独立serde接受集合测试不等于完整SaveSlot已通过。 |
| G72/G74–G76 | matrix短fixture、`pipeline/adaptive_plan_chain_tests.rs`、`packages/engine/tests/calendar/` | matrix canonical判断先于内容读取，两个P3预检入口失败后停用，CivilInstant秒域与政策source摘要变化的身份一致性；保留合法路径，不运行完整matrix或完整回归。 |

新增G40–G68的短验证应对应真实消费链：异步控制确认/取消/重同步终态、非推进generation、现金不足后日结/恢复/公开查询、超过8只非保护记忆及淡出后重新发现、零/非零量和间断null、1/2/5档盘口、半分正负成本、中文/键盘/选中状态、React/IPC错误、无行情baseline、非默认setup、库级非法输入、多lender和承保期限。工具项用阻塞/采样失败短fixture验证整个子树在原期限内退出，并保留守恒和真实受理重放负控；不能靠删断言、放宽deadline或恢复自动发布测试解决。各项新记录给出对应入口，本轮未运行这些行为用例。

## 5. 已实现与旧要求核销

- **G27已核销：** `scripts/publish-release.mjs:109` 在draft上传和远端资产核验后重新查询tag SHA，变化时抛错并保留draft，113行才执行公开。`publish-release.test.mjs` 验证正常二次查询顺序及上传期间移动标签时禁止公开；上一基线复核定向测试通过，本轮代码未变且未重跑。它兑现了原缺失守卫，不宣称GitHub提供了原子不可变标签锁。详见 [工具复核](reaudit-tools.md)。
- 公司公开报告刷新选择已修正：`CompanyPanel.tsx:60` 仅在ready/empty协调选择，loading/error不再因临时空列表抹掉用户选择。新增组件测试覆盖临时状态、真实空结果和换公司回退；本轮仅核对源码，没有重跑浏览器或这些组件用例。此为已修行为，不新增待办编号。

- 更新后的 [核心账户/撮合](reaudit-core-contracts.md)、[Session/pipeline](reaudit-pipeline-contracts.md)、[账套/报告](reaudit-accounting-contracts.md) 复核未确认相应既有契约在重构中丢失；这不是完整回归通过声明。
- 当前新增测试按实际成交事件和收据核对现金加实收费用、股份、日K量额笔数及日界；立即恢复仍要求存档字节一致，不再要求两个自由调度实例的未来成交完全一致。规模测试另核对计划引用、原有计划保留、历史公开材料不变和公开时间边界；DEV查询测试改为同一会话查询前后状态不变，Server测试显式区分诊断feature。以上是已读的测试契约改进，不是新生产功能或本轮运行通过，不能核销G37/G39或长期规模验收债。
- 底层撮合中途溢出的部分写入、房地产计息 post 后子账更新失败在旧基线已有。新测试固定旧失败顺序不等于本次引入故障；Session 候选回滚与底层方法边界须区分。没有证据证明历史要求承诺这些底层方法全部强原子，因此不新增 G。静态追踪未确认正常默认局或存档恢复链存在该复现路径，未运行相关场景。

- A01–A11的主要生产能力已经存在：公开财报、远程查询方法、个体机构风险、冻结UrgencyPolicy、日终最小存档、内存日结回滚、工作台拖拽、错误详情、Rust指标、账户/订单增量及DEV当前宿主诊断。G项是局部断链/边界，不能用局部已实现证明整个宿主或整个策略模块无缺陷。
- 共同隐藏V/TrackV、资金循环/补钱、公共日内存档、任意挂单配额、旧格式兼容、按来源固定交易优先、自由并发整局字节一致已经被替代或明确排除。
- `AllocationExperience::default()` 不单独列缺口：机构信心已在上游按真实失败/净获利退出调整，成本与风险也走个人阈值；再接旧失败helper会重复扣信心，不能恢复统一20日强卖。
- 360根负时间虚拟日K、真实成交更新量额、T+1/费用/占用、开收盘撮合、符号最高/最低限价、初始持仓、账户结算、计划执行与日终子单清理均有生产实现。
- 8MiB远程存档上限、午休时钟遗漏、公共财报期间格式、WASM空值、旧测试使用玩家快照查NPC等历史发现已经有后续修正；不沿用旧REJECT或保留二进制失败判断当前源码。
- 七个手动入口、三平台打包、纯Server/WebUI Server、Release、Pages与缓存清理已有代码及后续发布记录，见 `docs/build-and-deployment.md:300`。本轮没有重新请求GitHub或重新验证线上状态。
- 普通commit/PR不自动CI、macOS/Windows不签名，以及发布不调用CI/测试/lint/smoke，均为用户决定，不是待恢复的缺口。
- 旧 sealed corpus 适配器、重放 example、bundle 装配与旧测试逐字冻结工具已经退役；`docs/test-cleanup-checklist.md` §12 明确接受历史证据不再可执行复验。不得把旧 zero-cash witness、映射表缺席重新登记成现行代码任务；保留 helper 仅测试调用是获批范围，不是生产接线遗漏。详见 [删除历史核销](coverage/h01.md)。Q05 与 G39 各有独立现行契约，不随旧工具退役一并核销。

## 6. 确实未完成但不属于现行必做

保留 [旧盘点B表](../../docs/implementation-gaps.md#3-确实没有完整实现但需确认范围或属于未来扩展) 的范围，不自动启动未来产品：

| 类别 | 未完成能力 / 当前边界 |
|---|---|
| 行情与内容 | 五日分时/跨日分钟查询、更多周期和均线配置、看点/资讯/社区/简况、首页资金/资讯/资产/分析快捷页、更多分类；按钮禁用或占位，不能称完整实现。独立收盘竞价曲线尚无，收盘撮合已实现。 |
| 玩家产品 | 多存档槽管理、成就、完整个人交易流水/复盘与云同步。当前快速槽和100条成交带不是这些功能。 |
| 部署与运维 | 公网账号/多人归属、数据库/迁移/重启恢复、完整TLS/Origin/运营控制、签名/公证/自动更新。已有私有会话token，不等于账号体系。 |
| 计算与长期架构 | 实际GPU内核/蒙特卡洛、冷热历史/区间查询、页级COW、反向唤醒索引、WAL/durable水位、旧观察令牌/保留期、2099年后规则；G16已接受的局部所有权目标与未接受整套ADR0018方案分开。 |
| 领域扩展 | 股东分红/增发/回购/清算、额外市场板块/订单类型/停复牌/融资融券、高级银行保险/集团会计模式、复杂学习/社会传播/组合风险、第二语言；具体简化见交易规则与会计文档，不按旧愿望清单一并实现。 |

### 仅缺数据依据或验收证据

官方休市原文覆盖、部分会计/税务依据仍有取证债；不使用真实行情不等于可以编造制度。
其中 CAS 8 减值原文在政策 fixture 中仍标 blocked，但工商日结已有减值调用；须补法源或显式登记游戏假设，不能称“尚无减值代码”或“已经核验准则合规”。详见 S03-C1 与候选核销记录。
稳定多线程收益、历史年龄矩阵、三宿主跨日真实旅程、安装器GUI/运行库兼容、移动/Wayland视觉及完整统计不能以短测或源码存在核销。[main最新验收](../main-release-validation/summary.md) 登记了绑定 `2247f4f` 产品树的默认/all-feature回归、脚本、12个Chromium E2E、九项ignored及build-only test Release/Pages的结果，本轮逐章核对来源和代码，没有重跑或重新查询GitHub。10万完整日档591,344,527 bytes超过Server有界解码536,870,912 bytes；typed恢复通过不证明Server能加载，WASM/Desktop入口又不同，不能泛称三宿主统一限制。K7 after/sensitivity fresh矩阵、真实UI性能和安装器GUI仍无本轮补证；G39/G60–G63明确列代码契约缺口。
旧host-parity/release-contract/verify-plan名称未找到，现有WASM导出、制品manifest、K7验证各有不同覆盖范围；缺的是未被替代的真实验收能力，不要求按旧名重复造工具。

### 文档漂移另行登记

旧ADR0008仍称Rust指标/跨股撮合“待落地”，但主要能力已有；旧财报披露模块注释称封账不可达，实际日终已经接线。
量价命令示例有遗漏diagnostics feature；因果诊断文档仍称决策时钟漏午休，代码已修。
报告批准08:00的假设登记、年报日期fixture算术文字也需校正，不能混成新的会计产品。
旧Money设计的负数舍入示例、GameConfig佣金最低额示例有算术错误；ADR0020状态、ADR0008旧规模耗时及positions Vec路线需与当前实现区分。
量价清单的散户异常选股仍是未来扩展，机构曝光权重不能证明散户已接线；tick-only诊断不能证明完整公司自然日经营/披露验收。
历史中文化、handoff和验证签署还存在证据闭环债：group08复核状态冲突、domain摘要指向另一版本、views/delta指纹已变化、strategy specificity未通过项未解释处置，以及测试文书两项建议缺完成证据。逐项见 [材料完整性记录](hidden-review/README.md#材料完整性与可追溯性)。本次不改写这些原历史证据，也不把缺证当作产品G；只有本轮报告失实的当前状态/ADR措辞被纠正。
补充全文中发现的旧 API/算术/文案差异也不得机械转成新功能：账户总资产已在会话个人权益计算中消费，不要求恢复同名 `Account::total_assets`；Money 小数解析在 ASCII 和长度校验后使用 `expect`，只是与旧计划的禁用写法不一致，未证明存在可触发的解析 panic；初始 HTML 标题 `web` 在 App 首次 effect 后会更新，启动壳与运行时标题范围需区别。逐项主控复核见 [候选核销记录](candidate-checks.md)。

## 7. 需求覆盖对照

本节保留初次全文审计建立的契约族映射及原基线代码位置；本轮已按 [当前全文记录](coverage-index.md) 重核其具体承诺。局部反例及后续取代以第2、3节为准，当前行号见新记录，不用旧行号定位新版本。

本节由各批补充的章节/任务对照收口；“已实现”仅表示该契约族存在生产路径，未承诺通过当前基线运行验收。局部反例以G/Q表为准。

表中 R01–R20 对应本轮 20 个并发文档组；S01–S06 对应上轮其余全文批次。
这是原概述编号；最新逐篇证据采用 `coverage/r01.md`–`r20.md`、`s01.md`–`s31.md`、`h01.md`，按 [索引](coverage-index.md) 查阅，不把两种 S 编号混用。
“已有”指生产契约族，不表示整篇每项完成；各组的局部缺口必须同时读取第二、三节。
路径未附行号的测试目录只作检索入口，不是执行结果。历史计划的 RED/GREEN、提交与最终 DoD 统一归验收，不重复当作运行功能。

| 组 | 原文覆盖章节/任务族 | 生产路径与测试源码入口 | 判定与后续 |
|---|---|---|---|
| R01 | ADR-0018 §1–15：版本根、历史、观察、提交、COW、WAL、长期性能；ADR-0025 全部保存/加载契约 | `packages/engine/src/session/protocol/civil/session.rs`、`apps/web/src/save/day-end-persistence.ts`；`apps/web/src/save/day-end-archive.test.ts`、`packages/engine/tests/save_contract/` | 公共日终候选、加载隔离已有；内部quiet-point快照不违反日终档。完整版本根/COW/冷历史/WAL仍是提议或明确不做；已接受的局部复制目标见G16。 |
| R02 | ADR-0017 P0–P9、双账本、预算、股票任务、回滚、计划续行及全部验收；escrow计划各波次 | `packages/engine/src/session/pipeline/authoritative_tick.rs`、`continuous_tick_transaction.rs`（同目录）、`candidate_commit.rs`（同目录）；该目录 `adaptive_plan_chain_tests.rs`、`pipeline_contract.rs` | 实际权威入口接线；来源类固定优先和跨线程整局字节一致被替代。长期吞吐/全门禁属验收，不能用历史REJECT断言现在失败。 |
| R03 | ADR-0016 全文；会计§1–7、四行业/报表/日历/法源；公司行为§1–4 | `packages/engine/src/session.rs` 日终经营/结账/披露；`packages/engine/src/accounting/closing/mod.rs`；`packages/engine/tests/industry_reports/`、`packages/engine/tests/consolidation/` | 默认工商日常经营/单体披露已有；固定集团G28、工商期末G35、四行业会话G36；Q03规则冻结关系待明确。股东分红/增发/回购/清算明确不做，法源债不冒充代码缺口。 |
| R04 | ADR-0006 策略边界/独立参数/工厂/注意力全部修订；ADR-0021 仓位/报价/费用；ADR-0026 用户决定及补漏 | `packages/engine/src/strategy/factory.rs`、`packages/engine/src/session/institutional_behavior.rs`、`packages/engine/src/session/decision_chain/quote.rs`；`packages/engine/tests/experience_feedback/`、`packages/engine/tests/urgency/` | 个体风险/成本/恢复、合法报价已有；G06–G09/G38为不同环节。主动成交可用Highest/Lowest限价，不要求PlaceMarket。sealed注册扩展方式是架构澄清，非当前策略失效。 |
| R05 | ADR-0011 分钟/日窗口/等权市场/个人风险；ADR-0012 #1–10；ADR-0013 原始记忆及K5修订 | `packages/engine/src/observation.rs`、`packages/engine/src/session/pipeline/decision_snapshot_capture.rs`、`retail_projection.rs`（同目录）；`packages/engine/tests/observations.rs`、`packages/engine/tests/experience.rs` | 观察、真实成交记忆、原有行为已有；日期与20日衰减漏接见G08。内部save不等于公共日内存档，不能重开已核销项。 |
| R06 | ADR-0009 全阶段/坐标/事件/严格档；ADR-0014 尾盘撮合/前端边界；ADR-0015 母单/生命周期/K6 | `packages/engine/src/session/pipeline/stock_auction.rs`、`packages/engine/src/session/plan_execution.rs`；`packages/engine/tests/auction.rs`、`apps/web/src/mobile/market-model.test.ts` | 开收盘撮合、母单协调和日终清理已有；尾盘独立曲线明确未来；分时UI局部错误见G10–G14。 |
| R07 | account计划Tasks1–7、spec§1–10：账户/策略骨架/成本/T+1/结算/快照 | `packages/engine/src/account.rs`、`packages/engine/src/session/pipeline/settlement.rs`；`packages/engine/tests/account.rs`、`packages/engine/tests/session.rs` | 已有；生产用receipt→apply_settlement而非旧apply_trade，不按旧入口漏调误报；账户整体serde被显式档映射替代。 |
| R08 | orderbook计划Tasks1–7、spec§1–9：类型/构造/校验/价时撮合/撤单/深度/序列化 | `packages/engine/src/orderbook.rs`、`packages/engine/src/session/pipeline/continuous_matching.rs`；`packages/engine/tests/orderbook.rs` | 已有；ID由Session分配、深度用u64、零价拒绝是演进；spec中的命名字段错误与实现元组变体为文档漂移。 |
| R09 | market计划Tasks1–5、spec§1–10：构造/涨跌停/撮合/旧V/日终/盘口 | `packages/engine/src/market.rs`、`packages/engine/src/session/pipeline/auction_day_end.rs`；`packages/engine/tests/market/` | 成交价、限制、日终已有；V演化已由ADR0016及公司计划明确删除，不登记为未来必须恢复。整数基点涨跌停优先于旧Money.apply_rate草案。 |
| R10 | session计划Tasks1–7、spec§1–9：装配/玩家队列/推进/投影/存档/错误 | `packages/engine/src/session/failure.rs`、`packages/engine/src/session/snapshot.rs`、`packages/engine/src/session/protocol/civil/session.rs`；`packages/engine/tests/session.rs` | 已有；P0–P9原子候选取代旧失败后继续，公开玩家投影不泄露全部NPC状态，集合竞价不再是未来。 |
| R11 | strategy-impl计划Tasks1–6、spec§1–11：三策略/多股视图/RNG/工厂/验证/分层 | `packages/engine/src/strategy/mod.rs`、`packages/engine/src/strategy/factory.rs`、`packages/engine/src/session/pipeline/npc_decisions.rs`；`packages/engine/tests/strategy.rs`、`packages/engine/tests/strategy_state.rs` | 主干已有；Value/TrackV被个人信念替代，Momentum改用完成分钟。不能据旧骨架完成核销G07/G08。 |
| R12 | initial-positions计划/spec全部任务；gameconfig全部配置/拒绝/默认/费用契约 | `packages/engine/src/session.rs` 新局分配、`packages/engine/src/config.rs`；`packages/engine/tests/session.rs`、`packages/engine/tests/config.rs` | 初始持仓和费用主干已有；零NPC边界G29，分配政策Q06。旧隐藏V/统一仓位上限不恢复；金额范围Q01，新局取种G20。 |
| R13 | money计划Tasks1–7、spec§1–9：分/溢出/解析/费率/半偶/serde/导出 | `packages/engine/src/money.rs`、`packages/engine/src/config.rs`；`packages/engine/tests/money.rs` | 已有，库级字符串解析不要求生产必须调用；跨端范围见Q01。spec把精确-5写成-4是旧算术错误，不是应恢复的预期。 |
| R14 | 两份公司计划K1–K7、任务1–42、F1–F4及完成条件 | 日历/会计/披露/个人信念/计划→`packages/engine/src/session.rs`；`packages/engine/tests/company_decision_session/`、`packages/engine/tests/company_scenarios/`、`packages/engine/tests/company_scale.rs` | 主要模块与宿主入口已有；G06–G09、G15/G16、G28/G35–G38、Q02/Q03等不能被任务勾选掩盖。W6长验收/统计/视觉证据单列，不宣称全绿。 |
| R15 | Sept24单局多线程；Sept25生产入口/线程池；Sept26 ready receipt/线程边界全部步骤 | `packages/engine/src/session/pipeline/ready_ingress.rs`、`packages/engine/src/session/pipeline/stock_stream.rs`；`packages/engine/examples/production_entry_performance.rs`、流水线相关测试 | 生产并行受理/股票工作主干已有；历史plans复制见G16；稳定多核收益属于需实测的验收，不从Rayon存在推导。 |
| R16 | sparse-continuous-book-feedback全部任务；量价清单已定/待定项及CLI；ADR-0022符号限价全文 | `packages/engine/src/session/pipeline/continuous_tick_transaction.rs`、`packages/engine/examples/price_volume_baseline.rs`；`packages/engine/tests/orderbook.rs`、pipeline测试 | 符号报价/实际受理解析已有；CLI输入投影G21。统计必须用虚拟前史＋真实游戏撮合，不恢复真实行情授权等待。 |
| R17 | DESIGN全部页面/部件/尺寸；UX全部显示与Flow ledger；移动QA全部清单 | `apps/web/src/App.tsx`、`apps/web/src/mobile/MobileStockDetail.tsx`、`apps/web/src/app/useMarketChartRuntime.ts`；`apps/web/src/mobile/mobile-component-render.test.ts`、`apps/web/e2e/mobile-layout.spec.ts` | 已有页面不能核销G10–G14/G22–G25/G30–G34；Q04标题、Q07/Q08派生口径冲突；内容占位/更多周期未来，视觉与平台矩阵尚需验收。 |
| R18 | ADR-0008 D1–D5/N1–N3/T1–T6及后续；ADR-0020全部；tech-stack全部选型/门禁 | `packages/engine/src/compute.rs`、`packages/engine/src/indicators.rs`、`packages/engine/src/lib.rs`；`packages/engine/tests/compute.rs`、`scripts/ci-workflow.test.mjs` | Rayon权威路径、Rust指标、平台allocator已有；ComputeBackend是库级接缝非会话切换。G17/G26；GPU未来；positions Vec字面改造被新协议改变前提，需澄清而非立即重构。 |
| R19 | ADR-0005三宿主/取种/协议/调度/心跳；ADR-0010更新/节奏/背压/订阅；ADR-0007前端框架/交互 | `apps/web/src/host/`、`apps/server/src/actor.rs`、`apps/desktop/src-tauri/src/actor.rs`；宿主适配器测试及 `apps/server/tests/ws.rs` | 三宿主骨架/局部刷新已有；真实端到端缺口G01–G05/G18–G20；框架选型不等于重连或背压完成。 |
| R20 | 两份resolve-blockers-wayland计划全部任务/验收；公司archive索引与适用边界 | `scripts/performance/`、`scripts/desktop/`、宿主与协议生产入口；相关脚本测试和历史证据 | 历史修正不能回退成现行缺口；Wayland/GUI/K7最终证据债保留，归档不新增产品要求。 |
| S01 | trading-rules、simulation-calendar、ADR0019/0023/0024现行范围全部 | `packages/engine/src/calendar/holidays.rs`、`packages/engine/src/session/candles.rs`、市场/结算；`packages/engine/tests/calendar/` | 合成前史/撮合/不补钱/交易简化已有；G15覆盖替代边界，Q03政策关系；不得把未支持市场制度写成已实现。 |
| S02 | 根工程守则/README、architecture、principles、error-handling、naming、ADR0000–0004、Git与贡献说明 | Rust engine依赖边界、RTK投影、宿主启动/错误入口；`apps/web/src/App.tsx`、workspace manifests | 架构主干已有；G22字段错误；模板/协作规范不算新增产品功能，历史命令/路线差异按新决定核销。 |
| S03 | ADR0027/0028、build-and-deployment、actions-cache、ci-build-fixes全部目标/权限/运行边界 | `.github/workflows/`、`scripts/build-targets.mjs`、`scripts/publish-release.mjs`、`scripts/prune-actions-cache.mjs` | 七按钮、三平台制品、Pages、标签发行和清理已有；原G27守卫窗口已由当前代码补上并在第5节核销。普通提交无自动任务/不签名是决定；线上状态未在本轮重验。 |
| S04 | testing、test-cleanup、diagnostics/causal、naming-refactor、performance说明全部 | `scripts/run-web-tests.mjs`、`scripts/performance/`、引擎diagnostics与性能examples | 工具存在不等于完整验收；Q05发现策略；旧脚本被替代、旧午休诊断文字过时；长期/统计/真实宿主矩阵单列。 |
| S05 | implementation-gaps、roadmap、work-status、open-questions全部 | A01–A11对应生产代码；第二节反例与第六节未来范围 | 原批完成记录保留，但不外推整个模块无缺口；旧待定已由最新用户决定核销，B表未来产品不自动启动。 |
| S06 | .omo全部Markdown证据/notepads/HANDOFF、superpowers交接issues/learnings/problems/README、三份draft | 历史报告按所指模块与现行生产链对照；不以旧二进制或旧测试名代替源码 | 证据仅对原提交有效；未验收事项保留第六节。重复归档非新要求，draft不是已批准决定，原始非Markdown日志不在逐字覆盖集合。 |

## 8. 文档覆盖清单

完整来源清单已覆盖1110个路径，包含原234来源及新增876来源；CLAUDE.md是AGENTS.md别名，仅同SHA正文共享阅读。下表保留原142个跟踪来源的历史映射，后续87份工作记录和本轮忽略目录/历史分支来源统一见 [覆盖索引](coverage-index.md) 与 [完整指纹索引](hidden-review/expanded-source-index.json)。不把旧表行数当作当前全部来源数量。
覆盖根文档、docs全部ADR/规范/历史specs/plans、.omo计划/交接/notepad/Markdown审查记录、Web/性能/移动QA说明及PR模板。
完整读取记录来自上轮全文批次及本轮补充对照；不把上轮“未发现”自动升级成全部断言已经证明。
不包含依赖/构建产物、未跟踪.worktree副本、原始TXT/JSON日志及参考HTML的逐字审查。
原始证据按需要追查；以下清单固定到源码基线，不含本次新增报告自身。

| 文档路径（仓库根目录相对） | 对照族 / 阅读边界 |
|---|---|
| `.github/pull_request_template.md` | S02：工程/架构/协作；全文读取 |
| `.omo/HANDOFF.md` | S06：历史交接/证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/compatibility-removal.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/notepad-recovery.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-1-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-10-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-11-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-12-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-13-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-14-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-15-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-16-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-17-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-19-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-2-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-20-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-21-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-22-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-24-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-25-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-26-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-27-manual-continuation.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-27-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-28-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-29-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-3-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-30-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-33-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-34-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-36-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-4-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-7-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-8-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-9-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/worktree-baseline.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/F3/manual-qa/README.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-10/divergence-audit.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-11/execution-log.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-12/validation.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-3-8-smoke.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-3/d6-d7/README.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-3/d6-d7/structured-comparison.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-8/acceptance-map.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-9/corpus-diff.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-9/historical-witness-audit.md` | S06：历史验收证据；全文读取 |
| `.omo/notepads/company-information-npc-intentions/decisions.md` | S06：历史交接/证据；全文读取 |
| `.omo/notepads/company-information-npc-intentions/issues.md` | S06：历史交接/证据；全文读取 |
| `.omo/notepads/company-information-npc-intentions/learnings.md` | S06：历史交接/证据；全文读取 |
| `.omo/notepads/company-information-npc-intentions/problems.md` | S06：历史交接/证据；全文读取 |
| `.omo/plans/company-information-npc-intentions.md` | R14；全文读取 |
| `.omo/plans/escrow-parallel-engine.md` | R02；全文读取 |
| `.omo/plans/resolve-blockers-wayland.md` | R20；全文读取 |
| `AGENTS.md` | S02：工程/架构/协作；全文读取 |
| `CLAUDE.md` | AGENTS.md 别名，正文不重复计数 |
| `CONTRIBUTING.md` | S02：工程/架构/协作；全文读取 |
| `DESIGN.md` | R17；全文读取 |
| `README.md` | S02：工程/架构/协作；全文读取 |
| `UX-CONTRACT.md` | R17；全文读取 |
| `apps/web/README.md` | S02：工程/架构/协作；全文读取 |
| `design/ui/mobile/qa/README.md` | R17；全文读取 |
| `docs/actions-cache.md` | S03：部署发布；全文读取 |
| `docs/architecture.md` | S02：工程/架构/协作；全文读取 |
| `docs/build-and-deployment.md` | S03：部署发布；全文读取 |
| `docs/causal-diagnostics.md` | S04：验收/诊断；全文读取 |
| `docs/ci-build-fixes.md` | S03：部署发布；全文读取 |
| `docs/company-accounting.md` | R03；全文读取 |
| `docs/company-actions-design.md` | R03；全文读取 |
| `docs/decisions/0000-template.md` | S02：工程/架构/协作；全文读取 |
| `docs/decisions/0001-record-architecture-decisions.md` | S02：工程/架构/协作；全文读取 |
| `docs/decisions/0002-engine-rust-wasm.md` | S02：工程/架构/协作；全文读取 |
| `docs/decisions/0003-backend-rust.md` | S02：工程/架构/协作；全文读取 |
| `docs/decisions/0004-frontend-state-redux-toolkit.md` | S02：工程/架构/协作；全文读取 |
| `docs/decisions/0005-unified-engine-three-deployments.md` | R19；全文读取 |
| `docs/decisions/0006-npc-strategy-module.md` | R04；全文读取 |
| `docs/decisions/0007-three-deployment-frontend-framework.md` | R19；全文读取 |
| `docs/decisions/0008-gpu-and-compute-offload.md` | R18；全文读取 |
| `docs/decisions/0009-call-auction-and-intraday-axis.md` | R06；全文读取 |
| `docs/decisions/0010-unified-host-protocol-and-local-refresh.md` | R19；全文读取 |
| `docs/decisions/0011-market-time-observations-and-position-risk.md` | R05；全文读取 |
| `docs/decisions/0012-retail-observation-to-target-position-loop.md` | R05；全文读取 |
| `docs/decisions/0013-retail-experience-memory.md` | R05；全文读取 |
| `docs/decisions/0014-closing-call-auction.md` | R06；全文读取 |
| `docs/decisions/0015-parent-order-execution.md` | R06；全文读取 |
| `docs/decisions/0016-fundamental-factor-model.md` | R03；全文读取 |
| `docs/decisions/0017-escrow-parallel-tick.md` | R02；全文读取 |
| `docs/decisions/0018-long-running-immutable-timeline.md` | R01；全文读取 |
| `docs/decisions/0019-draft-market-scope-and-capacity.md` | S01：现行领域边界；全文读取 |
| `docs/decisions/0020-native-allocator-for-concurrent-ticks.md` | R18；全文读取 |
| `docs/decisions/0021-strategy-position-choice-and-noise-pricing.md` | R04；全文读取 |
| `docs/decisions/0022-symbolic-limit-prices.md` | R16；全文读取 |
| `docs/decisions/0023-synthetic-history-and-matching-only.md` | S01：现行领域边界；全文读取 |
| `docs/decisions/0024-shrinking-investor-cash-pool.md` | S01：现行领域边界；全文读取 |
| `docs/decisions/0025-day-end-only-persistence.md` | R01；全文读取 |
| `docs/decisions/0026-individual-institution-experience.md` | R04；全文读取 |
| `docs/decisions/0027-runtime-deployment-and-build-targets.md` | S03：部署发布；全文读取 |
| `docs/decisions/0028-tagged-release-and-static-pages.md` | S03：部署发布；全文读取 |
| `docs/diagnostics.md` | S04：验收/诊断；全文读取 |
| `docs/error-handling.md` | S02：工程/架构/协作；全文读取 |
| `docs/git/AGENTS.md` | S02：工程/架构/协作；全文读取 |
| `docs/git/daily-workflow.md` | S02：工程/架构/协作；全文读取 |
| `docs/git/initialization.md` | S02：工程/架构/协作；全文读取 |
| `docs/implementation-gaps.md` | S05：范围与进度；全文读取 |
| `docs/naming-conventions.md` | S02：工程/架构/协作；全文读取 |
| `docs/naming-refactor-validation.md` | S04：验收/诊断；全文读取 |
| `docs/open-questions.md` | S05：范围与进度；全文读取 |
| `docs/price-volume-simulation-gap-checklist.md` | R16；全文读取 |
| `docs/principles.md` | S02：工程/架构/协作；全文读取 |
| `docs/roadmap.md` | S05：范围与进度；全文读取 |
| `docs/simulation-calendar.md` | S01：现行领域边界；全文读取 |
| `docs/superpowers/2026-09-13-company-information-archive.md` | R20；全文读取 |
| `docs/superpowers/README.md` | S02：工程/架构/协作；全文读取 |
| `docs/superpowers/plans/2026-06-29-account.md` | R07；全文读取 |
| `docs/superpowers/plans/2026-06-29-initial-positions.md` | R12；全文读取 |
| `docs/superpowers/plans/2026-06-29-market.md` | R09；全文读取 |
| `docs/superpowers/plans/2026-06-29-money-fixed-point.md` | R13；全文读取 |
| `docs/superpowers/plans/2026-06-29-orderbook.md` | R08；全文读取 |
| `docs/superpowers/plans/2026-06-29-session.md` | R10；全文读取 |
| `docs/superpowers/plans/2026-06-29-strategy-impl.md` | R11；全文读取 |
| `docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md` | R14；全文读取 |
| `docs/superpowers/plans/2026-09-13-resolve-blockers-wayland.md` | R20；全文读取 |
| `docs/superpowers/plans/2026-09-24-single-world-multithreading.md` | R15；全文读取 |
| `docs/superpowers/plans/2026-09-25-production-entry-and-thread-pool.md` | R15；全文读取 |
| `docs/superpowers/plans/2026-09-26-ready-receipt-and-thread-boundary.md` | R15；全文读取 |
| `docs/superpowers/plans/2026-09-26-sparse-continuous-book-feedback.md` | R16；全文读取 |
| `docs/superpowers/specs/2026-06-29-account-design.md` | R07；全文读取 |
| `docs/superpowers/specs/2026-06-29-gameconfig-design.md` | R12；全文读取 |
| `docs/superpowers/specs/2026-06-29-initial-positions-design.md` | R12；全文读取 |
| `docs/superpowers/specs/2026-06-29-market-design.md` | R09；全文读取 |
| `docs/superpowers/specs/2026-06-29-money-fixed-point-design.md` | R13；全文读取 |
| `docs/superpowers/specs/2026-06-29-orderbook-design.md` | R08；全文读取 |
| `docs/superpowers/specs/2026-06-29-session-design.md` | R10；全文读取 |
| `docs/superpowers/specs/2026-06-29-strategy-impl-design.md` | R11；全文读取 |
| `docs/superpowers/specs/2026-09-11-company-information-handoff.md` | S06：历史交接/证据；全文读取 |
| `docs/superpowers/specs/2026-09-13-company-information-issues.md` | S06：历史交接/证据；全文读取 |
| `docs/superpowers/specs/2026-09-13-company-information-learnings.md` | S06：历史交接/证据；全文读取 |
| `docs/superpowers/specs/2026-09-13-company-information-problems.md` | S06：历史交接/证据；全文读取 |
| `docs/tech-stack.md` | R18；全文读取 |
| `docs/test-cleanup-checklist.md` | S04：验收/诊断；全文读取 |
| `docs/testing.md` | S04：验收/诊断；全文读取 |
| `docs/trading-rules.md` | S01：现行领域边界；全文读取 |
| `docs/work-status.md` | S05：范围与进度；全文读取 |
| `scripts/performance/README.md` | S04：验收/诊断；全文读取 |
| `.omo/drafts/resolve-blockers-wayland.md` | S06：未跟踪历史draft；全文读取、非已批准决定 |
| `.omo/drafts/k7-deterministic-multicore-utilization.md` | S06：未跟踪历史draft；全文读取、非已批准决定 |
| `.omo/drafts/escrow-parallel-engine.md` | S06：未跟踪历史draft；全文读取、非已批准决定 |
