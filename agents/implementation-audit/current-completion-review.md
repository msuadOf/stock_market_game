# 当前任务完成度独立复核

## 范围与结论

本复核按用户要求只读，不运行测试/编译、不读 Cargo、不查 Git。全文阅读根 AGENTS.md、docs/principles.md、docs/implementation-gaps.md、agents/implementation-audit/implementation-audit-2026-10-02.md 与 agents/remaining-questions-and-features/future-implementation-contracts.md 至 EOF；针对当前生产代码核对关键 caller、state owner、协议/Host consumer 和 UI。源码搜索用于定位，未把无匹配单独当作不存在证明。

本文件早期基线叙述与P8／Q14旧状态均为历史快照。当前Simple状态以本轮Q14／D01更新和[公司系统实施清单](../company-system/implementation-checklist.md)、[接线记录](../company-system/session-integration.md)、[最终复核](../company-system/decision-sync-final-review.md)为准；不要把旧host51／host55失败快照或早期UI恢复工作树当作当前生产状态。

范围内结论：确认G已完成限定修复，不代表Q及全部未来任务完成。永久历史Core12项、Host请求1项、Native恶意存档1项、Web严格schema6项、五日UI6项及私人日期Core6／Web7项已短绿并独立复核，见[Core复核](../retained-history/core-independent-review.md)及[日期交割单](../retained-history/trade-date-pagination.md)。实时分钟Core当前7项通过并独立复核，见[live Core](../retained-history/live-core-review.md)；Host Web5／Server route1的限定短测及接线复核通过，见[live Host复核](../retained-history/live-hosts-independent-review.md)。MA当前36项短绿并由非作者复核，见[MA独立复核](../market-indicator-settings/review.md)。真实Protocol双档fresh fixture、restore深等及恢复续行三帧真实NPC受理已通过；root75 WASM构建与root78最终Web bundle检查通过，详见[公司系统交接](../company-system/current-handoff.md)与实施清单。以上均不代表完整回归、长期浏览器验收、跨Host E2E或Windows／macOS runtime已执行。

本轮Simple状态更新以对应Q14／D01记录、[公司系统实施清单](../company-system/implementation-checklist.md)、[接线记录](../company-system/session-integration.md)和[最终复核](../company-system/decision-sync-final-review.md)为准：当前HEAD为`04d3c49e`，Simple代码基线为`a001681d`。`8d07a779`周期算法、`e488cbfe`四类财务／披露、`ce5b3a1c`Session严格当前契约、`f1bc6f84`native hosts、`a001681d`Web接线均已提交。required cfg kind→issuer→finance/chart已串通；Simple按配置支持Monthly／Quarter／HalfYear／Annual，利润由营收、固定开支和变动开支按所选期间变化推导，不以月度为完整范围上限；年化使用定点half-even，四类noise独立。editable seed preset只影响Day 0，可用price/share及明确虚拟P/E、P/B反推，不设后续anchor或涨跌保证。无SyntheticFunding、公司实际资金跟踪、schema兼容或migration。股本cash cap=false实际caller未接，公司行为偏好仍缺；Simulation在用户另分支，Q14／D01仍未整体核销。验证：root 70 Rust／typegen 128、Web 45、root 76五个tiny case、root 75 all-targets与WASM＋Vite build、root 77 Panel 7项与tsc通过；root78 Web types、Vite 6.88秒、Release WASM check通过；未跑完整回归或Windows／macOS runtime。旧SessionGroup/shock integration仅为futureSim门禁，不算Simple通过。

## 真实缺失代码

