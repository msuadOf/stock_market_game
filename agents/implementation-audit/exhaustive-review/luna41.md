# ADR-0003/0004/0007 全文实现复核

## 范围与方法

- 复核基线：`08e4fc7`（工作树 HEAD `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`，用户说明产品提交与 merge 相同）。
- 按 EOF 连续阅读全文：`docs/decisions/0003-backend-rust.md` 38 行、`0004-frontend-state-redux-toolkit.md` 41 行、`0007-three-deployment-frontend-framework.md` 83 行。逐章核对状态/上下文、Decision、Alternatives、Consequences、Related，并沿当前调用方追 Server、RTK、响应式布局、界面文字、键盘和三宿主；不是片段代读。
- 依据源码静态追踪，不运行测试或浏览器，不以测试源码代替运行证据。当前依据包括更新决定 ADR-0005、0010、0027、0028 与 `UX-CONTRACT.md`。未审 A 股交易规则本身；本组结论不改变 engine 权威、分/股单位或交易行为。

## 逐章承诺矩阵

| ADR 章节/原文位置 | 当前生产链、实现与消费 | 复核结论 |
|---|---|---|
| ADR-0003 §状态/上下文 1–14 | 状态行已说明 ADR-0005 将“Stage 2 才有后端”更新为三宿主并行可选且框架为 Axum。`apps/server/src/main.rs:13–35` 解析部署选项、构造 router 并启动 HTTP/WS；`apps/server/src/lib.rs:60–110` 路由到 API/WS；`apps/server/src/routes.rs` 校验请求并调用 `SessionManager`/actor。 | Rust 后端与阶段状态表述已更新；不把早期“Stage 2 起步”误读为现行串行发布要求。 |
| ADR-0003 Decision 16–18 | `apps/server/Cargo.toml:23–37` 直接依赖 `engine` path crate 与 Axum；生产 API 使用 `engine::Intent/SessionSetup/SaveSlot` 等类型，经 actor 执行业务，再由 REST/WS 向前端传输。 | Rust、engine 复用和 Axum 均已接入。 |
| ADR-0003 Alternatives 20–24 | Go 与 Node 是决策理由，不是独立运行功能；当前部署构建边界由 ADR-0027/0028进一步钉定，纯 Server 可不依赖 Node，WebUI 部署使用 Rust/Axum。 | 未发现替代实现导致决策漂移。 |
| ADR-0003 Consequences / 后续 26–32 | `Result` 错误在 route/actor 映射为显式 HTTP 错误或 WS failure；crate 直接依赖已选定，Axum 已由 ADR-0005 固化。公共持久化现受 ADR-0025 的日终保存边界约束。 | 早期待办已由后续实现/决策收口；不扩展成 WAL 或进程崩溃恢复承诺。 |
| ADR-0003 Related 34–38 | Q2 作为历史开放问题已解决；ADR-0002、技术栈仍为关联背景。 | 无独立待实现功能。 |
| ADR-0004 Context 1–11 | ADR-0005/0010 将来源明确扩为 Worker、Remote WS、Tauri IPC 的 `HostUpdate`，应用层 coordinator 校验协议，再更新 Redux；React 经 Provider/selectors 消费。`App.tsx:193–196` 把宿主更新交给 `ProtocolCoordinator`。 | engine 仍为游戏权威；Redux 的角色为 UI 编排与经验证的状态视图。 |
| ADR-0004 Decision / 理由 13–22 | `apps/web/src/store/store.ts:9,232–245` 使用 RTK `configureStore` 装配 snapshot/trades/settings/priceHistory/selectedStock/autoOrders/company；`render-app.tsx:7–14` 通过 Provider 注入。各 slice 由协调器派发更新，组件 selectors 消费。 | RTK 已接入。RTK Query 在原文是后续能力/铺路，不是必须新增的功能。 |
| ADR-0004 Alternatives 24–28 | Zustand 与 Context 是决策偏好取舍；当前没有要求两者并存或提供库替换接口。 | 不衍生缺口。 |
| ADR-0004 Consequences / 后续 30–36 | `store.ts:47–81` 安装带 generation 的 baseline 并验证 delta 游标；`:89–95` 保留未变行情分支引用。交易意图由 host 适配器送往 engine，结果由协议更新进入 store。 | 权威边界符合决策；Redux 并未取代 engine 执行撮合。 |
| ADR-0004 Related 38–41 | Q5 已解决，架构/技术栈文档为关联依据。 | 无新增候选；`RTK Query` 未见调用不能单独判未实现。 |
| ADR-0007 Context 1–10 | 同一 Web app 在 Worker/Remote/Tauri 运行。启动生命周期按 target 选 host：`useSessionHostLifecycle.ts:204–208` 分派 `createTauriHost/createRemoteHost/createWorkerHost`；三个 adapter 各自实现 `EngineHost`，App 仅消费统一协议。 | 三宿主适配主干已接。能力差异仍由各 host 显式暴露，不假定每宿主功能相同。 |
| ADR-0007 §1 14–17 | `engine-host.ts:28–69` 定义统一 host/能力接口；`App.tsx:193–196` 统一接受 HostUpdate；Remote REST/WS、Worker message、Tauri invoke/listen 是实际 caller。 | 已实现。不是每个接口方法都在每宿主拥有同一底层传输。 |
| ADR-0007 §2 技术栈/视觉/主题 19–26 | `App.tsx:12` 使用 Blueprint；`MarketGrid.tsx` 使用 AG Grid，图表在 `PriceChart.tsx`，布局工作区在 `WorkspaceGrid.tsx`；主题由 Redux→App CSS variables。 | 栈与布局消费路径存在。视觉效果和第三方库完整本地化需浏览器运行证据，本复核不宣称截图验收。 |
| ADR-0007 §2 全中文、无西文 23 | 自有启动界面含“远程 Server”“WASM 多线程 Worker”“HTTP(S)”等技术标识（`StartupScreen.tsx:27–38`）；AG Grid column headers 自有列名为中文（`MarketGrid.tsx:79–119`），但组件未传 `localeText`/`getLocaleText`（`:154–170`），全仓未找到相应 AG Grid locale 配置。 | 自有标签大体中文，不能证明 AG Grid 内建菜单/提示/辅助文本中文；该子项列为新的强候选，是否实际暴露英文需可访问树/浏览器核实。`WASM/Server/HTTP` 等协议或产品名不据字面认定违反“无西文”，需按产品术语口径判断。 |
| ADR-0007 §3 桌面/移动/判据 28–31 | `useOrientation.ts:10–25` 以 `innerWidth >= innerHeight` 选择 landscape，并监听 resize；`App.tsx:143,453–456` 将 orientation 传入 class，CSS 使用 `.layout-mobile/.layout-desktop`；`WorkspaceGrid` 消费方向分配工作区。 | 判据及布局分流已接。横竖切换之外还有 CSS 断点微调，不替代方向语义；未运行真实尺寸矩阵。 |
| ADR-0007 §4 WASM 边界 33–35 | Worker adapter 隔离 WASM 与 UI；生产启动对缺少跨源隔离/SharedArrayBuffer 显式报错且不静默降级（`startup-policy.ts:29–58`）。 | Worker 与失败边界有生产路径；每帧背压/采样是 ADR-0010 后续承诺且历史报告另跟，不据本 ADR 章节单独核销。 |
| ADR-0007 §5 类型同步 37–42 | 生成 TS 边界类型位于 `apps/web/src/types/generated/`，host/parser 使用生成边界；CI/发布策略已被 ADR-0028重定为手动 CI、标签发布构建，不能把旧 ADR 中泛指 CI 解读为普通提交必跑。 | 类型方向仍成立；CI 触发口径服从更新决定。 |
| ADR-0007 §6 并发/GPU 44–46 | Server actor-per-session 源码注释与 `apps/server/src/actor.rs:1–8` 明示每 session 单 task 独占会话、无锁消息交互；GPU 仍是可选默认关闭的架构预留，不是当期功能承诺。 | actor 模型已接；GPU 内核未来待启用不列未实现缺口。 |
| ADR-0007 §7 package/license 48–50 | pnpm workspace 与 MIT 许可证在仓库配置/`apps/LICENSE`。 | 已接。 |
| ADR-0007 §8 E2E 52–55 | Playwright 配置和 E2E 源码存在；ADR-0028规定标签发布不运行 E2E，CI 是独立手动入口。 | 当前测试代码存在，不宣称本轮已执行。 §8 对已覆盖用例的描述是历史/当前测试范围，不应由发布链路推出每次发布已验收。 |
| ADR-0007 Alternatives 57–64 | 备选是历史设计理由；当前依赖和组件实现与所选栈吻合。 | 无替代方案回归候选。 |
| ADR-0007 Consequences / 后续 66–77 | 第三方集成曲线仍属维护成本；Blueprint 中文/主题维护义务仍在。WS-0..WS-6 是历史计划参考，实际完成情况以上述当期生产消费为准。 | 不把“后续计划”整体重新立项；语言和键盘候选需单独验证。 |
| ADR-0007 Related 79–83 | ADR-0005/0004及已解决 Q3/Q4 为关联文档。 | 无新缺口。 |

