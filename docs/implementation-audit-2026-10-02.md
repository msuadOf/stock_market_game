# 历史需求到生产代码的实现审计（2026-10-02）

## 1. 基线、范围与判定

- 源码基线：`4e64dad8efac7a762108fe4aeb91c7e84f51f6bb`，分支 `fix/synthetic-history-policy`。
- 本报告补充并优先于 [旧缺口盘点](implementation-gaps.md) 的历史完成度描述；不删除旧证据，不把旧 A01–A11 整项重开。
- 本轮任务是查漏与文档登记，不是实现授权。没有修改游戏代码，没有运行游戏单测、构建、完整回归、浏览器或性能验收；下列“测试”均指已有测试源码或下一步应补的代表性验证。
- 按最多 20 个并发 subagent、每批 1–3 篇文档全文阅读；长文连续分段读至 EOF。主控用源码搜索定位后阅读原文、调用者和生产实现，并对关键发现安排交叉复核。
- “全文阅读”与“每个历史断言均已证明”不同。本文把确认缺口、已实现的契约族、未来范围、验收债和不能定性的冲突分别登记，不以未勾选框、旧符号消失或纯函数测试证明生产功能缺失/完成。
- 这是指定基线、指定文档集合的静态审计，不是“整个程序不再有任何未知缺陷”的保证。后续代码变化需重新核对；行号均指上述基线。
- 本次不重新联网核验交易所制度，不修改 A 股规则。金额为分、数量为股；界面手数只是换算。DCF、个体策略参数是游戏模型，不冒充交易制度或真实市场校准。

### 最新决定优先

ADR-0023：开局前虚拟历史，内部 day=0 起实际撮合，不使用真实行情校准。
ADR-0024：现金池可以减少，不补钱、返费或保证成交。
ADR-0025：仅成功自然日日结保存，启动/明确换档才读档；内部候选回滚不是公共日内存档。
ADR-0026：机构个人阈值、真实经历、暂停买入和本人观察恢复，不自动强卖。
ADR-0019：当前单局重点，不恢复任意订单条数配额。
ADR-0027/0028：运行时宿主选择、无 Node 部署、七个手动入口、有效标签 Release/Pages；普通 commit/PR 不自动 CI，签名暂不做。
ADR-0018 整体仍为 proposed；只有已被后续接受的具体实施目标才列为现行缺口。

## 2. 已确认的现行缺口

状态均为“未修复，静态确认”；分组不是运行失败复现或性能优先级证明。
“未接线”指模块/类型可能已有，但生产路径没有完成承诺；“行为错误”不能靠补一个空接口解决。

### 2.1 宿主与远程链路

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G01 | 浏览器远程 HTTP/WS 可用且鉴权；ADR-0005 §6、ADR-0027 | `apps/web/src/host/remote-request.ts:10` 将 token 放 query，`apps/web/src/host/remote-host.ts:42` 使用原生 WebSocket；`apps/server/src/routes.rs:1068` 只读 Authorization。浏览器该构造方式不发送这个自定义头，双方契约不一致。修复必须保留鉴权，不能以放开私有路由代替兼容传输。 |
| G02 | 三宿主实际倍率；ADR-0005 §5、UX-CONTRACT 模拟控制 | `apps/web/src/host/remote-host.ts:218` 的 GET speed 没有凭据，`apps/server/src/routes.rs:909` 要求凭据；`apps/web/src/App.tsx:523` 是实际消费路径。 |
| G03 | remote push/pull 都可持续取帧；ADR-0005 §6、ADR-0010 | `apps/web/src/host/remote-host.ts:221` 只切模式/重连，没有生产 GetFrame 发送循环；`apps/server/src/routes.rs:1286` 的 pull 分支等待该命令。服务端测试手动拉帧不证明浏览器适配器已实现。 |
| G04 | baseline 仅初始化、读档、显式重同步；ADR-0010 §统一更新 | 继续模拟调用 `apps/web/src/App.tsx:725` 的 start；Remote `apps/web/src/host/remote-host.ts:180`、Tauri `apps/web/src/host/tauri-host.ts:171` 重发缓存 baseline，普通协议更新不推进缓存。协调器 `apps/web/src/host/protocol-coordinator.ts:75` 重新安装旧状态，后续 tick/seq 可能不连续。这里“继续”不是从磁盘读档。 |
| G05 | 心跳失活处理与重连；ADR-0005 §6、ADR-0010 宿主能力 | `apps/server/src/routes.rs:1243` 发 Ping，但无 Pong 到期判据；`apps/web/src/host/remote-host.ts:150` 意外断线转 fatal，没有自动恢复链。手动 start 能重建连接，不应写成完全没有连接能力。 |
| G18 | Worker uiFrame 背压；ADR-0010 宿主能力/不做 | `apps/web/src/host/wasm-worker.ts:124` 每步直接 postMessage；`apps/web/src/host/worker-host.ts:214` 直接交付 callback，未见可证明有界的消费背压或等价合并机制。定时让出执行机会不等于消费背压；不限定必须ACK，不据此声称已测得卡顿。任何合并只能处理允许覆盖的绘图采样，不删除新协议要求保留的提交帧与交易事实。 |
| G19 | Tauri 固定高倍率 tick 在 Rust 聚合后约 16ms 发布；ADR-0010 | `apps/desktop/src-tauri/src/actor.rs:670` 固定倍率调用 run_cycle(1)，生产发布直接 emit；`apps/desktop/src-tauri/src/actor.rs:1041` 的聚合 helper 位于 cfg(test)。Fastest 批次已有，不能误报整个宿主完全不聚合。 |