| 优先级/范围 | 当前已由生产调用链确认 | 尚缺的最小非 stub 交付 |
|---|---|---|
| P1 行情历史/B02、P9历史查询 | packages/engine/src/session/retained_history.rs 保存真实成交分钟 OHLCV并进入成功日终档；日期分页经 ProtocolSession、Server actor 的本人 membership 查询、/api/market-history、RemoteHost、Tauri command/actor、WorkerHost/WASM export、EngineHost 和 MarketRuntimeProvider 接至 web。RetainedHistoryPanel 已接桌面/移动 LocalRefreshViews：日期范围分页、最近五个已结束开市日、稀疏成交、休市/零成交/未结束区分、空错状态与切换范围请求失效。Core12／Host1／Native guard1／Web schema6／五日UI6及私人日期Core6／Web7限定短测已绿并复核。实时分钟Core当前7项通过并独立复核；Host Web5／Server route1限定短测通过，见对应live复核记录。 | 已知短测范围已通过；普通浏览器/Remote/Tauri/Server 非空成交全宿主矩阵、长期浏览器验收、完整回归及Windows／macOS runtime未执行，作为验证限制保留。不得再写live新增2阶段case待fresh；这不核销其他历史、容量或所有市场历史未来范围。 |
| P2 周期/均线 | 自然日／周／月／季／年及120／60／30／15／5／1分钟K折叠选择已由两端共享ChartPeriodTabs／MarketKlinePanel消费，实际caller与聚合短测见[周期实施](../kline-periods/implementation.md)。MA默认5／10／20／30／60，可编辑周期、逐线开关及单Redux owner／双消费者／BigInt计算均已接线。 | MA专项36项短测及非作者完整复核通过，见[MA独立复核](../market-indicator-settings/review.md)。未执行完整回归、长期运行或全平台视觉矩阵；这属于验收边界，不再将MA实现或复核写成尚未完成。保留旧数量聚合已取消及不恢复独立Mobile绘图owner。 |
| P3 收盘集合竞价/B05 | Engine 与协议产生收盘竞价阶段事件；retained_history.rs 只从真实 Event::Trade 生成分钟 OHLCV，AuctionCompleted 对应实际竞价成交落在 900 分钟槽；日 K 收盘仍来自真实结算结果。 | 收盘 AuctionTick 的完整指示价/累计量序列未被历史分钟 owner 留存；apps/web/src/mobile/market-model.ts 的 PricePointCollector、AuctionPointCollector 也跳过 ClosingAuction。补所需真实权威收盘竞价投影/保留、坐标、量能、结束清算结果、空态及移动 UI，不能把单个成交分钟柱冒充整段指示价曲线。撮合不缺，不改撮合器。展示前官方核验沪深时段及参考价差异，并登记来源/日期及简化。 |
| P4 内容与入口/B04 | 移动详情已有财务、盘口、资金统计；财报和玩家资产已有可复用模块。 | MobileStockDetail.tsx 的看点/资讯/社区/简况仍为占位，MarketGrid.tsx 的资金/资讯/资产/分析快捷页与更多分类仍 disabled。需先定内容源/权限，再连真实页面和路由；不能编造资讯或接入真实市场统计。 |
| P5 复盘/Q08 | 完整逐 Fill PersonalTradeConfirmation、日终存档恢复及旧本人分页已有；新增按日期/证券/方向筛选、as-of receipt 稳定分页的 engine query 接入 Server/Remote、Worker/WASM、Tauri 与 PersonalTradeHistoryPanel 日期复盘 UI。页面仅读本人真实 Fill/实际费用并拒绝盈亏因果猜测。 | 日期 query Core 6/6、Host44 限定短测通过，Web 7/7 通过，完整非作者 review PASS。收到的证据是短测/限定接线验收，不等于公共 EOD、跨 Host restart 等复杂验收。若复盘要超出日期/证券/方向查询，新增产品决策再做。 |
| P6 成就/云同步 | ArchiveStore、多槽列表/重命名/复制/删除/明确读取及 IndexedDB/Native/Server 存储已有。 | 成就消费模块与云同步产品链仍缺。成就需规则及依据，并只派生展示；云需日终档账号范围、上传/下载、冲突/恢复交互及保留策略，不能同步活动挂单或日内快照。 |
| P7 多人收口 | Server 共享会话、主体到账户 membership、身份、owner 事件投影、本人历史查询、多槽及日终 writer 已有生产调用链；不可再报成“多人/身份/数据库全无代码”。真实Protocol双档fixture由release Engine生成，包含真实恢复深等，主场景恢复后继续三帧并产生NPC真实受理；root75 WASM构建及root78最终Web bundle检查通过，见[公司系统交接](../company-system/current-handoff.md)及[实施清单](../company-system/implementation-checklist.md)。 | 测试／构建通过不等同完整产品验收。长时间Browser验收、全Host真实多人入队／撤单／保存／重开E2E、长期运行及Windows／macOS runtime未执行，仅作为验证限制；按当前caller不再列Protocol fixture或WASM bundle为待实现。Remote权威engine仍在Server，不另跑浏览器WASM。 |
| P8 Q14经营／D01股本 | Simple当前已接入核心财务／披露生产链，具体提交、Period、noise、Day 0 preset及真实调用链见本节状态摘要与[公司系统实施清单](../company-system/implementation-checklist.md)。Simple不跟踪公司实际资金；分红准入依可分配利润／方案，认购看投资者本人现金，回购依真实委托。Simulation由用户另分支，本轮不实现。 | `股本 cash cap=false`实际caller未接，公司行为偏好仍缺；分红及其他股本行为规则仍须依官方来源核验并形成生产owner。股息税底层11项绿及复核仅是低层模块且无生产caller，不等同D01完成。整项Q14／D01未核销；完整回归和Windows／macOS runtime未执行。 |
| P9长期历史/运行/B13–B18 | 分钟/日 K/交割单保存及按日期分页的 engine、三宿主、web UI 查询链已有。CivilDate 可表示至 2199，但默认 CalendarPolicy 到 2099。 | 仍需按长期规模需要完成热/冷分层、年代/规模/恢复测量；2100+默认日历需规则来源和 CalendarPolicy 扩展；旧 observation token 生命周期未定义。全面页级 COW、所有反向索引不是 ADR-0018 提案中逐项强制照搬的目标。WAL、durable watermark、实时模拟 GPU 明确不做。 |
| P10 部署/平台/B09–B11 | 身份认证、授权和 SQLite/IndexedDB 已有；Tauri 桌面壳及发布构建已有。 | Server 缺明确可信 Origin/TLS 部署责任与运营级限流/指标接线；桌面 updater 缺渠道、包校验、下载/失败提示与 UI；第二语言缺完整 locale/UI/error/辅助文本与切换。需部署/平台参数先定；代码签名/公证保持排除。 |
| B12 离线 GPU 回测 | engine-gpu 当前以探测/CPU 委托为主，实时模拟 GPU 已明确不做。 | 真正 GPU 计算内核及 GPU Monte Carlo 回测仍属未来能力，需先确定目标硬件、任务/API 和收益标准，再独立实现/测量；不可把“实时 GPU 不做”扩成核销离线回测，也不可把 CPU 委托称 GPU 已实现。 |
| 未来大 A 制度/B19候选 | 当前已支持规则按 docs/trading-rules.md 登记了简化边界。 | 不可把旧 B19 当一个获批大包；特殊板块、额外证券类型/订单类型、停复牌/临停、融资融券逐项取交易所/中国结算官方规则、记录适用日期及是否纳入，再实现 engine/存档校验/UI/边界。尚未批准的逐项保持待定而不是核销。 |
| B19 复杂策略研究项 | 当前已有混合分析、个人记忆、失败衰减、账户回撤与持续计划等生产能力。 | 复杂学习/社会传播、机构自选组合风险、多尺度个人锚点、L2逐档观察仍属未核销扩展；逐项定义可观察输入、状态 owner、是否会改变委托/资产，再按用户批准范围实施。不得恢复公共仓位硬上限、真实市场数据或补钱。 |

