# 尚未实现与部分实现的功能代码盘点

- 核对日期：2026-10-01；代码基线：`b5c2e6d`。
- 本页是文档与代码现状盘点，不是新的实施授权，也不是完整运行验收报告。
- “有类型/有纯函数/有测试/计划已勾选”不自动等于生产调用链和玩家界面已经完成。

## 2026-10-01 实施更新（优先于下方历史盘点）

用户追加要求以 [ADR-0025](decisions/0025-day-end-only-persistence.md) 为准：日内不更新
浏览器快速槽或文件；成功自然日日结生成不可变候选，按已结日期、seq 和宿主 generation
读取对应候选，再更新快速槽和授权文件。休市日经营/披露日结同样适用。
订单界面改用只读查询与增量协议，不再通过完整存档获取日内委托余量。
用户进一步明确：启动时恢复一次，运行期只使用内存会话；仅明确换档才再次读取。
日级档不包含日内活动挂单，B20 的“修改现金与已有买单冲突”不再作为待确认产品问题。

| 项目 | 本轮代码状态与仍需明确的边界 |
|---|---|
| A01 | 完整公开五产物、附注、范围、更正关系及比较合计已落地，独立复核和定向验证通过，本地提交 `6ccc4bd`。历史行中的“仅摘要/拼表”已核销；公司 E2E 未执行。 |
| A02 | 正确公司路由、token、分页、按 ID 查询及跨读档缓存失效已接线；宿主批次定向验证/提交记录见工作状态。 |
| A03 | 急跌暂停、真实撤单/不可撤延后、本人观察后的冻结门槛恢复、停止看好的观点修订与终止，以及散户个人回撤紧迫度已接线。机构风险压力/被动不利选择仍明确 `Unavailable`；已确认的机构风格、观察节奏和紧迫度须按较新文档继续接线，不把这部分列为完成，也不重复询问统一止损规则。 |
| A04 | 会话、严格存档、恢复、clone、hash、P2 与报价使用必填的七字段 `UrgencyPolicy`；旧“每次取默认值”已核销，不恢复现金留底或统一仓位上限。 |
| A05 | 本轮已识别的派生冗余已精简：专用 `SaveSnapshot` 不存盘口、最佳价、预留或 day/phase；时间按已验证的 setup/tick 重建。Envelope 只存身份与实际累计收费，其余按活动订单/config 重建；专用 `SaveParentOrderPlan` 不存子单剩余量。合法资产修改不校验历史来源；原始计划承诺、历史成交和随机/个人状态保留。内部快照可含活动订单，公共日终档不得包含；B20 的已有买单资金冲突提问已核销，真正非法的日级事实仍显式拒绝。不宣称未来所有字段都不可再精简。 |
| A06 | 自然日日结改用内存检查点与完整对象回滚，不再以 SaveSlot 备份/restore。诊断与 observer 注册保留；不承诺已执行 observer 外部副作用可回滚。 |
| A07 | 六个桌面面板接入拖拽/缩放及容器测宽，保留移动布局；窄横屏不再留下隐藏持仓的空格。未运行真实浏览器拖拽矩阵。 |
| A08 | 结构化详情、脱敏 context/cause、恢复建议及 Clipboard 成败反馈已贯通，独立复核和短测通过，本地提交 `7befc90`。 |
| A09 | MACD、close-only/OHLC KDJ 在 Rust 计算并通过三宿主、context 和图表消费；非法输入/非有限输出显式失败，未恢复 Web 计算 fallback。 |
| A10 | 玩家账户与活动委托 reset/upsert/remove 增量贯通生产协调器、Redux、委托列表及日内 K 缓存；完整 baseline/CivilUpdate 保留。两项 Rust wire 用例缺少实施前运行的红灯记录，不冒充完整 TDD 时序证据。 |
| A11 | DEV 检查器读取当前 EngineHost；能力取决于真实 debug/feature 协商，旧 generation 明确拒绝。release 产物检查真实 WASM exports，禁止携带私有 trace 包；未执行完整三宿主浏览器矩阵。 |

这里只登记本轮实施及代表性短证据，不代替完整回归或长期验收。下方 A 表保留为最初
全文审查的历史发现，不能再把已经核销的描述当作当前未实现代码。

后续 A05 定向验证：Rust v2 32/32、无诊断 feature 的日终候选 30/30、恢复 4/4、
竞价占用 1/1、收盘母单恢复 1/1；Web 存档与日终链 45/45、类型检查及实际 WASM/
生产构建通过。新增 Rust 契约断言缺少实施前可核对的断言级红灯证据，Web 首次红测
只有文件加载失败信息，不将其冒充完整 TDD；后续补验证及具体 fixture 修复单独记录。
另一个既有 Continuous 部分成交用例 `live_plan_partial_fill_survives_restore_and_second_real_tick_fill`
因母单键缺失失败；精简前保留的编译产物同样复现，根因尚未核定。本轮保留其
300/200 股断言，不修改无关生产路径，不把该用例核销为通过。