### 2.2 策略、个人信息与估值

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G06 | 五年权益现金流折现；公司计划 K5/K5a，`docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md:150` | `packages/engine/src/strategy/fundamental/valuation.rs:159` 逐年增长 CF，却每年仅除一次 1+r，而终值折现五年。真实路径为个人观察→BeliefBook→cash_flow；不是未使用代码。恒定 FCFE=17,820,000 分、r=10%、g=gt=0，现有 gold=191,648,179 分，逐年折现应约 178,200,000 分，差异不是分级舍入。注释及 gold 同时固化了错误，修复需独立推导而非为过测试改预期。 |
| G07 | 身份不决定分析能力，散户可分析基本面及五路权重；公司计划 K5、任务17/26，ADR-0016 | `packages/engine/src/session.rs:2120` 仅给机构装配个人分析/信念；散户仍走 `packages/engine/src/strategy/zi_noise.rs:193` 旧行为内核。散户 AnalysisProfile 金样存在不代表真实散户消费。不是要求每个散户都估值，也不是给全部散户套机构执行复杂度。 |
| G08 | 失败日期与每20交易日无新受挫衰减；ADR-0013 计划契约修订、公司计划 K5 | 散户观察 `packages/engine/src/session/pipeline/decision_snapshot_capture.rs:348` 和成交 `packages/engine/src/session/pipeline/retail_projection.rs:480` 仍走旧 writer；`packages/engine/src/behavior/heuristics.rs:88` 消费未衰减计数。dated writer/failure_influence 有纯逻辑代码但未接此链；机构 ADR-0026 已有自己的衰减，不应重做。 |
| G09 | 本人已知完整年报及可用中期更新；公司计划 K5a:148 | `packages/engine/src/session/decision_chain.rs:846`、`packages/engine/src/session/decision_chain.rs:904` 只投递新年报；`packages/engine/src/strategy/fundamental/update.rs:254` 拒非年报。历史 issue 将中期更新后置22/26评估，并未取消。需中期补充/修订路径，不可把季报直接当全年报表。 |
| G20 | 新局从熵取种、测试可固定；ADR-0005 §4 | `apps/web/src/App.tsx:292` 无初始档时固定使用 `apps/web/src/config/defaults.ts:121` 的 42n。存档 RNG、固定 seed 注入已存在；缺的是普通新局取种，不要求自由并发同 seed 整局字节一致。 |

