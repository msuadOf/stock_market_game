# sweep43：实施盘点与命名规范/验证记录全文复核

目标 `b76ece3`，worktree HEAD `4ad5a2e`。已只读检查目标与当前 `packages/engine/src`、`packages/engine/examples`、`apps/server/tests`、`apps/web/src`及本组三文档差异为空，以下证据行号适用目标提交。根AGENTS/principles已读（承接sweep06）；仅新增本工作记录，没有产品/Git写操作或长测试。

连续阅读全文：`docs/implementation-gaps.md` **259行**、`docs/naming-conventions.md` **48行**、`docs/naming-refactor-validation.md` **62行**，共**369行**。implementation-gaps中输出截断的B14及§4–6另按连续行段补读，§7覆盖表读至EOF；命名两文档全文读完。以下旧A行明确以顶部实施更新与现行G/Q为准，未擅自恢复已退役工具或旧路线。

## 结论

没有新增已确认独立代码遗漏。A01–A11当批主能力均有当前生产caller，原始“尚未实现”描述不能重开；后续遗漏保留既有G/Q。B01与B07私有授权已实现，B20已由ADR-0025核销；其他B项维持未来/提案范围。

命名新标签与producer/consumer已有配对，真实版本号、历史格式、serde spelling保留。历史扩展失败中Server save缺参、Worker错误where已找到当前源码反证，**没有运行测试，不宣称全批翻绿**。两harness执行、成交顺序与超时项仍仅是需最新运行证据的验收状态，不能凭旧失败日志直接建产品G，也不能因改名后代码存在就报通过。

## implementation-gaps 逐章与A全表

| 原文条款/位置 | 状态 | 当前caller证据与后续边界 |
|---|---|---|
| 顶部审计优先级（3）、实施更新日级档/启动一次恢复（11–21） | 已有最新决定，不扩大未来产品 | ADR-0025公共日级档；`session.rs:409` SaveSlot、宿主保存带generation。当前最新G/Q仍优先，本文不是新实施授权。 |
| A01公开完整五产物（26；历史87） | 主能力已有；集团生产G28独立 | `company/query.rs:95/102/104`公开scope/cash_flow/notes，356行从完整ReportSet构造；`ReportNotes.tsx:22/36/37`实际展示范围和附注，`FinancialStatementTable.tsx:22`比较列。不能重开“仅摘要拼表”；默认日终Standalone链仍G28。 |
| A02三宿主公司查询（27；历史88） | Remote正确caller已有；通用WS鉴权G01独立 | `remote-host.ts:279/287`正确/api/companies/报告路径、token、分页/按ID；153行capability true；`company-query-coordinator.ts:77/101/129/209`generation失效与迟到请求门控。不能以另一个WS浏览器边界否定财报路由本身。 |
| A03个人风险/暂停恢复（28；历史89） | 主链已有；G06–09/G38不同环节 | `session/institutional_behavior.rs:58`读取账户暂停记忆；`decision_chain/lifecycle.rs:290`真实assess_recovery消费、296行现行policy。不是所有风险输入仍固定false，也不是assess_recovery仅测试。 |
| A04必填UrgencyPolicy（29；历史90） | 已有权威冻结 | `session.rs:480/1178`权威字段、2665行存、2717行校验、2961行恢复；1448行default仅初始化新会话，不能再报“每次报价构造默认”。 |
| A05最小事实/编辑/代际（30；历史91） | 主能力已有，未来可精简不新开 | `session/minimal_snapshot.rs:13/21` SaveAccountSnap/SaveSnapshot不含展示派生盘口；`session.rs:2594`save_projection；`remote-host.ts:236/240`请求与响应代际、`tauri-host.ts:209/211`同样检查。公开日终无活动挂单不等于内部测试不得保存低层内存快照。 |
| A06日结回滚（31；历史92） | 已生产内存checkpoint | `session.rs:2088`clone_for_tick_shadow，2094行失败整对象回滚；不是SaveSlot备份/restore。observer外部副作用回滚非承诺。 |
| A07桌面拖拽/缩放（32；历史93） | 真实组件已接，浏览器矩阵仍证据 | `app/WorkspaceGrid.tsx:2/55/85`ResponsiveGridLayout/useContainerWidth；不是只有依赖未使用。容器几何/真实拖动仍需浏览器验收，不造云布局同步。 |
| A08完整错误详情（33；历史94） | 生产格式已有，字段级表单仍G22 | `host/wasm-worker.ts:83`workerFailureDetails→postFailureDetails，保留真实where/context/source；`app/HostStatusViews.tsx:16/26`复制反馈实际消费。普通operationError与fatal分开，不要求全升Fatal。 |
| A09Rust MACD/KDJ及桌面周期（34；历史95） | 计算/消费已有；Rayon batch接线G17独立 | `components/PriceChart.tsx:51/70`日K完整输入与切周期runtime更新；Rust指标宿主adapter存在。单项Rust计算不能核销批量生产G17，Q08移动MA/均价口径另列。 |
| A10账户/委托增量（35；历史96） | 主能力已有 | `store/store.ts:61`活动委托baseline按generation/tick/seq核对；77行真实applyRuntimeDelta、`host/protocol/reduce.ts:71/72`实际消费runtime_delta；不是每帧整存档。分时同分钟/跨日G10–14另列。 |
| A11当前host DEV与换档代际（36；历史97） | 主能力已有；真实订单因果G37独立 | `dev/NpcDecisionInspector.tsx:13/23/35/41/44/47`接EngineHost/timelineGeneration，effect清状态、旧success/catch/finally门控；`remote-host.ts:291`真实诊断查询。不是另建WASM局；root空订单events问题仍G37。 |
| 后续A05/TDD/母单旧失败根因（40–60） | 历史验证与明确fixture修正 | 旧snapshot玩家投影读取NPC键不是生产母单丢失。代码已有内部完整投影；不弱化断言、不拓宽公共日内存档、不把历史复测当本轮测试。 |
| §1范围方法与判定优先级（59–78） | 审计方法记录/现行范围 | ADR0023排实证、0024不补现金、0019不任意配额、0018proposed。历史20代理阅读规模不是本轮运行事实。 |
| §2历史A表（80–97） | 逐项已追当前caller | 以上A01–11反证与顶部实施更新合读，不把保留旧表误当现行待办。 |

