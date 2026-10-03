# Sweep 41：后端 Rust、RTK 与三宿主前端框架

- 产品基线 `b76ece3`，工作树 HEAD `4ad5a2e` 为审计合入提交、生产文件相同；已遵守根 AGENTS/principles。
- 连续全文读至 EOF：`docs/decisions/0003-backend-rust.md` 38 行、`0004-frontend-state-redux-toolkit.md` 41 行、`0007-three-deployment-frontend-framework.md` 83 行，共162行，首次读取无截断。
- 只新增本记录，未运行测试/构建/浏览器/长验收。补充读取现行 ADR-0005 联机排除与 UX/DESIGN 无障碍条款，避免把未来账号与 GPU 当当前任务。

## ADR-0003 所有章节

| 原文章节/行号 | 当前caller与状态 |
|---|---|
| Status/日期 1–5 | accepted保留Rust与engine复用，Stage2串行被ADR0005三端并行替代；不能按31行旧顺序说当前后端应等Stage2 |
| Context 7–14 | engine Rust/Tauri同栈、防御性强类型；`apps/server/Cargo.toml:1`Rust crate与path engine依赖、`apps/server/src/lib.rs:6–12`直接rlib+actor组成支持 |
| Decision 16–18 | Rust后端已实现；`apps/server/src/lib.rs:49`router工厂→61路由，`src/main.rs:35`Axum serve；`actor.rs:927`新session→962组actor→976spawn，真实复用engine，不是只有manifest依赖 |
| Alternatives 20–24 | Go/Node未选，不要求实现或删除TS前端 |
| Consequences 26–32 | Axum已定、直接crate复用已落实。请求/错误通过routes→SessionHandles→actor→engine；HTTP/WS与持久化格式见统一宿主契约，现有G01–G05/G18–G20不因Rust后端存在核销 |
| Related 34–38 | Q2已解决；关联导航不产生账号系统新任务 |

联机语境反证：ADR-0005:18、77、120明确多前端账户联机“预留/日后接/不在本阶段实现”；0003:9“联机、远程持久化”背景不能推导注册登录、数据库多人PvP为现行未实现。Server有session鉴权token与save/load端点(`lib.rs:85–86`)，不等于未来多用户账号已实现。

## ADR-0004 所有章节

| 原文章节/行号 | 当前caller与状态 |
|---|---|
| Status 1–5 | 数据源扩展WASM Worker/WS，当前同一RTK store接三宿主，已实现主干 |
| Context 7–13 | engine权威、UI选中/表单/弹窗/缓存编排；`store/store.ts:9`RTK、39 snapshot slice、148settings；`app/useTradingCommands.ts:25`表单local state、74提交；React局部useState不违反RTK选择 |
| Decision/理由 15–22 | Redux Toolkit依赖并实际configureStore；RTK Query“可覆盖后续”为能力说明，不要求当下强制RTK Query替换统一Host |
| Alternatives 24–28 | Zustand/useReducer未选为全局管理，但移动UI局部`useMobileUiController.ts:12` reducer仍只导航，不构成架构违约 |
| Consequences 30–36 | `App.tsx:217`ProtocolCoordinator→220基线/229应用→254 acceptReduction；`store.ts:47`安装基线、66增量→77applyRuntimeDelta；`useTradingCommands.ts:80`host.submitIntent。RTK缓存权威返回，输入预校验/展示派生与engine成交语义不同，已有Q08指标口径待澄清，不把所有前端计算判双写 |
| Related 38–41 | Q5已定，architecture/tech-stack扩展契约由对应sweep复核 |

前端条件单当前有明确UI边界：`components/auto-order-manager.ts:78`消费权威价点→96异步submit→99 PlaceLimit，`useTradingCommands.ts`只创建/管理UI条件，实际成交仍engine负责。不因该类拥有trigger/enabled状态认作公司经营/持仓双写；失败107回调显式反馈，不能仅看到catch就称吞错。表单字段关联缺失仍G22。

## ADR-0007 所有章节与条款族