### 2.3 行情显示与日历边界

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G10 | 连续竞价一分钟成交量；DESIGN:88、UX-CONTRACT:47 | `apps/web/src/app/useMarketChartRuntime.ts:16` 用累计量减上一帧，`apps/web/src/mobile/market-model.ts:564` 同分钟替换；同分钟累计100→200→200最终只留0。首个连续点还可能把开盘累计量算入连续分钟。旧 collector 测试没有覆盖这条实际 hook。 |
| G11 | 新日清旧分时且保留新日已到采样；UX-CONTRACT:81 | `apps/web/src/app/useMarketChartRuntime.ts:109` 重建刚结束交易日帧，后续按分钟键合并，缺日期隔离；新日未覆盖的槽保留旧点，量基数也未重置。读档/新局清理不能代替正常日界清理。 |
| G12 | 分时量涨红空心、跌绿实心；UX-CONTRACT:54 | `apps/web/src/app/useMarketChartRuntime.ts:23` 连续点固定 buy=true，竞价以价格非空判断；`apps/web/src/mobile/MobileStockDetail.css:126` 红柱仍实心。这里审计的是展示涨跌语义，不把 buy 字段误称真实主动买卖方向。 |
| G13 | 逐笔展示最近成交；DESIGN:126、移动QA | `apps/web/src/store/store.ts:119` 最新在前，按股票过滤保序，但 `apps/web/src/mobile/MobileStockDetail.tsx:155` slice(-7).reverse 选较旧端。100条带不是全天完整流水，不能通过无限积累来修。 |
| G14 | 逐笔时间对应真实成交；DESIGN:126 | `apps/web/src/mobile/MobileStockDetail.tsx:220` 所有行统一使用当前 elapsedMinutes，而非各自成交 tick；跨分钟旧成交显示当前时间。 |
| G15 | 官方年度覆盖替代模拟回退；`docs/simulation-calendar.md:59` | `packages/engine/src/calendar/holidays.rs:76` 官方年度存在但当日不在休市区间时仍叠加模拟假日，无法撤销模拟结果。默认 official_coverage 为空，不能声称默认局已触发；这是非空覆盖输入的代码边界，不要求导入真实市场行情。 |

### 2.4 工程、交互和发布

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G16 | 普通tick不随多年历史线性复制；`docs/superpowers/plans/2026-09-24-single-world-multithreading.md:60` | `packages/engine/src/session.rs:1481` 克隆全部 plans；`packages/engine/src/plans/mod.rs:139` 为普通 BTreeMap，终止只移活跃索引，历史仍在。计划根另有 clone。缺所有权优化，不等于允许删除历史，也不证明已实测性能不达标。 |
| G17 | Rust指标与Rayon生产加速；ADR-0008 D2 | 指标功能已实现，但 `packages/engine/src/indicators.rs:207` 批量并行函数只有测试调用；三宿主调用单请求顺序计算函数。缺并行生产接线，不把整个A09重报未实现；是否值得对单股并行要先测量。 |
| G21 | 基线CLI只取setup、不验证其余档字段；`docs/price-volume-simulation-gap-checklist.md:265` | `packages/engine/examples/price_volume_baseline.rs:31` 先完整反序列化SaveSlot，无关快照/订单字段非法也拒绝。该工具已有诊断输出，缺输入投影契约。 |
| G22 | 表单即时/字段级错误；`docs/error-handling.md:114` | `apps/web/src/App.tsx:797` 校验失败仅全局notice，`apps/web/src/App.tsx:1027` 数量框无字段错误关联。不是静默吞错或全站无错误详情。 |
| G23 | 进入详情聚焦返回、返回聚焦原列表；UX-CONTRACT Flow ledger | `apps/web/src/App.tsx:211` 与返回回调仅更新状态；交易底页有焦点管理，但不能代替详情导航焦点。需浏览器短验收确认修复。 |
| G24 | 信息标签切换保持滚动；UX-CONTRACT:69 | `apps/web/src/app/useMobileUiController.ts:71` 切换后主动scrollIntoView。与约定冲突，不是信息内容业务未实现。 |
| G25 | 返回/切股至少44px点击热区；DESIGN:86 | `apps/web/src/mobile/MobileStockDetail.css:190` 后续规则把切股宽度限制22–26px，返回所在列30–35px；未见扩大点击区域。可见图标可以小，点击区域契约仍需实现。未做本轮像素测量。 |
| G26 | 前端CI warning作为错误；`docs/tech-stack.md:23` | `apps/web/package.json:9` 仅oxlint，配置存在warn，CI直接调用，不设置warning失败门槛。Rust -D warnings已有。不涉及恢复普通commit/PR自动运行。 |
| G27 | 发布拒绝标签移动；ADR-0028 | `scripts/publish-release.mjs:83` 上传前查SHA，资产上传/校验后 `scripts/publish-release.mjs:109` 直接公开，未在公开前重查。记录的是守卫窗口，不声称线上发生过错误发布；再次查询也不等于平台级原子不可变标签保证。 |