## 1. 范围、方法与结论边界

按用户要求，以最多 20 个同时在途的 `gpt-6-luna medium` 分批全文阅读。
共 60 个业务批次，每批仅 1–3 份文档，覆盖 134 份不同的 Markdown 正文；每批报告
从第 1 行读至 EOF，长文连续分段，截断输出补读后才关闭。共同协作指令不计入业务配额。
`CLAUDE.md` 是 `AGENTS.md` 的符号链接，不重复计为不同正文。

覆盖根目录、`docs/`（全部 ADR、现行规范、历史 specs/plans）、`.omo/`（草案、计划、
交接、notepad、Markdown 审查记录）、Web README、移动 QA、脚本 README 和 PR 模板。
不把依赖/构建产物、原始 JSON/逐笔数据、TXT 编译日志和参考 HTML 当作需求文档全读；
需要的历史证据按对应主张核对，不能宣称所有原始日志均已审阅。

总控随后做全仓文档/源码搜索、调用点和原文复核，并去重。Git 跟踪的 132 个 Markdown
路径按真实路径检查无遗漏（其中包含上述别名）；另覆盖 3 份未跟踪的历史草案。
源码做全仓索引搜索及关键实现/调用链阅读，不宣称逐字审阅了全部源码，也不以零搜索匹配
单独证明任意功能不存在。没有运行功能测试、构建、完整回归或长验收。

判定优先级为用户最新决定、已确认后续修订、原计划/旧正文。ADR-0023 排除真实市场数据
校准；ADR-0024 核销资金循环；ADR-0019 移除任意订单配额并聚焦单局。ADR-0018 整体
仍为 proposed，不能把其中全部架构建议变成当前必做项。

## 2. 已确认方向中尚未落实的生产或界面环节

下表包含缺接线、部分交付及已记录的工程完善方向，不表示每项都阻塞当前可玩版本。
“当前已有”与“还缺”分开列；实现这些项目仍须先补对应失败测试及独立复核。