| 原文章节/行号 | 当前caller与判定 |
|---|---|
| Metadata/Context 1–10 | 同份engine/UI三部署承诺，旧144测试仅当时状态；`app/useSessionHostLifecycle.ts:204–207`实际选择Tauri/Remote/Worker，统一App，不是三份独立产品UI |
| Decision §1 14–17 | `host/engine-host.ts:38`统一接口、39start/44stop/46setSpeed/54submit/55snapshot，扩展异步save/query/indicators仍同契约；`useSessionHostLifecycle.ts:205–207`调用三工厂。旧WasmWorkerHost类名不是必须；当前createWorkerHost等有效替代 |
| §2 React/Vite/TS/RTK/Blueprint 19–21 | `apps/web/package.json`三库依赖，`App.tsx:12`实际Blueprint、`MarketGrid.tsx:6`AG Grid、`components/PriceChart.tsx`与`app/useMarketChartRuntime.ts`图表生产消费；不是只有安装依赖 |
| §2亮暗/中文 22–23 | `store.ts:143`默认light、`App.tsx:514`切换、456data-theme；`index.css:3,17–21,91`亮/暗颜色与中文字体/红涨绿跌；中文表头已有，AG Grid辅助文本本地化漏项S41-N1 |
| §2表格/图表/工作台 24–26 | `MarketGrid.tsx:157`真实AG Grid；`PriceChart.tsx:49`volume默认，90–94 MACD/KDJ按钮可选，非默认全开；`WorkspaceGrid.tsx:55,65,66`ResponsiveGridLayout拖动缩放。指标transport真实接线的G17不由按钮存在核销；未来GPU内核不列此处缺口 |
| §3布局 28–31 | `App.tsx:524`统一WorkspaceGrid、portrait分支移动详情/底栏/交易底页；`useOrientation.ts:12`width>=height横屏、23resize更新；`WorkspaceGrid.tsx:99`移动/103桌面分支。按方向而非设备宽度是明确选择；不能要求另一套断点政策。G33局部字号仍在 |
| §4 WASM边界 33–35 | `host/worker-host.ts:255`启动Worker、264消息；`wasm-worker.ts`创建/调用WASM会话，JSON/句柄边界沿用，后续协议增加civil/delta/report并未要求只能四种旧类型。旧PriceTick合并/Trade全保留已被TickBatch事实协议与分层刷新替代；`wasm-update-delivery.ts:14,21`解析TickBatch/CivilUpdate。G10–G14与背压接线由现行实际链评定，不从旧rAF字面要求重复造路径 |
| §5类型 37–42 | `types/engine.ts:4–5`生成薄别名、11起reexport；根package.json:17 types:generate；CI `.github/workflows/ci.yml:207–210`Rust测试后检查生成；`scripts/check-generated-types.mjs:3–15`git status包含新增未跟踪，已落实漂移门禁。typeshare已被ts-rs替代 |
| §6多线程/GPU 44–46 | Server actor独占GameSession(`actor.rs:1006`)、每局spawn976；Desktop actor源码已有，host真实线程与背压仍G18/G19；GPU §73明确仅设备探测、权威CPU/真实核未来，不因缺真实GPU计算列当前G。API Send/Sync与当前engine内部并行不等价“所有处理必须单线程” |
| §7管理器/许可 48–50 | pnpm workspace/package manifest与MIT已有，不要求改许可证或引入另一包管理器 |
| §8 E2E 52–55 | `apps/web/playwright.config.ts:8–9`并行/2workers、15chromium、18–22Vite build+preview；CI225–227 Linux browser E2E，其余平台Rust/TS。当前e2e mode单步桥`App.tsx:84`与`useSessionHostLifecycle.ts:143`是fixture/控制通道，同一UI，不凭该桥判整套UI测试专用。源码覆盖不等于本轮浏览器通过 |
| Alternatives 57–64 | Ant/antd-mobile/共享Mutex/f32/手写类型/typeshare均否决；本轮不重新引入方案或要求名称一致 |
| Consequences 66–74 | 三端同UI、Blueprint本地化、CPU权威、生成结果同步的持续要求保持；对照S41-N1、既有host/UI总账；73明确GPU仅探测为未来排除 |
| Followups/Related 75–83 | WS0–6计划入口/相关ADR导航，完成状态必须看当前生产链，不能把泛指“后续需要做”全部当未实现 |

## S41-N1：AG Grid 内置辅助文本未按中文界面本地化

原文 `0007-three-deployment-frontend-framework.md:23` 要求“全中文、无西文（Blueprint英文标签本地化覆盖）”；UX-CONTRACT:8 active locales=zh-CN、10 WCAG2.2AA，界面整体语言承诺不只自写表头。

实际 `components/MarketGrid.tsx:80–127`确实中文headerName，131–135 defaultColDef启用sortable。完整 `<AgGridReact>`157–170没有localeText/getLocaleText，全 `apps/web/src`检索也无全局Grid本地化配置。根工作区实际安装的AG Grid36发行源码（只读依赖文件）：`node_modules/.pnpm/ag-grid-community@36.0.0/node_modules/ag-grid-community/dist/ag-grid-community.noStyle.js:1632`无Locale服务则defaultLocaleTextFunc；同文件15119 sortable header的 `ariaSortableColumn`默认“Press ENTER to sort”。当前代码没有为它提供中文覆盖，因此中文标题不能兑现内置可访问说明全中文。

反证与边界：未运行浏览器可访问树，不报告用户当前已看见某个英文空表overlay；`MACD/KDJ`系技术术语，不能简单作为一般英文标签遗漏。Blueprint自写按钮大多中文，不能说全部UI未本地化。该项与 sweep02 N05 `<html lang=en>`不同：修lang不会翻译组件默认文本，翻locale也不会修lang。建议在实际sortable表头/空数据及辅助名称验证中文，而不只源码查headerName。

## S41-N2：桌面行情选股缺键盘等价入口（候选，需实际浏览器核查）

现行UX:6主任务含“浏览自选、查看个股”，10 WCAG2.2AA；桌面行情 `MarketGrid.tsx:139–144`仅 onRowClicked 触发onSelect，165接入该click，169 suppressCellFocus=true，没有onCellKeyDown/onRowKeyDown或原生按钮/链接替代，单元格只valueFormatter。与之相反，移动行情196–201原生button有标准键盘激活，移动链不能反证桌面有替代入口。

依赖源码（同AG Grid36文件）12942将suppressCellFocus作为停用cell focus条件，36240–36245此值为true时移除cell tabindex，说明该选项不是只隐藏焦点样式。桌面仍可聚焦排序header不等于能选择股票；没有browser验收，先列待验证候选，不声称已证明所有键盘路径失效。它也不同于G23详情导航焦点恢复和G30图表工具焦点样式。

## 反证/排除

- 暗色CSS实际存在 `index.css:91–112`，不能从旧“强制浅色”注释或未调用useTheme推断切换无效；App456自己的data-theme足以对子树变量生效。移动高对比浅色有后续UX:26选择，应与桌面主题分开。
- 临时表单/导航useState/useReducer是UI本地状态，不是engine业务规则双写；RTK Query属可选能力说明。
- 完整联机/注册登录/多用户持久化数据库、真实GPU整数内核与蒙特卡洛属于明确未来或未接受路线；不新增待办。
- 已有G01–G05/G10–G14/G17–G20/G22–G25/G30–G34仍应由对应生产证据核查，框架/库/入口“存在”不能核销这些细节。