### 2.5 补充逐章核对发现

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G28 | 固定集团合并报表进入完整交付；ADR-0016:97、公司计划K3/任务12–13/完成条件 | `packages/engine/src/accounting/closing/mod.rs:332` 生产只构造Standalone；`packages/engine/src/accounting/reports/mod.rs:201` 有Consolidated分支，但未见会话结账/披露构造该请求。合并算法和五产物测试已有，不等于固定集团的生产发布已接线。单层等简化及合并更正不支持，不取消普通合并报告承诺；不要求默认每家公司都有子公司。 |
| G29 | 无NPC时跳过初始流通盘分配；initial-positions spec§2.4、计划Task3 | `packages/engine/src/session.rs:1095` 对正流通盘的ByKind先要求现存NPC类别有效权重和>0，零NPC必被拒绝；`packages/engine/src/session.rs:1913` 的空NPC早退无法到达。此为可配置新局边界，不声称默认20,007户受影响，也不建议凭空分配给玩家。 |
| G30 | 图表窗口按钮可见键盘焦点；DESIGN:82、UX-CONTRACT:56 | `apps/web/src/mobile/MobileStockDetail.css:218` 用未定义的--msd-focus覆盖已有outline，计算时失效。应补有效焦点样式；按钮有名称/SSR存在不证明焦点可见。 |
| G31 | 图表只随当前股票历史刷新；UX-CONTRACT:88 | `apps/web/src/app/useMarketChartRuntime.ts:133` 每批无条件复制当前三组历史数组，`apps/web/src/app/MarketRuntimeProvider.tsx:79` 因新引用广播；当前股票没变化也失去引用隔离。Provider拆分未保证当前股票数据引用隔离；未实测性能幅度。 |
| G32 | 昨收中轴与0.00%位置一致；DESIGN:90 | `apps/web/src/mobile/MobileStockDetail.tsx:126` 昨收位于SVG中点；`apps/web/src/mobile/MobileStockDetail.css:79` 中轴用整个容器50%，SVG高度却扣除23px时间轴。两坐标系中点差11.5px；此为样式推导，未做浏览器像素验收。 |
| G33 | 移动端字号随容器宽度响应；DESIGN:64/127 | `apps/web/src/mobile/MobileStockDetail.css:199` 起的后置规则将主报价、报价摘要和盘口字号固定为px，覆盖此前clamp/cqw。部分组件仍响应式，不能误报整个页面都未适配；320/390/430px需代表性验证。 |
| G34 | 交易底页遵守减少动态效果偏好；DESIGN:104、UX-CONTRACT:95 | `apps/web/src/App.css:524` 底页保留300ms transform transition；唯一reduce覆盖在 `apps/web/src/mobile/MobileStockDetail.css:171`，仅作用详情子树，底页在其外。缺样式覆盖，不声称本轮肉眼观察过动画。 |