| ID | 功能与依据 | 当前已有 / 还缺 | 代码核对入口 |
|---|---|---|---|
| A01 | 玩家完整公开财报；公司计划任务 34，`.omo/plans/company-information-npc-intentions.md:588` | 公司会计和完整报表产物已有；公共 DTO 只提供资产/利润/现金流等合计，前端用少量行拼四张表。还缺已披露报表明细、行业附注、单体/合并范围及完整比较信息的公共投影和 UI；不得因此暴露未披露总账或 NPC 私有状态。 | `packages/engine/src/company/query.rs:57`、`apps/web/src/components/company/company-presentation.ts:102`、`apps/web/src/components/company/ReportNotes.tsx:16` |
| A02 | 远程财报查询与三宿主一致入口；公司计划任务 31/33，`.omo/plans/company-information-npc-intentions.md:559` | Server 已有受 token 保护的公司报告路由，Worker/Tauri 已能查询；RemoteHost 却标记 `publicCompanyReports: false`，列表使用不存在的 `/api/public-reports/...` 路径且未传分页参数，按 ID 方法直接抛“未实现”。还缺远程适配器的正确接线，不是服务端完全没有查询功能。 | `apps/web/src/host/remote-host.ts:137`、`apps/web/src/host/remote-host.ts:218`、`apps/server/src/lib.rs:65`、`apps/web/src/host/company-query-coordinator.ts:83` |
| A03 | 计划风险紧迫度与暂停/恢复闭环；K5a/K6，`.omo/plans/company-information-npc-intentions.md:170`、`.omo/plans/company-information-npc-intentions.md:181` | 紧迫度、暂停/恢复纯函数和 PlanBook 状态机已有，急跌可影响报价/撤单；生产报价把风险压力、不利选择、风险减仓固定为 false，账户回撤为 None。还缺这些输入的真实接线及 Paused/Resumed、下一本人观察后恢复的生产状态闭环；`assess_recovery` 未找到生产调用。散户既有账户回撤减仓并非缺失，不能笼统称“没有风险模型”。 | `packages/engine/src/session/decision_chain/quote.rs:76`、`packages/engine/src/plans/urgency.rs:84`、`packages/engine/src/plans/urgency.rs:144`、`packages/engine/src/plans/mod.rs:112`、`packages/engine/src/behavior/decision.rs:60` |
| A04 | 执行紧迫度参数随存档冻结；K5a，`.omo/plans/company-information-npc-intentions.md:171` | `UrgencyPolicy` 可序列化并有版本校验，但会话报价直接构造 `UrgencyPolicy::default()`；未找到其作为会话/SaveSlot 权威字段的接线。还缺实际使用并保存配置的路径，不是单纯缺一个 serde 派生。此项不恢复已由 ADR-0021 取消的仓位上限或现金留底。 | `packages/engine/src/plans/urgency/policy.rs:9`、`packages/engine/src/session/decision_chain/quote.rs:94`、`packages/engine/src/session.rs:403` |
| A05 | 最小事实存档与修改器友好恢复；`docs/decisions/0019-draft-market-scope-and-capacity.md:31` | 当前存读档、原子校验恢复已有；SaveSlot 仍复用展示 snapshot，另存订单、envelope/预留和母单子单余量，恢复校验多份派生值一致。还缺存档专用最小事实结构，以及读取时重建盘口、报价、资金/股份预留、子单剩余量。资产修改造成真实冲突时的提示/整理策略尚待决定，不能静默撤单或补钱。 | `packages/engine/src/session.rs:403`、`packages/engine/src/session.rs:2616`、`packages/engine/src/session/persistence.rs:1465` |
| A06 | 自然日日结内部备份精简；`docs/decisions/0019-draft-market-scope-and-capacity.md:45` | 协议/宿主批次回滚已改内存候选；`end_civil_day` 仍生成完整 SaveSlot 备份，失败再 restore。还缺该内部失败边界的精简，不是用户保存/读取功能没写，也不是协议 checkpoint 仍做 JSON 往返。 | `packages/engine/src/session.rs:2175` |
| A07 | 桌面工作台拖拽/缩放；`docs/decisions/0007-three-deployment-frontend-framework.md:26` | `react-grid-layout` 依赖在，但全仓前端实现未找到使用，App 仍是静态 `.app-grid`。还缺面板拖动/缩放交互；不据此新增“布局云同步”等未承诺功能。 | `apps/web/package.json:26`、`apps/web/src/App.tsx:786` |
| A08 | 完整错误详情和一键复制反馈；`docs/error-handling.md:98` | 当前会显式展示错误、位置/代码和刷新动作；HostFailure 只有 code/where/message。还缺脱敏 context、cause 链、恢复建议和一键复制反馈的贯通；不是错误完全不可见。 | `apps/web/src/host/host-update.ts:25`、`apps/web/src/app/HostStatusViews.tsx:4` |
| A09 | MACD/KDJ 迁至 Rust/CPU 并行计算；`docs/decisions/0008-gpu-and-compute-offload.md:42` | Web 指标计算与绘制已有；Rust 的 SMA/RSI/ATR 不等于 MACD/KDJ。还缺 ADR D2 的引擎计算与前端消费接线，这是已记录的计算分层/性能方向，不是玩家完全看不到指标。D3 跨股并行撮合已实现，不重复列为缺口。 | `apps/web/src/components/PriceChart.tsx:49`、`apps/web/src/mobile/market-model.ts:355`、`packages/engine/src/observation/technical.rs:34` |
| A10 | 账户/委托增量更新；`docs/decisions/0010-unified-host-protocol-and-local-refresh.md:89`、`UX-CONTRACT.md:78` | 纯行情增量、局部订阅及行事务已有；成交/委托等仍带 runtime snapshot，前端低频整体换基线。还缺账户/委托增量建模及消费。现行契约明确允许该过渡行为，不把它说成每帧都全量替换或发布阻塞。 | `apps/web/src/host/protocol/reduce.ts:66`、`apps/web/src/store/store.ts:27` |
| A11 | 三宿主在开发模式读取当前局 NPC trace；公司计划任务 35，`.omo/plans/company-information-npc-intentions.md:596` | 引擎、WASM、Server/Tauri 诊断入口及独立 DEV 检查器已有；普通三个 EngineHost 都标记 `npcDecisionDiagnostics: false`，未实现可选 `npcDecisionTrace`，独立检查器另建一个诊断 WASM 会话。还缺正在游玩的三宿主会话到统一开发查询/UI 的接线；release 禁用私有诊断仍是正确边界，不能为了补接线向普通客户端泄漏信息。 | `apps/web/src/host/engine-host.ts:62`、`apps/web/src/host/remote-host.ts:137`、`apps/web/src/host/worker-host.ts:222`、`apps/web/src/host/tauri-host.ts:142`、`apps/web/src/dev/NpcDecisionInspector.tsx:93` |

## 3. 确实没有完整实现，但需确认范围或属于未来扩展

这部分不能直接转成当前实施清单；某些能力只是旧设计/预留，优先级须另定。