## 新候选复核与反证

### N1：AG Grid 辅助文本未显式中文化

- **原始承诺：** ADR-0007 §2 第 23 行“全中文、无西文”，并指出需覆盖 Blueprint 英文标签；UX-CONTRACT 第 8 行规定活动 locale 为 `zh-CN`。
- **当前证据：** `apps/web/src/components/MarketGrid.tsx:79–119` 只定义中文业务列标题；`AgGridReact` 配置在 `:154–170`，没有 `localeText/getLocaleText`，仓库生产源码搜索也未发现 locale 配置。第三方 grid 的标签、提示与辅助说明没有被项目显式覆盖，因此自有中文列名不能作为完整本地化证明。
- **可能反证/限制：** ADR-0007 指定的是 “Blueprint 英文标签本地化覆盖”，没有逐字说明 AG Grid 每个 aria 文本；AG Grid runtime 内置默认语言/版本行为不能只靠源码搜索断言实际暴露英语。需要锁定依赖版本、检查可访问树或实际浏览器，再确认缺陷范围。本轮未运行浏览器，候选保持待证而非已证实用户可见缺陷。

### N2：桌面行情行没有键盘选股路径

- **原始承诺/目标：** ADR-0007 §8 仅明确移动端详情键盘导航用例；UX-CONTRACT 第 10 行整体声明 WCAG 2.2 AA。ADR-0007 §3 将桌面行情区指定为 AG Grid。
- **当前证据：** 桌面 AG Grid 在 `MarketGrid.tsx:139–144` 只以 `onRowClicked` 调用 `onSelect`，`:157–170` 明确 `suppressCellFocus={true}`；没有 grid 的键盘事件处理器、行内按钮或链接。相同组件的移动列表 `:196–201` 用原生 button，可键盘触发，但不能为桌面 AG Grid 提供选股操作路径。UX flow ledger 的开个股触发记录只有点击行情行（`UX-CONTRACT.md:64–67`）。
- **可能反证/限制：** 没有逐行浏览器/辅助技术实测；AG Grid 的外层容器或当前运行版本是否留下可达键盘操作需检查 DOM/可访问树。移动端明确有键盘导航 E2E，但测试范围只覆盖移动详情，不是桌面行。静态证据足以保留强候选，尚不据此声称完整 WCAG 失败结论。