### 2.6 公司与计划生产闭环补漏

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G35 | 工商折旧、所得税及商业债务支付进入经营闭环；公司计划K3/任务8、14 | `packages/engine/src/company/operations/industrial.rs:43` 日常流未调用折旧/所得税/付息处理器，`packages/engine/src/session.rs:2453` 期末直接封账；开局已配置固定资产寿命及商业借款（`packages/engine/src/session/company_assembly.rs:355`）。`packages/engine/src/company/operations/dispatch.rs:42` 已接利息计提，不能误写成利息完全未实现；缺的是相应期末处理/支付生产调度，而非股东分红或投资者补钱。 |
| G36 | 四行业可自定义并经公共报告查询跑通；公司计划K3:104、任务26 | `packages/engine/src/session/company_assembly.rs:296` 全部装配工商；会话封账遍历公司调用books_mut（`packages/engine/src/session.rs:2453`），`packages/engine/src/company/operations/config.rs:40` 对非工商unreachable。银行/保险/地产经营和报表内核已有，但自定义行业的完整会话装配/封账/披露仍未贯通；不要求增加默认股票，也不声称默认局已触发该panic。 |
| G37 | DEV因果记录关联实际订单ID和计划变化；公司计划任务35 | `packages/engine/src/session/plan_chain_candidates.rs:337` 用观察快照的计划簿记录；`packages/engine/src/session/decision_chain.rs:983` 固定传空events，而order_ids只从events抽取。`apps/web/src/dev/NpcDecisionInspector.tsx:81` 将空列表标“未提交”，不能反映随后真实执行。DEV入口/隔离已实现，缺事实关联，不应把未成交伪装成交或向产品快照泄露私有状态。 |
| G38 | 本人预算区分已有计划续行与新机会；公司计划K6:165/任务22 | `packages/engine/src/session/decision_chain.rs:1803`、:2040 的生产请求均标ExistingPlan；`packages/engine/src/plans/allocation.rs:115` 有分类优先级却收不到新机会类别。新旧买计划争用本人有限现金时仍按信心排序，未接“续行优先”。这是同一账户软预算，不得扩为跨账户/来源类撮合优先；风险暂停不等于自动强卖。 |

### 2.7 验收工具契约

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G39 | K7按现行并发受理契约验证；Sept24多线程计划:154 | `scripts/simulation/run-escrow-verification-matrix.mjs:755` 仍要求不同worker完整artifact相等；`packages/engine/examples/escrow_verification_harness/committed.rs:494` 只固定入队脚本，:530调用真实生产step，并未重放同一已发生受理轨迹。`packages/engine/src/session/pipeline/local_admission.rs:282` 保留真实局部受理顺序。故“同seed/脚本”不能保证四类artifact完全一致。此为工具适用契约遗漏，未运行矩阵或声称实际失败；不能为通过而删除断言，应保留守恒/价时/依赖/失败负控，并单独验证同一受理事实的重放。 |

## 3. 候选项与契约冲突：不能冒充已确认漏实现

| ID | 事实与证据 | 处理边界 |
|---|---|---|
| Q01 | Money为i64裸JSON整数，`packages/engine/src/money.rs:40`；Web `apps/web/src/host/protocol/wire-values.ts:48` 要求安全整数；配置仅校验现金非负。 | 跨端支持范围未统一登记。需选择共同范围或无损编码，不直接要求删掉Web精度守卫，也不以默认金额小证明无风险。 |
| Q02 | 公司计划K5要求个人读取公开历史留记录；`packages/engine/src/experience/price_memory.rs:138` 的record_public_history_read仅测试调用，技术日K已用于真实候选。 | 确认未接调用，但应按“实际主动读取”而非每次共享缓存构建记账；实际消费边界需明确，不能伪造未观察经历。 |
| Q03 | 日历/会计文档要求RegulationProfile冻结；存档已有强制匹配a-share-simulation-v2的simulation_policy_id。 | 未发现允许跨政策恢复却被覆盖的路径。不能仅因缺同名结构判缺功能；应明确ID与冻结规则集合的关系。 |
| Q04 | UX-CONTRACT:60要求应用标题固定，`apps/web/src/app/useMobileUiController.ts:20` 详情显示股票标题。 | 这是文档/交互选择冲突，不擅自把当前标题行为认作交易错误。 |
| Q05 | 测试清理清单登记部分脚本helper只有测试调用，scripts测试未统一自动发现。 | 验收工具覆盖策略需单独收口；不得凭旧函数名不存在重建已经被新验证器替代的整套工具。 |
| Q06 | 初始持仓spec§2–3要求ByKind比例和≈1、类内随机；`packages/engine/src/session.rs:1087` 允许正有效权重归一化，散户还筛选eligible并用Pareto分配（:1994）。 | 容差及分布的最新批准依据未定位；先明确当前政策与旧spec关系，不要求为旧算法回退。独立于G29零NPC校验矛盾。 |
| Q07 | 周K/月K现按5/20交易日分组，`apps/web/src/mobile/market-model.ts:336`；UX仅列周期名称，未明确分组算法。 | 当前已有图表，但是否应按公历周/月及合成负时间历史衔接需明确；不能宣称已核实真实周月口径。 |
| Q08 | 移动MA及分时均价仍由前端推导，`apps/web/src/mobile/MobileStockDetail.tsx:87`、:135；移动QA要求指标来自引擎。 | MACD/KDJ已由Rust返回；需区分允许的展示派生与权威指标，尤其均价口径。不是凭此证明伪造行情，也不能写“全部指标均来自Rust”。 |
| Q09 | ADR0006扩展承诺与sealed策略注册；ADR0008的ComputeMode/positions Vec旧路线与现行协议不同。 | 库级接缝不等于生产后端切换；扩展方式与优化范围待文档明确，不自动授权重构或GPU。见R04/R18。 |
| Q10（已转G39） | 二次核对fixture→runner→生产step，确认未冻结实际受理轨迹。 | 不再作为未定产品方向；保留编号记录分类变化，运行结果仍待验收。 |
| Q11 | 公司计划K5a:154允许会计更正/离散违约直接重估并记事件ID；`packages/engine/src/strategy/fundamental/update.rs:26` 有原因原语，生产仅NewMaterial/HorizonExpired。 | 更正年报仍会走普通λ修订，不是完全不更新；没有Correction/CreditDefault专门分发。需明确何时选直接重估与真正违约信号，不能把CreditDeterioration信用恶化自动当违约，不能要求所有公告一律重估。 |