| ID | 功能/缺少环节 | 依据与当前边界 |
|---|---|---|
| B01 | 已实现：玩家“最高/最低”限价选项 | 限价方式现在可选择指定/最高/最低；符号选价仍提交 `PlaceLimit`，受理时才解析合法价格，不冒充市价或持续追价。三项定向短测及独立复核通过；首次红测仅缺 helper 的加载失败，不冒充断言级 TDD。 |
| B02 | 五日分时图及跨交易日分钟数据查询 | `UX-CONTRACT.md:45`、`apps/web/src/mobile/MobileStockDetail.tsx:273`；按钮禁用并说明等待跨日分钟数据。完整日 K 已保留，不等于已有历史分钟查询。 |
| B03 | 更多图表周期、均线配置 | `apps/web/src/mobile/MobileStockDetail.tsx:98`、`apps/web/src/mobile/MobileStockDetail.tsx:277` 均为 disabled；尚未定义完整选项/交互，周/月 K 的既有展示不能算这些入口已开放。 |
| B04 | 看点/资讯/社区/简况内容，以及首页资金/资讯/资产/分析快捷页和更多行情分类 | `apps/web/src/mobile/MobileStockDetail.tsx:286` 仍为占位，`apps/web/src/components/MarketGrid.tsx:182`、`apps/web/src/components/MarketGrid.tsx:190` 为禁用入口。标签切换/布局已有，但内容业务没实现；需产品范围，不能因有“占位”就称该业务已完成。个股资金统计与盘口并非缺失。 |
| B05 | 收盘集合竞价独立曲线/尾盘可视区 | `docs/decisions/0014-closing-call-auction.md:18` 明确当前只画开盘竞价，收盘真实结果经日 K 传递；独立尾盘展示需扩展坐标契约，不是收盘撮合缺失。 |
| B06 | 多存档槽管理、成就、完整玩家交易流水与复盘查询、云同步 | `docs/open-questions.md:67` 为 Stage 2 产品决策；`apps/web/src/save/save-repository.ts:66` 是单快速槽，`apps/web/src/store/store.ts:60` 的最新 100 笔成交带不是完整流水。 |
| B07 | 公网账号身份、全路由授权和多人同局账户归属 | `docs/roadmap.md:12`、`docs/decisions/0005-unified-engine-three-deployments.md:77`；已有会话 token/部分路由检查，不是完全无鉴权。但 intent/snapshot/save/load/speed/running/delete 等 HTTP 入口未统一校验 token，普通意图仍走单玩家账户。正式公网部署前必须解决，不能把 token 存在等同完整账号/权限体系。代码：`apps/server/src/routes.rs:46`、`apps/server/src/routes.rs:601`、`apps/server/src/routes.rs:658`、`apps/server/src/routes.rs:899`。 |
| B08 | 服务端数据库持久化、数据库迁移、重启后自动恢复 | `docs/roadmap.md:12`；当前 actor 与 DashMap 是进程内状态，save/load API 传输存档不等于数据库。数据库/迁移策略需决定；WAL 的更强保证另见 B16。 |
| B09 | 公网 TLS、明确 Origin 白名单、运营限流/运维指标 | `docs/roadmap.md:12`；`apps/server/src/main.rs:55` 为 TCP/HTTP serve，`apps/server/src/lib.rs:116` 的 CORS 为 Any。tracing/healthz 已有，不是完全无日志或健康检查；TLS 可由部署层提供，但仓库当前未形成已配置的公网方案。运营限流不能冒充 A 股规则或恢复任意挂单配额。 |
| B10 | 桌面签名、自动更新、更新签名/分发链 | `docs/roadmap.md:18`；`apps/desktop/src-tauri/tauri.conf.json:31` 已有 bundle 打包，不等于签名/更新；Cargo 无 updater 接线。需目标平台、证书与分发决定，不能称桌面壳尚未实现。 |
| B11 | 第二语言/i18n | `docs/open-questions.md:56`；首发中文且不引入 i18n 已确定，其他语言何时加入未定。 |
| B12 | GPU 真正计算内核、GPU 蒙特卡洛回测 | `docs/decisions/0008-gpu-and-compute-offload.md:44`；`packages/engine-gpu/src/lib.rs:39` 只有探测并委托 CPU。实时 GPU decide 当前明确不做，离线回测需单独设计。 |
| B13 | 超长期冷热历史、按年月/区间查询、历史分钟/逐笔/盘口归档 | `docs/decisions/0018-long-running-immutable-timeline.md:494`、`docs/decisions/0018-long-running-immutable-timeline.md:1111`；日 K 已追加保留，冷热落盘及范围查询未落地，永久逐笔/盘口/诊断粒度未决定。 |
| B14 | 多年稳态不可变状态根、全面页级 COW、增量状态摘要、终止计划/信息/诊断历史分层 | ADR-0018 §5/§8 仍为提案；账户页与日 K 分享、活跃计划索引已有，普通 tick 仍构造整会话候选，PlanBook 等仍克隆历史。不能笼统说所有历史每 tick 都深复制，也不能说多年稳态已实现。代码：`packages/engine/src/session/pipeline/shadow.rs:7`、`packages/engine/src/session.rs:1325`、`packages/engine/src/plans/mod.rs:139`。 |
| B15 | 长期计划价格/公告反向唤醒索引、前端日内追加与跨日防重表增长控制 | `docs/decisions/0018-long-running-immutable-timeline.md:568`；策略复核/活跃索引已有，完整反向索引未落地。`apps/web/src/host/protocol/reduce.ts:58` 克隆 accepted Map、`apps/web/src/host/protocol/reduce.ts:72` 复制追加日内帧；终端旧版本/防重保留期仍需定，不能为省内存直接删权威防重事实。 |
| B16 | WAL/持续 checkpoint/可靠投递、durable watermark、强发布或可见回退 | `docs/decisions/0018-long-running-immutable-timeline.md:1150` 是条件方案；ADR-0010 现行明确不引入 WAL/崩溃恢复。需要先决定是否扩大保证，不是当前遗漏。 |
| B17 | 玩家实际观察版本随请求记录、过时观察处理、旧私有版本查询及到期策略 | `docs/decisions/0018-long-running-immutable-timeline.md:1130`、`docs/decisions/0018-long-running-immutable-timeline.md:1180`；普通 generation/seq 门控已有，不等于完整观察令牌与历史私有账户查询。政策尚未选定。 |
| B18 | 2099 年后的日历/规则包延续 | `docs/decisions/0018-long-running-immutable-timeline.md:1192`；当前范围明确到 2099-12-31，延伸规则与官方资料边界未决，不能只扩大整数日期。 |
| B19 | 复杂学习、社会传播、机构自选组合风险模型、多尺度个人锚点、L2 排队/逐档观察增强 | `docs/price-volume-simulation-gap-checklist.md:61`、`docs/price-volume-simulation-gap-checklist.md:63`、`docs/price-volume-simulation-gap-checklist.md:67`、`docs/price-volume-simulation-gap-checklist.md:52` 是研究/扩展候选，不是全部批准。当前已有混合分析、个人记忆、失败衰减、账户回撤与持续计划；不要恢复公共仓位硬上限，也不靠外部资金吸引/补钱制造交易。 |
| B20 | 已核销：日终档修改现金与日内挂单的冲突提问 | 最新 ADR-0025 明确只加载日级档，活动委托仅在内存；不存在正常日终档中现金不足以支撑已有买单的问题。持续个人计划不等于已冻结订单；真正非法的事实显式报错，不自动补钱或撤单。 |