## B01–B20 全表逐项

| 原文项/行号 | 当前状态与证据 | 结论 |
|---|---|---|
| B01（106）Highest/Lowest | 玩家限价symbol经PlaceLimit受理时解析，ADR0022现行链已有 | 已实现，不造市价/持续追价。 |
| B02（107）五日分时 | `MobileStockDetail.tsx:225/226`disabled且等待跨日分钟 | 未来查询范围，不以完整日K冒充分钟。 |
| B03（108）更多周期/均线设置 | 同组件93行均线disabled、228行更多disabled | 尚未定义完整范围，周月展示已有不核销新入口。 |
| B04（109）资讯/社区/快捷页 | 相应标签/入口为占位或disabled，实际盘口/资金统计另已有 | 未来内容产品，不升当前G。 |
| B05（110）收盘竞价曲线 | ADR0014明确独立尾盘图尚未做；真实收盘撮合和日K已有 | 未来坐标契约，不重开撮合。 |
| B06（111）多槽/成就/完整流水/云 | `save/save-repository.ts:70`单快速槽保存；最近100笔展示不是完整历史 | 未来Stage2产品。 |
| B07（112）公网身份/多人 | `apps/server/src/routes.rs:51`authorized_session；Remote 279/287等携token | 私有会话授权已有；浏览器WS凭据路径G01不能用HTTP token核销。公网账号/多人未批准。 |
| B08（113）DB迁移/重启恢复 | actor进程内状态、save/load传输不是数据库 | 未来存储决策，不造DB。 |
| B09（114）TLS/Origin/运维 | HTTP/CORS Any，日志与health已有 | 未来公网配置；不恢复任意挂单数配额。 |
| B10（115）桌面签名/updater | bundle存在，无已配置证书/updater消费 | 分发产品范围未定，桌面壳已实现。 |
| B11（116）i18n | 首发中文已决，无第二语言框架 | 未批准扩展。 |
| B12（117）GPU内核/回测 | engine-gpu探测并委托CPU | 明确实时GPU不做，离线需另设计；不要求把探测伪称计算。 |
| B13（118）冷热/历史分钟归档 | 权威日K追加已有，完整分钟/逐笔/盘口长期归档未选定 | ADR0018proposed，不全面升级。 |
| B14（119）全面COW/稳态根/摘要 | account页与candle分享已有；`RootReadContext::capture`历史PlanBook复制仍G16 | 未来全面架构不能核销已决普通tick复制G16，也不能说所有历史每tick深复制。 |
| B15（120）反向唤醒/防重留存 | 活跃计划索引已有；完整反向索引/终端版本保留政策未定 | 不为省内存删除权威防重事实。 |
| B16（121）WAL/durable watermark | ADR0010明确当前不引入崩溃WAL | 条件提案，非当前遗漏。 |
| B17（122）观察token/旧私有查询 | generation/seq门控已有，完整版本token策略未定 | 未来契约，不能因没有全token判普通门控无效。 |
| B18（123）2099后 | current日期上界明确2099-12-31 | 延伸需规则包，不仅放大整数。 |
| B19（124）高级学习/组合/L2 | 混合机构分析/个人记忆/计划已有；散户日期衰减仍G08 | 未来复杂模型与既有G08区分，不用“失败衰减已有”泛称散户已接。 |
| B20（125）现金编辑冲突 | 正常公共日级档无日内买挂单，ADR0025明确边界 | 已核销，不自动撤单/补钱。 |

