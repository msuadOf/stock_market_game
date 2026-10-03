# 历史需求到生产代码的实现审计（2026-10-02）

## 1. 基线、范围与判定

本报告核对历史需求与生产实现，补充并优先于 [旧缺口盘点](../../docs/implementation-gaps.md) 的完成度描述；保留旧证据，不把 A01–A11 整项重开，也不将审计视为实现授权。

当前源码基线为主工作区已提交的 `2247f4f`，复核日期为2026-10-03，已合入 `.worktree/implementation-reaudit` 的 `docs/implementation-reaudit` 分支。初次全文审计针对 `4e64dad`，OOP复核针对 `8cf34a1`，后续发布政策及代码已核对至 `7198348`。本轮完整核对 `7198348..2247f4f` 的9个变化文件：7个测试文件、`diagnostics.rs` 对自由调度报告的注释修正，以及 `diagnostics/causal/microstructure.rs` 等价合并嵌套模式匹配；没有新增生产功能或需求文档变化，因此未变路径承接此前代码复核，不冒充全仓重读。核对期间新提交的诊断整理已一并纳入，未跟踪的 `agents/main-release-validation/` 不属于此提交基线；主工作区未由本任务改写。结论不随其他工作区后续修改自动更新。

全文阅读覆盖 142 个跟踪 Markdown 路径、3 份历史草稿及2份已删除文档，采用20个并发 subagent、每批1–3篇连续读至 EOF。更新后的复核承接这份需求映射，重新追踪 owner、调用方和结果消费，并对新增提交逐项核对实现与政策变更，而非把对象抽取或测试增加视为功能完成。第2节给出当前缺口及详细复核记录；R/S/H逐篇记录和第7节历史映射保留原审计基线，来源与版本关系见 [覆盖清单](coverage-index.md)。

“生产已接”仅针对所列契约，不保证整模块无缺陷。确认缺口、待定需求、未来范围、文档漂移与验收证据分别登记，不用未勾选框、旧符号消失或纯函数测试证明生产功能缺失或完成。本轮没有修改游戏代码，只做源码差异与文档静态核对，未运行游戏测试、构建、浏览器、完整回归、性能矩阵或发布流程。此前发布脚本与工作流契约的4个定向短测文件通过，属于上一基线复核的结果；本轮未重跑，不将测试源码或其他任务的验收记录写成本轮通过。

本次未重新联网核验制度，沿用文档中登记的 A 股规则及简化；金额为分、数量为股，界面手数仅作换算。DCF和个体策略参数是游戏模型，不冒充交易制度或真实市场校准。这是指定基线和文档集合的静态审计，不是程序没有未知缺陷的保证。

### 最新决定优先

ADR-0023：开局前虚拟历史，内部 day=0 起实际撮合，不使用真实行情校准。
ADR-0024：现金池可以减少，不补钱、返费或保证成交。
ADR-0025：仅成功自然日日结保存，启动/明确换档才读档；内部候选回滚不是公共日内存档。
ADR-0026：机构个人阈值、真实经历、暂停买入和本人观察恢复，不自动强卖。
ADR-0019：当前单局重点，不恢复任意订单条数配额。
ADR-0027/0028：运行时宿主选择、无 Node 部署、七个手动入口、有效标签 Release/Pages；普通 commit/PR 不自动 CI，签名暂不做。2026-10-03最新决定进一步明确发布仅构建、打包、核验和部署；CI仅保留为手动开发诊断，发布及产品构建不运行测试、lint或smoke。
ADR-0018 整体仍为 proposed；只有已被后续接受的具体实施目标才列为现行缺口。

## 2. 已确认的现行缺口

原G01–G39中，G27已实现并移至第5节核销，其余38项仍有未完成部分；编号保留，不重新排序，未确认新增G。分组不是运行失败复现或性能优先级证明。
“未接线”指模块/类型可能已有，但生产路径没有完成承诺；“行为错误”不能靠补一个空接口解决。