## 4. 代码未模拟，但当前明确不做或显式简化

下列没有完整实现并不构成当前需要推进的任务。

- 分红、增发、回购、送转/拆并股、股东清算分配、除权除息和红利税结算；公司利润留存，
  商业借款/经营收付款照常记账。依据：`docs/company-actions-design.md:3`、`docs/decisions/0024-shrinking-investor-cash-pool.md:28`。
- NPC 收入/进入退出或资金重置、税费返还、维持固定流动性/成交量的补钱机制；Q12 已核销。
  真实市场历史导入/实证行情校准也不在范围，虚拟前史不是伪造本局逐笔。
- 新股上市初期无涨跌幅限制、科创板/北交所完整差异、停复牌/复牌竞价、盘中临停、
  退市整理、大宗交易、融资融券及真实券商个性化佣金舍入，见 `docs/trading-rules.md:113`。
- 沪深细分市价申报（对手/本方最优、最优五档、余量转限价、FOK 等）尚未实现；
  当前保护价/余量撤销是明确游戏简化。深市盘中/收盘竞价真实参照差异也尚未修正，
  当前统一昨收并已登记，见 `docs/trading-rules.md:33`、`docs/trading-rules.md:41`。
  若以后修改，需重查当时官方规则/实施通知，不凭本盘点直接实施。
- ETF/指数被动调仓/套利、新板块、内幕/传闻玩法、整个国民经济仿真、特殊保证流动性的
  做市账户均非当前承诺；当前只打磨单局，固定两局服务端不意味着当前要做多局压力/隔离项目。
- 结构化银行/合同挂钩工具、FVTPL/FVOCI/证券化/套期、再保险、投连/分红险、保险保费分配法/
  浮动收费法，以及合同获取现金流摊销等超支持模式，见 `docs/company-accounting.md:213`。
- 会计完整准则扩展：多层集团、长期股权投资权益抵销、合并 scope 差错更正、保险/银行/地产
  未支持的开局子账种子等仍受明示简化/拒绝边界约束。不是四行业会计整体没写，
  也不意味着真实所有会计业务已支持，见 `docs/company-accounting.md:51`、`docs/company-accounting.md:148`。
- 旧格式迁移/旧引擎兼容、本地存档加密/反作弊、恢复共同 V、按来源/账户号恢复全局顺序、
  自由并发不同运行逐字节一致，不作为待补功能。

## 5. 规则数据、证据或验收缺口，不算游戏功能代码未实现