## implementation-gaps 其余章节

| 章节/行号 | 状态及反证 |
|---|---|
| §4明确不做/简化（127–149） | 股东动作/资金重置/真实历史导入/新增板块订单/高级会计/旧兼容等属于明示范围，不新增待办。高级集团简化不取消G28普通单层固定集团公开承诺；现实未支持制度不得由默认值冒充。 |
| §5规则证据（153–159） | official_coverage默认空与官方资料blocked是证据范围；非空官方覆盖取消模拟休市分支仍G15。不能借“不用真实市场数据”编造官方制度，也不把CAS6取证提及扩为新资产产品。 |
| §5验收缺口（160–166） | 真实宿主/性能/浏览器/最终矩阵仍需证据。旧host-parity/release-contract/verify-plan名字不直接要求复活；较新发布/架构决定与现有矩阵按相应任务核对。当前K7跨worker整artifact比较仍G39，不据旧工具缺名新开。 |
| §5测试增强与调度（167–170） | 银行到期/工商零金额等需按行为找测试，不能只因旧文件不存在列漏功能。候选Mutex/私有候选先改状态不是已确认提交泄漏。 |
| §6排除旧说法（172–190） | `routes.rs:41`引擎MAX_SAVE_DECODE_BYTES+1MiB；`session/observation_clock.rs`午休时间；经营/指标/并行/allocator/HTTP授权均已有；并发受理实际轨迹允许变化。旧方法名、ID字段不是现行功能存在的充分或必要条件。 |
| §7全文覆盖表（192–259） | 60批与134份是历史覆盖证据，不代表本轮读完表所列全部正文或全部日志；本轮仅核本分片三文件。 |

## naming-conventions 全文逐条

| 原文条款 | 当前状态/证据 |
|---|---|
| 1–6行业职责命名，不以任务号代领域 | 已有职责重命名，具体示例12–21：account_validation、IntentCandidateKey、PreparedContinuousTick、prepare_auction_tick及SIMULATION_CHILD_TIMEOUT_MS。不据旧路径缺失说撮合校验没写。 |
| 8–21当前示例全表 | 已有相应职责模块/类型/工具；`scripts/simulation/baseline-run.mjs:24`与verify-simulation-artifacts.mjs:14共用300000ms上限名字；地产project_id/日历saved_policy类局部名是命名规则，不是新业务功能。 |
| 23–25真实版本/ECL阶段保留 | runtime_v2/industrial_chart_v2/ReceiptSourceV2与Stage1–3保留不是漏清理，不擅自改JSON或会计阶段语义。 |
| 27–35serde与事件身份不变 | `pipeline/event_key.rs:47`QuoteExpiry仍serde P0；`receipt_key.rs:37`P0Expiry；`envelope.rs:8`P3Created；`ledger.rs:41`p0_released。EventFact与Receipt source是不同契约，不能仅看名字重写实际Account/Sealed身份。 |
| 37–39历史格式/证据原样保留 | `verify-simulation-artifacts.mjs:227/327`检查fresh_current_k7_setup与k7-bounded-representative-profile-v1；P0–P9测量键保留。历史.omo无需重写。 |
| 41–45矩阵运行身份同步迁移 | `run-escrow-verification-matrix.mjs:24/25/27`新schema/version/scenario；`examples/escrow_verification_harness/cli.rs:3`同scenario，matrix测试83/225行同标签；未找到现行producer保留旧task9标签的反例。 |
| 47–48重命名不改A股/资源释放/提交语义 | serde旧spellings和现有交易路径保留；不能以命名整理取消T+1/费用/价时/同批释放边界。未运行交易回归。 |