### 旧结论复核与新决定优先级

- `coverage/s21.md` ADR-0003/0004 旧结论仍准确：Rust/Axum 复用 engine 已落地；RTK 的选择与状态边界已落实；RTK Query 是能力意向而非强制功能。较新的 ADR-0027/0028进一步细化部署形态与构建触发，不改变上述产品实现判定。
- `coverage/r19.md` 中 ADR-0007 对 React/Blueprint/AG Grid/图表/工作区/主题的“生产已接”结论仍准确，但其“未发现明确未接功能”需要被本次 N1/N2 候选修正；原报告也已保留所有本地化及视觉验收证据不足的限制。
- `luna02.md` 已记录的 S41-N1（AG Grid 英文辅助文本）与 S41-N2（桌面行键盘选股）本轮以完整 ADR 原文、当前 `MarketGrid` caller、UX-CONTRACT 和部署更新再次反证核查；两项候选均仍成立为待浏览器确认的问题，不重复新编号、不冒充运行时实测。
- 本次不是把 UI 术语中所有拉丁字母一律判违规：协议名/技术名是否属于“界面语言”需按产品术语决定；N1 精确针对第三方组件自动生成的辅助/菜单文本。
- 静态源码复核未发现 ADR-0003/0004 主干实现遗漏。除 N1/N2 待确认候选外，不新增 G/Q；未执行测试、构建、可访问树、屏幕阅读器或真实尺寸/方向视觉验收。