- 官方休市事实：默认 `CalendarPolicy::default_v1` 的 `official_coverage` 为空，2026 通知
  仍待原文取证，历史年份亦有模拟标签；插入官方覆盖的代码与模拟回退已有。
  见 `docs/simulation-calendar.md:35`、`packages/engine/src/calendar/policy/mod.rs:54`。
  “不用真实市场数据”不等于可以编造交易所制度/休市依据，本次未联网补证。
- 部分会计/税务/披露官方原文仍 blocked；对应已支持计算不应据此全判未实现。
  CAS 6 无形资产摊销在材料中有取证提及，但本次未确认独立产品业务入口已获批准，
  不把它擅自增加为必须实现的合同/资产类型。
- 稳定整轮多线程收益、多年历史年龄矩阵、真实 Worker/Remote/Tauri 与跨日 E2E、
  移动浏览器/Wayland 截图、release 分发/平台级 E2E、覆盖率与独立最终门禁仍有证据缺口。
  SSR/Node/一次性能配对不替代真实宿主验收。
- 旧计划指定的 `scripts/simulation/host-parity.mjs`、`release-contract.mjs`、
  `verify-plan.mjs` 未找到；现有 probes/短测试不自动满足完整验收驱动。
  这些是验收工具代码缺口，不混入玩家功能，见
  `docs/superpowers/plans/2026-09-13-resolve-blockers-wayland.md:93`。
- 银行收本成功/ECL 零变化/到期计息/365 天边界、工商零金额分支等旧复核记录仍有测试增强
  观察，当前全仓是否已被别的测试覆盖不能只靠旧文件名判断。本次不运行回归。
- 调度/计划写入时点尚需有界场景核查，不把“使用局部登记 Mutex”或“修改可丢弃候选”
  本身判成缺功能/权威状态泄漏；接受时点、真实依赖和单点提交要分别核对。

## 6. 总控复核后排除的过时报法

- “Server 仍被 8 MiB 门槛卡住”：已过时，当前是引擎 512 MiB 加 1 MiB 封装空间；
  见 `apps/server/src/routes.rs:37`、`docs/decisions/0019-draft-market-scope-and-capacity.md:29`。
- “决策/诊断时钟漏计午休”：已修，`packages/engine/src/session/observation_clock.rs:4`
  添加午休间隔，`packages/engine/src/session/decision_chain.rs:497` 使用它；旧诊断说明未全更新。
- “公司经营/披露、混合基本面、个人经历/关注发现、母单并入计划完全没实现”：
  已有生产代码，原案和量价清单头部已声明历史记录不能直接变待办。
- “MACD/KDJ 没有 UI”“跨股撮合还没并行”“jemalloc 只停留文档”：均不准确；
  分别已有 Web 指标、股票流并行和 Linux/macOS 分配器接线，A09 只差计算搬至 Rust。
- “所有 Server 接口无鉴权”：不准确，报告/诊断/WS/暂停偏好等已有 token 检查；
  B07 记录的是其余 HTTP 路径与完整公网身份体系还未闭合。
- “同 tick 调度不同导致成交不同就是缺接收事实”：不能直接成立，较新 ADR 允许实际
  局部受理先后随并发变化；也不能据此宣称所有接收/候选时点已通过完整验收。
- “PlanBook 生命周期动作在 P3 前执行就必然错误”：当前是私有候选，且对应股票待反馈会阻挡
  相关动作，新子单母单按 P4 接受事实安装；未复现具体反例，不作为已确认代码缺口。
- 旧 `Account::total_assets`/`book_mut` 方法名没有原样落地、历史 ID 分配/深度字段演进、
  旧 `schema_version` 删除记录等，不自动等于当前资产汇总、撤单、ID 或存档功能缺失。

## 7. 全文阅读覆盖清单

下列各批由独立读文 subagent 完成，业务文档全部报告 1..EOF、截断已补读。
括号数字为盘点开始时文件内容的换行计数（用于覆盖记录，不是实现/验收结果）。
各轮是分配编号；实际按完成结果补充空位，最多 20 批同时在途。