### 2.1 宿主与远程链路

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G01 | 浏览器远程 HTTP/WS 可用且鉴权；ADR-0005 §6、ADR-0027 | 浏览器 WS query token 与服务端 header 认证仍不一致；`remote-request.ts` → `remote-host.ts` → Server `routes.rs`。 修复须保留鉴权，不得通过开放私有路由绕过契约。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G02 | 三宿主实际倍率；ADR-0005 §5、UX-CONTRACT 模拟控制 | Remote `readSpeedMetrics` GET 仍不附凭据；真实 UI 轮询迁至 `useSpeedMetricsPolling.ts`。  当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G03 | remote push/pull 都可持续取帧；ADR-0005 §6、ADR-0010 | Remote 切 pull 后仍没有 `GetFrame` 发送循环。 服务端测试手动拉帧不证明浏览器适配器已接线。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G04 | baseline 仅初始化、读档、显式重同步；ADR-0010 §统一更新 | `App.tsx:362` 暂停后继续再次 `host.start`，Remote/Tauri 重送旧 baseline；Worker 的 generation 守卫原已存在。 缓存不随 delta 推进，重新交付可能回退状态及游标；这里的继续不是从磁盘读档。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G05 | 心跳失活处理与重连；ADR-0005 §6、ADR-0010 宿主能力 | Server 无 Pong 截止判据，Remote 意外 close 仍转 fatal 而非自动恢复。 手动 start 可以重建连接，但不等于自动恢复。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G18 | Worker uiFrame 背压；ADR-0010 宿主能力/不做 | `WasmTickLoop.publish` 每步发完整更新，仍无消费者驱动的有界背压。 定时让出执行机会不等于消费背压；任何合并不得丢弃协议要求保留的提交帧及交易事实，未实测卡顿。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |
| G19 | Tauri 固定高倍率 tick 在 Rust 聚合后约 16ms 发布；ADR-0010 | Desktop 固定倍率仍 `run_cycle(1)` 后直接 emit；Fastest 批次不等于固定倍率聚合。  当前路径、行号及调用链见[宿主](reaudit-host.md)。 |

### 2.2 策略、个人信息与估值

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G06 | 五年权益现金流折现；公司计划 K5/K5a，`docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md:150` | `strategy/fundamental/valuation.rs:159` 五年显式现金流仍各只折现一次。 恒定 FCFE=17,820,000 分、r=10%、g=gt=0 时，现有 gold=191,648,179 分，逐年折现应约 178,200,000 分；差异不是分级舍入。修复需独立推导，不能只改测试预期。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G07 | 身份不决定分析能力，散户可分析基本面及五路权重；公司计划 K5、任务17/26，ADR-0016 | 基本面认识和五路信号仍只装配到机构链；Retail 仍走 ZiNoise/retail 内核。 不是要求所有散户都估值，也不是赋予全部散户机构执行复杂度。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G08 | 失败日期与每20交易日无新受挫衰减；ADR-0013 计划契约修订、公司计划 K5 | 散户观察/成交仍走 legacy writer，dated writer/衰减没有进入对应消费链；不重做已有机构衰减。 不能用已有机构 ADR-0026 衰减核销散户链路，也不应重复实现机构衰减。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G09 | 本人已知完整年报及可用中期更新；公司计划 K5a:148 | `decision_chain/roots.rs` 仍只将年报投递到信念更新，中期补充链缺失。 中期材料须作为补充或修订，不能直接当全年报告。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G20 | 新局从熵取种、测试可固定；ADR-0005 §4 | `useSessionHostLifecycle.ts:90` 普通新局仍使用固定 `DEFAULT_SEED=42n`。 存档 RNG 和固定 seed 测试注入已存在；不要求自由并发同 seed 整局字节一致。 当前路径、行号及调用链见[宿主](reaudit-host.md)。 |

### 2.3 行情显示与日历边界

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G10 | 连续竞价一分钟成交量；DESIGN:88、UX-CONTRACT:47 | `market-chart-projection.ts` 仍以每帧累计量差替换分钟量，而非累加该分钟量。 同分钟累计100→200→200最终会只留0；首个连续点还可能带入竞价累计量。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G11 | 新日清旧分时且保留新日已到采样；UX-CONTRACT:81 | 分时合并仍只按日内槽位，无正常日界隔离。 新日尚未覆盖的分钟槽会保留旧日点；新局或读档清理不能代替正常日界处理。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G12 | 分时量涨红空心、跌绿实心；UX-CONTRACT:54 | 连续点方向固定 true、竞价按非空判方向，涨量柱仍非红色空心。 这里审计价格涨跌展示，不把 buy 字段称为真实主动买卖方向。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G13 | 逐笔展示最近成交；DESIGN:126、移动QA | `MobileIntradayProjection` 仍对最新优先成交数组取 `slice(-7).reverse()`。 100条成交带不是全天流水，不得以无限积累修补方向错误。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G14 | 逐笔时间对应真实成交；DESIGN:126 | 逐笔仍共用当前 `tradeTime`，不取各笔成交时间。  当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G15 | 官方年度覆盖替代模拟回退；`docs/simulation-calendar.md:59` | `calendar/holidays.rs:72` 官方覆盖当年非假日仍落入模拟回退。 默认 official_coverage 为空；这是非空覆盖输入边界，不声称默认局已触发，也不要求真实行情数据。 当前路径、行号及调用链见[基础](reaudit-foundations.md)。 |