## naming-refactor-validation 全文章节

| 原文章节/行号 | 当前判断与代码反证 |
|---|---|
| 首部（1–5） | 历史命名批次与职责规则，不是重新授权全部重构。 |
| 已保存改动（7–22） | 6历史提交、压缩档仅改名/格式身份配对/真实Termination与Expired保留/cfg(test)helper记录；没有凭缺旧文件名新造工具。低层内存checkpoint与公共日终保存始终分开。 |
| 通过定向验证（24–39） | 历史515等计数/资源预算保存，不称本轮重跑；Node双deadline/Cargo多线程是历史执行参数，不推导完整回归通过。 |
| 未通过扩展：Server旧save缺参（45–46） | **当前caller反证**：`tests/actor.rs:166/197/282/305`均handles.save(session,None)，`api_contract.rs:128/474/833/854/877/901/1167/1208`同样。剩下session.save()/restored.save()是低层GameSession，不是遗漏Option参数的handles。未编译，不报全target通过。 |
| 未通过扩展：Worker预期step实际message（47–48） | **当前caller反证**：test38行显式postFailure("wasm-worker.step",…)；worker.ts:83–89透传传入where或真实failure.where。336行消息catch传message是另一个生产调用，不能据此判断这些直接helper测试仍错。不运行7例，不宣称完整Web翻绿。 |
| 未通过扩展：两Rust harness局部索引（49–52） | 当前runtime.rs:985/1056仍真实执行capture；contracts.mjs:403/406/431/432按journal/source/key作用域检查ordinal。没运行harness，不能把本轮静态存在当修复或通过，也未凭旧日志确认当前依旧失败。保留验证证据欠账，现行跨worker矩阵契约G39另列。 |
| 未通过扩展：多轮成交顺序（53–54） | `continuous_tick_finalizer_tests.rs:330`当前固定trade_session(4)并337/341行精确价格、345行成交数；仍需当前运行结果。不能因旧一次失败推断默认生产价时错误，也不弱化断言。 |
| 未通过扩展：两company_scenarios超10秒（55–58） | fixture与跨日验收时长属测试资源/证据问题；本轮未运行，不能确认当前超时或宣称通过。普通命令硬上限不放宽，ignored年界仍长验收。 |
| 未通过扩展：全类型导出超10秒（59–60） | 两目标类型通过不是全导出通过；仍保留测试批次划分与覆盖证据范围，不新增游戏功能G。 |
| 本地日志未Git（62） | 历史原始日志范围诚实保留，不编造当前.raw产物。 |

## 候选反证收口

- 旧A01–A11未实现：均已找到现行caller/结构/UI反证，维持核销；不能因此核销后续G17/G22/G28/G37及个人分析G06–09。
- B表全面实施：明确不授权；B01/B07私有授权/B20已核销，B14对已决普通tick性能遗漏仍G16，B19散户dated未接仍G08。
- 名称变化造成证据格式漂移：未找到已确认新反例；P0/P0Expiry/P3Created显式serde与矩阵新标签配对已有。真实版本、历史schema和阶段号不清除。
- 旧测试失败直接报当前失败：两项明确caller反证，其他保留待运行证据；没有测试执行则不报当前绿或红。
- 按§5缺旧工具名重造host-parity/release-contract/verify-plan：不做。工具能力/新批准发布边界需按现行契约，不恢复退役路径；本分片未新增独立代码缺口。

本轮仅静态全文与caller审计，没有运行测试、构建或长验收。报告交父agent纳入汇总与完整diff独立复核。