| 批次 | 全文阅读的业务文档 |
|---|---|
| R1.01 | `README.md`（182）；`docs/roadmap.md`（18）；`docs/work-status.md`（55） |
| R1.02 | `DESIGN.md`（129）；`UX-CONTRACT.md`（94）；`design/ui/mobile/qa/README.md`（5） |
| R1.03 | `docs/open-questions.md`（152）；`docs/decisions/0023-synthetic-history-and-matching-only.md`（61）；`docs/decisions/0024-shrinking-investor-cash-pool.md`（53） |
| R1.04 | `docs/decisions/0019-draft-market-scope-and-capacity.md`（60）；`docs/decisions/0022-symbolic-limit-prices.md`（73） |
| R1.05 | `docs/decisions/0018-long-running-immutable-timeline.md`（1390） |
| R1.06 | `docs/decisions/0017-escrow-parallel-tick.md`（157）；`docs/decisions/0009-call-auction-and-intraday-axis.md`（38）；`docs/trading-rules.md`（117） |
| R1.07 | `docs/decisions/0016-fundamental-factor-model.md`（119）；`docs/company-actions-design.md`（94） |
| R1.08 | `docs/company-accounting.md`（251）；`docs/simulation-calendar.md`（125） |
| R1.09 | `docs/price-volume-simulation-gap-checklist.md`（305） |
| R1.10 | `docs/diagnostics.md`（68）；`docs/causal-diagnostics.md`（49） |
| R1.11 | `docs/decisions/0005-unified-engine-three-deployments.md`（144）；`docs/decisions/0010-unified-host-protocol-and-local-refresh.md`（104） |
| R1.12 | `docs/decisions/0006-npc-strategy-module.md`（244）；`docs/decisions/0011-market-time-observations-and-position-risk.md`（103）；`docs/decisions/0012-retail-observation-to-target-position-loop.md`（65） |
| R1.13 | `docs/decisions/0013-retail-experience-memory.md`（51）；`docs/decisions/0014-closing-call-auction.md`（25）；`docs/decisions/0015-parent-order-execution.md`（47） |
| R1.14 | `docs/decisions/0008-gpu-and-compute-offload.md`（142）；`docs/decisions/0020-native-allocator-for-concurrent-ticks.md`（47）；`docs/decisions/0021-strategy-position-choice-and-noise-pricing.md`（111） |
| R1.15 | `docs/decisions/0002-engine-rust-wasm.md`（48）；`docs/decisions/0003-backend-rust.md`（38）；`docs/decisions/0004-frontend-state-redux-toolkit.md`（41） |
| R1.16 | `docs/decisions/0007-three-deployment-frontend-framework.md`（83）；`docs/architecture.md`（165）；`docs/tech-stack.md`（64） |
| R1.17 | `docs/testing.md`（170）；`docs/test-cleanup-checklist.md`（116）；`CONTRIBUTING.md`（91） |
| R1.18 | `AGENTS.md`（115）；`docs/principles.md`（92）；`docs/error-handling.md`（137） |
| R1.19 | `.omo/plans/company-information-npc-intentions.md`（713） |
| R1.20 | `.omo/plans/escrow-parallel-engine.md`（362） |
| R2.01 | `docs/decisions/0000-template.md`（37）；`docs/decisions/0001-record-architecture-decisions.md`（41）；`docs/superpowers/README.md`（11） |
| R2.02 | `docs/git/AGENTS.md`（18）；`docs/git/daily-workflow.md`（22）；`docs/git/initialization.md`（11） |
| R2.03 | `apps/web/README.md`（36）；`scripts/performance/README.md`（23）；`.github/pull_request_template.md`（46） |
| R2.04 | `.omo/HANDOFF.md`（88）；`docs/superpowers/specs/2026-09-11-company-information-handoff.md`（85）；`docs/superpowers/2026-09-13-company-information-archive.md`（122） |
| R2.05 | `.omo/notepads/company-information-npc-intentions/issues.md`（1373） |
| R2.06 | `.omo/notepads/company-information-npc-intentions/learnings.md`（1474） |
| R2.07 | `.omo/notepads/company-information-npc-intentions/problems.md`（78）；`.omo/notepads/company-information-npc-intentions/decisions.md`（7）；`docs/superpowers/specs/2026-09-13-company-information-problems.md`（95） |
| R2.08 | `docs/superpowers/specs/2026-09-13-company-information-issues.md`（1391） |
| R2.09 | `docs/superpowers/specs/2026-09-13-company-information-learnings.md`（1499） |
| R2.10 | `docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md`（700） |
| R2.11 | `docs/superpowers/plans/2026-09-24-single-world-multithreading.md`（187） |
| R2.12 | `docs/superpowers/plans/2026-09-25-production-entry-and-thread-pool.md`（79）；`docs/superpowers/plans/2026-09-26-ready-receipt-and-thread-boundary.md`（69）；`docs/superpowers/plans/2026-09-26-sparse-continuous-book-feedback.md`（30） |
| R2.13 | `docs/superpowers/plans/2026-06-29-account.md`（860）；`docs/superpowers/specs/2026-06-29-account-design.md`（152） |
| R2.14 | `docs/superpowers/plans/2026-06-29-initial-positions.md`（525）；`docs/superpowers/specs/2026-06-29-initial-positions-design.md`（149）；`docs/superpowers/specs/2026-06-29-gameconfig-design.md`（120） |
| R2.15 | `docs/superpowers/plans/2026-06-29-market.md`（663）；`docs/superpowers/specs/2026-06-29-market-design.md`（152） |
| R2.16 | `docs/superpowers/plans/2026-06-29-money-fixed-point.md`（789）；`docs/superpowers/specs/2026-06-29-money-fixed-point-design.md`（113） |
| R2.17 | `docs/superpowers/plans/2026-06-29-orderbook.md`（868）；`docs/superpowers/specs/2026-06-29-orderbook-design.md`（141） |
| R2.18 | `docs/superpowers/plans/2026-06-29-session.md`（954）；`docs/superpowers/specs/2026-06-29-session-design.md`（250） |
| R2.19 | `docs/superpowers/plans/2026-06-29-strategy-impl.md`（913）；`docs/superpowers/specs/2026-06-29-strategy-impl-design.md`（183） |
| R2.20 | `.omo/drafts/resolve-blockers-wayland.md`（58）；`.omo/plans/resolve-blockers-wayland.md`（64）；`docs/superpowers/plans/2026-09-13-resolve-blockers-wayland.md`（161） |
| R3.01 | `.omo/drafts/k7-deterministic-multicore-utilization.md`（167） |
| R3.02 | `.omo/evidence/company-information-npc-intentions/task-29-review.md`（256） |
| R3.03 | `.omo/evidence/company-information-npc-intentions/task-11-review.md`（332） |
| R3.04 | `.omo/evidence/company-information-npc-intentions/task-25-review.md`（191） |
| R3.05 | `.omo/evidence/escrow-parallel-engine/task-11/execution-log.md`（299） |
| R3.06 | `.omo/evidence/company-information-npc-intentions/task-28-review.md`（89） |
| R3.07 | `.omo/evidence/company-information-npc-intentions/task-26-review.md`（260）；`.omo/evidence/escrow-parallel-engine/task-10/divergence-audit.md`（28）；`.omo/evidence/company-information-npc-intentions/worktree-baseline.md`（9） |
| R3.08 | `.omo/evidence/company-information-npc-intentions/task-12-review.md`（168）；`.omo/evidence/company-information-npc-intentions/task-33-review.md`（26） |
| R3.09 | `.omo/evidence/company-information-npc-intentions/task-14-review.md`（230）；`.omo/evidence/company-information-npc-intentions/task-3-review.md`（21） |
| R3.10 | `.omo/evidence/company-information-npc-intentions/task-13-review.md`（141）；`.omo/evidence/escrow-parallel-engine/task-9/historical-witness-audit.md`（50） |
| R3.11 | `.omo/evidence/company-information-npc-intentions/task-10-review.md`（253）；`.omo/evidence/company-information-npc-intentions/task-27-review.md`（54）；`.omo/evidence/escrow-parallel-engine/task-9/corpus-diff.md`（9） |
| R3.12 | `.omo/drafts/escrow-parallel-engine.md`（173）；`.omo/evidence/escrow-parallel-engine/task-3/d6-d7/README.md`（58）；`.omo/evidence/company-information-npc-intentions/task-22-review.md`（29） |
| R3.13 | `.omo/evidence/company-information-npc-intentions/task-15-review.md`（170）；`.omo/evidence/company-information-npc-intentions/task-1-review.md`（27）；`.omo/evidence/escrow-parallel-engine/task-12/validation.md`（23） |
| R3.14 | `.omo/evidence/company-information-npc-intentions/task-16-review.md`（121）；`.omo/evidence/company-information-npc-intentions/task-36-review.md`（57）；`.omo/evidence/company-information-npc-intentions/task-34-review.md`（24） |
| R3.15 | `.omo/evidence/company-information-npc-intentions/task-24-review.md`（52）；`.omo/evidence/company-information-npc-intentions/task-30-review.md`（58）；`.omo/evidence/company-information-npc-intentions/task-7-review.md`（19） |
| R3.16 | `.omo/evidence/company-information-npc-intentions/task-8-review.md`（162）；`.omo/evidence/escrow-parallel-engine/task-8/acceptance-map.md`（33）；`.omo/evidence/escrow-parallel-engine/task-3/d6-d7/structured-comparison.md`（51） |
| R3.17 | `.omo/evidence/company-information-npc-intentions/task-9-review.md`（203）；`.omo/evidence/company-information-npc-intentions/notepad-recovery.md`（63）；`.omo/evidence/company-information-npc-intentions/task-2-review.md`（26） |
| R3.18 | `.omo/evidence/company-information-npc-intentions/task-20-review.md`（92）；`.omo/evidence/escrow-parallel-engine/task-3-8-smoke.md`（69）；`.omo/evidence/company-information-npc-intentions/task-21-review.md`（24） |
| R3.19 | `.omo/evidence/company-information-npc-intentions/task-19-review.md`（93）；`.omo/evidence/company-information-npc-intentions/compatibility-removal.md`（80）；`.omo/evidence/company-information-npc-intentions/task-4-review.md`（22） |
| R3.20 | `.omo/evidence/company-information-npc-intentions/task-17-review.md`（110）；`.omo/evidence/escrow-parallel-engine/F3/manual-qa/README.md`（78）；`.omo/evidence/company-information-npc-intentions/task-27-manual-continuation.md`（25） |