## 只欠简单验证或复杂验收

这些不属于“代码未实现”；按用户本轮要求不启动长回归、平台运行或视觉矩阵。

| 目标 | 代码状态 | 仍缺验证证据 |
|---|---|---|
| B08/ADR-0033 日终数据库 | 浏览器 IndexedDB 与 Native SQLite repository 已进入生产链；Server 启动读取已选档、actor 日终候选写入 SQLite，明确新局/选档入口在 apps/server/src/main.rs 与 deployment.rs。ADR-0033 已 accepted，文档自身注明平台验证尚待。 | 代码已有，不再列数据库/迁移为零实现；Windows/macOS 同文件锁、故障恢复和真实 Server restart 属平台/故障验收，未做不能宣称通过。SQLite 不引入 WAL、旧档迁移。 |
| 确认 G 台账 | agents/implementation-gap-implementation/final-integration-review.md 记录限定 79 项 G 已补齐、G 剩余 0；历史静态发现不能重开。 | 当前总控仅需简单核对关键修复对应定向短测/非作者记录，不把 79 项报告外推为整仓无 bug。 |
| Q08 流水/VWAP | 权威 Fill receipts、原本人分页以及新增日期筛选分页的三宿主/UI 生产 caller 已接入；Core 6/6、Host44、Web 7/7 限定短测通过且完整非作者复核 PASS；Web tsc exit 0。 | 公共 EOD/跨 Host 全链是复杂验收限制，不是当前应重写 caller 的缺失；本轮不要求长验收。 |
| Q22 多人共享市场 | Membership、一次AdmissionFunding、owner投影及真实busy ingress生产caller已有，限定短测及独立复核按总账登记。真实Protocol双档fixture由release Engine生成，实际restore深等且续行三帧产生真实NPC受理；Web45合同短测通过。root75 WASM构建、root78 final bundle及Release WASM产物检查通过，证据见[公司系统交接](../company-system/current-handoff.md)及[实施清单](../company-system/implementation-checklist.md)。 | 长时间Browser验收、全Host真实多人入队／撤单／保存／重开E2E、长期运行及Windows／macOS runtime未执行，作为验证限制保留，不再把当前WASM／Protocol fixture接线列作待收口代码。未执行E2E不能反推生产功能不存在，也不把限定短绿外推为完整多人产品验收。 |
| Q23 税务/报告 | 年税幂等、四行业若干 owner、月/季频率及更正链已有多个实现子项。 | 集团/跨年级联、结构化子账更正、官方税务依据及最新WASM／Protocol EOD契约同步仍须逐项收口；跨HostE2E未执行仅作验证限制，不是代码完成门禁。已记录“仅实现局部”不得改称整个 Q23 完成。 |
| Q15 视觉参考 | 对比度/文字颜色修复已有。 | 缺原始同花顺手机版参考和真实截图视觉比对，是验收材料缺失，不是上述 CSS 修复没有代码。 |
| 跨平台/长期运行 | 产品主干、构建脚本和短测存在。 | Windows 实机、长年限规模/恢复、多核收益、桌面安装器 GUI、发布运行库兼容、完整回归均不在本轮简单核对内；它们是复杂验收，不能算实现缺失，也不得宣称通过。 |