## 4. 最小验证缺口与现有测试入口

下表是后续修复的代表性验证方向，不表示本轮新增或运行了测试；普通case和整命令遵守10秒上限，不能借审计启动全回归。

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
| G26/G27 | `scripts/ci-workflow.test.mjs`、`scripts/publish-release.test.mjs` | warning退出状态；上传期间标签变化保留draft并明确失败，不改普通提交触发政策。 |
| G28/G29 | `packages/engine/tests/consolidation/`、`packages/engine/tests/industry_reports/`、`packages/engine/tests/session.rs` | 固定集团报告由真实日终生成并公开、无集团明确不适用；零NPC/正流通盘/ByKind与Random对照，仍拒绝非法权重。 |
| G30–G34 | 移动组件及chart runtime测试、`apps/web/e2e/mobile-layout.spec.ts` | 可见焦点、未变股票保持图表引用、中轴坐标一致、容器字号及reduce偏好；不能仅SSR或源码字符串断言。 |
| G35–G38 | `packages/engine/tests/industrial_accounting/`、`packages/engine/tests/company_operations/`、`packages/engine/tests/diagnostic_parity.rs`、`packages/engine/tests/plan_allocation/` | 代表性月结折旧/所得税/商业债务支付、四行业自定义会话日结查询、DEV真实订单关联、新旧买计划有限现金竞争；纯处理器测试不替代生产入口。 |
| G39 | `scripts/simulation/escrow-verification-contracts.test.mjs`、`scripts/simulation/run-escrow-verification-matrix.test.mjs` | 区分固定受理事实重放与自由调度，合法局部顺序差异不误判，同时仍拒绝资金/股份/价时/依赖错误。无需本轮运行完整K7。 |

## 5. 已实现与旧要求核销

- A01–A11的主要生产能力已经存在：公开财报、远程查询方法、个体机构风险、冻结UrgencyPolicy、日终最小存档、内存日结回滚、工作台拖拽、错误详情、Rust指标、账户/订单增量及DEV当前宿主诊断。G项是局部断链/边界，不能用局部已实现证明整个宿主或整个策略模块无缺陷。
- 共同隐藏V/TrackV、资金循环/补钱、公共日内存档、任意挂单配额、旧格式兼容、按来源固定交易优先、自由并发整局字节一致已经被替代或明确排除。
- `AllocationExperience::default()` 不单独列缺口：机构信心已在上游按真实失败/净获利退出调整，成本与风险也走个人阈值；再接旧失败helper会重复扣信心，不能恢复统一20日强卖。
- 360根负时间虚拟日K、真实成交更新量额、T+1/费用/占用、开收盘撮合、符号最高/最低限价、初始持仓、账户结算、计划执行与日终子单清理均有生产实现。
- 8MiB远程存档上限、午休时钟遗漏、公共财报期间格式、WASM空值、旧测试使用玩家快照查NPC等历史发现已经有后续修正；不沿用旧REJECT或保留二进制失败判断当前源码。
- 七个手动入口、三平台打包、纯Server/WebUI Server、Release、Pages与缓存清理已有代码及后续发布记录，见 `docs/build-and-deployment.md:300`。本轮没有重新请求GitHub或重新验证线上状态。
- 普通commit/PR不自动CI、macOS/Windows不签名是用户决定，不是待恢复的缺口。