### 2.4 工程、交互和发布

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G16 | 普通tick不随多年历史线性复制；`docs/superpowers/plans/2026-09-24-single-world-multithreading.md:60` | 全历史 PlanBook 复制移至 `RootReadContext::capture`；Arc 共享该副本不等于消除复制。 缺的是所有权优化，不意味着可以删除历史；未测量性能幅度。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G17 | Rust指标与Rayon生产加速；ADR-0008 D2 | Rust 指标已有，三宿主仍只调单项函数；Rayon batch 仍仅测试调用。 缺批量生产接线，不把指标功能整体重开；是否值得对现有负载并行须先测量。 当前路径、行号及调用链见[基础](reaudit-foundations.md)。 |
| G21 | 基线CLI只取setup、不验证其余档字段；`docs/price-volume-simulation-gap-checklist.md:265` | `price_volume_baseline.rs:29` 仍完整反序列化 SaveSlot 后才取 setup。 工具输入投影与公共 SaveSlot 深度验证是不同契约，不应放宽业务读档校验。 当前路径、行号及调用链见[工具](reaudit-tools.md)。 |
| G22 | 表单即时/字段级错误；`docs/error-handling.md:114` | 表单错误迁至 `useTradingCommands` 后仍只有全局 notice，缺字段关联。 错误并未静默吞掉；缺的是即时、字段级反馈。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G23 | 进入详情聚焦返回、返回聚焦原列表；UX-CONTRACT Flow ledger | 详情进入/返回仍只变状态，没有对应导航焦点恢复。 交易底页焦点管理不能代替详情导航；后续需浏览器短验收。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G24 | 信息标签切换保持滚动；UX-CONTRACT:69 | 信息 tab 仍调用 `scrollIntoView`。  当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G25 | 返回/切股至少44px点击热区；DESIGN:86 | 返回/切股横向点击区域仍小于约定 44px。 可见图标可以小，但热区应满足契约；本轮未做像素测量。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G26 | 手动开发CI的前端warning作为错误；`docs/tech-stack.md:23` | Web lint 仍裸 `oxlint`，未设置 warning 失败门槛。 Rust -D warnings 已有；仅指手动开发CI，不要求恢复普通 commit/PR 自动运行或发布链路lint。 当前路径、行号及调用链见[工具](reaudit-tools.md)。 |

### 2.5 补充逐章核对发现

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G28 | 固定集团合并报表进入完整交付；ADR-0016:97、公司计划K3/任务12–13/完成条件 | 日终仍发布 Standalone，未接固定集团合并公开链；等额往来申报仍不保证账面上界。 合并算法和五产物测试已有；单层等简化不取消普通集团报告承诺，也不要求默认每家公司有子公司。Task12 F6要求调用方从真实账簿派生申报，现配对校验仅确认双方等额，不保证未超账面余额；重复成员对双抵及销售金额上界已修，不能混报或声称默认游戏已发生超额抵销。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G29 | 无NPC时跳过初始流通盘分配；initial-positions spec§2.4、计划Task3 | ByKind 正流通盘且零 NPC 仍在 setup 校验阶段拒绝，空 NPC seed 早退不可达。 这是可配置新局边界，不声称默认20,007户受影响，也不应凭空分给玩家。 当前路径、行号及调用链见[基础](reaudit-foundations.md)。 |
| G30 | 图表窗口按钮可见键盘焦点；DESIGN:82、UX-CONTRACT:56 | 图表窗口按钮仍使用未定义 `--msd-focus`。 按钮名称和 SSR 存在不能证明键盘焦点可见。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G31 | 图表只随当前股票历史刷新；UX-CONTRACT:88 | `MarketChartProjection` accessor 每批重建数组，当前股票未变化也更新引用。 Provider 拆分未保证数据引用隔离；未实测性能幅度。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G32 | 昨收中轴与0.00%位置一致；DESIGN:90 | 昨收中轴与图形中点仍使用不同高度坐标系。 当前样式推导中两中点相差时间轴23px的一半，未做浏览器像素验收。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G33 | 移动端字号随容器宽度响应；DESIGN:64/127 | 后置 px 字号仍覆盖关键报价响应式规则，不是全部页面不响应。 局部响应式能力旧基线已存在，不算本次修复；320/390/430px仍需代表性验收。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |
| G34 | 交易底页遵守减少动态效果偏好；DESIGN:104、UX-CONTRACT:95 | reduced-motion 规则仍未覆盖详情外的交易底页。 本轮未运行浏览器观察动画。 当前路径、行号及调用链见[界面](reaudit-ui.md)。 |