## 需要用户确认的事实

以下仅问当前文档/现行决定无法推出的值或产品选择；已由 ADR-0034 决定分钟量价、日 K、完整交割单全部保留，五日仅展示，不再询问。已答决 Q01–Q25 亦不重问。

| 范围 | 精准待确认项 |
|---|---|
| Q14经济模型 | 利润按所选配置期间内营收、固定开支与变动开支变化推导；Simple支持月、季、半年、年，不把月度设为完整范围上限，也不独立生成net margin／利润目标。Simple与Simulation共享完整财务、披露及股本目标；Simple仅账面展示、不跟踪公司实际资金、不设SyntheticFunding。分红检查可分配利润／方案但不因展示现金拒绝，认购检查投资者本人现金，回购依真实委托成交。用户允许可编辑虚拟preset。当前Simple具体状态与测试证据见本节状态摘要；公司行为偏好与cash cap=false实际caller仍未完成。Simulation由用户另分支，不因此核销其范围。 |
| D01 股本行为 | 用户已授权实施分红及其他股本行为，先核验现行官方规则及适用日期，显式处理沪深／证券类别差异。Simple不跟踪公司实际资金，展示现金不作为分红拒绝条件但仍检查可分配利润／方案；认购检查投资者真实现金，回购依真实委托。公司行为偏好及cash cap=false实际caller未完成；底层股息税11项绿无生产caller，不能核销D01。 |
| B02/B13历史产品 | 分页/API 的技术实现可由开发决定；仍需确认是否要对玩家提供按日期浏览/导出，以及浏览器与 Server 的历史导航/容量体验。数据事实全部保留已定，不询问保留期限。 |
| B04内容 | 资讯只引用公司公开披露还是允许明确标为虚构的游戏事件；社区是单人笔记还是多人 UGC；首页分析指标和更多分类清单。多人内容的可见性/权限不可自行假设。 |
| B05规则 | 官方现行沪深收盘集合竞价时段、指示价及参考价差异待研究并登记来源/适用日期；不是询用户替代法源核查。若规则存在游戏无法实现部分，询是否接受明确简化/不支持。 |
| B06产品 | 成就条件；复盘最小形态（过滤/统计或成交上下文）；云账号范围、冲突裁决、恢复方式与保留策略。日终档同步边界不变。 |
| B09/B10/B11 | 部署的域名/Origin/TLS由代理还是 Server 承担、运营指标与限流口径；updater目标 OS/渠道；第二语言纳入时间及语言顺序。签名/公证不纳入。 |
| B14/B17/B18/B19 | 长期年龄/容量目标和导出删除体验；旧观察 token 有效期/过期反馈；2099 后规则来源；额外市场制度的获批范围。这些不阻塞 P1–P8 的独立实现。 |