## 6. 确实未完成但不属于现行必做

保留 [旧盘点B表](implementation-gaps.md#3-确实没有完整实现但需确认范围或属于未来扩展) 的范围，不自动启动未来产品：

| 类别 | 未完成能力 / 当前边界 |
|---|---|
| 行情与内容 | 五日分时/跨日分钟查询、更多周期和均线配置、看点/资讯/社区/简况、首页资金/资讯/资产/分析快捷页、更多分类；按钮禁用或占位，不能称完整实现。独立收盘竞价曲线尚无，收盘撮合已实现。 |
| 玩家产品 | 多存档槽管理、成就、完整个人交易流水/复盘与云同步。当前快速槽和100条成交带不是这些功能。 |
| 部署与运维 | 公网账号/多人归属、数据库/迁移/重启恢复、完整TLS/Origin/运营控制、签名/公证/自动更新。已有私有会话token，不等于账号体系。 |
| 计算与长期架构 | 实际GPU内核/蒙特卡洛、冷热历史/区间查询、页级COW、反向唤醒索引、WAL/durable水位、旧观察令牌/保留期、2099年后规则；G16已接受的局部所有权目标与未接受整套ADR0018方案分开。 |
| 领域扩展 | 股东分红/增发/回购/清算、额外市场板块/订单类型/停复牌/融资融券、高级银行保险/集团会计模式、复杂学习/社会传播/组合风险、第二语言；具体简化见交易规则与会计文档，不按旧愿望清单一并实现。 |

### 仅缺数据依据或验收证据

官方休市原文覆盖、部分会计/税务依据仍有取证债；不使用真实行情不等于可以编造制度。
稳定多线程收益、历史年龄矩阵、三宿主跨日真实旅程、安装器GUI/运行库兼容、移动/Wayland视觉、完整统计与最终独立门禁不能以短测或源码存在核销。
旧host-parity/release-contract/verify-plan名称未找到，现有WASM导出、制品manifest、K7验证各有不同覆盖范围；缺的是未被替代的真实验收能力，不要求按旧名重复造工具。

### 文档漂移另行登记

旧ADR0008仍称Rust指标/跨股撮合“待落地”，但主要能力已有；旧财报披露模块注释称封账不可达，实际日终已经接线。
量价命令示例有遗漏diagnostics feature；因果诊断文档仍称决策时钟漏午休，代码已修。
报告批准08:00的假设登记、年报日期fixture算术文字也需校正，不能混成新的会计产品。
旧Money设计的负数舍入示例、GameConfig佣金最低额示例有算术错误；ADR0020状态、ADR0008旧规模耗时及positions Vec路线需与当前实现区分。
量价清单的散户异常选股仍是未来扩展，机构曝光权重不能证明散户已接线；tick-only诊断不能证明完整公司自然日经营/披露验收。
本次不逐篇改写历史证据，只通过最新审计入口解释适用关系。

## 7. 需求覆盖对照

本节由各批补充的章节/任务对照收口；“已实现”仅表示该契约族存在生产路径，未承诺通过当前基线运行验收。局部反例以G/Q表为准。

表中 R01–R20 对应本轮 20 个并发文档组；S01–S06 对应上轮其余全文批次。
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
| S03 | ADR0027/0028、build-and-deployment、actions-cache、ci-build-fixes全部目标/权限/运行边界 | `.github/workflows/`、`scripts/build-targets.mjs`、`scripts/publish-release.mjs`、`scripts/prune-actions-cache.mjs` | 七按钮、三平台制品、Pages、标签发行和清理已有；G27守卫窗口。普通提交无自动任务/不签名是决定；线上状态未在本轮重验。 |
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