### 2.6 公司与计划生产闭环补漏

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G35 | 工商折旧、所得税及商业债务支付进入经营闭环；公司计划K3/任务8、14 | 工商折旧/所得税/商业债务支付调度仍未补齐；已有利息计提、月年封账不重开。 开局已有固定资产寿命与商业借款；缺的是相应处理与支付，不是股东分红或给投资者补钱。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G36 | 四行业可自定义并经公共报告查询跑通；公司计划K3:104、任务26 | 新局仍固定 Industrial，非工商 `books_mut` 不支持；行业不适用冲击仍进入 active 至公告链。 银行/保险/地产经营及报告内核已有，不等于会话装配/封账/披露闭环；不声称默认工商局触发非工商 panic。sample_company_shock 不接收行业类型，银行/保险可能得到不消费的 ProductionInterruption/AssetImpairmentSignal；披露器直接读 active 状态按开始日期公布，丢弃日报告并不能阻止公告。获批只记录的 CreditDeterioration 另列，不视作同一缺口。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G37 | DEV因果记录关联实际订单ID和计划变化；公司计划任务35 | DEV 根诊断仍传空交易 events，无法关联随后真实订单 ID/计划变化。新增诊断查询只读/重复查询稳定测试未补上真实订单关联；DEV入口和隔离已有，不得伪造成交或向产品快照泄露私有状态。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |
| G38 | 本人预算区分已有计划续行与新机会；公司计划K6:165/任务22 | 两个生产预算请求仍全标 ExistingPlan，新机会优先级未获实际分类输入。 仅是同一账户软预算分类，不得扩为跨账户撮合优先；AllocationExperience 默认值不单列缺口，避免重复扣减上游已处理的个人信心。 当前路径、行号及调用链见[策略与公司](reaudit-engine.md)。 |

### 2.7 验收工具契约

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G39 | K7按现行并发受理契约验证；Sept24多线程计划:154 | K7 仍比较不同 worker 完整 artifact，未固定同一实际受理轨迹。新增 Session/规模测试已区分立即恢复等价与自由调度后各自对账，但两个K7比较入口均未改变，不能核销本项。应保留守恒、价时、依赖及失败负控，另验同一受理事实的重放；未运行矩阵。 当前路径、行号及调用链见[工具](reaudit-tools.md)。 |

## 3. 候选项与契约冲突：不能冒充已确认漏实现

Q01–Q09、Q11 在当前基线仍待定；Q10 已转 G39，不重复计数。下表将当前代码事实与需要裁决的契约边界分开，不能把待定方向当成已确认的漏实现。

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

## 4. 最小验证缺口与现有测试入口

下表主要列后续修复的代表性验证方向；G27的既有行为测试已在本轮短测中通过，其余条目不据测试源码宣称通过。普通case和整命令遵守10秒上限，不借审计启动全回归。

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
稳定多线程收益、历史年龄矩阵、三宿主跨日真实旅程、安装器GUI/运行库兼容、移动/Wayland视觉及完整统计不能以短测或源码存在核销。新增 [发布验收记录](../oop-release-validation/summary.md) 已登记本地完整回归、fresh浏览器验收和build-only发布结果；这些是所述基线与场景的历史证据，本轮没有重跑或重新查询线上状态，不能扩张为全部长期/GUI/统计验收通过。
旧host-parity/release-contract/verify-plan名称未找到，现有WASM导出、制品manifest、K7验证各有不同覆盖范围；缺的是未被替代的真实验收能力，不要求按旧名重复造工具。

### 文档漂移另行登记

旧ADR0008仍称Rust指标/跨股撮合“待落地”，但主要能力已有；旧财报披露模块注释称封账不可达，实际日终已经接线。
量价命令示例有遗漏diagnostics feature；因果诊断文档仍称决策时钟漏午休，代码已修。
报告批准08:00的假设登记、年报日期fixture算术文字也需校正，不能混成新的会计产品。
旧Money设计的负数舍入示例、GameConfig佣金最低额示例有算术错误；ADR0020状态、ADR0008旧规模耗时及positions Vec路线需与当前实现区分。
量价清单的散户异常选股仍是未来扩展，机构曝光权重不能证明散户已接线；tick-only诊断不能证明完整公司自然日经营/披露验收。
本次不逐篇改写历史证据，只通过最新审计入口解释适用关系。
补充全文中发现的旧 API/算术/文案差异也不得机械转成新功能：账户总资产已在会话个人权益计算中消费，不要求恢复同名 `Account::total_assets`；Money 小数解析在 ASCII 和长度校验后使用 `expect`，只是与旧计划的禁用写法不一致，未证明存在可触发的解析 panic；初始 HTML 标题 `web` 在 App 首次 effect 后会更新，启动壳与运行时标题范围需区别。逐项主控复核见 [候选核销记录](candidate-checks.md)。

## 7. 需求覆盖对照

本节保留初次全文审计建立的契约族映射及原基线代码位置；当前缺口状态和迁移后的调用链统一见第2、3节链接的复核记录，不以历史行号定位新版本。

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

基线Git跟踪142个Markdown路径，另有3份历史draft；CLAUDE.md是AGENTS.md别名，不重复算独立正文。
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