## 独立复核判定

1. 大 A 语义：本轮文档同步不改变交易规则或股本业务语义。分钟量按股、金额按分；分钟与日K使用真实成交事实。D01仍须依官方规则核实，不能从短测推导公司行为规则已经实现。
2. 必要性和最小范围：历史查询复用retained_history／Protocol现有接缝；Q14 Simple当前接通按配置月／季／半年／年驱动的汇总财务与披露，不强迫建立完整客户现金模拟；股本实际结算和公司行为偏好仍按P8缺口推进。Simulation由用户另一分支推进。D01投资者真实资金／股份／税务与回购成交不能被展示模型绕过，未定方案参数不得默认补齐。
3. 边界与跨层：永久历史及本人日期查询的生产Host／UI caller已接通；live Core七项、MA 36项及Protocol双档恢复fixture均有对应独立复核，root75 WASM构建与root78最终bundle通过。未执行长时间Browser、跨Host E2E、长期运行和Windows／macOS runtime只作为验证限制保留。Q14／D01实际生产股本owner未收口。审查止于列明范围，不是全仓无缺陷保证。

本轮校正：Q14 Simple五个源码提交与限定验证见本页状态更新；`股本 cash cap=false`实际caller未接，公司行为偏好仍缺。D01底层税务当前11项绿且有复核，先前报告的P1第7项已修复；该底层仍无生产caller，故分红税与公司行为生产链尚未闭环。Q14／D01均不整体标记完成。当前主工作树source baseline用对应实施记录的提交为准；历史UI基线／暂存文件数量只用于说明早期审计快照，不代表当前产品来源状态。

本次复核者未运行测试／构建；产品提交与验证基线以本页状态及链接证据为准。永久历史Core12、Host1、Native guard1、Web schema6、五日UI6及私人日期Core6／Web7限定短测通过并独立复核。实时分钟Core当前7项通过并独立复核；Host Web5／Server route1限定短测及接线复核通过，见[live Core](../retained-history/live-core-review.md)与[live Host复核](../retained-history/live-hosts-independent-review.md)。MA36项通过并独立复核，见[MA复核](../market-indicator-settings/review.md)。Protocol双档fixture真实恢复深等与三帧NPC受理、Web45、root75 WASM、root78 final bundle及Release WASM check均通过，见[公司系统交接](../company-system/current-handoff.md)及[实施清单](../company-system/implementation-checklist.md)。长时间Browser、跨Host E2E、长期运行及Windows／macOS runtime未执行；D01税底层11项绿仍无生产caller。不能将限定通过外推为完整产品验收或无未知缺陷保证。
